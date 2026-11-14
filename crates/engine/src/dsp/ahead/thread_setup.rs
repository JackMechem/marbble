//! What a render thread asks of the system (#500, #511): a priority above every normal thread and
//! below the audio threads', on Linux and macOS, and on Linux a watchdog so that the kernel's
//! limit on real-time threads can't end the app.
//!
//! At normal priority render threads starve when the machine is busy. With 16 busy loops beside the
//! owner's project (12 TONE3000s, 1024 frames), `ahead_probe` counted hundreds of missed blocks
//! a run at normal priority, about as many at nice -10 or kept to the performance cores, and none at
//! real-time priority (see the pull request for the table). Round-robin at [`PRIORITY`] puts them
//! above every normal thread and below the callback and the pool's workers (priority 10), so they
//! never hold up a block that's due.
//!
//! A real-time thread that runs 200 ms without sleeping is killed with the whole process
//! (RLIMIT_RTTIME's hard limit, `audio_io::survive_slow_blocks`). A render thread sleeps whenever
//! its strips are a lead ahead, but while it catches up, or inside a slow plug-in, it may not:
//!
//! - Before each block, a thread awake for [`GUARD`] sleeps for [`GUARD_NAP`] first.
//! - The watchdog, a thread of its own above the render threads, demotes one that has been awake
//!   for [`DEMOTE_AFTER`] (a plug-in that takes that long over one block). It's promoted again
//!   once it's out, and sleeps before it renders on, so the kernel counts from zero.
//!
//! Only asked of the kernel (RLIMIT_RTPRIO, as `dsp/pool/thread_setup.rs` does), never of rtkit,
//! whose few real-time threads per user are the pool's.
//!
//! On macOS (#511) they get a fixed priority, round-robin at [`PRIORITY`] 47 (the most
//! `pthread_setschedparam` gives): the level of the foreground app's own interface threads, above
//! every default and lower QoS thread, and far below the callback's and the workers'
//! time-constraint policy (priority 97). They don't join the device's workgroup or take the
//! time-constraint policy: they aren't bound to the callback's deadline. On an M1 beside 16 busy
//! loops, a synthetic project of 64 tracks of four built-in effects (about three cores of
//! rendering, 60 s of playback) missed 9,000 to 12,600 blocks a run at normal priority or with QoS
//! user-interactive, and 6 to 13 at a fixed priority (see the pull request for the table). Mach's
//! precedence policy at importance 63 missed as few, but overloaded the callback at the start in two
//! runs of three. Threads a plug-in starts from a render thread don't inherit it. The kernel's
//! fail-safe takes a fixed-priority thread that has run about a second without blocking down to
//! timesharing for about two seconds, then gives it back: that is the watchdog there, and nothing is
//! killed. The guard's nap keeps a render thread that's catching up from tripping it; one held in a
//! plug-in that long is demoted by the kernel.
//!
//! Refused, and on Windows, render threads stay at normal priority, as before.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Round-robin priority of the render threads: above every normal thread, below the audio
/// callback's and the pool's workers' (10, `dsp/pool/thread_setup.rs`).
#[cfg(target_os = "linux")]
const PRIORITY: libc::c_int = 1;
/// The render threads' fixed priority on macOS: `sched_get_priority_max(SCHED_RR)`, the foreground
/// app's interface threads' level, below the time-constraint audio threads (97).
#[cfg(target_os = "macos")]
const PRIORITY: libc::c_int = 47;
/// The watchdog's, above the render threads it watches.
#[cfg(target_os = "linux")]
const WATCHDOG_PRIORITY: libc::c_int = 2;
/// The longest a real-time render thread starts blocks without having slept: far inside RLIMIT_RTTIME
/// on Linux and the fail-safe's second on macOS.
const GUARD: Duration = Duration::from_millis(30);
const GUARD_NAP: Duration = Duration::from_micros(200);
/// How long a render thread may be awake before the watchdog demotes it: far enough inside
/// RLIMIT_RTTIME's 200 ms for the watchdog to be late.
#[cfg(target_os = "linux")]
const DEMOTE_AFTER: Duration = Duration::from_millis(100);
#[cfg(target_os = "linux")]
const WATCH_EVERY: Duration = Duration::from_millis(10);
/// A park that took at least this long blocked for real (one that finds a wake already waiting
/// returns in far less).
const TRUE_SLEEP: Duration = Duration::from_micros(10);
/// [`Awake::since`] of a thread that is asleep, or not real time.
const ASLEEP: u64 = 0;

/// One render thread as the watchdog sees it.
#[derive(Default)]
struct Awake {
    /// The kernel's id for it, once it's real time.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    tid: AtomicU32,
    /// Since when it has run without sleeping, in µs from [`Watch::epoch`] (plus one, so never
    /// [`ASLEEP`]).
    since: AtomicU64,
    /// The watchdog demoted it: it promotes itself again when it's out of the plug-in.
    demoted: AtomicBool,
}

