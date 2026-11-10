//! What a worker thread asks of the system: its stack touched before the first block, real-time
//! priority as cpal gets it for the callback (and losing it again when stuck), and the CPUs it
//! keeps to.

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

/// Real-time priority, the way cpal gets it for its audio callback (rtkit over D-Bus, at
/// `audio_thread_priority`'s priority), so a worker and the callback preempt each other evenly.
/// False if it was asked for and refused: a worker at normal priority beside a real-time callback
/// can be late whenever the system is busy, so it plays nothing, and the pool plays on with the
/// workers that were granted it (or the caller alone). Not asked for (the null device), or away
/// from Linux, where it isn't tried yet, any worker is fit.
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
pub(super) const SCHED_RESET_ON_FORK: libc::c_int = 0x4000_0000;

/// Takes a stuck worker off real-time priority. RLIMIT_RTTIME only counts a thread's time under a
/// real-time policy, so the plug-in it's in can run on as long as it likes without the kernel
/// ending the app.
pub(super) fn demote(tid: i32) {
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
pub(super) fn keep_to(cpus: &[usize]) {
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
