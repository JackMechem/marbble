//! Settings › Audio › Processing Threads: how many threads play tracks, what Automatic means on
//! this machine, and keeping the audio threads to its fast cores (`keep_to`).
//!
//! Measured on an i7-13700KF (8 performance and 8 efficiency cores), `Engine::process` at real-time
//! pace with real-time workers (`parallel_bench` measures the same way): wide projects (24
//! CHOWTapeModels, 16 Dragonfly reverbs, 12 tape models in three summing stacks) ran close to the
//! thread count faster, down to a fifth of the one-thread block time at 8 threads. Going from 6 to 8
//! threads helped them another 10 to 25 % and a five-track project not at all, so Automatic first
//! stopped at 6. Neural amp models gain far more, hyperthreads included (#440): the owner's project
//! of twelve TONE3000 guitars at 512 frames (`stress_check --project` on a dummy JACK server) had
//! every block late at 6 threads (p50 113 %), 771 of 1773 at 8, none at 10 (66 %), and played at
//! 60 % from 12 threads up to 16, one guitar's own time being the floor. A worker only wakes when a
//! task is ready for it (`dsp/pool/`), so threads a project can't use cost it nothing: on Linux,
//! Automatic fills every hardware thread of the performance cores but one core's. Keeping workers to the performance
//! cores took 24 CHOWTapeModels at 128 frames from a mean of 340 µs to 270 µs, and five TONE3000s'
//! 99th percentile from 1510 µs to about 900 µs; the scheduler places real-time threads without
//! regard to a core's speed. A light project stays on the callback (`ENGAGE_LOAD` in
//! `dsp/parallel/`), so it pays nothing for the pool.
//!
//! The callback keeps to the performance cores too (issue #36). A real-time thread stays on the core
//! it first ran on, and a SCHED_FIFO thread playing eight TONE3000s sat on an efficiency core for
//! a whole run in one of three tries: there a block took 4.3 ms on average against 2.7 ms, and the
//! plug-in's own heavy block every 8192 frames 14.5 ms against 9.4, past a 512-frame buffer's
//! 10.7 ms (vst3-host's `editor_load`).
//!
//! On macOS (issue #59) Automatic is one thread per performance core: the efficiency cores run
//! everything else, so no performance core needs leaving free, and the output device's IO workgroup
//! (`coreaudio.rs`), which the workers join, suggests as many (`os_workgroup_max_parallel_threads`,
//! 4 on an M1); Automatic never goes past what it suggests. Measured on an M1 (4 performance and 4
//! efficiency cores) with `stress_check` (PR #386): 32 instrument and 8 audio tracks of built-in
//! plug-ins (12 and 4 for the second row) on the built-in speakers, real-time workers in the
//! workgroup, the median block load in % of the deadline (p99 within 4 points). The 40 tracks came
//! within 2 points of each other at 128, 256 and 512 frames and in a second run; other agents'
//! builds kept the load average at 10 to 17 throughout.
//!
//! | threads        | 1    | 2  | 3  | 4  | 5  | 6  |
//! |----------------|------|----|----|----|----|----|
//! | 40 tracks      | 121* | 64 | 45 | 36 | 38 | 37 |
//! | 16 tracks, 128 | 47   | 26 | 21 | 17 | 17 | 17 |
//!
//! (* over its deadline: every block late.) A fourth thread took the heavy project from 45 % to
//! 36 %; a fifth or sixth, which must share a performance core or run on an efficiency one,
//! gained nothing and cost a little. An Intel Mac has one kind of core and leaves one free, as
//! Linux does.

/// The setting's value for Automatic.
pub const AUTOMATIC: u32 = 0;
/// The most Automatic ever picks: as far as has been measured (#440).
pub const AUTOMATIC_MAX: usize = 16;
/// The most the setting offers.
pub const MANUAL_MAX: usize = 16;

/// A fresh install's setting.
pub fn default_setting() -> u32 {
    AUTOMATIC
}

/// The CPUs of this machine, as far as they matter here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Topology {
    /// Logical CPUs we may use.
    pub logical: usize,
    /// Physical cores of the fastest kind (on a machine with performance and efficiency cores, the
    /// performance ones).
    pub fast_cores: usize,
    /// The logical CPUs of those cores, on a machine with two kinds of core. Empty on a uniform
    /// machine, where it can't be told, and on macOS (which has no affinity).
    pub fast: Vec<usize>,
    /// Hardware threads of those cores (both of a hyper-threaded core's), which Automatic fills on
    /// Linux. 0 where it isn't counted (macOS, Windows).
    pub fast_threads: usize,
    /// Physical cores of a slower kind (efficiency cores) that the rest of the system can run on, so
    /// that Automatic needn't leave a fast core free for it. Counted on macOS only, where that was
    /// measured; 0 elsewhere.
    pub others: usize,
}

