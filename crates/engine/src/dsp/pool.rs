//! A small pool of real-time threads that run a [`TaskGraph`]'s tasks, with the thread that asked
//! (the audio callback) working too. Knows nothing about audio: see `parallel.rs` for what the
//! tasks do.
//!
//! The rules of the audio path hold here. A block allocates nothing, takes no lock and uses no
//! channel. Tasks are bits in a few words of atomics: finishing a task's last dependency sets its
//! bit, and whoever clears a bit owns the task. So there's no queue a preempted thread could hold
//! up: a ready task can always be seen and taken by the caller, however late any worker is.
//!
//! Waiting is bounded:
//!
//! - **Workers block between blocks.** A real-time thread that never blocks is killed at
//!   RLIMIT_RTTIME's hard limit (200 ms, `audio_io::survive_slow_blocks`). A worker with nothing to
//!   do spins for [`WORKER_SPIN`] at most, then parks in a futex. Blocks arriving back to back can
//!   keep finding it awake, so a worker that hasn't truly slept for [`RT_GUARD`] sleeps before its
//!   next task (the caller does the work meanwhile). Under real overload the engine sheds load
//!   first, at `OVERLOAD_SECS`, as it always did.
//! - **The caller never waits blindly.** It takes whatever is ready and only waits with nothing
//!   to take: spinning for [`CALLER_SPIN`], then in naps of [`CALLER_NAP`], which also let a worker
//!   that wasn't granted real-time priority run on this core. Past a time limit it gives up and
//!   reports the block unfinished ([`Pool::finish`]).
//! - **A panic is contained to its task.** It's counted, its dependants still run, and the pool
//!   carries on.
//! - **A plug-in that never returns costs its task, not the block.** A job says when it calls out
//!   into a plug-in ([`Call`]). A worker the caller gave up on while it was out there is left in it
//!   ([`Pool::abandon`]): its task and everything that waits for it are counted out of the block,
//!   which ends without them, and the worker touches nothing of the pool's again.
//!
//! Only a worker that is fit to play takes tasks ([`Health`]): one refused the real-time priority it
//! asked for never does, and one the caller gave up on ([`Pool::abandon`], [`Pool::retire_stuck`])
//! is demoted to normal priority, so the kernel's limit can't end the app, and takes nothing more.

