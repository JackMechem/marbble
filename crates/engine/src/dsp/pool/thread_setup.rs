//! What a worker thread asks of the system: its stack touched before the first block, and real-time
//! priority as cpal gets it for the callback (and losing it again when stuck). The CPUs it keeps
//! to are `audio_io::threads::keep_to`'s.

use std::sync::atomic::Ordering;

use super::{PoolOptions, Shared, STACK_TOUCH};

#[inline(never)]
pub(super) fn touch_stack() {
    let mut page = [0u8; STACK_TOUCH];
    for b in page.iter_mut().step_by(4096) {
        unsafe { std::ptr::write_volatile(b, 1) };
    }
    std::hint::black_box(&page);
}

/// Real-time priority, the way the system gives it to the audio callback, so a worker and the
/// callback preempt each other evenly: on Linux as cpal gets it (rtkit over D-Bus, at
/// `audio_thread_priority`'s priority), on macOS a Mach time-constraint policy for one buffer
/// ([`macos::promote`]). False if it was asked for and refused: a worker at normal priority beside
/// a real-time callback can be late whenever the system is busy, so it plays nothing, and the pool
/// plays on with the workers that were granted it (or the caller alone). Not asked for (the null
/// device), or elsewhere (Windows), where it isn't tried yet, any worker is fit.
pub(super) fn promote(sh: &Shared, index: usize, opts: &PoolOptions) -> bool {
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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        #[cfg(target_os = "linux")]
        let granted = {
            crate::audio_io::survive_slow_blocks();
            audio_thread_priority::promote_current_thread_to_real_time(frames, rate).map(drop).map_err(|e| e.to_string())
        };
        #[cfg(target_os = "macos")]
        let granted = macos::promote(frames, rate, opts.io_thread.policy);
        match granted {
            Ok(()) => {
                sh.workers[index - 1].realtime.store(true, Ordering::Relaxed);
                sh.health.realtime.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(e) => refused(e),
        }
    }
    // Workers aren't made real time here yet, so Automatic starts none, and a number chosen by
    // hand gets workers at normal priority.
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (frames, rate);
        true
    }
}

