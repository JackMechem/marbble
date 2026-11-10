//! A small pool of real-time threads that run a [`TaskGraph`]'s tasks, with the thread that asked
//! (the audio callback) working too. Knows nothing about audio: see `parallel/` for what the
//! tasks do. A task's plug-in calls are tracked in `call.rs`; what a worker asks of the system
//! (stack, priority) is in `thread_setup.rs`, and the cores it keeps to in `audio_io::threads`.
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

mod call;
use call::{phase, GONE, LEFT};
pub use call::{Call, Job};
mod thread_setup;
use thread_setup::{demote, promote, touch_stack};

use std::cell::{Cell, UnsafeCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use super::graph::{TaskGraph, MAX_TASKS};
use crate::dsp::denormals::NoDenormals;

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
            if let Some(hook) = LEFT_HOOK.get().filter(|_| thread != 0) {
                hook();
            }
            call.state.store(GONE as u64, Ordering::SeqCst);
            return false;
        }
        if panicked {
            self.panics.fetch_add(1, Ordering::Relaxed);
        }
        self.done(task, graph);
        true
    }

    /// Counts a task out without running it, as if it had: what waits for it gets ready.
    fn pass(&self, task: usize) {
        // SAFETY: as in `run_task`.
        let graph = unsafe { &*(*self.job.get()).expect("a task without a job").1 };
        self.done(task, graph);
    }

    /// A task the thread owns is over: its dependants that now have all they wait for get ready.
    fn done(&self, task: usize, graph: &TaskGraph) {
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

/// Runs in a worker that comes back out of the plug-in it was left in, before it lets go of its
/// task and ends, for the same checks: from there it is no longer one of the pool's (it frees what
/// its thread was set up with as it ends).
static LEFT_HOOK: OnceLock<fn()> = OnceLock::new();

pub fn set_left_hook(f: fn()) {
    let _ = LEFT_HOOK.set(f);
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
        self.work(limit, true)
    }

    /// Like [`Pool::finish`], but the caller runs none of the tasks it takes: it counts them out as
    /// if they had run (whatever waits for them still gets ready), while the workers run theirs.
    /// For a block given up on, whose output is thrown away: on the way to its end the caller never
    /// calls into a plug-in, which could be another instance of the one that stopped responding,
    /// or be on a thread that isn't the audio callback (`Engine::settle`). True once the block is
    /// over; `Duration::ZERO` takes what's ready and doesn't wait.
    pub fn pass_over(&self, limit: Duration) -> bool {
        self.work(Some(limit), false)
    }

    fn work(&self, limit: Option<Duration>, run: bool) -> bool {
        let sh = &*self.shared;
        let started = Instant::now();
        let mut idle_since = started;
        let mut spins = 0u32;
        loop {
            if let Some(task) = sh.claim(0) {
                if run {
                    // The caller is never left in a plug-in.
                    sh.run_task(task, 0);
                    idle_since = Instant::now();
                } else {
                    sh.pass(task);
                }
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
    if let Err(e) = crate::audio_io::threads::keep_to(&opts.cpus) {
        log::warn!("a processing thread can't keep to the fast cores: {e}");
    }
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

#[cfg(test)]
mod tests;