impl Topology {
    pub fn detect() -> Topology {
        let logical = std::thread::available_parallelism().map_or(1, |n| n.get());
        #[cfg(target_os = "linux")]
        {
            let read = |p: &str| std::fs::read_to_string(p).ok();
            let sibling = |c: usize| read(&format!("/sys/devices/system/cpu/cpu{c}/topology/thread_siblings_list"));
            detect_linux(logical, read("/sys/devices/cpu_core/cpus"), read("/sys/devices/system/cpu/online"), &sibling)
        }
        #[cfg(target_os = "macos")]
        {
            let (perf0, perf1) = (sysctl_count(c"hw.perflevel0.physicalcpu"), sysctl_count(c"hw.perflevel1.physicalcpu"));
            detect_macos(logical, perf0, perf1, sysctl_count(c"hw.physicalcpu"))
        }
        // Workers at normal priority beside a real-time callback would cost more than they bring
        // (Windows, issue #59): a number can be chosen, but Automatic stays on one thread.
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        Topology { logical, fast_cores: 1, ..Default::default() }
    }
}

/// A whole-number `sysctl`, such as `hw.physicalcpu`. None where the system doesn't have it
/// (`hw.perflevel0.*` is Apple Silicon on macOS 12 and later).
#[cfg(target_os = "macos")]
fn sysctl_count(name: &std::ffi::CStr) -> Option<usize> {
    let mut v: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>();
    // SAFETY: a NUL-terminated name and an out-buffer of the size given.
    let r = unsafe { libc::sysctlbyname(name.as_ptr(), (&raw mut v).cast(), &mut len, std::ptr::null_mut(), 0) };
    (r == 0 && len == std::mem::size_of::<libc::c_int>() && v > 0).then_some(v as usize)
}

/// macOS: the fast cores are the performance cores (`hw.perflevel0.physicalcpu`) on Apple Silicon,
/// the efficiency cores (`hw.perflevel1.physicalcpu`) the others; on an Intel Mac or a system
/// without perflevels, all physical cores (`hw.physicalcpu`) and no others. No list of them: macOS
/// has no affinity, and the workers reach the performance cores by their time-constraint policy and
/// by joining the device's workgroup (`coreaudio.rs`).
#[allow(dead_code)]
fn detect_macos(logical: usize, perf0: Option<usize>, perf1: Option<usize>, physical: Option<usize>) -> Topology {
    let (fast, others) = match perf0 {
        Some(p) => (p, perf1.unwrap_or(0)),
        None => (physical.unwrap_or(logical), 0),
    };
    Topology { logical, fast_cores: fast.min(logical).max(1), fast: Vec::new(), fast_threads: 0, others }
}

/// Keeps the calling thread to `cpus` (empty: leaves it where it is). Real-time threads aren't
/// placed by a core's speed: one stays on whichever core it first ran on, an efficiency core as
/// likely as not, where a heavy plug-in takes about twice as long. The audio callback and the
/// processing threads keep to [`Topology::fast`]. One system call, which allocates nothing.
pub(crate) fn keep_to(cpus: &[usize]) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    if !cpus.is_empty() {
        // SAFETY: a plain value struct, and a call about this thread only.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for &c in cpus {
                libc::CPU_SET(c, &mut set);
            }
            if libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cpus;
    Ok(())
}

/// Parses a kernel CPU list such as "0-15,18".
fn parse_cpus(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    out.extend(a..=b);
                }
            }
            None => out.extend(part.parse::<usize>().ok()),
        }
    }
    out
}

#[allow(dead_code)]
fn detect_linux(logical: usize, p_cores: Option<String>, online: Option<String>, sibling: &dyn Fn(usize) -> Option<String>) -> Topology {
    let fast = p_cores.map(|s| parse_cpus(&s)).unwrap_or_default();
    let all = online.map(|s| parse_cpus(&s)).unwrap_or_else(|| (0..logical).collect());
    let among = if fast.is_empty() { &all } else { &fast };
    // A core counts once, however many hardware threads it has: its first sibling stands for it.
    let cores = among.iter().filter(|&&c| sibling(c).and_then(|l| parse_cpus(&l).first().copied()).is_none_or(|first| first == c)).count();
    Topology { logical, fast_cores: cores.max(1), fast_threads: among.len().min(logical), fast, others: 0 }
}

