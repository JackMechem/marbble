//! Strips playing at the same time: what makes `Engine::process_block`'s strip loop safe to run on
//! several threads, with output bit-identical to the serial loop. "Multi-core processing" in
//! docs/ARCHITECTURE.md has the design in brief; what a block touches is listed at the end here.
//!
//! The serial loop lets a strip write straight into the strips after it (their `sum_l`/`sum_r`),
//! and into the Stereo Out bus. Floats summed in another order give other bits, so here nothing
//! writes across strips. A strip leaves what it plays with its own [`Outlets`], and a strip that
//! receives ([`Deferred::inputs`]) adds up its feeders' outlets, in the graph's order, which is the
//! order the serial loop's writes land in. Stereo Out's bus is summed the same way by the calling
//! thread once the strips are done (`Engine::process_block`). `run_strip` is the same code for both;
//! only its `Routes` differ.
//!
//! The rest of a block stays on the calling thread, before or after the strips: commands, garbage,
//! live MIDI and recorded MIDI's ring, folder automation (read in `Parallel::run`), the analyzer's
//! ring (the strip keeps a copy for it), Stereo Out and the click.
//!
//! A plug-in that stops responding on a worker costs its strip, not the engine: the worker is left
//! in it (`Pool::abandon`), the strip gives way to a stand-in, and that and everything it feeds sit
//! out from then on (`Parallel::leave_out`), while the rest plays on.
//!
//! **What a block touches, and who may:**
//! - Commands, `Garbage`, the command ring, `garbage`: the calling thread only, before the strips;
//!   the graph and the workers change only there, between blocks.
//! - `live_midi`, `live`, `inputs`: filled before the strips, read-only for them.
//! - `midi_rec` (recorded MIDI): moved out before the strips, serial loop included; it comes from
//!   one track anyway.
//! - The analyzer ring: on the pool the watched strip fills `Outlets::analysis` and the calling
//!   thread pushes it after the strips. Stereo Out's tap is on the calling thread.
//! - `bus_l`, `bus_r`: summed by the calling thread after the strips, in order. `sum_l`, `sum_r`:
//!   the serial loop only. `tap_*`, `gain_*`: per-thread `Scratch`.
//! - A strip's own `TrackDsp`, its plug-ins, regions and recording ring: its task only, so one thread
//!   at a time, handed over by the pool's Acquire/Release. Which thread can change from block to
//!   block, as VST3 allows.
//! - A feeder's `l`, `r`, `outs`, `key_*`, an instrument's output buses and `instrument_ran`: read
//!   by its receivers only once its task is done (an edge), by any number at once.
//! - A folder's parameters and lanes: read before the strips start (`fold_now`).
//! - Parameters, tempo, `Isolate`, chase, the MIDI target and the transport: `BlockCtx`, copied or
//!   shared immutably. `TransportInfo` for plug-ins is a copy per strip, at its own input time.
//! - `Gate`, `OutGate` (VST3 bus changes): atomics; `Cmd::VstGate` applies between blocks.
//! - Meters, `FxShared`, the `EngineStatus` counters, a strip's `StripLoad`: atomics. `bad_audio_uid` is a plain store,
//!   so with two bad plug-ins in one block either may be named. Input meters and the load figures:
//!   the calling thread only.
//! - Global state inside third-party plug-ins: their concern, as in any host that plays instances
//!   in parallel. Nothing in `fx/`, `instrument/`, `synth_dsp/` or the VST3 processing path keeps
//!   mutable globals; an instrument's randomness comes from its slot's seed.
//!
//! The `unsafe` here and in `pool/` has its argument beside it. The parallel tests,
//! `parallel_check` and `soak_check` run clean under ThreadSanitizer (see docs/DEVELOPING.md).

use std::cell::UnsafeCell;
use std::time::Duration;

use super::graph::{GraphRoom, TaskGraph, MAX_TASKS};
use super::pool::{Call, Health, Job, Pool, PoolOptions};
use super::*;

/// Below this share of the buffer the callback plays alone: waking threads costs more than a
/// project this light can gain, and a sleeping worker can't add to the worst block.
const ENGAGE_LOAD: f32 = 0.15;
/// The least the callback waits for a block's last strips before it gives the block up. Only a stuck
/// plug-in or a worker kept off the CPU gets near it. The callback naps through most of this, so the
/// kernel's 200 ms real-time limit isn't at stake for it. The stuck worker is demoted then
/// (`Pool::retire_stuck`), which must come before that limit: this, plus the pool's `RT_GUARD`, is
/// well under it.
const GIVE_UP_SECS: f64 = 0.1;

