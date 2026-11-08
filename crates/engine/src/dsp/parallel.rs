//! Strips playing at the same time: what makes `Engine::process_block`'s strip loop safe to run on
//! several threads, with output bit-identical to the serial loop. See "Multi-core processing" in
//! docs/ARCHITECTURE.md for the design and the list of what a block touches.
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

use std::cell::UnsafeCell;
use std::time::Duration;

use super::graph::{TaskGraph, MAX_TASKS};
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
    pub test: ParallelTest,
    /// Threads in the last block, for the meter's tooltip.
    last_threads: u32,
}

impl Parallel {
    pub fn routes_changed(&mut self) {
        self.stale = true;
    }

    pub fn set_graph(&mut self, g: Option<Box<TaskGraph>>) -> Option<Box<TaskGraph>> {
        self.stale = true;
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
        let Some(g) = &self.graph else { return false };
        if self.stale {
            self.stale = false;
            self.covered = covers(g, tracks);
        }
        if !self.covered {
            return false;
        }
        if offline {
            return self.test.renders;
        }
        self.test.always || (self.engaged && g.heavy_width() >= 2)
    }

    pub fn fits(&self, tracks: &[Box<TrackDsp>]) -> bool {
        self.graph.as_ref().is_some_and(|g| covers(g, tracks))
    }

