//! Render ahead (#448): tracks that aren't being played live have their source (regions or MIDI
//! and instrument, then their inserts) rendered by render threads up to a lead ahead of the
//! playhead, so a slow block is absorbed by what's already rendered. The strip's mix (fader,
//! sends, output) stays in the callback and plays the rendered block for the playhead's position.
//!
//! **Ownership.** A strip that goes ahead (is *lent*) trades its source state into its shell, a
//! spare `TrackDsp` only its render thread touches ([`trade`]); taking it back (*reclaiming*)
//! trades it back. So the callback and a render thread never share a strip: the callback plays
//! the mix of the strip it owns, the render thread the source in the shell. Whatever changes a
//! strip's source (an edit, a locate, Stop) reclaims it first: the rendered blocks go, and it plays
//! live from the playhead, its plug-ins as far ahead as they had rendered (a reverb may jump a
//! little). It goes ahead again at the end of a callback, when it's still eligible ([`eligible`])
//! and the callbacks are calm ([`Engine::lend_ahead`]).
//!
//! **Misses.** The callback never waits on a render thread past its deadline, and never plays a
//! block again that a render thread is making ([`take_block`]): a strip whose block is late plays
//! silence for it rather than holding up every track.
//!
//! **Positions.** The render thread follows the path the playhead will take ([`Course`]): the
//! callback's frames, split at [`MAX_BLOCK`] and at the cycle's end, as `process_io` splits them,
//! wrapping to the cycle's start. Each block is marked with its position; the callback takes the
//! block for its own, so what plays is exactly what would have played live. A block marked for
//! another position means the playhead went elsewhere: the strip is reclaimed.
//!
//! **Lifetimes.** Lent strips are entries in a table the render threads scan. A render thread
//! posts the strip it's looking at as its hazard before it touches it; reclaiming takes the entry
//! out, then waits until no hazard names the strip, so its shell and ring are never freed or traded
//! under a render thread. A render thread that doesn't come back (a plug-in that never returns)
//! leaves the strip lost: it plays silence, and its control is never freed.
//!
//! The audio path's rules hold on the callback's side: no allocation, no lock. Render threads run
//! at real-time priority below the audio threads' where the system allows it, so a busy machine
//! doesn't starve them ([`thread_setup`]).

mod ring;
#[cfg(test)]
mod tests;
mod thread_setup;

pub(crate) use ring::{FrameRing, Mark};

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

use super::*;

/// The most strips lent at once.
pub const MAX_LENT: usize = 256;
/// The most tracks kept live: the selected track and those with a plug-in window open.
pub const KEEP_LIVE: usize = 16;
/// What a strip's ring holds, in frames: a lead of 0.5 s at 48 kHz, with room for a block over.
const RING_FRAMES: usize = 24_000 + 2 * MAX_BLOCK;
/// How many blocks a ring holds, whatever their size: a lead at the shortest buffer, cut at cycle
/// ends.
const RING_BLOCKS: usize = 512;
/// The longest reclaiming waits for a render thread to come out of a strip.
const RECLAIM_WAIT: Duration = Duration::from_millis(500);
/// How long a render thread with nothing to do sleeps before looking again, if nobody wakes it.
const IDLE_NAP: Duration = Duration::from_millis(2);
/// The most blocks in a row a strip plays silent while its render thread catches up; then it's
/// taken back and plays live.
const MAX_SHORT: u8 = 2;
/// How much of a callback's time the strips wait for their render threads, from its start. One
/// deadline for all of them, so the waits overlap (on the pool, and serially once it has passed)
/// and leave the rest of the callback the remaining quarter.
pub(super) const WAIT_SHARE: f64 = 0.75;
/// The most of its time a callback may have taken for strips to be lent at its end: the rest, and
/// the next one's wait, is the render threads' time for their first blocks (from 0.9, 0.85 of a
/// buffer: 18 ms at 1024 frames for a TONE3000's 8–12). Much less, and a project whose live strips
/// take most of the buffer would never go ahead.
const LEND_SHARE: f64 = 0.9;
/// Callbacks in a row on time, without a miss, before strips are lent again.
const CALM: u32 = 4;