/// How a block in the pool ended.
pub(super) enum Outcome {
    Done,
    /// This many strips panicked, and the others ran. The block is silenced.
    Panicked(u32),
    /// Some strip still isn't done (`GIVE_UP_SECS`). The block stays open.
    Stalled,
    /// Workers were left in plug-ins that didn't return (`GIVE_UP_SECS`), and the block is done
    /// without their strips and what those feed. `Parallel::leave_out` comes next.
    LeftOut,
}

/// One block's work, as the pool's threads see it.
struct StripJob {
    /// Lives as long as the block is open: see `Parallel::run`.
    cx: BlockCtx<'static>,
    tracks: *const *mut TrackDsp,
    scratch: *mut Scratch,
    graph: *const TaskGraph,
    /// The track each thread is in (0: none).
    busy: *const AtomicU64,
}

// SAFETY: the pointers are what `run` says they are, and `BlockCtx` is Sync by itself.
unsafe impl Sync for StripJob {}

impl Job for StripJob {
    fn run(&self, task: usize, thread: usize, call: &Call) {
        // SAFETY: the pool gives each task to one thread and each thread runs one task at a time,
        // so this is the only reference to this strip and to this thread's scratch. No other task
        // looks at this strip until it's over: only tasks that wait for it do (`Deferred::earlier`).
        let (t, sc) = unsafe { (&mut **self.tracks.add(task), &mut *self.scratch.add(thread)) };
        // SAFETY: valid and unchanging while the block is open.
        let graph = unsafe { &*self.graph };
        // SAFETY: one per thread, and it lives as long as the pool.
        let busy = unsafe { &*self.busy.add(thread) };
        busy.store(t.id, Ordering::Relaxed);
        #[cfg(test)]
        test_hooks::task_starts(t.id);
        let at = std::time::Instant::now();
        t.outs.reset();
        let mut routes = Deferred { tracks: self.tracks, graph, ti: task, n: self.cx.n, call };
        run_strip(&self.cx, task, t, sc, &mut routes);
        // Left in a plug-in: nothing here is this thread's any more.
        if call.left() {
            return;
        }
        t.spent = at.elapsed().as_secs_f32();
        t.load.played(thread, t.spent, (self.cx.n as f64 / self.cx.sr) as f32);
        busy.store(0, Ordering::Relaxed);
    }
}

/// The threads and what they use, built off the audio thread and sent in a `Cmd::Workers`.
///
/// A thread left in a plug-in (`Pool::abandon`) keeps hold of its strip, its scratch, its `busy`,
/// the job and the strip pointers for as long as it's in there, which may be for ever: none of
/// them is used for anything else or freed until it's gone, and if it never is, they're leaked.
pub struct Workers {
    pool: Pool,
    /// One per thread.
    scratch: Vec<Scratch>,
    busy: Vec<AtomicU64>,
    /// Where the open block's job stays, whatever happens to the engine's own fields: `jobs[job_at]`,
    /// in a buffer that never grows. One per thread, since each time a thread is left in a plug-in,
    /// still in the job, the next block takes the next.
    jobs: Vec<UnsafeCell<Option<StripJob>>>,
    job_at: usize,
    /// The strips of the open block.
    tracks: Vec<*mut TrackDsp>,
    /// Ready to take the place of a strip left in a plug-in: one per worker.
    #[allow(clippy::vec_box)]
    spares: Vec<Box<TrackDsp>>,
    /// The strips threads were left holding, from `Box::into_raw`, each with its thread and its
    /// token in `EngineStatus::held`.
    held: Vec<(*mut TrackDsp, usize, u64)>,
    /// Threads left in plug-ins in the open block and not yet seen to by `Parallel::leave_out`:
    /// the thread, its task and the plug-in's uid.
    fresh: Vec<(usize, usize, Id)>,
    /// Workers these replaced while their threads still held strips (`Parallel::set_workers`).
    older: Option<Box<Workers>>,
    /// Where the engine builds a graph of its own when the session's doesn't hold (`Parallel::own`).
    room: GraphRoom,
    /// A thread was left in a plug-in in a block on that graph, so it never changes again.
    room_held: bool,
}

// SAFETY: the raw pointers only point into what the engine owns, and are only used while a block is
// open, under the rules in `StripJob::run`; or they're the strips in `held`, which nothing uses.
unsafe impl Send for Workers {}