use std::cell::{Cell, UnsafeCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use super::graph::{TaskGraph, MAX_TASKS};
use crate::denormals::NoDenormals;

/// The most threads a pool can have, the caller included.
pub const MAX_THREADS: usize = 32;
const WORDS: usize = MAX_TASKS.div_ceil(64);

/// How long a worker with nothing to do spins before it blocks. A task's dependants usually become
/// ready within this, and a wake from a futex costs much more than it (and a slow start from a deep
/// sleep state). Short enough to be small next to any buffer.
const WORKER_SPIN: Duration = Duration::from_micros(30);
/// How long the caller spins for a task a worker is running, before it naps between looks.
const CALLER_SPIN: Duration = Duration::from_micros(300);
const CALLER_NAP: Duration = Duration::from_micros(50);
/// The longest a worker keeps taking tasks without truly having slept: well inside RTTIME's 200 ms.
const RT_GUARD: Duration = Duration::from_millis(50);
const GUARD_NAP: Duration = Duration::from_micros(200);
/// A park that took at least this long blocked for real (one that finds a wake already waiting
/// returns in far less).
const TRUE_SLEEP: Duration = Duration::from_micros(10);
/// VST3 plug-ins expect an audio thread's roomy stack, and this much of it is touched before the
/// first block so that the first deep call doesn't take page faults on a real-time thread.
const STACK_SIZE: usize = 8 << 20;
const STACK_TOUCH: usize = 256 << 10;

/// Work the pool runs: `task` indexes the graph, `thread` is 0 for the caller and otherwise a
/// worker's number, so the job can keep per-thread scratch space. `call` is the thread's, to say
/// when it calls into a plug-in.
pub trait Job: Sync {
    fn run(&self, task: usize, thread: usize, call: &Call);
}

/// Where a thread's task is: in the pool's own code, or out in a plug-in, which may never return.
/// Only there can the caller leave a worker it gave up on ([`Pool::abandon`]), since a plug-in call
/// touches what its own task owns and nothing else of the block's.
pub struct Call {
    /// The phase in the low byte, and above it the number of the call, so that the caller leaves
    /// the very call it looked at and not one that began since.
    state: AtomicU64,
    /// When the call began, in nanoseconds from `epoch`.
    since: AtomicU64,
    epoch: Instant,
    /// What the job said it was calling (a plug-in's uid), for the message.
    what: AtomicU64,
}

const IN_TASK: u8 = 0;
const IN_PLUGIN: u8 = 1;
/// Left in the plug-in by the caller. From here the thread touches nothing of the block, the job or
/// the graph, which may all be gone by the time the plug-in returns, if it ever does.
const LEFT: u8 = 2;
/// Left, then back out of the plug-in, and done with everything it was given.
const GONE: u8 = 3;

fn phase(state: u64) -> u8 {
    state as u8
}

fn with_phase(state: u64, p: u8) -> u64 {
    (state & !0xff) | p as u64
}

impl Call {
    fn new(epoch: Instant) -> Call {
        Call { state: AtomicU64::new(IN_TASK as u64), since: AtomicU64::new(0), epoch, what: AtomicU64::new(0) }
    }

    fn now(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Before a call into a plug-in, `what` naming it. Reads the clock (a vDSO call on Linux).
    pub fn enter(&self, what: u64) {
        // Only this thread moves the state on from `IN_TASK`, the phase it's in now.
        let s = self.state.load(Ordering::Relaxed);
        self.what.store(what, Ordering::Relaxed);
        self.since.store(self.now(), Ordering::Relaxed);
        self.state.store(with_phase(s.wrapping_add(1 << 8), IN_PLUGIN), Ordering::SeqCst);
    }

    /// After it. False if the caller left the thread there meanwhile: the task is no longer its own,
    /// and the job must return at once, touching nothing more.
    #[must_use]
    pub fn leave(&self) -> bool {
        let s = self.state.load(Ordering::SeqCst);
        match phase(s) {
            IN_PLUGIN => self.state.compare_exchange(s, with_phase(s, IN_TASK), Ordering::SeqCst, Ordering::SeqCst).is_ok(),
            // Not in a call at all, which is no harm.
            p => p == IN_TASK,
        }
    }

    /// Whether the caller left the thread in a plug-in.
    pub fn left(&self) -> bool {
        phase(self.state.load(Ordering::SeqCst)) >= LEFT
    }

    /// The caller's side: leaves the thread in the call it's in, if it has been in it for `least`.
    fn leave_behind(&self, least: Duration) -> bool {
        let s = self.state.load(Ordering::SeqCst);
        if phase(s) != IN_PLUGIN {
            return false;
        }
        // Written before the state: this call's start, or a later call's, whose number then fails
        // the exchange below.
        let since = self.since.load(Ordering::Relaxed);
        if self.now().saturating_sub(since) < least.as_nanos() as u64 {
            return false;
        }
        self.state.compare_exchange(s, with_phase(s, LEFT), Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    fn gone(&self) -> bool {
        phase(self.state.load(Ordering::SeqCst)) == GONE
    }
}

/// How to start a pool.
#[derive(Debug, Clone, Default)]
pub struct PoolOptions {
    /// Threads including the caller, so it starts this many less one.
    pub threads: usize,
    /// Promote the workers to real time as cpal does its callback's thread, for this buffer size
    /// and sample rate. A worker that is refused it plays nothing (see `promote`).
    pub realtime: Option<(u32, u32)>,
    /// Logical CPUs the workers keep to (empty: anywhere). See `threads::Topology::fast`.
    pub cpus: Vec<usize>,
    /// For checks: the workers whose bit is set (bit 1 for the first) act as if real-time priority
    /// was refused them, whether or not `realtime` asks for it.
    #[doc(hidden)]
    pub refuse_for_checks: u32,
}

/// A worker's part in the pool. Only an `ACTIVE` one takes tasks.
const STARTING: u8 = 0;
const ACTIVE: u8 = 1;
/// Refused real-time priority, or never started: it has ended.
const LEFT_OUT: u8 = 2;
/// The caller gave up on a block while it was in a task: demoted, and it ends once it has helped
/// finish that block, or at once if it was left in a plug-in (`Pool::abandon`).
const RETIRED: u8 = 3;

/// How the workers of a pool are doing, for the session and Settings › Audio. Shared, so the session
/// can read it while the engine owns the pool.
#[derive(Debug, Default)]
pub struct Health {
    workers: u32,
    active: AtomicU32,
    /// Workers whose start is over, whichever way it went.
    started: AtomicU32,
    refused: AtomicU32,
    stuck: AtomicU32,
    realtime: AtomicU32,
    /// The first refusal's reason, for the log.
    why: OnceLock<String>,
}

/// What [`Health`] says right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HealthReport {
    /// Threads asked for, the caller included.
    pub threads: u32,
    /// Threads that play now, the caller included.
    pub playing: u32,
    /// Workers still starting (asking for real-time priority).
    pub starting: u32,
    /// Workers left out because they were refused real-time priority (or couldn't start).
    pub refused: u32,
    /// Workers left out because a block was given up on while they were in it.
    pub stuck: u32,
    /// Workers granted real-time priority.
    pub realtime: u32,
}

impl Health {
    pub fn report(&self) -> HealthReport {
        let started = self.started.load(Ordering::Acquire);
        HealthReport {
            threads: self.workers + 1,
            playing: self.active.load(Ordering::Relaxed) + 1,
            starting: self.workers.saturating_sub(started),
            refused: self.refused.load(Ordering::Relaxed),
            stuck: self.stuck.load(Ordering::Relaxed),
            realtime: self.realtime.load(Ordering::Relaxed),
        }
    }

    /// Why the first worker refused real-time priority was.
    pub fn refusal(&self) -> Option<&str> {
        self.why.get().map(String::as_str)
    }
}

#[repr(align(64))]
struct Line<T>(T);

struct Slot {
    thread: OnceLock<Thread>,
    parked: AtomicBool,
    /// `STARTING`, `ACTIVE`, `LEFT_OUT` or `RETIRED`.
    state: AtomicU8,
    /// In a task, or about to take one: set before the claim, so a worker preempted between taking a
    /// task and starting it still counts as in it.
    busy: AtomicBool,
    /// Granted real-time priority, so a demotion is due if it gets stuck.
    realtime: AtomicBool,
    /// The kernel's id for the thread, to demote it from another (Linux).
    tid: AtomicI32,
    /// The block it was retired in (`Shared::block`).
    retired_in: AtomicU32,
    /// The task it took last.
    task: AtomicU32,
}

impl Slot {
    fn active(&self) -> bool {
        self.state.load(Ordering::Relaxed) == ACTIVE
    }
}

struct Shared {
    /// Bit `i` is set while task `i` is ready and nobody has taken it.
    ready: Box<[Line<AtomicU64>]>,
    /// Dependencies left per task.
    pending: Box<[AtomicU32]>,
    /// Tasks not finished yet. Zero means nobody is in a task and no bit is set.
    remaining: Line<AtomicU32>,
    /// The job and its graph. Written by the caller only with `remaining` at zero, read only by a
    /// thread that has taken a task, whose bit was set after the write.
    job: UnsafeCell<Option<(*const (dyn Job + 'static), *const TaskGraph)>>,
    /// Which tasks each thread takes first, so that a strip tends to stay on one thread from block
    /// to block and find its data in that core's cache.
    home: Box<[[u64; WORDS]]>,
    workers: Box<[Slot]>,
    /// One per thread, the caller's first.
    calls: Box<[Line<Call>]>,
    panics: AtomicU32,
    shutdown: AtomicBool,
    health: Arc<Health>,
    /// Counts the blocks started, so a retired worker can tell the block it was given up in from
    /// the next.
    block: AtomicU32,
}

// SAFETY: `job` is the only field that isn't Sync. See its comment; the pointers it holds are only
// dereferenced by the thread owning a task, as `Pool::start`'s contract keeps them valid until the
// last task finishes.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

impl Shared {
    fn claim(&self, thread: usize) -> Option<usize> {
        for preferred in [true, false] {
            for w in 0..WORDS {
                let mut bits = self.ready[w].0.load(Ordering::Relaxed);
                if preferred {
                    bits &= self.home[thread][w];
                }
                while bits != 0 {
                    let bit = 1u64 << bits.trailing_zeros();
                    let was = self.ready[w].0.fetch_and(!bit, Ordering::AcqRel);
                    if was & bit != 0 {
                        return Some(w * 64 + bit.trailing_zeros() as usize);
                    }
                    bits &= !bit;
                }
            }
        }
        None
    }

    fn any_ready(&self) -> bool {
        self.ready.iter().any(|w| w.0.load(Ordering::SeqCst) != 0)
    }

    /// Wakes up to `count` parked workers. Never blocks: at worst it's a futex call per worker.
    fn wake(&self, mut count: usize) {
        for w in self.workers.iter() {
            if count == 0 {
                break;
            }
            if w.active() && w.parked.load(Ordering::SeqCst) && w.parked.swap(false, Ordering::SeqCst) {
                if let Some(t) = w.thread.get() {
                    t.unpark();
                }
                count -= 1;
            }
        }
    }

    /// Runs a task and counts it out. False if the thread was left in a plug-in meanwhile
    /// ([`Pool::abandon`]), when it must end.
    fn run_task(&self, task: usize, thread: usize) -> bool {
        // SAFETY: taking the task's bit synchronized with the `start` that wrote this, and the
        // caller won't write it again until this task has counted itself out below.
        let (job, graph) = unsafe { (*self.job.get()).expect("a task without a job") };
        // SAFETY: `Pool::start`'s contract keeps both alive until `remaining` reaches zero.
        let (job, graph) = unsafe { (&*job, &*graph) };
        let call = &self.calls[thread].0;
        let panicked = catch_unwind(AssertUnwindSafe(|| job.run(task, thread, call))).is_err();
        // A panic can leave the call open. Whichever way it went, a thread left in it has no task
        // any more: the caller counted it out, and the next block may be running.
        if !call.leave() {
            call.state.store(GONE as u64, Ordering::SeqCst);
            return false;
        }
        if panicked {
            self.panics.fetch_add(1, Ordering::Relaxed);
        }
        let mut freed = 0usize;
        for &s in graph.succs(task) {
            let s = s as usize;
            if self.pending[s].fetch_sub(1, Ordering::AcqRel) == 1 {
                self.ready[s / 64].0.fetch_or(1 << (s % 64), Ordering::SeqCst);
                freed += 1;
            }
        }
        // This thread takes one of them itself.
        if freed > 1 {
            self.wake(freed - 1);
        }
        // The last thing: after this the caller may start the next block.
        self.remaining.0.fetch_sub(1, Ordering::AcqRel);
        true
    }
}

/// The worker threads that run a [`TaskGraph`]'s tasks alongside the caller, with no lock or queue. It
/// knows nothing of audio.
pub struct Pool {
    shared: Arc<Shared>,
    /// Each with its worker's number.
    joins: Vec<(usize, JoinHandle<()>)>,
    threads: usize,
    /// Tasks of the open block counted out with a thread left in a plug-in (`Pool::abandon`).
    dropped: Cell<[u64; WORDS]>,
}

/// Runs in each worker once it's set up and before its first task, for checks that need to know
/// the worker threads (`rt_alloc_check` counts what they allocate).
static WORKER_HOOK: OnceLock<fn()> = OnceLock::new();

pub fn set_worker_hook(f: fn()) {
    let _ = WORKER_HOOK.set(f);
}

impl Pool {
    /// Starts the workers. Off the audio thread: this allocates and spawns.
    pub fn new(opts: PoolOptions) -> Pool {
        let threads = opts.threads.clamp(1, MAX_THREADS);
        let home = (0..threads)
            .map(|t| {
                let mut m = [0u64; WORDS];
                for i in (t..MAX_TASKS).step_by(threads) {
                    m[i / 64] |= 1 << (i % 64);
                }
                m
            })
            .collect();
        let shared = Arc::new(Shared {
            ready: (0..WORDS).map(|_| Line(AtomicU64::new(0))).collect(),
            pending: (0..MAX_TASKS).map(|_| AtomicU32::new(0)).collect(),
            remaining: Line(AtomicU32::new(0)),
            job: UnsafeCell::new(None),
            home,
            workers: (1..threads)
                .map(|_| Slot {
                    thread: OnceLock::new(),
                    parked: AtomicBool::new(false),
                    state: AtomicU8::new(STARTING),
                    busy: AtomicBool::new(false),
                    realtime: AtomicBool::new(false),
                    tid: AtomicI32::new(0),
                    retired_in: AtomicU32::new(0),
                    task: AtomicU32::new(0),
                })
                .collect(),
            calls: {
                let epoch = Instant::now();
                (0..threads).map(|_| Line(Call::new(epoch))).collect()
            },
            panics: AtomicU32::new(0),
            shutdown: AtomicBool::new(false),
            health: Arc::new(Health { workers: threads as u32 - 1, ..Default::default() }),
            block: AtomicU32::new(0),
        });
        let mut joins = Vec::with_capacity(threads - 1);
        for index in 1..threads {
            let sh = shared.clone();
            let o = opts.clone();
            match thread::Builder::new().name(format!("marbble-dsp-{index}")).stack_size(STACK_SIZE).spawn(move || worker(sh, index, o)) {
                Ok(j) => joins.push((index, j)),
                // The caller can do every task alone, with whichever workers did start.
                Err(e) => {
                    log::error!("no processing thread {index}: {e}");
                    shared.workers[index - 1].state.store(LEFT_OUT, Ordering::Relaxed);
                    shared.health.refused.fetch_add(1, Ordering::Relaxed);
                    shared.health.started.fetch_add(1, Ordering::Release);
                }
            }
        }
        Pool { shared, joins, threads, dropped: Cell::new([0; WORDS]) }
    }

    /// Threads including the caller, as asked for (see [`Pool::playing`] for those that play).
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// Threads that take tasks now, the caller included.
    pub fn playing(&self) -> usize {
        self.shared.health.active.load(Ordering::Relaxed) as usize + 1
    }

    /// Whether any worker plays or may yet: false once every one was refused or given up on, when
    /// the pool is only the caller doing the serial loop's work the long way round.
    pub fn has_workers(&self) -> bool {
        let h = &*self.shared.health;
        // A pool made without workers (in tests) is what it was asked to be.
        h.workers == 0 || h.active.load(Ordering::Relaxed) > 0 || h.started.load(Ordering::Relaxed) < h.workers
    }

    pub fn health(&self) -> Arc<Health> {
        self.shared.health.clone()
    }

    /// How many workers were granted real-time priority so far.
    pub fn realtime_workers(&self) -> u32 {
        self.shared.health.realtime.load(Ordering::Relaxed)
    }

    /// After [`Pool::finish`] gave up: every worker that has been out in one plug-in call
    /// ([`Call::enter`]) for at least `least` is left there. A call shorter than that is more likely
    /// a hiccup (a page fault, a slow first block) than a hang, and is for a later look. Its task,
    /// and every task that waits for it, are counted out of the block, which ends once the rest is
    /// done (follow with [`Pool::finish`] again). The worker is demoted and retired as in
    /// [`Pool::retire_stuck`], and once out of the plug-in, if ever, it ends, touching nothing of
    /// the pool's but its [`Call`]. `left` hears of each: its thread, its task and what it called.
    /// Returns how many it left. A worker held up anywhere else is for `retire_stuck`, and if it
    /// then goes into a plug-in that doesn't return, a later call here leaves it after all.
    ///
    /// On the audio thread, with `graph` the open block's: allocates nothing and takes no lock, and
    /// makes one `sched_setscheduler` call per real-time worker it demotes.
    pub fn abandon(&self, graph: &TaskGraph, least: Duration, mut left: impl FnMut(usize, usize, u64)) -> u32 {
        let sh = &*self.shared;
        let block = sh.block.load(Ordering::Relaxed);
        let mut dropped = self.dropped.get();
        let mut n = 0;
        for (i, w) in sh.workers.iter().enumerate() {
            let call = &sh.calls[i + 1].0;
            // Playing, or retired in this block (`retire_stuck`) and helping finish it. Only this
            // thread moves a worker on from `ACTIVE`, and a call from `IN_PLUGIN` to `LEFT`.
            let active = w.active();
            let draining = w.state.load(Ordering::SeqCst) == RETIRED && w.retired_in.load(Ordering::Relaxed) == block;
            if !(active || draining) || !call.leave_behind(least) {
                continue;
            }
            if active {
                w.retired_in.store(block, Ordering::Relaxed);
                w.state.store(RETIRED, Ordering::SeqCst);
                let h = &*sh.health;
                h.active.fetch_sub(1, Ordering::Relaxed);
                h.stuck.fetch_add(1, Ordering::Relaxed);
                if w.realtime.load(Ordering::Relaxed) {
                    demote(w.tid.load(Ordering::Relaxed));
                }
            }
            // Stored before the claim's task ran, so before the call it's in.
            let task = w.task.load(Ordering::Relaxed) as usize;
            // What waits for it never gets ready: none of it has run, and none of it will.
            let mut out = 0;
            let mut count_out = |t: usize, d: &mut [u64; WORDS]| {
                if d[t / 64] & (1 << (t % 64)) == 0 {
                    d[t / 64] |= 1 << (t % 64);
                    out += 1;
                }
            };
            count_out(task, &mut dropped);
            // Every edge points forward, so one pass finds all of it.
            for t in task + 1..graph.len() {
                if graph.deps(t).iter().any(|&d| dropped[d as usize / 64] & (1 << (d % 64)) != 0) {
                    count_out(t, &mut dropped);
                }
            }
            sh.remaining.0.fetch_sub(out, Ordering::AcqRel);
            left(i + 1, task, call.what.load(Ordering::Relaxed));
            n += 1;
        }
        self.dropped.set(dropped);
        n
    }

    /// Whether `task` of the open (or last) block was counted out by [`Pool::abandon`].
    pub fn dropped(&self, task: usize) -> bool {
        self.dropped.get()[task / 64] & (1 << (task % 64)) != 0
    }

    /// Whether a thread left in a plug-in has come back out of it and let go of everything it was
    /// given (false for one never left).
    pub fn gone(&self, thread: usize) -> bool {
        self.shared.calls.get(thread).is_some_and(|c| c.0.gone())
    }

    /// Whether the caller left `thread` in a plug-in (it may have come back out since).
    pub fn is_left(&self, thread: usize) -> bool {
        self.shared.calls.get(thread).is_some_and(|c| c.0.left())
    }

    /// After [`Pool::finish`] gave up: every worker still in a task (the one holding the block up)
    /// loses real-time priority at once, so that a plug-in that never returns can't have the kernel
    /// end the app at RLIMIT_RTTIME's hard limit, and takes no task of a later block. The block stays
    /// open: once out of its task the worker helps finish it (the caller has gone, and there may be
    /// no other worker), then ends. The other workers play on. Returns how many it retired.
    ///
    /// On the audio thread: allocates nothing and takes no lock, and makes one `sched_setscheduler`
    /// call per real-time worker it demotes, only ever here.
    pub fn retire_stuck(&self) -> u32 {
        let mut n = 0;
        let block = self.shared.block.load(Ordering::Relaxed);
        for w in self.shared.workers.iter() {
            // Only this thread moves a worker on from `ACTIVE`, and `busy` is only ever set by one.
            if !w.busy.load(Ordering::SeqCst) || !w.active() {
                continue;
            }
            // Before the state, which the worker reads first (`drain`).
            w.retired_in.store(block, Ordering::Relaxed);
            w.state.store(RETIRED, Ordering::SeqCst);
            let h = &*self.shared.health;
            h.active.fetch_sub(1, Ordering::Relaxed);
            h.stuck.fetch_add(1, Ordering::Relaxed);
            if w.realtime.load(Ordering::Relaxed) {
                demote(w.tid.load(Ordering::Relaxed));
            }
            n += 1;
        }
        n
    }

    /// Opens a block: the graph's tasks can now be taken. Follow with [`Pool::finish`].
    ///
    /// # Safety
    ///
    /// `job` and `graph` must stay valid, and `job` must not be mutated, until [`Pool::is_done`]
    /// is true. The previous block must be done, and only one thread may be the caller at a time.
    pub unsafe fn start(&self, graph: &TaskGraph, job: *const (dyn Job + 'static)) {
        let sh = &*self.shared;
        debug_assert!(self.is_done());
        debug_assert!(graph.len() <= MAX_TASKS);
        // Before any task can be seen: a worker that takes one sees the new count.
        sh.block.fetch_add(1, Ordering::SeqCst);
        self.dropped.set([0; WORDS]);
        // SAFETY: `remaining` is zero (the contract), so nobody holds a task, and no bit is set for
        // anyone to take one.
        unsafe { *sh.job.get() = Some((job, graph as *const TaskGraph)) };
        for i in 0..graph.len() {
            sh.pending[i].store(graph.deps(i).len() as u32, Ordering::Relaxed);
        }
        sh.remaining.0.store(graph.len() as u32, Ordering::Relaxed);
        let mut bits = [0u64; WORDS];
        for &r in graph.roots() {
            bits[r as usize / 64] |= 1 << (r % 64);
        }
        // Everything above is published by these (a thread takes a bit with Acquire).
        for (w, b) in bits.iter().enumerate().filter(|(_, b)| **b != 0) {
            sh.ready[w].0.fetch_or(*b, Ordering::SeqCst);
        }
        sh.wake(graph.roots().len().saturating_sub(1));
    }

    /// Works on the open block until it's done, and returns true; or false if it isn't done after
    /// `limit`, which can only be because a worker is stuck in a task or isn't getting the CPU. The
    /// block then stays open, and [`Pool::is_done`] says when it's over.
    pub fn finish(&self, limit: Option<Duration>) -> bool {
        let sh = &*self.shared;
        let started = Instant::now();
        let mut idle_since = started;
        let mut spins = 0u32;
        loop {
            if let Some(task) = sh.claim(0) {
                // The caller is never left in a plug-in.
                sh.run_task(task, 0);
                idle_since = Instant::now();
                continue;
            }
            if sh.remaining.0.load(Ordering::Acquire) == 0 {
                return true;
            }
            std::hint::spin_loop();
            spins += 1;
            // Looking at the clock costs more than a look at the atomics.
            if spins.is_multiple_of(32) {
                let now = Instant::now();
                if limit.is_some_and(|l| now - started > l) {
                    return false;
                }
                if now - idle_since > CALLER_SPIN {
                    thread::sleep(CALLER_NAP);
                }
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.shared.remaining.0.load(Ordering::Acquire) == 0
    }

    /// Tasks that panicked since the last call.
    pub fn take_panics(&self) -> u32 {
        if self.shared.panics.load(Ordering::Relaxed) == 0 {
            return 0;
        }
        self.shared.panics.swap(0, Ordering::Relaxed)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        for w in self.shared.workers.iter() {
            w.parked.store(false, Ordering::SeqCst);
            if let Some(t) = w.thread.get() {
                t.unpark();
            }
        }
        for (index, j) in self.joins.drain(..) {
            // One still in the plug-in it was left in may never end: it's let go, holding the
            // shared state it needs.
            if phase(self.shared.calls[index].0.state.load(Ordering::SeqCst)) == LEFT {
                continue;
            }
            let _ = j.join();
        }
    }
}

fn worker(sh: Arc<Shared>, index: usize, opts: PoolOptions) {
    let slot = &sh.workers[index - 1];
    let _ = slot.thread.set(thread::current());
    #[cfg(target_os = "linux")]
    // SAFETY: a plain system call about this thread.
    slot.tid.store(unsafe { libc::gettid() }, Ordering::Relaxed);
    let fit = promote(&sh, index, &opts);
    if !fit {
        slot.state.store(LEFT_OUT, Ordering::Relaxed);
        sh.health.started.fetch_add(1, Ordering::Release);
        return;
    }
    keep_to(&opts.cpus);
    // Freed now rather than when the worker ends, which for one left in a plug-in is whenever the
    // plug-in returns, maybe in the middle of a block.
    drop(opts);
    // Flush-to-zero is per thread. It holds for the worker's life, as the thread is ours alone.
    let _ftz = NoDenormals::new();
    touch_stack();
    if let Some(hook) = WORKER_HOOK.get() {
        hook();
    }
    slot.state.store(ACTIVE, Ordering::SeqCst);
    sh.health.active.fetch_add(1, Ordering::Relaxed);
    sh.health.started.fetch_add(1, Ordering::Release);
    let mut slept = Instant::now();
    loop {
        if sh.shutdown.load(Ordering::Acquire) {
            return;
        }
        // Given up on (`Pool::retire_stuck`): nothing wakes it any more, and it ends.
        if !slot.active() {
            drain(&sh, index);
            return;
        }
        slot.busy.store(true, Ordering::SeqCst);
        if let Some(task) = sh.claim(index) {
            slot.task.store(task as u32, Ordering::Relaxed);
            if !sh.run_task(task, index) {
                return;
            }
            slot.busy.store(false, Ordering::Release);
            if slept.elapsed() > RT_GUARD {
                thread::sleep(GUARD_NAP);
                slept = Instant::now();
            }
            continue;
        }
        slot.busy.store(false, Ordering::Release);
        if spin_for_work(&sh) {
            continue;
        }
        // A waker sets a ready bit and then looks at `parked`; this sets `parked` and then looks at
        // the ready bits. So one of the two always sees the other (all SeqCst), and a wake that
        // finds the thread not yet parked leaves its token for `park` to return at once.
        slot.parked.store(true, Ordering::SeqCst);
        if sh.any_ready() || sh.shutdown.load(Ordering::SeqCst) {
            slot.parked.store(false, Ordering::SeqCst);
            continue;
        }
        let before = Instant::now();
        thread::park();
        slot.parked.store(false, Ordering::SeqCst);
        if before.elapsed() >= TRUE_SLEEP {
            slept = Instant::now();
        }
    }
}

/// A retired worker's last work: what's left of the block it was given up in, as the caller has
/// stopped waiting for it and every other worker may be gone. A task of a later block goes back.
fn drain(sh: &Shared, index: usize) {
    let slot = &sh.workers[index - 1];
    // Pairs with `retire_stuck`'s store of the state the worker saw, after `retired_in`.
    std::sync::atomic::fence(Ordering::Acquire);
    debug_assert_eq!(slot.state.load(Ordering::Relaxed), RETIRED);
    let block = slot.retired_in.load(Ordering::Relaxed);
    let ours = || sh.block.load(Ordering::SeqCst) == block;
    while ours() && sh.remaining.0.load(Ordering::Acquire) > 0 && !sh.shutdown.load(Ordering::Acquire) {
        let Some(task) = sh.claim(index) else {
            thread::sleep(CALLER_NAP);
            continue;
        };
        // A task is only ready once its block has started, so if the count is still ours, so is
        // the task.
        if !ours() {
            sh.ready[task / 64].0.fetch_or(1 << (task % 64), Ordering::SeqCst);
            sh.wake(1);
            return;
        }
        slot.task.store(task as u32, Ordering::Relaxed);
        if !sh.run_task(task, index) {
            return;
        }
    }
}

/// Spins for at most `WORKER_SPIN`; true if a task became ready meanwhile.
fn spin_for_work(sh: &Shared) -> bool {
    let until = Instant::now() + WORKER_SPIN;
    loop {
        for _ in 0..16 {
            std::hint::spin_loop();
            if sh.ready.iter().any(|w| w.0.load(Ordering::Relaxed) != 0) {
                return true;
            }
        }
        if sh.shutdown.load(Ordering::Relaxed) || Instant::now() >= until {
            return false;
        }
    }
}

#[inline(never)]
fn touch_stack() {
    let mut page = [0u8; STACK_TOUCH];
    for b in page.iter_mut().step_by(4096) {
        unsafe { std::ptr::write_volatile(b, 1) };
    }
    std::hint::black_box(&page);
}

/// Real-time priority, the way cpal gets it for its audio callback (rtkit over D-Bus, at
/// `audio_thread_priority`'s priority), so a worker and the callback preempt each other evenly.
/// False if it was asked for and refused: a worker at normal priority beside a real-time callback
/// can be late whenever the system is busy, so it plays nothing, and the pool plays on with the
/// workers that were granted it (or the caller alone). Not asked for (the null device), or away
/// from Linux, where it isn't tried yet, any worker is fit.
fn promote(sh: &Shared, index: usize, opts: &PoolOptions) -> bool {
    // The session logs it, once every worker has started (`Session::sync_parallel`).
    let refused = |why: String| {
        let _ = sh.health.why.set(why);
        sh.health.refused.fetch_add(1, Ordering::Relaxed);
        false
    };
    if opts.refuse_for_checks & (1 << (index - 1)) != 0 {
        return refused("refused for a check".into());
    }
    let Some((frames, rate)) = opts.realtime else { return true };
    #[cfg(target_os = "linux")]
    {
        crate::audio_io::survive_slow_blocks();
        match audio_thread_priority::promote_current_thread_to_real_time(frames, rate) {
            Ok(_) => {
                sh.workers[index - 1].realtime.store(true, Ordering::Relaxed);
                sh.health.realtime.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(e) => refused(e.to_string()),
        }
    }
    // Workers aren't made real time here yet, so Automatic starts none, and a number chosen by
    // hand gets workers at normal priority.
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (frames, rate);
        true
    }
}

/// Not in `libc` (rust-lang/libc#1511).
#[cfg(target_os = "linux")]
const SCHED_RESET_ON_FORK: libc::c_int = 0x4000_0000;

/// Takes a stuck worker off real-time priority. RLIMIT_RTTIME only counts a thread's time under a
/// real-time policy, so the plug-in it's in can run on as long as it likes without the kernel
/// ending the app.
fn demote(tid: i32) {
    #[cfg(target_os = "linux")]
    if tid > 0 {
        let param = libc::sched_param { sched_priority: 0 };
        // SAFETY: a plain system call with a valid parameter. A thread may lower the policy of
        // another of its own process's threads, but not clear the reset-on-fork flag rtkit sets,
        // which is why it's kept.
        unsafe { libc::sched_setscheduler(tid, libc::SCHED_OTHER | SCHED_RESET_ON_FORK, &param) };
    }
    #[cfg(not(target_os = "linux"))]
    let _ = tid;
}

/// Real-time threads aren't placed by a core's speed: left alone they land on efficiency cores too,
/// where a heavy plug-in takes about twice as long.
fn keep_to(cpus: &[usize]) {
    #[cfg(target_os = "linux")]
    if !cpus.is_empty() {
        // SAFETY: a plain value struct, and a call about this thread only.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for &c in cpus {
                libc::CPU_SET(c, &mut set);
            }
            if libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
                log::warn!("a processing thread can't keep to the fast cores: {}", std::io::Error::last_os_error());
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cpus;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn pool(threads: usize) -> Pool {
        Pool::new(PoolOptions { threads, ..Default::default() })
    }

    /// Records the order tasks finished in, and which thread ran each.
    struct Log {
        graph: TaskGraph,
        done: Vec<AtomicU32>,
        clock: AtomicU32,
        thread: Vec<AtomicUsize>,
        early: AtomicU32,
    }

    impl Log {
        fn new(graph: TaskGraph) -> Log {
            let n = graph.len();
            Log {
                graph,
                done: (0..n).map(|_| AtomicU32::new(0)).collect(),
                clock: AtomicU32::new(1),
                thread: (0..n).map(|_| AtomicUsize::new(0)).collect(),
                early: AtomicU32::new(0),
            }
        }
    }

    impl Job for Log {
        fn run(&self, task: usize, thread: usize, _: &Call) {
            if self.graph.deps(task).iter().any(|&d| self.done[d as usize].load(Ordering::SeqCst) == 0) {
                self.early.fetch_add(1, Ordering::SeqCst);
            }
            self.thread[task].store(thread, Ordering::Relaxed);
            std::hint::black_box((0..200).sum::<u32>());
            self.done[task].store(self.clock.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
        }
    }

    fn wide_graph(n: u64) -> TaskGraph {
        // Everything feeds a bus and the bus feeds the last one, with a chain on the side.
        let ids: Vec<u64> = (0..n).collect();
        let mut edges: Vec<(u64, u64)> = (0..n - 2).map(|i| (i, n - 2)).collect();
        edges.push((n - 2, n - 1));
        edges.extend((0..8).map(|i| (i, i + 1)));
        TaskGraph::new(ids, &edges, |_| true).unwrap()
    }

    fn run_blocks(p: &Pool, g: &TaskGraph, job: &Arc<Log>, blocks: usize) {
        for _ in 0..blocks {
            for d in &job.done {
                d.store(0, Ordering::SeqCst);
            }
            let ptr: *const Log = Arc::as_ptr(job);
            unsafe { p.start(g, ptr) };
            assert!(p.finish(None));
            assert!(job.done.iter().all(|d| d.load(Ordering::SeqCst) != 0), "every task ran");
        }
    }

    #[test]
    fn every_task_runs_after_its_dependencies_whatever_the_thread_count() {
        for threads in [1, 2, 3, 8, 16] {
            let g = wide_graph(40);
            let job = Arc::new(Log::new(g.clone()));
            let p = pool(threads);
            run_blocks(&p, &g, &job, 300);
            assert_eq!(job.early.load(Ordering::SeqCst), 0, "{threads} threads");
        }
    }

    #[test]
    fn a_pool_with_only_the_caller_does_it_all() {
        let g = wide_graph(20);
        let job = Arc::new(Log::new(g.clone()));
        let p = pool(1);
        run_blocks(&p, &g, &job, 5);
        assert!(job.thread.iter().all(|t| t.load(Ordering::Relaxed) == 0));
    }

    #[test]
    fn large_graphs_use_every_ready_word() {
        let g = TaskGraph::new((0..MAX_TASKS as u64).collect(), &[], |_| true).unwrap();
        let job = Arc::new(Log::new(g.clone()));
        let p = pool(4);
        run_blocks(&p, &g, &job, 3);
    }

    struct Panicky {
        at: usize,
        ran: AtomicUsize,
    }

    impl Job for Panicky {
        fn run(&self, task: usize, _thread: usize, _: &Call) {
            if task == self.at {
                panic!("expected: a plug-in blowing up in a pool test");
            }
            self.ran.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_panic_is_counted_and_the_pool_carries_on() {
        let g = wide_graph(12);
        for threads in [1, 4] {
            let p = pool(threads);
            for at in [0, 5, 11] {
                let job = Arc::new(Panicky { at, ran: AtomicUsize::new(0) });
                let ptr: *const Panicky = Arc::as_ptr(&job);
                unsafe { p.start(&g, ptr) };
                assert!(p.finish(None), "dependants of the panicked task still run");
                assert_eq!(job.ran.load(Ordering::SeqCst), 11);
                assert_eq!(p.take_panics(), 1);
                assert_eq!(p.take_panics(), 0);
            }
        }
    }

    struct Sleepy {
        slow: AtomicUsize,
        ran: AtomicUsize,
    }

    impl Job for Sleepy {
        fn run(&self, _task: usize, thread: usize, _: &Call) {
            // Whichever worker gets a task first holds on to it, as if preempted. The caller gives
            // it the chance, as on a single core it might otherwise never run.
            if thread == 0 {
                thread::sleep(Duration::from_micros(200));
            } else if self.slow.swap(0, Ordering::SeqCst) == 1 {
                thread::sleep(Duration::from_millis(300));
            }
            self.ran.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_stuck_worker_cannot_hang_the_caller_and_everything_else_gets_done() {
        let ids: Vec<u64> = (0..64).collect();
        let g = TaskGraph::new(ids, &[], |_| true).unwrap();
        let job = Arc::new(Sleepy { slow: AtomicUsize::new(1), ran: AtomicUsize::new(0) });
        let p = pool(2);
        let ptr: *const Sleepy = Arc::as_ptr(&job);
        // Until the worker gets started and takes a task, the caller could do them all; tries go on
        // until it does.
        for _ in 0..200 {
            unsafe { p.start(&g, ptr) };
            let t = Instant::now();
            let done = p.finish(Some(Duration::from_millis(20)));
            assert!(t.elapsed() < Duration::from_millis(280), "only the time limit and a nap past it");
            if !done {
                assert!(!p.is_done());
                assert!(job.ran.load(Ordering::SeqCst) >= 63, "the caller took everything that was ready");
                while !p.is_done() {
                    thread::sleep(Duration::from_millis(1));
                }
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("the worker never got a task in 200 blocks");
    }

    #[test]
    fn workers_really_block_between_blocks() {
        let g = wide_graph(10);
        let job = Arc::new(Log::new(g.clone()));
        let p = pool(3);
        run_blocks(&p, &g, &job, 10);
        thread::sleep(Duration::from_millis(50));
        assert!(p.shared.workers.iter().all(|w| w.parked.load(Ordering::SeqCst)), "both parked");
        // And they wake for the next one.
        for _ in 0..100 {
            run_blocks(&p, &g, &job, 1);
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(job.early.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn workers_flush_denormals() {
        struct Denormal(AtomicU32);
        impl Job for Denormal {
            fn run(&self, _task: usize, thread: usize, _: &Call) {
                if thread != 0 && std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.5f32) > 0.0 {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        let g = TaskGraph::new((0..64).collect(), &[], |_| true).unwrap();
        let job = Arc::new(Denormal(AtomicU32::new(0)));
        let p = pool(4);
        let ptr: *const Denormal = Arc::as_ptr(&job);
        for _ in 0..200 {
            unsafe { p.start(&g, ptr) };
            assert!(p.finish(None));
        }
        assert_eq!(job.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stopping_a_pool_returns_at_once() {
        let t = Instant::now();
        drop(pool(8));
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    fn started(p: &Pool) -> HealthReport {
        let t = Instant::now();
        loop {
            let r = p.health().report();
            if r.starting == 0 {
                return r;
            }
            assert!(t.elapsed() < Duration::from_secs(10), "the workers never started");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn workers_refused_real_time_priority_play_nothing_and_the_rest_play_on() {
        let g = wide_graph(40);
        // Workers 1 and 3 of 4 are refused.
        let p = Pool::new(PoolOptions { threads: 5, refuse_for_checks: 0b0101, ..Default::default() });
        let r = started(&p);
        assert_eq!((r.threads, r.playing, r.refused, r.stuck), (5, 3, 2, 0));
        assert_eq!(p.playing(), 3);
        assert!(p.has_workers());
        assert_eq!(p.health().refusal(), Some("refused for a check"));
        let job = Arc::new(Log::new(g.clone()));
        run_blocks(&p, &g, &job, 300);
        assert_eq!(job.early.load(Ordering::SeqCst), 0);
        let mut ran = [false; 5];
        for t in &job.thread {
            ran[t.load(Ordering::Relaxed)] = true;
        }
        assert!(!ran[1] && !ran[3], "a refused worker took a task");
        // The refused threads have ended.
        assert!(p.joins.iter().filter(|(_, j)| j.is_finished()).count() >= 2);
    }

    #[test]
    fn with_every_worker_refused_the_pool_has_none() {
        let p = Pool::new(PoolOptions { threads: 3, refuse_for_checks: 0b11, ..Default::default() });
        let r = started(&p);
        assert_eq!((r.playing, r.refused), (1, 2));
        assert!(!p.has_workers());
        // It still works, the caller doing it all.
        let g = wide_graph(12);
        let job = Arc::new(Log::new(g.clone()));
        run_blocks(&p, &g, &job, 3);
        assert!(job.thread.iter().all(|t| t.load(Ordering::Relaxed) == 0));
    }

    #[test]
    fn a_worker_given_up_on_is_retired_and_the_others_play_on() {
        let ids: Vec<u64> = (0..64).collect();
        let g = TaskGraph::new(ids, &[], |_| true).unwrap();
        let job = Arc::new(Sleepy { slow: AtomicUsize::new(1), ran: AtomicUsize::new(0) });
        let p = pool(3);
        started(&p);
        let ptr: *const Sleepy = Arc::as_ptr(&job);
        let mut stuck = false;
        for _ in 0..200 {
            unsafe { p.start(&g, ptr) };
            if !p.finish(Some(Duration::from_millis(20))) {
                stuck = true;
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(stuck, "no worker ever got a task in 200 blocks");
        assert_eq!(p.retire_stuck(), 1, "the worker in the slow task");
        assert_eq!(p.retire_stuck(), 0, "and only once");
        let r = p.health().report();
        assert_eq!((r.playing, r.stuck), (2, 1));
        assert!(p.has_workers());
        let gone = p.shared.workers.iter().position(|w| w.state.load(Ordering::SeqCst) == RETIRED).unwrap() + 1;
        while !p.is_done() {
            thread::sleep(Duration::from_millis(1));
        }
        // Out of its task, it ends, and the pool plays on without it.
        let t = Instant::now();
        while !p.joins.iter().find(|(i, _)| *i == gone).unwrap().1.is_finished() {
            assert!(t.elapsed() < Duration::from_secs(5), "the retired worker didn't end");
            thread::sleep(Duration::from_millis(1));
        }
        let g = wide_graph(40);
        let log = Arc::new(Log::new(g.clone()));
        run_blocks(&p, &g, &log, 300);
        assert!(log.thread.iter().all(|t| t.load(Ordering::Relaxed) != gone));
        assert_eq!(log.early.load(Ordering::SeqCst), 0);
    }

    /// Holds up the first task a worker takes (of the first `to_hold` workers): first in the task
    /// itself while `in_task` is set, then out in a "plug-in" for `nap` and while `hold` is set.
    struct Hang {
        to_hold: AtomicU32,
        in_task: AtomicBool,
        hold: AtomicBool,
        nap: Duration,
        /// How long the caller takes over each task.
        caller_nap: Duration,
        /// The worker held up last, and its task.
        on: AtomicUsize,
        task: AtomicUsize,
        ran: Vec<AtomicU32>,
        /// Workers in the call now, out of it either way, and out of it to find they were left.
        in_call: AtomicU32,
        done: AtomicU32,
        back: AtomicU32,
    }

    impl Hang {
        fn new(n: usize) -> Hang {
            Hang {
                to_hold: AtomicU32::new(1),
                in_task: AtomicBool::new(false),
                hold: AtomicBool::new(true),
                nap: Duration::ZERO,
                caller_nap: Duration::from_micros(200),
                on: AtomicUsize::new(0),
                task: AtomicUsize::new(0),
                ran: (0..n).map(|_| AtomicU32::new(0)).collect(),
                in_call: AtomicU32::new(0),
                done: AtomicU32::new(0),
                back: AtomicU32::new(0),
            }
        }
    }

    impl Job for Hang {
        fn run(&self, task: usize, thread: usize, call: &Call) {
            if thread == 0 {
                thread::sleep(self.caller_nap);
            } else if self.to_hold.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
                self.on.store(thread, Ordering::SeqCst);
                self.task.store(task, Ordering::SeqCst);
                while self.in_task.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(1));
                }
                call.enter(77);
                self.in_call.fetch_add(1, Ordering::SeqCst);
                thread::sleep(self.nap);
                while self.hold.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(1));
                }
                let ours = call.leave();
                self.done.fetch_add(1, Ordering::SeqCst);
                if !ours {
                    self.back.fetch_add(1, Ordering::SeqCst);
                    return;
                }
            }
            self.ran[task].fetch_add(1, Ordering::SeqCst);
        }
    }

    fn until(what: &str, ok: impl Fn() -> bool) {
        let t = Instant::now();
        while !ok() {
            assert!(t.elapsed() < Duration::from_secs(10), "{what}");
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Half the tasks wait for one of the other half.
    fn pairs() -> TaskGraph {
        let edges: Vec<(u64, u64)> = (0..32).map(|i| (i, i + 32)).collect();
        TaskGraph::new((0..64).collect(), &edges, |_| true).unwrap()
    }

    /// Tries blocks of `job` until one is given up on with a worker in it.
    fn hang(p: &Pool, g: &TaskGraph, job: *const Hang) {
        for _ in 0..200 {
            // SAFETY: the test keeps the job alive for longer than the pool.
            for r in unsafe { &(*job).ran } {
                r.store(0, Ordering::SeqCst);
            }
            unsafe { p.start(g, job) };
            if !p.finish(Some(Duration::from_millis(20))) {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("no worker ever got a task in 200 blocks");
    }

    #[test]
    fn a_worker_left_in_a_plug_in_costs_its_task_and_what_waits_for_it_and_nothing_else() {
        let g = pairs();
        let job = Arc::new(Hang::new(64));
        let p = pool(3);
        started(&p);
        hang(&p, &g, Arc::as_ptr(&job));
        let mut left = Vec::new();
        assert_eq!(p.abandon(&g, Duration::ZERO, |thread, task, what| left.push((thread, task, what))), 1);
        let (on, s) = (job.on.load(Ordering::SeqCst), job.task.load(Ordering::SeqCst));
        assert_eq!(left, [(on, s, 77)]);
        assert!(p.is_left(on) && !p.gone(on));
        assert!(p.finish(Some(Duration::from_secs(5))), "the rest of the block ends without it");
        let out = |t: usize| t == s || (s < 32 && t == s + 32);
        for t in 0..64 {
            assert_eq!(p.dropped(t), out(t), "task {t}");
            assert_eq!(job.ran[t].load(Ordering::SeqCst), !out(t) as u32, "task {t} ran");
        }
        assert_eq!(p.retire_stuck(), 0, "it's retired already");
        let r = p.health().report();
        assert_eq!((r.playing, r.stuck), (2, 1));
        // The pool plays on without it.
        let log = Arc::new(Log::new(g.clone()));
        run_blocks(&p, &g, &log, 300);
        assert_eq!(log.early.load(Ordering::SeqCst), 0);
        assert!(log.thread.iter().all(|t| t.load(Ordering::Relaxed) != on));
        assert!((0..64).all(|t| !p.dropped(t)), "a new block has all its tasks");
        // The plug-in comes back: the thread lets go and ends, and the blocks after don't notice.
        job.hold.store(false, Ordering::SeqCst);
        let t = Instant::now();
        while !p.gone(on) || !p.joins.iter().find(|(i, _)| *i == on).unwrap().1.is_finished() {
            assert!(t.elapsed() < Duration::from_secs(5), "the thread didn't end");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(job.back.load(Ordering::SeqCst), 1);
        run_blocks(&p, &g, &log, 300);
        assert_eq!(log.early.load(Ordering::SeqCst), 0);
        assert_eq!(p.take_panics(), 0);
    }

    #[test]
    fn a_pool_with_a_worker_left_in_a_plug_in_stops_at_once() {
        let g = pairs();
        // The thread never comes back, so what it reads must outlive the test.
        let job: &'static Hang = Box::leak(Box::new(Hang::new(64)));
        let p = pool(2);
        started(&p);
        hang(&p, &g, job);
        assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| {}), 1);
        assert!(p.finish(Some(Duration::from_secs(5))));
        let t = Instant::now();
        drop(p);
        assert!(t.elapsed() < Duration::from_secs(5));
        assert_eq!(job.back.load(Ordering::SeqCst), 0);
    }

    /// Issue #8 again, but from inside the task: given up on before it went into the plug-in, the
    /// worker is retired, and once in a plug-in that doesn't return it's left there after all.
    #[test]
    fn a_worker_retired_in_its_task_is_left_once_it_is_out_in_a_plug_in() {
        let g = pairs();
        let mut h = Hang::new(64);
        h.in_task = AtomicBool::new(true);
        let job = Arc::new(h);
        let p = pool(2);
        started(&p);
        hang(&p, &g, Arc::as_ptr(&job));
        assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| panic!("left")), 0, "not in a plug-in yet");
        assert_eq!(p.retire_stuck(), 1);
        let (on, s) = (job.on.load(Ordering::SeqCst), job.task.load(Ordering::SeqCst));
        job.in_task.store(false, Ordering::SeqCst);
        until("it never went into the plug-in", || job.in_call.load(Ordering::SeqCst) == 1);
        assert_eq!(p.abandon(&g, Duration::from_secs(60), |_, _, _| panic!("left")), 0, "not in it for long enough");
        let mut left = Vec::new();
        assert_eq!(p.abandon(&g, Duration::ZERO, |thread, task, what| left.push((thread, task, what))), 1);
        assert_eq!(left, [(on, s, 77)]);
        assert!(p.finish(Some(Duration::from_secs(5))), "the rest of the block ends without it");
        let out = |t: usize| t == s || (s < 32 && t == s + 32);
        for t in 0..64 {
            assert_eq!((p.dropped(t), job.ran[t].load(Ordering::SeqCst)), (out(t), !out(t) as u32), "task {t}");
        }
        let r = p.health().report();
        assert_eq!((r.playing, r.stuck), (1, 1), "retired once");
        job.hold.store(false, Ordering::SeqCst);
        until("the thread didn't let go", || p.gone(on));
        assert_eq!(job.back.load(Ordering::SeqCst), 1);
        run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 20);
    }

    /// The plug-in returns at about the moment the caller gives up: whichever wins, the counts hold.
    #[test]
    fn a_plug_in_returning_as_the_caller_gives_up_is_either_left_or_not() {
        let g = pairs();
        let limit = Duration::from_millis(5);
        let mut left = 0;
        for k in 0..60u64 {
            let mut h = Hang::new(64);
            h.hold = AtomicBool::new(false);
            h.caller_nap = Duration::from_micros(20);
            // Either side of the limit.
            h.nap = Duration::from_micros(4000 + (k % 9) * 250);
            let job = Arc::new(h);
            let p = pool(2);
            started(&p);
            let ptr: *const Hang = Arc::as_ptr(&job);
            let mut done = true;
            for tries in 0.. {
                assert!(tries < 200, "the worker never got a task");
                for r in &job.ran {
                    r.store(0, Ordering::SeqCst);
                }
                unsafe { p.start(&g, ptr) };
                done = p.finish(Some(limit));
                if job.on.load(Ordering::SeqCst) != 0 {
                    break;
                }
                assert!(p.finish(None));
            }
            let n = if done { 0 } else { p.abandon(&g, Duration::ZERO, |_, _, _| {}) };
            assert!(done || p.finish(Some(Duration::from_secs(5))));
            until("the worker never came out of its call", || job.done.load(Ordering::SeqCst) == 1);
            let s = job.task.load(Ordering::SeqCst);
            let out = |t: usize| n == 1 && (t == s || (s < 32 && t == s + 32));
            for t in 0..64 {
                assert_eq!((p.dropped(t), job.ran[t].load(Ordering::SeqCst)), (out(t), !out(t) as u32), "try {k}, task {t}");
            }
            assert_eq!(job.back.load(Ordering::SeqCst), n, "try {k}");
            let on = job.on.load(Ordering::SeqCst);
            if n == 1 {
                until("the thread didn't let go", || p.gone(on));
            }
            left += n;
            run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 3);
        }
        eprintln!("{left} of 60 left");
    }

    #[test]
    fn two_workers_can_be_left_in_one_block_or_one_after_the_other() {
        let g = pairs();
        let mut h = Hang::new(64);
        h.to_hold = AtomicU32::new(2);
        let job = Arc::new(h);
        let p = pool(3);
        started(&p);
        let ptr: *const Hang = Arc::as_ptr(&job);
        let mut left = 0;
        for tries in 0.. {
            if left == 2 {
                break;
            }
            assert!(tries < 400, "only {left} left");
            unsafe { p.start(&g, ptr) };
            if p.finish(Some(Duration::from_millis(20))) {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            let n = p.abandon(&g, Duration::ZERO, |_, _, _| {});
            assert!(n >= 1);
            left += n;
            assert!(p.finish(Some(Duration::from_secs(5))));
        }
        let r = p.health().report();
        assert_eq!((r.playing, r.stuck), (1, 2));
        // The caller plays on alone.
        run_blocks(&p, &g, &Arc::new(Log::new(g.clone())), 20);
        job.hold.store(false, Ordering::SeqCst);
        until("a thread didn't let go", || p.gone(1) && p.gone(2));
        assert_eq!(job.back.load(Ordering::SeqCst), 2);
    }

    /// A worker held up in a task but not in a plug-in can't be left: that's for `retire_stuck`.
    #[test]
    fn a_worker_held_up_outside_a_plug_in_is_not_left() {
        let ids: Vec<u64> = (0..64).collect();
        let g = TaskGraph::new(ids, &[], |_| true).unwrap();
        let job = Arc::new(Sleepy { slow: AtomicUsize::new(1), ran: AtomicUsize::new(0) });
        let p = pool(2);
        started(&p);
        let ptr: *const Sleepy = Arc::as_ptr(&job);
        for _ in 0..200 {
            unsafe { p.start(&g, ptr) };
            if !p.finish(Some(Duration::from_millis(20))) {
                assert_eq!(p.abandon(&g, Duration::ZERO, |_, _, _| panic!("left")), 0);
                assert_eq!(p.retire_stuck(), 1);
                while !p.is_done() {
                    thread::sleep(Duration::from_millis(1));
                }
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("the worker never got a task in 200 blocks");
    }

    /// Spins without sleeping on the first worker that gets a task, as a plug-in in an endless loop
    /// would, until it's told to stop.
    struct Spinner {
        spin: AtomicBool,
        on: AtomicUsize,
    }

    impl Job for Spinner {
        fn run(&self, _task: usize, thread: usize, _: &Call) {
            if thread == 0 {
                thread::sleep(Duration::from_micros(200));
                return;
            }
            if self.on.compare_exchange(0, thread, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                let t = Instant::now();
                while self.spin.load(Ordering::Relaxed) && t.elapsed() < Duration::from_secs(2) {
                    std::hint::spin_loop();
                }
            }
        }
    }

    /// The real thing, where the system grants real-time priority: a real-time worker that never
    /// sleeps is killed with the whole process at RLIMIT_RTTIME's hard limit (200 ms), unless it's
    /// demoted first. A failure here ends the test binary with SIGKILL.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_real_time_worker_given_up_on_is_demoted_before_the_kernel_kills_it() {
        let p = Pool::new(PoolOptions { threads: 2, realtime: Some((512, 48000)), ..Default::default() });
        let r = started(&p);
        if r.realtime == 0 {
            eprintln!("skipped: this system doesn't grant real-time priority ({:?})", p.health().refusal());
            return;
        }
        let tid = p.shared.workers[0].tid.load(Ordering::SeqCst);
        // SAFETY: a plain query about a thread of this process.
        let policy = || unsafe { libc::sched_getscheduler(tid) } & !SCHED_RESET_ON_FORK;
        assert_ne!(policy(), libc::SCHED_OTHER, "promoted");
        let g = TaskGraph::new((0..64).collect(), &[], |_| true).unwrap();
        let job = Arc::new(Spinner { spin: AtomicBool::new(true), on: AtomicUsize::new(0) });
        let ptr: *const Spinner = Arc::as_ptr(&job);
        let mut stuck = false;
        for _ in 0..200 {
            unsafe { p.start(&g, ptr) };
            if !p.finish(Some(Duration::from_millis(40))) {
                stuck = true;
                break;
            }
        }
        assert!(stuck, "the worker never got a task");
        assert_eq!(p.retire_stuck(), 1);
        assert_eq!(policy(), libc::SCHED_OTHER, "demoted");
        // Past the hard limit, still spinning, and alive.
        thread::sleep(Duration::from_millis(400));
        job.spin.store(false, Ordering::Relaxed);
        while !p.is_done() {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(p.health().report().stuck, 1);
    }
}