const HOME: u8 = 0;
const LENT: u8 = 1;
/// A render thread didn't come out of it: it plays silence from now on.
const LOST: u8 = 2;

/// Where the next block a render thread renders for a strip plays, following the playhead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Course {
    pos: i64,
    /// Frames of the current callback already rendered.
    done: usize,
    /// Frames per callback.
    frames: usize,
    /// The cycle: on, start, end.
    cycle: (bool, i64, i64),
}

impl Course {
    pub(crate) fn new(pos: i64, frames: usize, cycle: (bool, i64, i64)) -> Course {
        Course { pos, done: 0, frames: frames.max(1), cycle }
    }

    /// The next block, as `process_io` cuts it.
    pub(crate) fn next(&self) -> Mark {
        let mut n = (self.frames - self.done).min(MAX_BLOCK);
        let (on, _, end) = self.cycle;
        if on && self.pos < end && self.pos + n as i64 > end {
            n = (end - self.pos) as usize;
        }
        Mark { pos: self.pos, n }
    }

    /// Past a block of `n` frames, as `Engine::advance` moves the playhead. True when it wrapped
    /// to the cycle's start.
    pub(crate) fn advance(&mut self, n: usize) -> bool {
        self.pos += n as i64;
        self.done += n;
        if self.done >= self.frames {
            self.done = 0;
        }
        let (on, start, end) = self.cycle;
        if on && self.pos >= end {
            self.pos = start;
            return true;
        }
        false
    }
}

/// What a render thread renders a lent strip with: set by the callback when it lends the strip,
/// fixed until it's reclaimed (a change to any of it reclaims first).
struct Lending {
    course: Course,
    /// The transport, but its position.
    base: TransportInfo,
    tempo: *const TempoMap,
    sr: f64,
    chase: (bool, bool),
    low_latency: bool,
    auto_monitor: bool,
    /// Frames to keep rendered.
    lead: usize,
}

/// A strip's side of render ahead: its ring and its shell. Made off the audio thread
/// (`TrackDsp::prepare_ahead`) for audio and instrument tracks.
pub struct AheadCtl {
    pub(crate) ring: FrameRing,
    state: AtomicU8,
    /// A render thread is rendering it.
    busy: AtomicBool,
    /// Blocks in a row it played silent, its render thread still on them (the callback's).
    short: AtomicU8,
    /// The callback wants it back: render threads start no more blocks of it.
    come_back: AtomicBool,
    /// Its entry in the table while lent.
    entry: AtomicUsize,
    /// The render thread's while lent, the callback's while home.
    shell: UnsafeCell<Box<TrackDsp>>,
    lending: UnsafeCell<Lending>,
}

// SAFETY: `shell` and `lending` are touched by one side at a time, as the module docs set out.
unsafe impl Sync for AheadCtl {}
unsafe impl Send for AheadCtl {}

impl AheadCtl {
    pub(crate) fn new(kind: TrackKind) -> AheadCtl {
        let shell = Box::new(TrackDsp::new(0, kind, TrackDsp::shell_params(), Arc::default()));
        let lending = Lending {
            course: Course::new(0, 1, (false, 0, 0)),
            base: TransportInfo::default(),
            tempo: std::ptr::null(),
            sr: 48_000.0,
            chase: (true, true),
            low_latency: false,
            auto_monitor: false,
            lead: 0,
        };
        AheadCtl {
            ring: FrameRing::new(RING_FRAMES, RING_BLOCKS),
            state: AtomicU8::new(HOME),
            busy: AtomicBool::new(false),
            short: AtomicU8::new(0),
            come_back: AtomicBool::new(false),
            entry: AtomicUsize::new(usize::MAX),
            shell: UnsafeCell::new(shell),
            lending: UnsafeCell::new(lending),
        }
    }

    pub(crate) fn lent(&self) -> bool {
        self.state.load(Ordering::Acquire) == LENT
    }