impl Workers {
    pub fn new(opts: PoolOptions) -> Box<Workers> {
        let pool = Pool::new(opts);
        let threads = pool.threads();
        let scratch = (0..threads).map(|_| Scratch::default()).collect();
        let busy = (0..threads).map(|_| AtomicU64::new(0)).collect();
        Box::new(Workers {
            pool,
            scratch,
            busy,
            jobs: (0..threads).map(|_| UnsafeCell::new(None)).collect(),
            job_at: 0,
            tracks: vec![std::ptr::null_mut(); MAX_TASKS],
            spares: (1..threads).map(|_| stand_in()).collect(),
            held: Vec::with_capacity(threads),
            fresh: Vec::with_capacity(threads),
            older: None,
            room: GraphRoom::default(),
            room_held: false,
        })
    }

    pub fn threads(&self) -> usize {
        self.pool.threads()
    }

    /// Threads that play now: the caller and the workers fit to (see [`Pool::playing`]).
    pub fn playing(&self) -> usize {
        self.pool.playing()
    }

    pub fn realtime_workers(&self) -> u32 {
        self.pool.realtime_workers()
    }

    /// How the workers are doing, to be read while the engine has them.
    pub fn health(&self) -> Arc<Health> {
        self.pool.health()
    }

    /// A strip a thread was left in, once the thread has come back out and let go of it.
    fn release(&mut self, status: &EngineStatus) -> Option<Box<TrackDsp>> {
        let k = self.held.iter().position(|&(_, thread, _)| self.pool.gone(thread))?;
        let (strip, _, token) = self.held.swap_remove(k);
        status.release(token);
        // SAFETY: from `Box::into_raw` in `leave_out`, untouched since, and its thread is done with it.
        Some(unsafe { Box::from_raw(strip) })
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        for (strip, thread, _) in self.held.drain(..) {
            if self.pool.gone(thread) {
                // SAFETY: from `Box::into_raw` in `leave_out`, and its thread has let go of it.
                drop(unsafe { Box::from_raw(strip) });
            }
        }
        // A thread still in its plug-in may yet come back to these, whether or not `leave_out` has
        // taken its strip yet. The pool lets it go.
        if (1..self.pool.threads()).any(|t| self.pool.is_left(t) && !self.pool.gone(t)) {
            std::mem::forget(std::mem::take(&mut self.scratch));
            std::mem::forget(std::mem::take(&mut self.busy));
            std::mem::forget(std::mem::take(&mut self.jobs));
            std::mem::forget(std::mem::take(&mut self.tracks));
            std::mem::forget(std::mem::take(&mut self.room));
        }
    }
}

/// What takes the place of a strip left in a plug-in: it plays nothing and goes nowhere. Commands
/// meant for the strip reach it instead, and it sits out every block (`TrackDsp::left_out`).
fn stand_in() -> Box<TrackDsp> {
    let p = TrackParams {
        volume: 0.0,
        pan: 0.0,
        mute: true,
        solo: false,
        armed: false,
        monitor: false,
        input: InputSel::None,
        output: None,
        sends: [None; SEND_SLOTS],
        stack_gain: 0.0,
        stack_mute: true,
        folder: None,
        own_gain: 0.0,
        own_mute: true,
        bus_mute: false,
        vca_gain: 0.0,
        vca_mute: false,
        off: false,
        instrument_output: None,
        midi: Default::default(),
    };
    let mut t = Box::new(TrackDsp::new(0, TrackKind::Audio, p, Arc::new(Meter::default())));
    t.left_out = true;
    t.stand_in = true;
    t
}

/// Checks route the blocks through the pool whatever the load, and renders too, which otherwise
/// always play serially: their blocks come back to back, and real-time workers that never sleep
/// would be killed by the kernel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParallelTest {
    pub always: bool,
    pub renders: bool,
    /// Never give a block up, and never shed load for a callback that runs late, on a machine too
    /// loaded to promise anything about time: a check's output must not depend on it. Holds
    /// whatever the thread count.
    pub patient: bool,
}

#[derive(Default)]
pub(super) struct Parallel {
    workers: Option<Box<Workers>>,
    graph: Option<Box<TaskGraph>>,
    /// The graph holds for the engine's tracks as they are now (see [`covers`]).
    covered: bool,
    /// The routes or the graph changed, so that needs looking at again.
    stale: bool,
    /// A block was given up on and is still open: until it's over nothing may touch the strips.
    pub stalled: bool,
    /// A thread was left in a plug-in holding the graph, which is never freed now.
    graph_held: bool,
    /// Workers replaced while threads of theirs still held strips, newest first (linked through
    /// `Workers::older`): kept until each strip is back (`take_released`).
    replaced: Option<Box<Workers>>,
    /// The time limit the open block was given up at.
    give_up: Duration,
    /// The load is high enough to be worth the workers.
    engaged: bool,
    /// The graph doesn't hold, and the blocks play on one the engine built from its own routes in
    /// the workers' room (`build_own`) until the session's next one does.
    pub(super) own: bool,
    pub test: ParallelTest,
    /// Threads in the last block, for the meter's tooltip.
    last_threads: u32,
}