/// What Automatic picks. On Linux, every hardware thread of the fast cores but one core's, left for
/// the interface and the plug-ins' own threads. On macOS, one thread per fast core, less one unless
/// efficiency cores take everything else ([`Topology::others`]), and no more than the audio
/// device's workgroup suggests (`suggested`, `IoThread::suggested_threads`). At most
/// [`AUTOMATIC_MAX`]; a machine with a core or two plays serially.
pub fn automatic(t: &Topology, suggested: Option<u32>) -> usize {
    let n = if t.fast_threads > 0 {
        let per_core = (t.fast_threads / t.fast_cores.max(1)).max(1);
        t.fast_threads.saturating_sub(per_core)
    } else {
        t.fast_cores.min(t.logical).saturating_sub(usize::from(t.others == 0))
    };
    n.min(suggested.map_or(usize::MAX, |s| s as usize)).clamp(1, AUTOMATIC_MAX)
}

/// How many render threads render ahead starts (#448, #469), whatever Processing Threads says:
/// they aren't real time, don't join the device's workgroup, and the callback never waits on them
/// past its deadline, so only the machine sets their number. As many as Automatic without a
/// workgroup's limit (every hardware thread of the performance cores but one core's on Linux, the
/// performance cores on macOS) or, where the cores aren't counted (Windows, which keeps Automatic
/// on one thread), every logical CPU but one. At most [`AUTOMATIC_MAX`].
pub fn render_threads(t: &Topology) -> usize {
    let counted = t.fast_threads > 0 || t.fast_cores > 1;
    let n = if counted { automatic(t, None) } else { t.logical.saturating_sub(1) };
    n.clamp(1, AUTOMATIC_MAX)
}

/// The threads a setting means here, `suggested` as for [`automatic`]. A number is taken at its
/// word, up to what the setting offers (more threads than cores works, just not well).
pub fn resolve(setting: u32, t: &Topology, suggested: Option<u32>) -> usize {
    match setting {
        AUTOMATIC => automatic(t, suggested),
        n => (n as usize).min(MANUAL_MAX),
    }
}

/// What Settings › Audio offers, for the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThreadChoices {
    /// What Automatic is on this machine.
    pub automatic: u32,
    /// The highest number on offer.
    pub max: u32,
    /// Threads the running setting asked for that play nothing, when there are any.
    pub left_out: Option<LeftOut>,
}

/// Threads playing fewer than the setting asked for, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct LeftOut {
    /// Threads that play tracks, the audio thread included.
    pub playing: u32,
    /// Threads asked for, the audio thread included.
    pub threads: u32,
    /// Refused real-time priority by the system.
    pub refused: u32,
    /// Stopped after a plug-in kept one past the time limit.
    pub stuck: u32,
}