    pub(crate) fn lost(&self) -> bool {
        self.state.load(Ordering::Acquire) == LOST
    }
}

/// What the callback and the render threads share: the table of lent strips, each render thread's
/// hazard, and how to wake them.
pub struct AheadShared {
    table: Box<[AtomicPtr<AheadCtl>]>,
    hazards: Box<[AtomicPtr<AheadCtl>]>,
    parked: Box<[AtomicBool]>,
    threads: Box<[OnceLock<Thread>]>,
    watch: Arc<thread_setup::Watch>,
    stop: AtomicBool,
    status: Arc<EngineStatus>,
}

impl AheadShared {
    /// Wakes the render threads that sleep. No allocation; a futex call per thread at worst.
    pub(crate) fn wake(&self) {
        for (p, t) in self.parked.iter().zip(self.threads.iter()) {
            if p.load(Ordering::SeqCst) && p.swap(false, Ordering::SeqCst) {
                if let Some(t) = t.get() {
                    t.unpark();
                }
            }
        }
    }

    fn named(&self, ctl: *const AheadCtl) -> bool {
        self.hazards.iter().any(|h| std::ptr::eq(h.load(Ordering::SeqCst), ctl))
    }
}

/// The render threads. Made and dropped on the session's thread; dropping stops and joins them.
pub struct AheadThreads {
    shared: Arc<AheadShared>,
    handles: Vec<JoinHandle<()>>,
    _watchdog: thread_setup::Watchdog,
}

impl AheadThreads {
    /// Starts `threads` render threads.
    pub fn start(threads: usize, status: Arc<EngineStatus>) -> AheadThreads {
        let threads = threads.max(1);
        let null = || AtomicPtr::new(std::ptr::null_mut());
        let shared = Arc::new(AheadShared {
            table: (0..MAX_LENT).map(|_| null()).collect(),
            hazards: (0..threads).map(|_| null()).collect(),
            parked: (0..threads).map(|_| AtomicBool::new(false)).collect(),
            threads: (0..threads).map(|_| OnceLock::new()).collect(),
            watch: Arc::new(thread_setup::Watch::new(threads)),
            stop: AtomicBool::new(false),
            status,
        });
        let handles = (0..threads)
            .filter_map(|k| {
                let s = shared.clone();
                std::thread::Builder::new().name(format!("marbble-ahead-{}", k + 1)).spawn(move || render_loop(&s, k)).ok()
            })
            .collect();
        let _watchdog = thread_setup::Watchdog::start(&shared.watch);
        AheadThreads { shared, handles, _watchdog }
    }

    pub fn shared(&self) -> Arc<AheadShared> {
        self.shared.clone()
    }
}