impl Parallel {
    pub fn routes_changed(&mut self) {
        self.stale = true;
    }

    /// A new graph starts engaged: what its strips cost is unknown until a block has measured it,
    /// and a heavy project's first block played alone would overload (#456).
    pub fn set_graph(&mut self, g: Option<Box<TaskGraph>>) -> Option<Box<TaskGraph>> {
        self.stale = true;
        self.engaged = true;
        let old = std::mem::replace(&mut self.graph, g);
        if std::mem::take(&mut self.graph_held) {
            // Leaked, which allocates and frees nothing.
            std::mem::forget(old);
            return None;
        }
        old
    }

    /// The old workers, to be freed elsewhere. Not while a thread of theirs still holds a strip:
    /// they're kept until it lets go, so that its track can come back (`take_released`).
    pub fn set_workers(&mut self, w: Option<Box<Workers>>) -> Option<Box<Workers>> {
        // A graph of the engine's own is in the old workers' room.
        self.stale = true;
        let mut old = std::mem::replace(&mut self.workers, w)?;
        if old.held.is_empty() {
            return Some(old);
        }
        old.older = self.replaced.take();
        self.replaced = Some(old);
        None
    }

    /// Whether this block goes through the pool.
    pub fn wanted(&mut self, offline: bool, tracks: &[Box<TrackDsp>]) -> bool {
        // With every worker refused real-time priority or given up on, the serial loop does the
        // same work without the pool's bookkeeping.
        if self.workers.as_ref().is_none_or(|w| !w.pool.has_workers()) {
            return false;
        }
        if self.stale {
            self.stale = false;
            self.covered = self.graph.as_deref().is_some_and(|g| covers(g, tracks));
            self.own = !self.covered && self.build_own(tracks);
            self.covered |= self.own;
        }
        let Some(g) = self.graph() else { return false };
        if !self.covered {
            return false;
        }
        if offline {
            return self.test.renders;
        }
        self.test.always || (self.engaged && g.heavy_width() >= 2)
    }

    /// Builds a graph from the routes the engine resolved, in the workers' room: what [`covers`]
    /// asks for and no more, so it holds for `tracks` by construction. False if it can't be (the
    /// room is held, too many strips).
    fn build_own(&mut self, tracks: &[Box<TrackDsp>]) -> bool {
        let Some(w) = self.workers.as_deref_mut().filter(|w| !w.room_held) else { return false };
        let edges = |i: usize, add: &mut dyn FnMut(usize, usize)| {
            let t = &tracks[i];
            for j in t.out_idx.into_iter().chain(t.send_idx.iter().flatten().copied()) {
                add(i, j);
            }
            for s in t.feed.source.into_iter().chain(t.inserts.iter().flatten().filter_map(|ins| ins.key.idx)) {
                add(s, i);
            }
        };
        let heavy = |i: usize| {
            let t = &tracks[i];
            !t.dormant && (t.instrument.is_some() || t.inserts.iter().any(Option::is_some))
        };
        let built = w.room.refill(tracks.len(), |i| tracks[i].id, edges, heavy);
        debug_assert!(!built || covers(w.room.graph(), tracks));
        built
    }

    /// The graph blocks on the pool play: the session's, or one of the engine's own.
    fn graph(&self) -> Option<&TaskGraph> {
        if self.own {
            self.workers.as_deref().map(|w| w.room.graph())
        } else {
            self.graph.as_deref()
        }
    }

    /// Whether the session's graph holds for `tracks`.
    pub fn fits(&self, tracks: &[Box<TrackDsp>]) -> bool {
        self.graph.as_ref().is_some_and(|g| covers(g, tracks))
    }

    /// Takes in a callback's load: `now` is its own, `average` the smoothed one, and `standby` what
    /// could come back in one block (plug-ins asleep, sources rendered ahead; see
    /// `Engine::standby_load`). It goes back to playing alone once the load, with the workers
    /// helping, says that even alone it would stay under `ENGAGE_LOAD` (`threads` threads can't
    /// have done more than that many times the work), and so would the work on standby were it to
    /// come back at once.
    pub fn weigh(&mut self, now: f32, average: f32, standby: f32) {
        if self.engaged {
            let threads = self.workers.as_ref().map_or(1, |w| w.playing()) as f32;
            self.engaged = average * threads + standby >= ENGAGE_LOAD;
        } else {
            self.engaged = now + standby >= ENGAGE_LOAD;
        }
    }

