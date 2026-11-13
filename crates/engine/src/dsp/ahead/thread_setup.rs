//! What a render thread asks of the system (#500): real-time priority below the audio threads', on
//! Linux, and a watchdog so that the kernel's limit on real-time threads can't end the app.
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
//! whose few real-time threads per user are the pool's. Refused, or on macOS and Windows, render
//! threads stay at normal priority, as before.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Round-robin priority of the render threads: above every normal thread, below the audio
/// callback's and the pool's workers' (10, `dsp/pool/thread_setup.rs`).
#[cfg(target_os = "linux")]
const PRIORITY: libc::c_int = 1;
/// The watchdog's, above the render threads it watches.
#[cfg(target_os = "linux")]
const WATCHDOG_PRIORITY: libc::c_int = 2;
/// The longest a real-time render thread starts blocks without having slept.
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

/// The watchdog's thread, stopped and joined when dropped. Linux only: elsewhere render threads
/// aren't real time.
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

/// A render thread's side: made on the thread itself as it starts.
pub(super) struct RenderPriority<'a> {
    watch: &'a Watch,
    me: &'a Awake,
    realtime: bool,
    /// When it last slept for real.
    slept: Instant,
}

impl<'a> RenderPriority<'a> {
    /// Asks for real-time priority for the calling render thread, `k`.
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
    }

    /// Before a block: sleeps first if it has been awake too long, and takes real-time priority
    /// back once the watchdog's demotion is over.
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