impl Drop for AheadThreads {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        for t in self.shared.threads.iter().filter_map(|t| t.get()) {
            t.unpark();
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

/// The routes of a strip's source on a render thread: a lent strip reaches no other strip.
struct Alone;

impl Routes for Alone {
    fn earlier(&self, _: usize) -> Option<&TrackDsp> {
        None
    }
    fn id_of(&self, _: usize) -> Id {
        0
    }
    fn fold(&self, read_ahead: auto::FoldState, _: usize, _: i64, _: bool) -> auto::FoldState {
        read_ahead
    }
    fn inputs(&mut self, _: (&mut [f32], &mut [f32]), _: &mut [f32], _: &mut [f32]) {}
    fn send(&mut self, _: &mut Outlets, _: SendOut, _: (&[f32], &[f32]), _: &mut f32, _: f32, _: f32) {}
    fn output(&mut self, _: &mut Outlets, _: Dest, _: &[f32], _: &[f32]) {}
    fn analyze(&mut self, _: &mut Outlets, _: Option<(&[f32], &[f32])>, _: usize) {}
}

fn render_loop(shared: &AheadShared, k: usize) {
    let _ = shared.threads[k].set(std::thread::current());
    let _ftz = crate::dsp::denormals::NoDenormals::new();
    let mut sc = Scratch::default();
    let mut priority = thread_setup::RenderPriority::start(&shared.watch, k);
    while !shared.stop.load(Ordering::SeqCst) {
        let mut rendered = false;
        for entry in shared.table.iter() {
            let ctl = entry.load(Ordering::SeqCst);
            if ctl.is_null() {
                continue;
            }
            priority.before_block();
            shared.hazards[k].store(ctl, Ordering::SeqCst);
            // Taken out meanwhile: it may be gone.
            if entry.load(Ordering::SeqCst) == ctl {
                // SAFETY: named as this thread's hazard while in the table, so not freed.
                let c = unsafe { &*ctl };
                let wanted = |c: &AheadCtl| c.lent() && !c.come_back.load(Ordering::SeqCst);
                if wanted(c) && c.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                    if wanted(c) {
                        rendered |= render_block(c, &mut sc, &shared.status);
                    }
                    c.busy.store(false, Ordering::SeqCst);
                }
            }
            shared.hazards[k].store(std::ptr::null_mut(), Ordering::SeqCst);
        }
        if !rendered {
            shared.parked[k].store(true, Ordering::SeqCst);
            priority.park(IDLE_NAP);
            shared.parked[k].store(false, Ordering::SeqCst);
        }
    }
}

/// Renders a lent strip's next block into its ring, if it isn't a lead ahead already. Whether it
/// did.
fn render_block(c: &AheadCtl, sc: &mut Scratch, status: &EngineStatus) -> bool {
    // SAFETY: lent and taken (`busy`): the shell and the lending are this thread's.
    let (shell, l) = unsafe { (&mut **c.shell.get(), &mut *c.lending.get()) };
    let Mark { pos, n } = l.course.next();
    if c.ring.buffered() + n > l.lead {
        return false;
    }
    // SAFETY: the engine's tempo map, which isn't replaced while anything is lent.
    let tempo = unsafe { &*l.tempo };
    let transport = transport_at(tempo, &l.base, pos);
    let cx = BlockCtx {
        n,
        pos,
        playing: true,
        recording: false,
        sr: l.sr,
        sources: true,
        punch_range: None,
        capture_from: i64::MAX,
        capture_end: i64::MAX,
        auto_monitor: l.auto_monitor,
        low_latency: l.low_latency,
        block_end: pos + n as i64,
        smooth: 0.0,
        any_solo: false,
        snap_gains: false,
        iso: None,
        transport,
        tempo,
        status,
        inputs: &[],
        live: &[],
        midi_target: None,
        midi_thru: false,
        chase: l.chase,
        analyzer_target: None,
        wait: false,
        ahead: None,
        ahead_until: None,
    };
    shell.instrument_ran = false;
    let played = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let plan = plan_block(&cx, shell, &Alone, None, false);
        play_source(&cx, shell, sc, &mut Alone, &plan)
    }));
    if !matches!(played, Ok(true)) {
        if played.is_err() {
            status.engine_errors.fetch_add(1, Ordering::Relaxed);
        }
        shell.src_l[..n].fill(0.0);
        shell.src_r[..n].fill(0.0);
    }
    c.ring.push(pos, &shell.src_l[..n], &shell.src_r[..n]);
    // As the playhead's wrap ends the notes held over it (`Engine::advance`).
    if l.course.advance(n) {
        shell.flush_notes = true;
        shell.chase_cc = true;
    }
    true
}