    /// Threads the last callback used, when it changed: the callback alone unless `pooled`.
    pub fn threads_now(&mut self, pooled: bool) -> Option<u32> {
        let n = if pooled { self.workers.as_ref().map_or(1, |w| w.playing() as u32) } else { 1 };
        (n != self.last_threads).then(|| {
            self.last_threads = n;
            n
        })
    }

    /// A track a thread is in right now, after a block was given up on, but for one a thread was
    /// left in (that's for `leave_out` to report).
    pub fn stuck_track(&self) -> Option<Id> {
        let w = self.workers.as_ref()?;
        w.busy.iter().enumerate().filter(|(th, _)| !w.pool.is_left(*th)).map(|(_, b)| b.load(Ordering::Relaxed)).find(|&id| id != 0)
    }

    /// Once a block in which threads were left in plug-ins is over: each strip they hold gives way to
    /// a stand-in, which, with every strip the graph has waiting for it, sits out every block until
    /// the plug-in returns (`Cmd::Back`) or the strips are rebuilt (`TrackDsp::left_out`). The
    /// strips held stay untouched until their threads are gone (`take_released`). `status` lists
    /// them (`EngineStatus::held`) and hears of it (`left_events`). True if any strip was left out.
    pub fn leave_out(&mut self, tracks: &mut [Box<TrackDsp>], status: &EngineStatus) -> bool {
        let Some(w) = self.workers.as_deref_mut() else { return false };
        if w.fresh.is_empty() {
            return false;
        }
        let graph = if self.own { w.room.graph() } else { self.graph.as_deref().expect("the block ran on a graph") };
        let (_, first, uid) = w.fresh[0];
        for (thread, task, uid) in w.fresh.drain(..) {
            // Every worker left has a spare, made for it.
            let mut stand_in = w.spares.pop().expect("a stand-in per worker");
            // The strip's own fields are its thread's: the graph has its id.
            let id = graph.id(task);
            stand_in.id = id;
            let token = status.hold(id, uid);
            stand_in.held_token = token;
            let held = std::mem::replace(&mut tracks[task], stand_in);
            w.held.push((Box::into_raw(held), thread, token));
        }
        for (i, t) in tracks.iter_mut().enumerate() {
            if w.pool.dropped(i) {
                t.sit_out();
            }
        }
        status.left_track.store(graph.id(first), Ordering::Relaxed);
        status.left_plugin.store(uid, Ordering::Relaxed);
        status.left_events.fetch_add(1, Ordering::Release);
        true
    }

    /// A strip a thread was left in, once the thread has come back out and let go of it, to be
    /// freed elsewhere. `status` no longer lists it as held, and the session brings the track back.
    /// Or workers replaced since that now hold no strip, to be freed too.
    pub fn take_released(&mut self, status: &EngineStatus) -> Option<Garbage> {
        if let Some(t) = self.workers.as_deref_mut().and_then(|w| w.release(status)) {
            return Some(Garbage::Track(t));
        }
        let mut link = &mut self.replaced;
        loop {
            let w = link.as_deref_mut()?;
            if let Some(t) = w.release(status) {
                return Some(Garbage::Track(t));
            }
            if w.held.is_empty() {
                let mut done = link.take().expect("just looked");
                *link = done.older.take();
                return Some(Garbage::Workers(done));
            }
            link = &mut link.as_mut().expect("just looked").older;
        }
    }

    /// While a block given up on stays open: leaves a worker that has since been out in a plug-in
    /// long enough (one that was held up in its own task at the give-up, say), and counts out what's
    /// ready of the block without running it (`Pool::pass_over`), never waiting. True once the
    /// block is over. Calls no plug-in, from the callback or from `Engine::settle`.
    pub fn press_on(&mut self) -> bool {
        let Some(w) = self.workers.as_deref_mut() else { return true };
        let Some(graph) = (if self.own { Some(w.room.graph()) } else { self.graph.as_deref() }) else { return true };
        let fresh = &mut w.fresh;
        let left = w.pool.abandon(graph, self.give_up * 3 / 4, |thread, task, uid| {
            if fresh.len() < fresh.capacity() {
                fresh.push((thread, task, uid));
            }
        });
        if left > 0 {
            w.job_at += 1;
            debug_assert!(w.job_at < w.jobs.len(), "a job per time a worker is left");
            if self.own {
                w.room_held = true;
            } else {
                self.graph_held = true;
            }
        }
        // The block is silent whatever happens, and a strip that misses it is like one in a block
        // shed for an overload. The workers still playing run what they take, and what the job
        // reads stays put while it's open, as `run` says, since no command is applied meanwhile.
        w.pool.pass_over(Duration::ZERO)
    }