    /// Takes in a callback's load: `now` is its own, `average` the smoothed one. It goes back to
    /// playing alone once the load, with the workers helping, says that even alone it would stay
    /// under `ENGAGE_LOAD`: `threads` threads can't have done more than that many times the work.
    pub fn weigh(&mut self, now: f32, average: f32) {
        if self.engaged {
            let threads = self.workers.as_ref().map_or(1, |w| w.playing()) as f32;
            self.engaged = average * threads >= ENGAGE_LOAD;
        } else {
            self.engaged = now >= ENGAGE_LOAD;
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
        let graph = self.graph.as_deref().expect("the block ran on a graph");
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
        let (Some(w), Some(graph)) = (self.workers.as_deref_mut(), self.graph.as_deref()) else { return true };
        let fresh = &mut w.fresh;
        let left = w.pool.abandon(graph, self.give_up * 3 / 4, |thread, task, uid| {
            if fresh.len() < fresh.capacity() {
                fresh.push((thread, task, uid));
            }
        });
        if left > 0 {
            w.job_at += 1;
            debug_assert!(w.job_at < w.jobs.len(), "a job per time a worker is left");
            self.graph_held = true;
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
        let graph = self.graph.as_deref().expect("wanted() saw a graph");
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
                self.graph_held = true;
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
    InsertDsp { uid, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::default() }
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
mod tests {
    use super::*;
    use crate::fx::{Effect, EffectInfo, FxContext, Key};

    const SR: f64 = 48000.0;
    const BLOCK: usize = 256;

    struct Rig {
        engine: Engine,
        tx: rtrb::Producer<Cmd>,
        status: Arc<EngineStatus>,
        // Holds the rings' far ends open for the rig's life; nothing reads this tuple.
        #[allow(clippy::type_complexity)]
        _keep: (rtrb::Consumer<Garbage>, rtrb::Producer<[u8; 3]>, rtrb::Consumer<(i64, [u8; 3])>, rtrb::Consumer<f32>),
    }

    fn rig() -> Rig {
        let (tx, rx) = rtrb::RingBuffer::new(256);
        let (gtx, grx) = rtrb::RingBuffer::new(256);
        let (live_tx, live_rx) = rtrb::RingBuffer::new(16);
        let (rec_tx, rec_rx) = rtrb::RingBuffer::new(16);
        let (an_tx, an_rx) = rtrb::RingBuffer::new(1 << 16);
        let status = Arc::new(EngineStatus::default());
        let engine = Engine::new(SR, rx, gtx, status.clone(), live_rx, rec_tx, an_tx);
        Rig { engine, tx, status, _keep: (grx, live_tx, rec_rx, an_rx) }
    }

    fn params(output: Option<Id>, sends: &[(Id, f32, bool)]) -> TrackParams {
        let mut s = [None; SEND_SLOTS];
        for (k, &(bus, gain, pre_fader)) in sends.iter().enumerate() {
            s[k] = Some(SendParams { bus, gain, pre_fader });
        }
        TrackParams {
            volume: 0.7,
            pan: 0.25,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input: InputSel::None,
            output,
            sends: s,
            stack_gain: 1.0,
            stack_mute: false,
            folder: None,
            own_gain: 0.7,
            own_mute: false,
            bus_mute: false,
            vca_gain: 1.0,
            vca_mute: false,
            off: false,
            instrument_output: None,
        }
    }

    /// An effect that makes its own signal (a sine at a pitch of its own) on top of what comes in,
    /// adds the key into it, and takes a little time, so that strips overlap.
    struct Tone {
        phase: f32,
        hz: f32,
        /// Where `reset` puts the phase back.
        start: f32,
    }

    impl Effect for Tone {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _: &FxContext) {
            for i in 0..l.len() {
                self.phase = (self.phase + self.hz / SR as f32) % 1.0;
                let s = (self.phase * std::f32::consts::TAU).sin() * 0.1;
                let k = key.map_or(0.0, |(a, b)| 0.25 * (a[i] + b[i]));
                l[i] += s + k;
                r[i] += s * 0.5 - k;
            }
        }
        fn reset(&mut self) {
            self.phase = self.start;
        }
    }

    fn tone(uid: Id, hz: f32, key: Option<Id>, pre: bool) -> InsertDsp {
        // `EffectInfo::new` is a plain function, so the pitch comes through the uid's slot below.
        let info = EffectInfo {
            id: "test:tone",
            name: "Tone",
            category: "Utility",
            params: &[],
            meters: &[],
            sidechain: true,
            new: |_| Box::new(Tone { phase: 0.0, hz: 0.0, start: 0.0 }),
        };
        let mut b = BuiltinDsp::new(&info, SR as f32, &[]);
        b.replace_effect_for_tests(Box::new(Tone { phase: uid as f32 * 0.01, hz, start: uid as f32 * 0.01 }));
        InsertDsp { uid, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::new(key, pre) }
    }

    fn strip(id: Id, kind: TrackKind, p: TrackParams, fx: Vec<InsertDsp>) -> Box<TrackDsp> {
        let mut t = Box::new(TrackDsp::new(id, kind, p, Arc::new(Meter::default())));
        for (k, f) in fx.into_iter().enumerate() {
            t.set_insert(k, Some(f));
        }
        t
    }

    /// Sends, an output into an aux, a summing stack, keys taken before and after the fader, a
    /// send with a gain changing under it, and several feeders per bus: order matters everywhere.
    fn project(e: &mut Rig) -> TaskGraph {
        let keyed = strip(4, TrackKind::Audio, params(None, &[(6, 0.3, false)]), vec![tone(40, 310.0, Some(1), false), tone(41, 390.0, Some(2), true)]);
        let tracks = vec![
            strip(1, TrackKind::Audio, params(Some(5), &[(6, 0.5, true), (7, 0.25, false), (6, 0.125, false)]), vec![tone(10, 110.0, None, false)]),
            strip(2, TrackKind::Audio, params(Some(5), &[(6, 0.4, false)]), vec![tone(20, 170.0, None, false)]),
            strip(3, TrackKind::Audio, params(None, &[(7, 0.6, true)]), vec![tone(30, 230.0, None, false)]),
            keyed,
            strip(5, TrackKind::Summing, params(None, &[(7, 0.2, false)]), vec![tone(50, 470.0, None, false)]),
            strip(6, TrackKind::Aux, params(None, &[]), vec![tone(60, 530.0, None, false)]),
            strip(7, TrackKind::Aux, params(None, &[]), vec![tone(70, 610.0, None, false)]),
        ];
        let ids = tracks.iter().map(|t| t.id).collect();
        e.engine.replace_graph(SR, tracks, (0..INSERT_SLOTS).map(|_| None).collect());
        let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)];
        TaskGraph::new(ids, &edges, |_| true).unwrap()
    }

    fn play(e: &mut Rig, blocks: usize) -> Vec<f32> {
        e.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
        let mut all = Vec::new();
        let mut out = vec![0.0f32; BLOCK * 2];
        for _ in 0..blocks {
            e.engine.process(&[], 0, &mut out, 2);
            all.extend_from_slice(&out);
        }
        all
    }

    fn bits(v: &[f32]) -> Vec<u32> {
        v.iter().map(|s| s.to_bits()).collect()
    }

    fn pooled(threads: usize, blocks: usize) -> (Vec<f32>, u64) {
        let mut e = rig();
        let g = project(&mut e);
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        let out = play(&mut e, blocks);
        (out, e.status.parallel_blocks.load(Ordering::Relaxed))
    }

    #[test]
    fn the_pool_plays_what_the_serial_loop_plays_bit_for_bit() {
        let mut e = rig();
        project(&mut e);
        let serial = play(&mut e, 200);
        assert!(serial.iter().any(|s| s.abs() > 0.01));
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 0);
        for threads in [1, 2, 3, 4, 8] {
            let (out, blocks) = pooled(threads, 200);
            assert_eq!(blocks, 200, "{threads} threads: every block went through the pool");
            assert!(bits(&out) == bits(&serial), "{threads} threads differ from the serial loop");
        }
    }