/// Swaps the source's state between a strip and its shell, and gives the shell what its source
/// reads of the strip's settings.
fn trade(t: &mut TrackDsp, shell: &mut TrackDsp) {
    use std::mem::swap;
    swap(&mut t.regions, &mut shell.regions);
    swap(&mut t.inserts, &mut shell.inserts);
    swap(&mut t.instrument, &mut shell.instrument);
    swap(&mut t.midi, &mut shell.midi);
    swap(&mut t.sounding, &mut shell.sounding);
    swap(&mut t.flush_notes, &mut shell.flush_notes);
    swap(&mut t.automation, &mut shell.automation);
    swap(&mut t.auto_mode, &mut shell.auto_mode);
    swap(&mut t.touched, &mut shell.touched);
    swap(&mut t.auto_sent, &mut shell.auto_sent);
    swap(&mut t.chase_cc, &mut shell.chase_cc);
    swap(&mut t.cc_sent, &mut shell.cc_sent);
    swap(&mut t.live_held, &mut shell.live_held);
    swap(&mut t.live_keys, &mut shell.live_keys);
    swap(&mut t.resume_left, &mut shell.resume_left);
    shell.id = t.id;
    shell.kind = t.kind;
    shell.params = t.params;
    shell.pdc = t.pdc;
}

/// Whether a strip may go ahead now (see the module docs).
pub(super) fn eligible(t: &TrackDsp, midi_target: Option<Id>, analyzer: Option<(Id, Id)>, fed: bool) -> bool {
    let p = &t.params;
    matches!(t.kind, TrackKind::Audio | TrackKind::Instrument)
        && t.ahead.as_ref().is_some_and(|a| !a.lost())
        && !p.off
        && !t.dormant
        && !t.left_out
        && !p.armed
        && !p.monitor
        && t.rec.is_none()
        && midi_target != Some(t.id)
        && analyzer.is_none_or(|a| a.0 != t.id)
        && !fed
        && t.inserts.iter().flatten().all(|i| i.key.source.is_none())
        // Something worth rendering ahead: a plug-in that runs.
        && t.inserts.iter().flatten().chain(t.instrument.iter()).any(|i| !i.bypass)
}

/// Made on the session's thread, for tracks that need one (see `Cmd::AheadCtls`).
pub fn ctl_for(kind: TrackKind) -> Option<Arc<AheadCtl>> {
    matches!(kind, TrackKind::Audio | TrackKind::Instrument).then(|| Arc::new(AheadCtl::new(kind)))
}

impl TrackDsp {
    /// Gives an audio or instrument track what it needs to go ahead. Allocates: off the audio
    /// thread, as the session builds the strip.
    pub fn prepare_ahead(&mut self) {
        if matches!(self.kind, TrackKind::Audio | TrackKind::Instrument) && self.ahead.is_none() {
            self.ahead = Some(Arc::new(AheadCtl::new(self.kind)));
        }
    }

    /// Settings for a shell, which its strip's replace when it's lent.
    fn shell_params() -> TrackParams {
        TrackParams {
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
        }
    }