    /// Whether a block given up on is over.
    pub fn open_block_over(&self) -> bool {
        self.workers.as_ref().is_none_or(|w| w.pool.is_done())
    }

    /// Runs a block's strips on the pool, this thread working too.
    ///
    /// `wanted` said yes, so there are workers and a graph that matches `tracks`. If this returns
    /// `Stalled` the block stays open with threads still using `tracks` and what `cx` points at:
    /// until `open_block_over`, the caller must not touch any of it, and sets `stalled` to say so.
    pub fn run(&mut self, tracks: &mut [Box<TrackDsp>], cx: &BlockCtx<'_>) -> Outcome {
        let w = self.workers.as_deref_mut().expect("wanted() saw workers");
        let graph = if self.own { w.room.graph() } else { self.graph.as_deref().expect("wanted() saw a graph") };
        // Folders' automation is read now, while nothing is writing: a subtrack doesn't wait for
        // its folder, which runs (doing next to nothing) at the same time.
        for ti in 0..tracks.len() {
            if let Some(j) = tracks[ti].fold_idx.filter(|&j| j > ti) {
                let at = cx.pos - tracks[ti].pdc.output_time as i64;
                tracks[ti].fold_now = auto::fold_at(&tracks[ti + 1..], ti + 1, j, at, cx.playing);
            }
        }
        for (slot, t) in w.tracks.iter_mut().zip(tracks.iter_mut()) {
            *slot = &mut **t;
        }
        // SAFETY: only the lifetime changes. What `cx` borrows (the input buffers and live MIDI in
        // their own heap blocks, the tempo map and status behind their `Arc`s, the isolated render's
        // boxed settings) stays where it is and unchanged while the block is open, because the
        // caller applies no commands until `open_block_over`, and the job doesn't outlive that.
        let cx: BlockCtx<'static> = unsafe { std::mem::transmute(*cx) };
        let job = StripJob { cx, tracks: w.tracks.as_ptr(), scratch: w.scratch.as_mut_ptr(), graph, busy: w.busy.as_ptr() };
        // SAFETY: the last block is over (the caller checks `stalled`), so nobody reads the cell (a
        // thread left in a plug-in holds an earlier one); and the job, the graph and the strips stay
        // put until the block is, as above.
        unsafe {
            let cell = w.jobs[w.job_at].get();
            *cell = Some(job);
            let ptr: *const StripJob = (*cell).as_ref().expect("just stored");
            w.pool.start(graph, ptr);
        }
        let limit = (!cx.wait).then(|| Duration::from_secs_f64(GIVE_UP_SECS.max(4.0 * cx.n as f64 / cx.sr)));
        if !w.pool.finish(limit) {
            // A worker out in a plug-in that hasn't returned for most of the limit is left there,
            // demoted so that the kernel can't kill the app for it. The block ends without its strip
            // and what that feeds, which sit out later blocks too (`leave_out`).
            let limit = limit.expect("only a limit gives up");
            self.give_up = limit;
            let fresh = &mut w.fresh;
            let left = w.pool.abandon(graph, limit * 3 / 4, |thread, task, uid| {
                if fresh.len() < fresh.capacity() {
                    fresh.push((thread, task, uid));
                }
            });
            if left > 0 {
                // It's still in this job and may be in the graph, which must stay.
                w.job_at += 1;
                debug_assert!(w.job_at < w.jobs.len(), "a job per worker left");
                if self.own {
                    w.room_held = true;
                } else {
                    self.graph_held = true;
                }
                // The block is silent: the rest is only counted out (`press_on` says why), and a worker
                // held up anywhere else must still be demoted before the kernel's limit
                // (`GIVE_UP_SECS`).
                if w.pool.pass_over(limit / 4) {
                    return Outcome::LeftOut;
                }
            }
            // Held up outside a plug-in, or not in one for long: the worker is demoted and plays no
            // more, and the block stays open until it's out, or is left in a plug-in after all
            // (`press_on`).
            w.pool.retire_stuck();
            return Outcome::Stalled;
        }
        match w.pool.take_panics() {
            0 => Outcome::Done,
            k => Outcome::Panicked(k),
        }
    }
}

impl Drop for Parallel {
    fn drop(&mut self) {
        if self.graph_held {
            std::mem::forget(self.graph.take());
        }
    }
}