/// What Settings › Audio › Processing Threads offers on this machine.
pub fn choices(t: &Topology, suggested: Option<u32>) -> ThreadChoices {
    ThreadChoices { automatic: automatic(t, suggested) as u32, max: t.logical.clamp(1, MANUAL_MAX) as u32, left_out: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_parse() {
        assert_eq!(parse_cpus("0-3,8,10-11\n"), [0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(parse_cpus(""), Vec::<usize>::new());
        assert_eq!(parse_cpus("x,4"), [4]);
    }

    fn hybrid_siblings(c: usize) -> Option<String> {
        // Eight performance cores with two hardware threads each, then eight efficiency cores.
        Some(if c < 16 { format!("{},{}", c & !1, c | 1) } else { c.to_string() })
    }

    #[test]
    fn performance_cores_count_once_and_efficiency_cores_not_at_all() {
        let t = detect_linux(24, Some("0-15\n".into()), Some("0-23\n".into()), &hybrid_siblings);
        assert_eq!(t.fast_cores, 8);
        assert_eq!(t.fast, (0..16).collect::<Vec<_>>());
        // Both hardware threads of every performance core but one (#440).
        assert_eq!(t.fast_threads, 16);
        assert_eq!(automatic(&t, None), 14);
    }

    #[test]
    fn a_uniform_machine_counts_its_physical_cores() {
        let t = detect_linux(8, None, Some("0-7".into()), &|c| Some(format!("{},{}", c & !1, c | 1)));
        assert_eq!((t.fast_cores, t.fast_threads), (4, 8));
        assert_eq!(automatic(&t, None), 6);
        // Without hyperthreads, one core is left all the same.
        let t = detect_linux(8, None, Some("0-7".into()), &|c| Some(c.to_string()));
        assert_eq!((t.fast_cores, t.fast_threads), (8, 8));
        assert_eq!(automatic(&t, None), 7);
    }

    #[test]
    fn apple_silicon_counts_its_performance_cores() {
        // M1: four performance and four efficiency cores. The efficiency cores take everything
        // else, so Automatic plays on all four performance cores, as the workgroup suggests.
        let t = detect_macos(8, Some(4), Some(4), Some(8));
        assert_eq!((t.fast_cores, t.others, t.fast.len()), (4, 4, 0));
        assert_eq!(automatic(&t, None), 4);
        assert_eq!(automatic(&t, Some(4)), 4);
        // M1 Pro / Max: eight and two.
        let t = detect_macos(10, Some(8), Some(2), Some(10));
        assert_eq!((t.fast_cores, t.others), (8, 2));
        assert_eq!(automatic(&t, None), 8);
    }

    #[test]
    fn automatic_takes_no_more_than_the_workgroup_suggests() {
        let t = detect_macos(10, Some(8), Some(2), Some(10));
        assert_eq!(automatic(&t, Some(3)), 3);
        assert_eq!(resolve(AUTOMATIC, &t, Some(3)), 3);
        assert_eq!(automatic(&t, Some(0)), 1);
        // A number is taken at its word.
        assert_eq!(resolve(5, &t, Some(3)), 5);
        assert_eq!(choices(&t, Some(3)).automatic, 3);
    }

    #[test]
    fn an_intel_mac_counts_its_physical_cores() {
        // Hyper-threaded, no perflevels: one core is left for everything else.
        let t = detect_macos(8, None, None, Some(4));
        assert_eq!((t.fast_cores, t.others), (4, 0));
        assert_eq!(automatic(&t, None), 3);
        // Nothing to go on: the logical CPUs, at least one.
        assert_eq!(detect_macos(2, None, None, None).fast_cores, 2);
        assert_eq!(detect_macos(1, Some(4), None, None).fast_cores, 1);
    }

    #[test]
    fn small_machines_stay_serial() {
        for (logical, cores) in [(1, 1), (2, 1), (2, 2)] {
            let t = Topology { logical, fast_cores: cores, ..Default::default() };
            assert_eq!(automatic(&t, None), 1, "{logical} logical, {cores} cores");
            let t = Topology { logical, fast_cores: cores, fast_threads: logical, ..Default::default() };
            assert_eq!(automatic(&t, None), 1, "{logical} logical, {cores} cores, Linux");
        }
    }

    #[test]
    fn automatic_is_bounded_on_a_big_machine() {
        let t = Topology { logical: 128, fast_cores: 64, ..Default::default() };
        assert_eq!(automatic(&t, None), AUTOMATIC_MAX);
        let t = Topology { logical: 128, fast_cores: 64, fast_threads: 128, ..Default::default() };
        assert_eq!(automatic(&t, None), AUTOMATIC_MAX);
        assert_eq!(resolve(1, &t, None), 1);
        assert_eq!(resolve(AUTOMATIC, &t, None), AUTOMATIC_MAX);
        assert_eq!(resolve(4, &t, None), 4);
        assert_eq!(resolve(1000, &t, None), MANUAL_MAX);
    }

    #[test]
    fn the_setting_offers_what_the_machine_has() {
        let c = choices(&Topology { logical: 24, fast_cores: 8, fast_threads: 16, ..Default::default() }, None);
        assert_eq!((c.automatic, c.max), (14, 16));
        let c = choices(&Topology { logical: 2, fast_cores: 1, ..Default::default() }, None);
        assert_eq!((c.automatic, c.max), (1, 2));
    }

    #[test]
    fn render_threads_follow_the_machine_not_the_setting() {
        // Linux, 8 hyper-threaded performance cores and 8 efficiency cores: as Automatic.
        let t = detect_linux(24, Some("0-15\n".into()), Some("0-23\n".into()), &hybrid_siblings);
        assert_eq!(render_threads(&t), 14);
        // An M1, whose workgroup holds Automatic to what it suggests: render threads aren't in it.
        let t = detect_macos(10, Some(8), Some(2), Some(10));
        assert_eq!(render_threads(&t), 8);
        assert_eq!(automatic(&t, Some(3)), 3);
        // Windows counts nothing and keeps Automatic on one thread: every logical CPU but one.
        let t = Topology { logical: 12, fast_cores: 1, ..Default::default() };
        assert_eq!((automatic(&t, None), render_threads(&t)), (1, 11));
        // Bounded on a big machine, and at least one on a small one.
        assert_eq!(render_threads(&Topology { logical: 128, fast_cores: 1, ..Default::default() }), AUTOMATIC_MAX);
        assert_eq!(render_threads(&Topology { logical: 128, fast_cores: 64, fast_threads: 128, ..Default::default() }), AUTOMATIC_MAX);
        assert_eq!(render_threads(&Topology { logical: 1, fast_cores: 1, ..Default::default() }), 1);
        assert_eq!(render_threads(&Topology { logical: 2, fast_cores: 2, ..Default::default() }), 1);
    }
}