/// What the render threads and their watchdog share.
pub(super) struct Watch {
    epoch: Instant,
    threads: Box<[Awake]>,
    stop: AtomicBool,
}

impl Watch {
    pub(super) fn new(threads: usize) -> Watch {
        Watch { epoch: Instant::now(), threads: (0..threads).map(|_| Awake::default()).collect(), stop: AtomicBool::new(false) }
    }

    fn now(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64 + 1
    }
}

/// The watchdog's thread, stopped and joined when dropped. Linux only: on macOS the kernel's
/// fail-safe demotes a render thread stuck in a plug-in, and on Windows they aren't raised.
pub(super) struct Watchdog {
    watch: Arc<Watch>,
    handle: Option<JoinHandle<()>>,
}

impl Watchdog {
    pub(super) fn start(watch: &Arc<Watch>) -> Watchdog {
        #[cfg(target_os = "linux")]
        let handle = {
            let w = watch.clone();
            std::thread::Builder::new().name("marbble-ahead-watch".into()).spawn(move || watchdog(&w)).ok()
        };
        #[cfg(not(target_os = "linux"))]
        let handle = None;
        Watchdog { watch: watch.clone(), handle }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.watch.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn watchdog(w: &Watch) {
    // Refused real-time priority, it couldn't preempt a busy render thread; but then the render
    // threads, asking the same way, are refused it too.
    if set_policy(0, libc::SCHED_RR, WATCHDOG_PRIORITY).is_err() {
        return;
    }
    while !w.stop.load(Ordering::SeqCst) {
        std::thread::sleep(WATCH_EVERY);
        let now = w.now();
        for t in w.threads.iter() {
            let since = t.since.load(Ordering::SeqCst);
            if since != ASLEEP && now.saturating_sub(since) >= DEMOTE_AFTER.as_micros() as u64 && !t.demoted.swap(true, Ordering::SeqCst) {
                let _ = set_policy(t.tid.load(Ordering::SeqCst) as i32, libc::SCHED_OTHER, 0);
                log::warn!("a render thread was {} ms in one block of a plug-in: at normal priority until it's out", DEMOTE_AFTER.as_millis());
            }
        }
    }
}

/// Sets a thread's (0: this one's) scheduling policy, not inherited by threads it starts (as
/// rtkit sets it: a plug-in's own threads stay normal).
#[cfg(target_os = "linux")]
fn set_policy(tid: i32, policy: libc::c_int, priority: libc::c_int) -> std::io::Result<()> {
    use crate::dsp::pool::SCHED_RESET_ON_FORK;
    let param = libc::sched_param { sched_priority: priority };
    // SAFETY: a plain system call about a thread of this process, with a valid parameter.
    if unsafe { libc::sched_setscheduler(tid, policy | SCHED_RESET_ON_FORK, &param) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Sets this thread's fixed priority (macOS), round-robin at [`PRIORITY`].
#[cfg(target_os = "macos")]
fn set_fixed() -> std::io::Result<()> {
    // SAFETY: zeroed is a valid `sched_param`; then a plain call about this thread.
    let mut param: libc::sched_param = unsafe { std::mem::zeroed() };
    param.sched_priority = PRIORITY;
    match unsafe { libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_RR, &param) } {
        0 => Ok(()),
        e => Err(std::io::Error::from_raw_os_error(e)),
    }
}

/// A render thread's side: made on the thread itself as it starts.
pub(super) struct RenderPriority<'a> {
    watch: &'a Watch,
    me: &'a Awake,
    realtime: bool,
    /// When it last slept for real.
    slept: Instant,
}

impl<'a> RenderPriority<'a> {
    /// Asks for real-time (on macOS fixed) priority for the calling render thread, `k`.
    pub(super) fn start(watch: &'a Watch, k: usize) -> RenderPriority<'a> {
        let mut p = RenderPriority { watch, me: &watch.threads[k], realtime: false, slept: Instant::now() };
        p.promote();
        if k == 0 {
            log::info!("render threads: {}", if p.realtime { "real time" } else { "normal priority" });
        }
        p
    }

    fn promote(&mut self) {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: a plain system call about this thread.
            self.me.tid.store(unsafe { libc::gettid() } as u32, Ordering::SeqCst);
            self.realtime = set_policy(0, libc::SCHED_RR, PRIORITY).is_ok();
            if self.realtime {
                // Asleep for a moment, so that the kernel's count starts afresh.
                self.sleep(GUARD_NAP);
            }
        }
        #[cfg(target_os = "macos")]
        {
            self.realtime = set_fixed().is_ok();
        }
    }

    /// Before a block: sleeps first if it has been awake too long, and takes real-time priority
    /// back once the watchdog's demotion is over (Linux; on macOS the kernel gives it back).
    pub(super) fn before_block(&mut self) {
        if self.me.demoted.load(Ordering::SeqCst) {
            self.me.since.store(ASLEEP, Ordering::SeqCst);
            self.me.demoted.store(false, Ordering::SeqCst);
            self.promote();
        } else if self.realtime && self.slept.elapsed() >= GUARD {
            self.sleep(GUARD_NAP);
        }
    }

    fn sleep(&mut self, d: Duration) {
        self.me.since.store(ASLEEP, Ordering::SeqCst);
        std::thread::sleep(d);
        self.woke();
    }

    fn woke(&mut self) {
        self.slept = Instant::now();
        if self.realtime {
            self.me.since.store(self.watch.now(), Ordering::SeqCst);
        }
    }

    /// Parks with nothing to do, until woken or `timeout`.
    pub(super) fn park(&mut self, timeout: Duration) {
        let started = Instant::now();
        let since = self.me.since.swap(ASLEEP, Ordering::SeqCst);
        std::thread::park_timeout(timeout);
        if started.elapsed() >= TRUE_SLEEP {
            self.woke();
        } else {
            self.me.since.store(since, Ordering::SeqCst);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn policy() -> libc::c_int {
        // SAFETY: a plain query about this thread.
        let policy = unsafe { libc::sched_getscheduler(0) };
        policy & !crate::dsp::pool::SCHED_RESET_ON_FORK
    }

    fn spin(d: Duration) {
        let t = Instant::now();
        while t.elapsed() < d {
            std::hint::spin_loop();
        }
    }

    /// Where the system grants real-time priority: a render thread sleeps before a block once it
    /// has been awake a while, and one held in a plug-in is demoted before RLIMIT_RTTIME's 200 ms,
    /// then promoted again once it's out.
    #[test]
    fn a_real_time_render_thread_never_runs_long_without_sleeping() {
        let watch = Arc::new(Watch::new(1));
        let _dog = Watchdog::start(&watch);
        let w = watch.clone();
        let seen = std::thread::spawn(move || {
            let mut p = RenderPriority::start(&w, 0);
            if !p.realtime {
                return None;
            }
            let promoted = policy();
            // Blocks that keep it busy past the guard: it naps before the next.
            spin(GUARD + Duration::from_millis(5));
            p.before_block();
            let napped = p.slept.elapsed() < Duration::from_millis(5);
            // A plug-in that takes 150 ms over one block.
            spin(Duration::from_millis(150));
            let in_plugin = policy();
            p.before_block();
            Some((promoted, napped, in_plugin, policy()))
        })
        .join()
        .unwrap();
        let Some((promoted, napped, in_plugin, after)) = seen else {
            eprintln!("skipped: this system doesn't grant real-time priority");
            return;
        };
        assert_eq!(promoted, libc::SCHED_RR);
        assert!(napped, "slept before the block");
        assert_eq!(in_plugin, libc::SCHED_OTHER, "demoted");
        assert_eq!(after, libc::SCHED_RR, "promoted again");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::*;

    /// Mach's `POLICY_TIMESHARE` and `POLICY_RR`.
    const TIMESHARE: libc::c_int = 1;
    const RR: libc::c_int = 2;

    /// This thread's policy as the scheduler has it now: the fail-safe's demotion shows here, not in
    /// `pthread_getschedparam`.
    fn policy() -> libc::c_int {
        // SAFETY: plain queries about this thread, into a zeroed struct of the size given.
        unsafe {
            let mut info: libc::thread_basic_info = std::mem::zeroed();
            let mut n = libc::THREAD_BASIC_INFO_COUNT;
            libc::thread_info(
                libc::pthread_mach_thread_np(libc::pthread_self()),
                libc::THREAD_BASIC_INFO as u32,
                &mut info as *mut _ as libc::thread_info_t,
                &mut n,
            );
            info.policy
        }
    }

    /// A render thread gets a fixed priority, which a thread it starts (a plug-in's) doesn't
    /// inherit, and one kept busy past the fail-safe's second, starting a block every few
    /// milliseconds, naps often enough that the kernel never takes it away.
    #[test]
    fn a_render_thread_keeps_its_fixed_priority_while_busy() {
        let watch = Arc::new(Watch::new(1));
        let _dog = Watchdog::start(&watch);
        let w = watch.clone();
        let (raised, child, busy) = std::thread::spawn(move || {
            let mut p = RenderPriority::start(&w, 0);
            let raised = p.realtime.then(policy);
            let child = std::thread::spawn(policy).join().unwrap();
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(1500) {
                let block = Instant::now();
                while block.elapsed() < Duration::from_millis(2) {
                    std::hint::spin_loop();
                }
                p.before_block();
            }
            (raised, child, policy())
        })
        .join()
        .unwrap();
        let Some(raised) = raised else {
            eprintln!("skipped: this system doesn't grant a fixed priority");
            return;
        };
        assert_eq!(raised, RR);
        assert_eq!(child, TIMESHARE, "a plug-in's thread stays normal");
        assert_eq!(busy, RR, "not demoted by the fail-safe");
    }
}