    /// Takes the strip back if it's lent: its rendered blocks go, and its source is the callback's
    /// again. Waits for a render thread in it, up to [`RECLAIM_WAIT`]; past that, the strip is lost.
    pub(super) fn reclaim(&mut self, shared: Option<&AheadShared>) {
        let (Some(shared), Some(ctl)) = (shared, self.ahead.as_ref()) else { return };
        let ctl = ctl.clone();
        if ctl.state.compare_exchange(LENT, HOME, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        let ptr = Arc::as_ptr(&ctl);
        if let Some(e) = shared.table.get(ctl.entry.load(Ordering::Relaxed)) {
            e.store(std::ptr::null_mut(), Ordering::SeqCst);
        }
        let start = Instant::now();
        while shared.named(ptr) || ctl.busy.load(Ordering::SeqCst) {
            if start.elapsed() > RECLAIM_WAIT {
                // A render thread is in one of its plug-ins and doesn't come out: the strip plays
                // silence from now on, and its control outlives it, as that thread may yet return.
                ctl.state.store(LOST, Ordering::SeqCst);
                std::mem::forget(ctl.clone());
                return;
            }
            std::hint::spin_loop();
            if start.elapsed() > Duration::from_micros(200) {
                std::thread::sleep(Duration::from_micros(50));
            }
        }
        ctl.ring.clear();
        ctl.short.store(0, Ordering::Relaxed);
        ctl.come_back.store(false, Ordering::SeqCst);
        self.load.ahead(false);
        // SAFETY: home, and no render thread names it: the shell is the callback's.
        trade(self, unsafe { &mut *ctl.shell.get() });
    }

    /// Lends the strip to the render threads, its next block at `course`. False if the table is
    /// full.
    fn lend(&mut self, shared: &AheadShared, course: Course, engine: LendFrom<'_>) -> bool {
        let Some(ctl) = self.ahead.as_ref() else { return false };
        if ctl.state.load(Ordering::Acquire) != HOME {
            return false;
        }
        let ptr = Arc::as_ptr(ctl) as *mut AheadCtl;
        let Some(entry) = shared.table.iter().position(|e| e.load(Ordering::SeqCst).is_null()) else { return false };
        let ctl = ctl.clone();
        // SAFETY: home: the shell and the lending are the callback's.
        unsafe {
            trade(self, &mut *ctl.shell.get());
            *ctl.lending.get() = Lending {
                course,
                base: engine.base,
                tempo: engine.tempo,
                sr: engine.sr,
                chase: engine.chase,
                low_latency: engine.low_latency,
                auto_monitor: engine.auto_monitor,
                lead: engine.lead,
            };
        }
        ctl.ring.clear();
        ctl.short.store(0, Ordering::Relaxed);
        ctl.come_back.store(false, Ordering::SeqCst);
        ctl.entry.store(entry, Ordering::Relaxed);
        ctl.state.store(LENT, Ordering::SeqCst);
        shared.table[entry].store(ptr, Ordering::SeqCst);
        self.load.ahead(true);
        true
    }
}

/// What a strip is lent with, from the engine.
struct LendFrom<'a> {
    base: TransportInfo,
    tempo: *const TempoMap,
    sr: f64,
    chase: (bool, bool),
    low_latency: bool,
    auto_monitor: bool,
    lead: usize,
    _engine: std::marker::PhantomData<&'a ()>,
}

/// What came of looking for a lent strip's block.
pub(super) enum Took {
    /// It's in `t.l` and `t.r`.
    Block,
    /// The strip is lost: it plays silence.
    Lost,
    /// It was reclaimed (the block wasn't there, or was for another position): play it live.
    Reclaimed,
}

/// The engine's side of render ahead.
#[derive(Default)]
pub(crate) struct Ahead {
    pub(super) shared: Option<Arc<AheadShared>>,
    /// The lead, in seconds.
    pub(super) lead_secs: f64,
    /// Frames in the last callback, which a strip is lent for only while they stay the same.
    pub(super) last_frames: usize,
    /// Tracks that play live whatever (`Cmd::KeepLive`).
    pub(super) keep_live: [Option<Id>; KEEP_LIVE],
    /// Until when this callback's lent strips wait for their blocks.
    pub(super) until: Option<Instant>,
    /// Callbacks in a row on time and without a miss (see [`CALM`]).
    calm: u32,
    /// `EngineStatus::ahead_misses` as the last callback ended.
    misses: u32,
}