/// For tests: who holds what in a block on the pool, by synchronisation rather than timing. A worker
/// can be held up in a strip's own task, outside any plug-in call (as one kept off the CPU would be,
/// or a slow part of the strip's own work); the caller can be made to wait in a strip until workers
/// have started others, so that those are the workers'; and a worker can be made to wait in a strip
/// until the caller is in such a gate, so that it can't take the gate first. Keyed by track id, so
/// each test uses ids of its own.
#[cfg(test)]
pub(crate) mod test_hooks {
    use super::Id;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static IN_TASK: Mutex<Vec<Id>> = Mutex::new(Vec::new());
    /// Strips the caller waits in, each until workers have started the strips listed with it.
    static GATES: Mutex<Vec<(Id, Vec<Id>)>> = Mutex::new(Vec::new());
    static STARTED: Mutex<Vec<Id>> = Mutex::new(Vec::new());
    /// Strips a worker waits in until the caller is in the gate listed with each, and the gates the
    /// caller is in or has been in.
    static AFTER: Mutex<Vec<(Id, Id)>> = Mutex::new(Vec::new());
    static ENTERED: Mutex<Vec<Id>> = Mutex::new(Vec::new());
    /// Longer than any test should ever wait.
    const PATIENCE: Duration = Duration::from_secs(60);

    fn on_worker() -> bool {
        // The pool names its threads.
        std::thread::current().name().is_some_and(|n| n.starts_with("marbble-dsp"))
    }

    fn wait(what: impl Fn() -> bool) {
        let t = Instant::now();
        while !what() && t.elapsed() < PATIENCE {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// As a strip's task starts, on whichever thread.
    pub(crate) fn task_starts(id: Id) {
        if on_worker() {
            let gate = AFTER.lock().unwrap().iter().find(|(x, _)| *x == id).map(|(_, g)| *g);
            if let Some(g) = gate {
                wait(|| ENTERED.lock().unwrap().contains(&g));
            }
            if GATES.lock().unwrap().iter().any(|(_, ids)| ids.contains(&id)) {
                STARTED.lock().unwrap().push(id);
            }
            wait(|| !IN_TASK.lock().unwrap().contains(&id));
            return;
        }
        let Some(ids) = GATES.lock().unwrap().iter().find(|(g, _)| *g == id).map(|(_, ids)| ids.clone()) else { return };
        ENTERED.lock().unwrap().push(id);
        wait(|| ids.iter().all(|i| STARTED.lock().unwrap().contains(i)));
    }

    /// Holds a worker up in the strip `id`'s task, outside any plug-in call, while `on`.
    pub(crate) fn hold_in_task(id: Id, on: bool) {
        let mut l = IN_TASK.lock().unwrap();
        l.retain(|&x| x != id);
        if on {
            l.push(id);
        }
    }

    /// Makes the caller wait as it starts the strip `gate` until workers have started each of
    /// `ids`: as it takes nothing else meanwhile, they're the workers'. First in the order of the
    /// strips, `gate` is the caller's first task (each thread takes the first ready one of its
    /// own share first); `after` keeps the workers from taking it before that. None for `ids`
    /// empty.
    pub(crate) fn gate(gate: Id, ids: &[Id]) {
        let mut g = GATES.lock().unwrap();
        let mut started = STARTED.lock().unwrap();
        for (_, old) in g.iter().filter(|(x, _)| *x == gate) {
            started.retain(|x| !old.contains(x));
        }
        g.retain(|(x, _)| *x != gate);
        started.retain(|x| !ids.contains(x));
        ENTERED.lock().unwrap().retain(|&x| x != gate);
        if !ids.is_empty() {
            g.push((gate, ids.to_vec()));
        }
    }

    /// Makes a worker wait as it starts the strip `id` until the caller is in `gate` (None: never).
    /// With one such strip first in each worker's share of the order, every worker takes its own
    /// and waits there, so none can take `gate` before the caller does.
    pub(crate) fn after(id: Id, gate: Option<Id>) {
        let mut a = AFTER.lock().unwrap();
        a.retain(|(x, _)| *x != id);
        if let Some(g) = gate {
            a.push((id, g));
        }
    }
}

/// For checks: a built-in effect that holds up its strip while `hold` is set, as a plug-in that
/// stops responding does, on a worker only: the callback is never given up on, so there it would
/// only hang the check.
#[doc(hidden)]
pub fn tar_for_checks(uid: Id, sample_rate: f32, hold: Arc<AtomicBool>) -> InsertDsp {
    struct Tar(Arc<AtomicBool>);
    impl crate::fx::Effect for Tar {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, _: &mut [f32], _: &mut [f32], _: crate::fx::Key, _: &crate::fx::FxContext) {
            // The pool names its threads.
            if std::thread::current().name().is_some_and(|n| n.starts_with("marbble-dsp")) {
                while self.0.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        fn reset(&mut self) {}
    }
    let info = crate::fx::EffectInfo {
        id: "test:tar",
        name: "Tar",
        category: "Utility",
        params: &[],
        meters: &[],
        sidechain: false,
        new: |_| Box::new(Tar(Arc::new(AtomicBool::new(false)))),
    };
    let mut b = BuiltinDsp::new(&info, sample_rate, &[]);
    b.replace_effect_for_tests(Box::new(Tar(hold)));
    InsertDsp::new(uid, false, Plugin::Builtin(Box::new(b)), KeyRoute::default())
}

/// Whether `g` allows for every way the engine's strips read each other, as `resolve_routes` left
/// them. A strip the graph doesn't make wait for its source would read it while it's being written.
pub(super) fn covers(g: &TaskGraph, tracks: &[Box<TrackDsp>]) -> bool {
    if g.len() != tracks.len() {
        return false;
    }
    tracks.iter().enumerate().all(|(i, t)| {
        g.id(i) == t.id
            && t.out_idx.is_none_or(|j| g.has_dep(j, i))
            && t.send_idx.iter().flatten().all(|&j| g.has_dep(j, i))
            && t.feed.source.is_none_or(|s| g.has_dep(i, s))
            && t.inserts.iter().flatten().filter_map(|ins| ins.key.idx).all(|s| g.has_dep(i, s))
    })
}

/// A strip's routes on the pool: it writes nothing of anyone else's.
struct Deferred<'a> {
    tracks: *const *mut TrackDsp,
    graph: &'a TaskGraph,
    ti: usize,
    n: usize,
    call: &'a Call,
}

impl Deferred<'_> {
    /// A strip this one waits for, which is finished and won't change while the block is open.
    fn dep(&self, idx: usize) -> &TrackDsp {
        // SAFETY: only called for this task's dependencies, whose tasks finished before this one
        // started (the pool's Acquire on `pending` and the bits sees all they wrote). Nobody holds a
        // `&mut` to them any more, and any number of receivers may read them at once.
        unsafe { &**self.tracks.add(idx) }
    }
}

impl Routes for Deferred<'_> {
    fn earlier(&self, idx: usize) -> Option<&TrackDsp> {
        // `covers` checked this, but reading a strip that may be running would be undefined
        // behaviour where a missed key is merely silence, so it's checked every time.
        self.graph.has_dep(self.ti, idx).then(|| self.dep(idx))
    }