/// Joins the audio device's IO workgroup (macOS), when the worker was promoted for it, until the
/// result is dropped on this thread. A refusal is logged, and the worker plays on without it.
#[cfg(target_os = "macos")]
pub(super) fn join_workgroup(sh: &Shared, opts: &PoolOptions) -> Option<crate::audio_io::coreaudio::Joined> {
    opts.realtime?;
    match opts.io_thread.workgroup.as_ref()?.join() {
        Ok(j) => {
            sh.health.joined.fetch_add(1, Ordering::Relaxed);
            Some(j)
        }
        Err(e) => {
            log::warn!("a processing thread couldn't join the audio device's workgroup (errno {e}); it plays on outside it");
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn join_workgroup(_: &Shared, _: &PoolOptions) -> Option<()> {
    None
}

/// The system's id for the calling thread, by which another thread can demote it: the kernel's
/// thread id on Linux, the thread's Mach port on macOS, 0 elsewhere.
pub(super) fn os_thread() -> u32 {
    // SAFETY: a plain system call about this thread.
    #[cfg(target_os = "linux")]
    let id = unsafe { libc::gettid() } as u32;
    // SAFETY: plain calls about this thread. The port name is the thread's for as long as it lives
    // (no reference is taken, so there's none to release).
    #[cfg(target_os = "macos")]
    let id = unsafe { libc::pthread_mach_thread_np(libc::pthread_self()) };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let id = 0;
    id
}

/// Not in `libc` (rust-lang/libc#1511).
#[cfg(target_os = "linux")]
pub(super) const SCHED_RESET_ON_FORK: libc::c_int = 0x4000_0000;

/// Takes a stuck worker off real-time priority, given its [`os_thread`]. On Linux, RLIMIT_RTTIME
/// only counts a thread's time under a real-time policy, so the plug-in it's in can run on as long
/// as it likes without the kernel ending the app. On macOS, a time-constraint thread that never
/// yields would otherwise hold its core at real-time priority until the scheduler's fail-safe
/// catches it. One system call (a Mach trap on macOS), which neither blocks nor allocates.
pub(super) fn demote(thread: u32) {
    #[cfg(target_os = "linux")]
    if thread > 0 {
        let param = libc::sched_param { sched_priority: 0 };
        // SAFETY: a plain system call with a valid parameter. A thread may lower the policy of
        // another of its own process's threads, but not clear the reset-on-fork flag rtkit sets,
        // which is why it's kept.
        unsafe { libc::sched_setscheduler(thread as i32, libc::SCHED_OTHER | SCHED_RESET_ON_FORK, &param) };
    }
    #[cfg(target_os = "macos")]
    if thread > 0 {
        macos::demote(thread);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = thread;
}

#[cfg(target_os = "macos")]
pub(super) mod macos {
    use crate::audio_io::coreaudio::TimeConstraint;
    use mach2::kern_return::KERN_SUCCESS;
    use mach2::mach_time::{mach_timebase_info, mach_timebase_info_data_t};
    use mach2::thread_policy::{
        thread_policy_set, thread_policy_t, thread_time_constraint_policy_data_t, THREAD_STANDARD_POLICY, THREAD_TIME_CONSTRAINT_POLICY,
        THREAD_TIME_CONSTRAINT_POLICY_COUNT,
    };

    /// The scheduler refuses a computation outside these (xnu's `min_rt_quantum` and
    /// `max_rt_quantum`, 50 µs and 50 ms).
    const MIN_COMPUTATION_NS: u64 = 50_000;
    const MAX_COMPUTATION_NS: u64 = 50_000_000;

    /// The time-constraint policy for a worker playing `frames` at `rate`: every buffer (`period`),
    /// up to half of it in work (`computation`), done within the buffer (`constraint`), and
    /// preemptible, as Core Audio asks for its own IO thread. For when the IO thread's own values
    /// aren't known (`IoThread::policy`): checks, or a device whose first callback came late.
    pub fn constraint_for(frames: u32, rate: u32) -> Option<thread_time_constraint_policy_data_t> {
        let mut tb = mach_timebase_info_data_t { numer: 0, denom: 0 };
        // SAFETY: fills in the struct it's given.
        if unsafe { mach_timebase_info(&mut tb) } != KERN_SUCCESS || tb.numer == 0 || tb.denom == 0 {
            return None;
        }
        let period_ns = (frames.max(1) as u64 * 1_000_000_000 / rate.max(1) as u64).max(MIN_COMPUTATION_NS);
        let computation_ns = (period_ns / 2).clamp(MIN_COMPUTATION_NS, MAX_COMPUTATION_NS);
        let ticks = |ns: u64| u32::try_from(ns * tb.denom as u64 / tb.numer as u64).unwrap_or(u32::MAX);
        let period = ticks(period_ns);
        Some(thread_time_constraint_policy_data_t {
            period,
            computation: ticks(computation_ns),
            // At least the computation, which a buffer shorter than 100 µs could undercut.
            constraint: period.max(ticks(computation_ns)),
            preemptible: 1,
        })
    }

    /// Puts the calling thread under the IO thread's time-constraint policy, `io`, so that it and
    /// the workers are scheduled alike, or failing that under [`constraint_for`]'s.
    pub fn promote(frames: u32, rate: u32, io: Option<TimeConstraint>) -> Result<(), String> {
        let copied =
            io.map(|p| thread_time_constraint_policy_data_t { period: p.period, computation: p.computation, constraint: p.constraint, preemptible: 1 });
        if copied.is_some_and(|p| set(p) == KERN_SUCCESS) {
            return Ok(());
        }
        let r = set(constraint_for(frames, rate).ok_or("no Mach timebase")?);
        if r == KERN_SUCCESS {
            Ok(())
        } else {
            Err(format!("thread_policy_set refused the time-constraint policy (kern_return_t {r})"))
        }
    }

    fn set(mut policy: thread_time_constraint_policy_data_t) -> mach2::kern_return::kern_return_t {
        // SAFETY: a valid policy of the flavour and size given, for this thread's own port.
        unsafe {
            thread_policy_set(super::os_thread(), THREAD_TIME_CONSTRAINT_POLICY, &mut policy as *mut _ as thread_policy_t, THREAD_TIME_CONSTRAINT_POLICY_COUNT)
        }
    }

    /// Back to the standard (timesharing) policy. The thread may be another of this process's.
    pub fn demote(thread: u32) {
        // SAFETY: the standard policy takes no data. A port that is no longer a thread only makes
        // the call fail.
        unsafe { thread_policy_set(thread, THREAD_STANDARD_POLICY, std::ptr::null_mut(), 0) };
    }
}