/// For a lent strip in a block: its rendered block for `cx.pos` into `t.l` and `t.r`, waiting for
/// its render thread until the callback's deadline (`BlockCtx::ahead_until`; forever, for a check
/// that must not depend on time). The callback never waits longer, nor for a render thread to come
/// out of the strip, nor plays a block again that its render thread is still making:
/// - Not there, and no render thread in the strip: it's taken back and plays live.
/// - Not there, and its render thread on it: it plays silence for this block and stays ahead; the
///   late block is let go when it comes. After [`MAX_SHORT`] blocks in a row, the render thread is
///   told to start no more (`come_back`), and the strip comes back once it's out.
pub(super) fn take_block(t: &mut TrackDsp, cx: &BlockCtx<'_>, shared: Option<&AheadShared>) -> Took {
    let Some(ctl) = t.ahead.clone() else { return Took::Reclaimed };
    if ctl.lost() {
        return Took::Lost;
    }
    let n = cx.n;
    let start = Instant::now();
    // Without the callback's deadline (a block played alone), half the block.
    let until = cx.ahead_until.unwrap_or_else(|| start + Duration::from_secs_f64(n as f64 / cx.sr / 2.0));
    let short = ctl.short.load(Ordering::Relaxed);
    let silent = |t: &mut TrackDsp| {
        t.l[..n].fill(0.0);
        t.r[..n].fill(0.0);
        Took::Block
    };
    loop {
        if ctl.come_back.load(Ordering::SeqCst) {
            if ctl.busy.load(Ordering::SeqCst) {
                return silent(t);
            }
            break;
        }
        match ctl.ring.head() {
            Some(Mark { pos, n: len }) if pos == cx.pos && len == n => {
                ctl.ring.pop(&mut t.l[..n], &mut t.r[..n]);
                ctl.short.store(0, Ordering::Relaxed);
                return Took::Block;
            }
            // Late, for where the playhead was when it played silence.
            Some(Mark { pos, n: len }) if short > 0 && pos + len as i64 <= cx.pos && len <= t.l.len() => {
                ctl.ring.pop(&mut t.l[..len], &mut t.r[..len]);
            }
            // For another position: the playhead went elsewhere.
            Some(_) => break,
            None if cx.wait || Instant::now() < until => {
                std::hint::spin_loop();
                if start.elapsed() > Duration::from_micros(100) {
                    std::thread::sleep(Duration::from_micros(50));
                }
            }
            None => {
                cx.status.ahead_misses.fetch_add(1, Ordering::Relaxed);
                if !ctl.busy.load(Ordering::SeqCst) {
                    break;
                }
                ctl.short.store(short.saturating_add(1), Ordering::Relaxed);
                if short + 1 >= MAX_SHORT {
                    ctl.come_back.store(true, Ordering::SeqCst);
                }
                return silent(t);
            }
        }
    }
    t.reclaim(shared);
    if ctl.lost() {
        Took::Lost
    } else {
        Took::Reclaimed
    }
}

impl Engine {
    /// Takes every lent strip back (a locate, Stop, a new tempo or cycle, a render).
    pub(super) fn reclaim_all(&mut self) {
        let shared = self.ahead.shared.clone();
        for t in &mut self.tracks {
            t.reclaim(shared.as_deref());
        }
    }