    fn id_of(&self, j: usize) -> Id {
        self.graph.id(j)
    }

    fn fold(&self, read_ahead: auto::FoldState, _j: usize, _at: i64, _playing: bool) -> auto::FoldState {
        read_ahead
    }

    fn inputs(&mut self, _sum: (&mut [f32], &mut [f32]), l: &mut [f32], r: &mut [f32]) {
        let add = |dst: &mut [f32], src: &[f32]| {
            for (d, s) in dst.iter_mut().zip(src) {
                *d += *s;
            }
        };
        // In the order the serial loop's strips write: by processing order, and within a strip the
        // pre-fader sends, then the output, then the post-fader sends.
        for &src in self.graph.deps(self.ti) {
            let s = self.dep(src as usize);
            let sends = |pre: bool, l: &mut [f32], r: &mut [f32]| {
                for k in 0..SEND_SLOTS {
                    if s.outs.sends[k] == Some(SendOut { slot: k, to: self.ti, pre }) {
                        let (a, b) = s.outs.send_buf(k, self.n);
                        add(l, a);
                        add(r, b);
                    }
                }
            };
            sends(true, l, r);
            if s.outs.dest == Dest::Track(self.ti) {
                add(l, &s.l[..self.n]);
                add(r, &s.r[..self.n]);
            }
            sends(false, l, r);
        }
    }

    fn send(&mut self, outs: &mut Outlets, send: SendOut, from: (&[f32], &[f32]), gain: &mut f32, target: f32, smooth: f32) {
        let (a, b) = outs.send_buf_mut(send.slot, from.0.len());
        let live = render_send((a, b), from, gain, target, smooth);
        outs.sends[send.slot] = live.then_some(send);
    }

    fn output(&mut self, outs: &mut Outlets, dest: Dest, _l: &[f32], _r: &[f32]) {
        // It stays in the strip's own buffers.
        outs.dest = dest;
    }

    fn enter(&self, uid: Id) {
        self.call.enter(uid);
    }

    fn leave(&self) -> bool {
        self.call.leave()
    }

    fn analyze(&mut self, outs: &mut Outlets, signal: Option<(&[f32], &[f32])>, n: usize) {
        outs.analyzed = true;
        match signal {
            Some((l, r)) => {
                for i in 0..n {
                    outs.analysis[i] = 0.5 * (l[i] + r[i]);
                }
            }
            None => outs.analysis[..n].fill(0.0),
        }
    }
}

#[cfg(test)]
mod tests;