    #[test]
    fn a_render_after_a_new_send_does_not_depend_on_how_long_it_played_live_first() {
        let renders: Vec<Vec<Vec<f32>>> = [1, 40]
            .into_iter()
            .map(|blocks| {
                let mut e = rig();
                project(&mut e);
                // Track 3 starts sending to aux 6, after the fader and before it.
                e.tx.push(Cmd::Params(3, params(None, &[(7, 0.6, true), (6, 0.5, false), (6, 0.3, true)]))).ok().unwrap();
                let mut out = vec![0.0f32; BLOCK * 2];
                for _ in 0..blocks {
                    e.engine.process(&[], 0, &mut out, 2);
                }
                e.engine.render(0, 4 * BLOCK as i64, 0, None, &mut |_| {})
            })
            .collect();
        assert!(renders[0][0].iter().any(|s| s.abs() > 0.01));
        assert!(renders[0].iter().zip(&renders[1]).all(|(a, b)| bits(a) == bits(b)), "the sends start at their levels");
    }

    #[test]
    fn without_a_graph_that_fits_it_plays_serially() {
        let mut e = rig();
        project(&mut e);
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 0, "no graph");
        // One missing route of each kind.
        for missing in [(1, 5), (1, 7), (2, 6), (1, 4), (2, 4)] {
            let mut r = rig();
            let full = project(&mut r);
            let edges: Vec<(Id, Id)> =
                [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)].into_iter().filter(|x| *x != missing).collect();
            let ids = (0..full.len()).map(|i| full.id(i)).collect();
            r.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(ids, &edges, |_| true).unwrap())))).ok().unwrap();
            r.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
            r.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
            play(&mut r, 2);
            assert_eq!(r.status.parallel_blocks.load(Ordering::Relaxed), 0, "without the route {missing:?}");
        }
    }

    #[test]
    fn a_route_the_graph_lacks_goes_serial_at_once_and_a_new_graph_brings_it_back() {
        let mut e = rig();
        let g = project(&mut e);
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 2);
        // Track 3 starts sending to aux 6.
        e.tx.push(Cmd::Params(3, params(None, &[(7, 0.6, true), (6, 0.5, false)]))).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 2, "those two blocks played serially");
        let ids = vec![1, 2, 3, 4, 5, 6, 7];
        let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4), (3, 6)];
        e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(ids, &edges, |_| true).unwrap())))).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn a_light_project_stays_on_the_callback_until_the_load_climbs() {
        let mut p = Parallel::default();
        p.weigh(0.05, 0.05);
        assert!(!p.engaged);
        p.weigh(0.2, 0.06);
        assert!(p.engaged, "one heavy block is enough");
        p.weigh(0.01, 0.2);
        assert!(p.engaged, "the average is still up");
        p.weigh(0.01, 0.1);
        assert!(!p.engaged, "no workers: under the mark means under it");
    }

    /// A panic in a strip, once it's told to, on whichever thread has it.
    struct Bomb(Arc<std::sync::atomic::AtomicBool>);

    impl Effect for Bomb {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, l: &mut [f32], _: &mut [f32], _: Key, _: &FxContext) {
            if self.0.swap(false, Ordering::SeqCst) {
                panic!("expected: a plug-in blowing up in an engine test");
            }
            l.fill(0.5);
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn a_panicking_strip_silences_its_block_and_only_that() {
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut e = rig();
        let g = project(&mut e);
        let info = EffectInfo {
            id: "test:bomb",
            name: "Bomb",
            category: "Utility",
            params: &[],
            meters: &[],
            sidechain: false,
            new: |_| Box::new(Tone { phase: 0.0, hz: 0.0, start: 0.0 }),
        };
        let mut b = BuiltinDsp::new(&info, SR as f32, &[]);
        b.replace_effect_for_tests(Box::new(Bomb(armed.clone())));
        e.tx.push(Cmd::Insert {
            track: 3,
            slot: 1,
            insert: Some(InsertDsp { uid: 99, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::default() }),
        })
        .ok()
        .unwrap();
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        let mut out = vec![0.0f32; BLOCK * 2];
        for _ in 0..5 {
            e.engine.process(&[], 0, &mut out, 2);
        }
        assert!(out.iter().any(|s| s.abs() > 0.01));
        armed.store(true, Ordering::SeqCst);
        e.engine.process(&[], 0, &mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0), "silence for the block");
        assert_eq!(e.status.engine_errors.load(Ordering::Relaxed), 1, "and it's counted");
        // Reporting a panic can take long enough to count as an overload (with a backtrace on
        // Windows it does), and the engine then stays quiet for `SHED_SECS`.
        let blocks = (2.0 * SHED_SECS * SR / BLOCK as f64) as usize;
        let heard = (0..blocks).any(|_| {
            e.engine.process(&[], 0, &mut out, 2);
            out.iter().any(|s| s.abs() > 0.01)
        });
        assert!(heard, "the pool isn't poisoned");
        assert_eq!(e.status.engine_errors.load(Ordering::Relaxed), 1);
    }

    /// A rig whose track 2 has Tar on worker threads, on a pool of `threads`.
    fn tarred(threads: usize, hold: &Arc<AtomicBool>) -> Rig {
        let mut e = rig();
        let g = project(&mut e);
        e.tx.push(Cmd::Insert { track: 2, slot: 1, insert: Some(tar_for_checks(99, SR as f32, hold.clone())) }).ok().unwrap();
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: false })).ok().unwrap();
        e
    }

    /// Plays until a worker gets track 2's strip and is left in Tar.
    /// Not until any overload: on a busy machine a block can be counted as one for running late, or
    /// be given up on with a worker kept off the CPU, and the block then stays open for a while.
    /// Should those give-ups have retired every worker before one got the strip, the pool is started
    /// afresh.
    fn until_stuck(e: &mut Rig, out: &mut [f32]) {
        let started = Instant::now();
        while e.status.left_events.load(Ordering::Relaxed) == 0 || e.engine.parallel.stalled {
            assert!(started.elapsed().as_secs() < 60, "no worker ever got the strip");
            e.engine.process(&[], 0, out, 2);
            let w = e.engine.parallel.workers.as_ref().unwrap();
            if e.status.left_events.load(Ordering::Relaxed) == 0 && !e.engine.parallel.stalled && !w.pool.has_workers() {
                let threads = w.threads();
                e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads, ..Default::default() })))).ok().unwrap();
                e.engine.process(&[], 0, out, 2);
            }
        }
    }

    /// Plays until a block given up on stays open.
    fn until_stalled(e: &mut Rig, out: &mut [f32]) {
        let started = Instant::now();
        while !e.engine.parallel.stalled {
            assert!(started.elapsed().as_secs() < 60, "no block was given up on");
            e.engine.process(&[], 0, out, 2);
        }
    }

    /// Track 2 and what it feeds, per `project`'s edges.
    const LEFT_OUT: [Id; 5] = [2, 4, 5, 6, 7];

    #[test]
    fn a_plug_in_that_never_returns_costs_its_track_and_what_it_feeds_and_the_rest_plays_on() {
        // Only a worker may be left in a plug-in: a task on the calling thread isn't given up on (as a
        // plug-in that never returns can't be). So everything is made to wait for a worker: a
        // two-thread pool whose caller finds nothing left to take is the sure way to get there, and
        // the test tries until the worker has the strip.
        let hold = Arc::new(AtomicBool::new(true));
        let mut e = tarred(2, &hold);
        let mut out = vec![1.0f32; BLOCK * 2];
        until_stuck(&mut e, &mut out);
        assert!(out.iter().all(|&s| s == 0.0), "the block given up on is silent");
        assert!(!e.engine.parallel.stalled, "and over");
        let st = &e.status;
        assert!(st.overload_stuck.load(Ordering::Relaxed) && st.overload_left_out.load(Ordering::Relaxed));
        let left = (st.left_events.load(Ordering::Relaxed), st.left_track.load(Ordering::Relaxed), st.left_plugin.load(Ordering::Relaxed));
        assert_eq!(left, (1, 2, 99), "the message names it");
        assert_eq!(ids(&st.held()), [(2, 99)]);
        assert_eq!(st.left_out.load(Ordering::Relaxed), 5);
        for t in &e.engine.tracks {
            assert_eq!(t.left_out, LEFT_OUT.contains(&t.id), "track {}", t.id);
        }
        // A stand-in has taken the strip's place.
        let two = e.engine.tracks.iter().find(|t| t.id == 2).unwrap();
        assert!(two.inserts.iter().all(Option::is_none));
        // Commands don't wait for the thread still in Tar, the one meant for its track included.
        let done = e.status.cmds_done.load(Ordering::Relaxed);
        e.tx.push(Cmd::Params(2, params(Some(5), &[]))).ok().unwrap();
        e.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert_eq!(e.status.cmds_done.load(Ordering::Relaxed), done + 2);
        // From here on a block running late on a busy machine mustn't be given up on or shed.
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        // Past the overload's silence, tracks 1 and 3 play, serially now (the worker was the only one).
        let started = Instant::now();
        while e.engine.shedding > 0 {
            assert!(started.elapsed().as_secs() < 20);
            e.engine.process(&[], 0, &mut out, 2);
        }
        for t in &e.engine.tracks {
            t.meter.take();
        }
        let pooled = e.status.parallel_blocks.load(Ordering::Relaxed);
        let mut heard = false;
        for _ in 0..20 {
            e.engine.process(&[], 0, &mut out, 2);
            heard |= out.iter().any(|s| s.abs() > 0.01);
        }
        assert!(heard, "the rest plays on");
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), pooled);
        for t in &e.engine.tracks {
            let (l, r) = t.meter.take();
            assert_eq!(l.max(r) > 0.0, !LEFT_OUT.contains(&t.id), "track {}'s meter", t.id);
        }
        let w = e.engine.parallel.workers.as_ref().unwrap();
        assert_eq!((w.playing(), w.health().report().stuck), (1, 1));
        // Tar returns at last: the strip it held comes back as garbage, its plug-in still in it, and
        // nothing else changes.
        hold.store(false, Ordering::SeqCst);
        let started = Instant::now();
        let held = loop {
            assert!(started.elapsed().as_secs() < 20, "the strip never came back");
            e.engine.process(&[], 0, &mut out, 2);
            match e._keep.0.pop() {
                Ok(Garbage::Track(t)) => break t,
                Ok(_) => {}
                Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        assert_eq!((held.id, held.inserts[1].as_ref().map(|i| i.uid)), (2, Some(99)));
        assert!(e.status.held().is_empty());
        let back = e.status.take_back();
        assert_eq!(ids(&back), [(2, 99)], "for the session to bring back");
        assert!(e.status.take_back().is_empty());
        assert!(e.engine.tracks.iter().all(|t| t.left_out == LEFT_OUT.contains(&t.id)), "until it brings them back");
        assert_eq!(e.engine.parallel.workers.as_ref().unwrap().health().report().stuck, 1);
        // The session sends the track a new strip, and the rest comes back with it.
        let fresh = strip(2, TrackKind::Audio, params(Some(5), &[(6, 0.4, false)]), vec![tone(20, 170.0, None, false)]);
        e.tx.push(Cmd::Back { token: back[0].token, track: Some(fresh), still_out: Vec::new() }).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert!(e.engine.tracks.iter().all(|t| !t.left_out));
        assert_eq!(e.status.left_out.load(Ordering::Relaxed), 0);
        for t in &e.engine.tracks {
            t.meter.take();
        }
        for _ in 0..20 {
            e.engine.process(&[], 0, &mut out, 2);
        }
        for t in &e.engine.tracks {
            let (l, r) = t.meter.take();
            assert!(l.max(r) > 0.0, "track {} plays again", t.id);
        }
    }

    use test_hooks::{after, gate, hold_in_task};

    /// Plays until the engine is over the block it gave up on.
    fn until_unstalled(e: &mut Rig, out: &mut [f32]) {
        let started = Instant::now();
        while e.engine.parallel.stalled {
            assert!(started.elapsed().as_secs() < 20, "the block given up on never ended");
            e.engine.process(&[], 0, out, 2);
        }
    }

    /// Issue #8 again, from inside the task: given up on while in its strip's own work, the worker
    /// is retired and the block stays open; when the strip then goes into a plug-in that doesn't
    /// return, the worker is left there after all, and the rest plays on.
    #[test]
    fn a_worker_given_up_on_in_its_task_is_left_once_it_is_stuck_in_a_plug_in() {
        let hold = Arc::new(AtomicBool::new(true));
        let mut e = rig();
        let g = project(&mut e);
        // Track 8 plays straight to Stereo Out, with Tar.
        let eight = strip(8, TrackKind::Audio, params(None, &[]), vec![tone(80, 290.0, None, false), tar_for_checks(98, SR as f32, hold.clone())]);
        e.tx.push(Cmd::AddTrack(eight, 7)).ok().unwrap();
        let order = (0..g.len()).map(|i| g.id(i)).chain([8]).collect();
        let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)];
        e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(order, &edges, |_| true).unwrap())))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 2, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: false })).ok().unwrap();
        hold_in_task(8, true);
        let mut out = vec![0.0f32; BLOCK * 2];
        until_stalled(&mut e, &mut out);
        assert!(e.engine.parallel.stalled && !e.status.overload_left_out.load(Ordering::Relaxed), "held up in its task");
        let w = e.engine.parallel.workers.as_ref().unwrap();
        assert_eq!(w.health().report().stuck, 1, "and retired");
        // On into Tar, which doesn't return: once it has been in there for most of the limit, the
        // worker is left in it, and the block ends without it.
        hold_in_task(8, false);
        until_unstalled(&mut e, &mut out);
        let st = &e.status;
        assert_eq!((st.left_events.load(Ordering::Relaxed), st.left_track.load(Ordering::Relaxed), st.left_plugin.load(Ordering::Relaxed)), (1, 8, 98));
        assert_eq!(ids(&st.held()), [(8, 98)]);
        assert!(e.engine.tracks.iter().all(|t| t.left_out == (t.id == 8)));
        let done = e.status.cmds_done.load(Ordering::Relaxed);
        e.tx.push(Cmd::Stop).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert_eq!(e.status.cmds_done.load(Ordering::Relaxed), done + 1, "commands go through");
        hold.store(false, Ordering::SeqCst);
        let started = Instant::now();
        while !e.status.held().is_empty() {
            assert!(started.elapsed().as_secs() < 20, "the strip never came back");
            e.engine.process(&[], 0, &mut out, 2);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Track ids for `stalled_with_tar`, each test's own: the caller's gate, a strip for each of the
    /// two workers to wait in, the track with Tar, and the one a worker is held up in.
    struct Stall {
        gate: Id,
        waits: [Id; 2],
        tar: Id,
        held: Id,
    }

    impl Stall {
        fn new(base: Id) -> Stall {
            Stall { gate: base, waits: [base + 1, base + 2], tar: base + 3, held: base + 4 }
        }

        /// Lets everything go.
        fn end(&self, hold: &AtomicBool) {
            gate(self.gate, &[]);
            for w in self.waits {
                after(w, None);
            }
            hold_in_task(self.held, false);
            hold.store(false, Ordering::SeqCst);
        }
    }

    /// A block on three threads that stays open with one worker left in Tar and the other held up
    /// in its own task, set up by synchronisation alone. `project`'s tracks come after five of
    /// `ids`', each to Stereo Out: the caller's first task is its gate, and each worker's first is
    /// a strip it waits in until the caller is in the gate, so no worker takes the gate; the caller
    /// waits there until the workers have the Tar track and the held one, so it takes neither.
    fn stalled_with_tar(ids: &Stall, hold: &Arc<AtomicBool>) -> Rig {
        let mut e = rig();
        project(&mut e);
        let tar = strip(ids.tar, TrackKind::Audio, params(None, &[]), vec![tone(92, 270.0, None, false), tar_for_checks(96, SR as f32, hold.clone())]);
        let plain = |id: Id| strip(id, TrackKind::Audio, params(None, &[]), vec![tone(93, 350.0, None, false)]);
        for t in [plain(ids.held), tar, plain(ids.waits[1]), plain(ids.waits[0]), plain(ids.gate)] {
            e.tx.push(Cmd::AddTrack(t, 0)).ok().unwrap();
        }
        let order = [ids.gate, ids.waits[0], ids.waits[1], ids.tar, ids.held].into_iter().chain(1..=7).collect();
        let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)];
        e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(order, &edges, |_| true).unwrap())))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 3, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: false })).ok().unwrap();
        gate(ids.gate, &[ids.tar, ids.held]);
        for w in ids.waits {
            after(w, Some(ids.gate));
        }
        hold_in_task(ids.held, true);
        let mut out = vec![0.0f32; BLOCK * 2];
        until_stalled(&mut e, &mut out);
        assert!(e.engine.parallel.stalled, "the worker in its own task holds the block open");
        // Once in Tar for long enough, the other worker is left there, from the callback if not at
        // the give-up already.
        let started = Instant::now();
        while e.engine.parallel.workers.as_ref().unwrap().fresh.is_empty() {
            assert!(started.elapsed().as_secs() < 60, "the worker in Tar was never left");
            e.engine.process(&[], 0, &mut out, 2);
        }
        assert!(e.engine.parallel.stalled);
        e
    }

    /// Two workers held up at the give-up, one in Tar for long enough, one in its own task: the
    /// block stays open after all, as the caller can't leave the second. Once that one is through,
    /// the first's strip is left out, from the callback or from `settle`, and the session hears of
    /// it then.
    #[test]
    fn a_strip_left_in_a_block_that_stays_open_is_left_out_once_it_ends() {
        for (settle, ids) in [(false, Stall::new(310)), (true, Stall::new(320))] {
            let hold = Arc::new(AtomicBool::new(true));
            let mut e = stalled_with_tar(&ids, &hold);
            assert!(!e.status.overload_left_out.load(Ordering::Relaxed), "the session stops the transport for a stalled block");
            assert_eq!(e.status.left_events.load(Ordering::Relaxed), 0, "nothing is left out yet");
            hold_in_task(ids.held, false);
            if settle {
                e.engine.settle();
            } else {
                let mut out = vec![0.0f32; BLOCK * 2];
                until_unstalled(&mut e, &mut out);
            }
            assert!(!e.engine.parallel.stalled);
            assert_eq!((e.status.left_events.load(Ordering::Relaxed), e.status.left_track.load(Ordering::Relaxed)), (1, ids.tar), "now it's said");
            assert!(e.engine.tracks.iter().all(|t| t.left_out == (t.id == ids.tar)), "settle: {settle}");
            ids.end(&hold);
        }
    }

    #[test]
    fn two_strips_can_be_left_out_and_come_back_one_after_the_other() {
        let hold = Arc::new(AtomicBool::new(true));
        let three = Arc::new(AtomicBool::new(true));
        let mut e = tarred(4, &hold);
        e.tx.push(Cmd::Insert { track: 3, slot: 1, insert: Some(tar_for_checks(97, SR as f32, three.clone())) }).ok().unwrap();
        let mut out = vec![0.0f32; BLOCK * 2];
        let started = Instant::now();
        while e.status.held().len() < 2 {
            assert!(started.elapsed().as_secs() < 30, "only {:?} left", e.status.held());
            e.engine.process(&[], 0, &mut out, 2);
        }
        until_unstalled(&mut e, &mut out);
        let both = [2, 3, 4, 5, 6, 7];
        assert!(e.engine.tracks.iter().all(|t| t.left_out == both.contains(&t.id)));
        // Both workers in Tar, and on a busy enough machine any given up on for being kept off the CPU.
        assert!(e.engine.parallel.workers.as_ref().unwrap().health().report().stuck >= 2);
        // Track 3's Tar returns first: 3 comes back, but 7 is still fed by 2.
        three.store(false, Ordering::SeqCst);
        let started = Instant::now();
        while e.status.held().len() > 1 {
            assert!(started.elapsed().as_secs() < 20);
            e.engine.process(&[], 0, &mut out, 2);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ids(&e.status.held()), [(2, 99)]);
        let back = e.status.take_back();
        assert_eq!(ids(&back), [(3, 97)]);
        let fresh = strip(3, TrackKind::Audio, params(None, &[(7, 0.6, true)]), vec![tone(30, 230.0, None, false)]);
        e.tx.push(Cmd::Back { token: back[0].token, track: Some(fresh), still_out: LEFT_OUT.to_vec() }).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert!(e.engine.tracks.iter().all(|t| t.left_out == LEFT_OUT.contains(&t.id)));
        hold.store(false, Ordering::SeqCst);
    }

    #[test]
    fn after_a_worker_is_left_in_a_plug_in_the_others_play_on() {
        let hold = Arc::new(AtomicBool::new(true));
        let mut e = tarred(4, &hold);
        let mut out = vec![0.0f32; BLOCK * 2];
        until_stuck(&mut e, &mut out);
        // From here on a block running late on a busy machine mustn't be given up on or shed.
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        while e.engine.shedding > 0 {
            e.engine.process(&[], 0, &mut out, 2);
        }
        // The worker in Tar, and on a machine busy enough, one given up on for being kept off the
        // CPU: each is out of the pool.
        let started = Instant::now();
        while e.engine.parallel.workers.as_ref().unwrap().health().report().starting > 0 {
            assert!(started.elapsed().as_secs() < 60, "the workers never started");
            std::thread::sleep(Duration::from_millis(1));
        }
        let w = e.engine.parallel.workers.as_ref().unwrap();
        let stuck = w.health().report().stuck;
        assert!(stuck >= 1 && w.playing() == 4 - stuck as usize, "{stuck} given up on, {} playing", w.playing());
        let playing = w.playing() as u32;
        // Removing the plug-in reaches the stand-in, and the rest plays on the threads left, with
        // Tar still holding the fourth.
        e.tx.push(Cmd::Insert { track: 2, slot: 1, insert: None }).ok().unwrap();
        let before = e.status.parallel_blocks.load(Ordering::Relaxed);
        let got = play(&mut e, 40);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed) - before, 40, "every block on the pool");
        assert_eq!(e.status.threads.load(Ordering::Relaxed), playing);
        assert!(got.iter().any(|s| s.abs() > 0.01));
        assert!(e.engine.tracks.iter().all(|t| t.left_out == LEFT_OUT.contains(&t.id)));
        hold.store(false, Ordering::SeqCst);
    }

    /// The garbage that has come back so far: strips of which tracks, and how many workers.
    fn garbage(e: &mut Rig) -> (Vec<Id>, usize) {
        let (mut tracks, mut workers) = (Vec::new(), 0);
        while let Ok(g) = e._keep.0.pop() {
            match g {
                Garbage::Track(t) => tracks.push(t.id),
                Garbage::Workers(_) => workers += 1,
                _ => {}
            }
        }
        (tracks, workers)
    }

    /// The thread count changes while a thread is left in a plug-in, twice: the workers holding the
    /// strip are kept until it's back, and its track then comes back as ever, with what it feeds.
    #[test]
    fn workers_replaced_while_a_thread_is_left_in_a_plug_in_hand_its_strip_back() {
        let hold = Arc::new(AtomicBool::new(true));
        let mut e = tarred(2, &hold);
        let mut out = vec![0.0f32; BLOCK * 2];
        until_stuck(&mut e, &mut out);
        assert_eq!(ids(&e.status.held()), [(2, 99)]);
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 3, ..Default::default() })))).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert_eq!(garbage(&mut e).1, 0, "the old workers are kept, as a thread of theirs holds a strip");
        // From here on a block running late on a busy machine mustn't be given up on or shed.
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        let got = play(&mut e, 200);
        assert!(got.iter().any(|s| s.abs() > 0.01), "the new pool plays");
        // Replaced again: those held nothing, and go at once.
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert_eq!(garbage(&mut e).1, 1);
        // A graph the old thread may still hold is let go of without being freed.
        e.tx.push(Cmd::Graph(None)).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert_eq!(ids(&e.status.held()), [(2, 99)], "still listed");
        // Tar returns: the strip comes back, and then the workers that held it.
        hold.store(false, Ordering::SeqCst);
        let (mut tracks, mut workers) = (Vec::new(), 0);
        let started = Instant::now();
        while workers == 0 {
            assert!(started.elapsed().as_secs() < 20, "the strip never came back");
            e.engine.process(&[], 0, &mut out, 2);
            let (t, w) = garbage(&mut e);
            tracks.extend(t);
            workers += w;
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!((tracks, workers), (vec![2], 1));
        assert!(e.status.held().is_empty());
        let back = e.status.take_back();
        assert_eq!(ids(&back), [(2, 99)], "for the session to bring back");
        let fresh = strip(2, TrackKind::Audio, params(Some(5), &[(6, 0.4, false)]), vec![tone(20, 170.0, None, false)]);
        e.tx.push(Cmd::Back { token: back[0].token, track: Some(fresh), still_out: Vec::new() }).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert!(e.engine.tracks.iter().all(|t| !t.left_out && !t.stand_in));
        assert_eq!(e.status.left_out.load(Ordering::Relaxed), 0);
        for t in &e.engine.tracks {
            t.meter.take();
        }
        play(&mut e, 20);
        for t in &e.engine.tracks {
            let (l, r) = t.meter.take();
            assert!(l.max(r) > 0.0, "track {} plays again", t.id);
        }
    }

    /// The engine goes while workers it replaced still have a thread left in a plug-in: it doesn't
    /// wait for the thread.
    #[test]
    fn an_engine_with_replaced_workers_still_holding_a_strip_drops_at_once() {
        let hold = Arc::new(AtomicBool::new(true));
        let mut e = tarred(2, &hold);
        let mut out = vec![0.0f32; BLOCK * 2];
        until_stuck(&mut e, &mut out);
        e.tx.push(Cmd::Workers(None)).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        let t = Instant::now();
        drop(e);
        assert!(t.elapsed() < Duration::from_secs(5));
        // Tar returns into what was left behind for it.
        hold.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
    }

    /// The track comes back from `Cmd::Back` still fed by a plug-in a thread is left in: it sits
    /// out with the rest of those, until that one is back too. A `Cmd::Back` for a stand-in that
    /// has gone (its track deleted, then brought back by undo with a strip of its own) leaves the
    /// strip the track has now.
    #[test]
    fn a_track_brought_back_while_its_feeder_is_still_held_sits_out() {
        let mut e = rig();
        project(&mut e);
        let mut out = vec![0.0f32; BLOCK * 2];
        // As `leave_out` would leave them: 1 and 4 are stand-ins (4 waits for 1), and aux 6 and 7
        // sit out with them.
        for t in e.engine.tracks.iter_mut().filter(|t| [1, 4, 6, 7].contains(&t.id)) {
            t.sit_out();
        }
        for (i, token) in [(0, 4), (3, 5)] {
            e.engine.tracks[i].stand_in = true;
            e.engine.tracks[i].held_token = token;
        }
        // Track 4's plug-in returns, but 1 is still held.
        let four = strip(4, TrackKind::Audio, params(None, &[(6, 0.3, false)]), vec![tone(40, 310.0, None, false)]);
        e.tx.push(Cmd::Back { token: 5, track: Some(four), still_out: vec![1, 4, 6, 7] }).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        let four = e.engine.tracks.iter().find(|t| t.id == 4).unwrap();
        assert!(four.left_out && !four.stand_in, "the new strip, sitting out");
        assert_eq!(e.status.left_out.load(Ordering::Relaxed), 4);
        // One for a stand-in that isn't there any more: the new strip goes, and 4's stays.
        let other = strip(4, TrackKind::Audio, params(None, &[]), vec![]);
        e.tx.push(Cmd::Back { token: 5, track: Some(other), still_out: vec![1, 4, 6, 7] }).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        let four = e.engine.tracks.iter().find(|t| t.id == 4).unwrap();
        assert_eq!(four.params.sends[0].map(|s| s.bus), Some(6), "the strip it had");
    }

    fn ids(v: &[HeldStrip]) -> Vec<(Id, Id)> {
        v.iter().map(|h| (h.track, h.plugin)).collect()
    }

    /// Removing a stand-in (its track deleted) says so in its entry, for the session: whatever strip
    /// the track gets later isn't the one held.
    #[test]
    fn a_stand_in_removed_with_its_track_is_noted_in_its_entry() {
        let hold = Arc::new(AtomicBool::new(true));
        let mut e = tarred(2, &hold);
        let mut out = vec![0.0f32; BLOCK * 2];
        until_stuck(&mut e, &mut out);
        assert!(!e.status.held()[0].stand_in_gone);
        e.tx.push(Cmd::RemoveTrack(2)).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert!(e.status.held()[0].stand_in_gone);
        hold.store(false, Ordering::SeqCst);
    }
}