    /// Takes a strip back.
    pub(super) fn reclaim_track(&mut self, id: Id) {
        let shared = self.ahead.shared.clone();
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
            t.reclaim(shared.as_deref());
        }
    }

    /// Takes back what a command is about to change: the strip it names, or everything when it
    /// moves the playhead or changes what every strip goes by. Fader, pan, mute and sends leave a
    /// strip ahead.
    pub(super) fn reclaim_for(&mut self, cmd: &Cmd) {
        if self.ahead.shared.is_none() {
            return;
        }
        match cmd {
            Cmd::MasterVolume(_)
            | Cmd::Metronome { .. }
            | Cmd::Click(_)
            | Cmd::MidiThru(_)
            | Cmd::Workers(_)
            | Cmd::Graph(_)
            | Cmd::ParallelTest(_)
            | Cmd::InputTap { .. }
            | Cmd::AddTrack(..)
            | Cmd::Order(_) => {}
            Cmd::Params(id, p) => {
                let changed = self.tracks.iter().find(|t| t.id == *id).is_some_and(|t| {
                    let q = &t.params;
                    q.armed != p.armed
                        || q.monitor != p.monitor
                        || q.input != p.input
                        || q.off != p.off
                        || q.instrument_output != p.instrument_output
                        || q.midi != p.midi
                });
                if changed {
                    self.reclaim_track(*id);
                }
            }
            // A track to keep live comes back now; the others may go ahead again.
            Cmd::KeepLive(ids) => {
                for id in ids.iter().flatten() {
                    self.reclaim_track(*id);
                }
            }
            // Only the tracks live MIDI or the analyzer moves between.
            Cmd::MidiTarget(new) => {
                for id in [self.midi_target, *new].into_iter().flatten() {
                    self.reclaim_track(id);
                }
            }
            Cmd::Analyzer(new) => {
                for (id, _) in [self.analyzer_target, *new].into_iter().flatten() {
                    self.reclaim_track(id);
                }
            }
            Cmd::RemoveTrack(id)
            | Cmd::Regions(id, _)
            | Cmd::Automation(id, _)
            | Cmd::AutoMode(id, _)
            | Cmd::Insert { track: id, .. }
            | Cmd::Instrument { track: id, .. }
            | Cmd::ReorderInserts { track: id, .. }
            | Cmd::Bypass { track: id, .. }
            | Cmd::RecordSink { track: id, .. }
            | Cmd::InstrumentData { track: id, .. }
            | Cmd::AutoTouch { track: id, .. }
            | Cmd::Pdc { track: id, .. }
            | Cmd::FxParam { track: id, .. }
            | Cmd::Sidechain { track: id, .. }
            | Cmd::SidechainPre { track: id, .. }
            | Cmd::VstGate { track: id, .. } => self.reclaim_track(*id),
            _ => self.reclaim_all(),
        }
    }

    /// At the end of a callback of `frames` frames, `started` then: lends every strip that may go
    /// ahead, its next block where the next callback starts, and wakes the render threads.
    ///
    /// Only once [`CALM`] callbacks in a row were on time without a miss, and only from one that
    /// left time to spare ([`LEND_SHARE`]): a strip is lent with nothing rendered, and its first
    /// block must be ready by the next callback's deadline. Lent sooner, it misses, and every miss
    /// makes the callback that much later.
    pub(super) fn lend_ahead(&mut self, frames: usize, started: Instant) {
        let Some(shared) = self.ahead.shared.clone() else { return };
        let steady = frames == self.ahead.last_frames;
        self.ahead.last_frames = frames;
        // A check that plays flat out (`ParallelTest::patient`) goes by its blocks, not the clock.
        let patient = self.parallel.test.patient;
        let budget = frames as f64 / self.sample_rate;
        let spent = started.elapsed().as_secs_f64();
        let misses = self.status.ahead_misses.load(Ordering::Relaxed);
        let missed = std::mem::replace(&mut self.ahead.misses, misses) != misses;
        self.ahead.calm = if missed || (!patient && spent > budget) { 0 } else { self.ahead.calm.saturating_add(1) };
        let room = patient || spent <= budget * LEND_SHARE;
        let going =
            self.playing && !self.recording && !self.draining && self.stopping.is_none() && !self.offline && self.isolate.is_none() && self.shedding == 0;
        if going && steady && frames > 0 && room && self.ahead.calm >= CALM {
            let lead = ((self.ahead.lead_secs * self.sample_rate) as usize).min(RING_FRAMES - MAX_BLOCK);
            let base = TransportInfo { playing: true, recording: false, sample_rate: self.sample_rate, cycle: self.transport().cycle, ..Default::default() };
            let course = Course::new(self.pos, frames, self.cycle);
            let (midi_target, analyzer, keep_live) = (self.midi_target, self.analyzer_target, self.ahead.keep_live);
            for ti in 0..self.tracks.len() {
                let t = &self.tracks[ti];
                if t.ahead.as_ref().is_none_or(|a| a.lent() || a.lost()) {
                    continue;
                }
                // An aux on one of its instrument's outputs reads the instrument as it plays.
                let fed = self.tracks.iter().any(|a| a.feed.source == Some(ti));
                if keep_live.contains(&Some(t.id)) || !eligible(t, midi_target, analyzer, fed) {
                    continue;
                }
                let from = LendFrom {
                    base,
                    tempo: Arc::as_ptr(&self.tempo),
                    sr: self.sample_rate,
                    chase: self.chase,
                    low_latency: self.pdc.low_latency,
                    auto_monitor: self.auto_monitor,
                    lead,
                    _engine: std::marker::PhantomData,
                };
                if !self.tracks[ti].lend(&shared, course, from) {
                    break;
                }
            }
        }
        shared.wake();
    }
}
