//! Settings › Audio › Lock audio memory: keeping the process's memory in RAM on Linux, so the audio
//! threads never wait for a page the system swapped out. Under memory pressure Linux swaps out
//! whatever was touched least recently (a clip's later minutes, a plug-in's tables, the engine's own
//! buffers), and an audio thread that reads one waits for the disk: a dropout however light the load.
//!
//! As Ardour and JACK do, the lock is `mlockall(MCL_CURRENT | MCL_FUTURE)`, taken only when
//! `RLIMIT_MEMLOCK` is unlimited (as the audio group's limits make it on most audio setups). A finite
//! limit locks nothing: `MCL_CURRENT` counts every mapping of the process against it, gigabytes of
//! address space in the app (WebKitGTK and the libraries reserve far more than they touch), so it
//! fails under any limit set for audio; and with `MCL_FUTURE` an allocation that would take the
//! process past the limit fails, which the engine can't survive. Locking only the clips instead
//! would fill the usual 8 MB default with twenty seconds of stereo audio.
//!
//! `MCL_ONFAULT` (Linux 4.4) is added, so a page is locked when it is first touched rather than every
//! mapping being made resident at once: what the process has touched (the clips, which decoding
//! writes in full, the engine, the plug-ins) stays in RAM, while thread stacks, a recording's spare
//! capacity and heap that nothing uses cost nothing. In `memlock_check` plain `mlockall` made the
//! process's resident size 29% bigger at once (78 to 101 MB) and 69% bigger after a render (88 to
//! 149 MB); with `MCL_ONFAULT` it didn't grow. Most of what gets pinned is clips, decoded whole as
//! f32 (11 MB a minute per mono channel at 48 kHz), which is why opening a project decodes only the
//! clips its tracks can play.
//! macOS and Windows have no such limit to work with, and the switch is hidden there.

use serde::Serialize;

/// A fresh install's setting: on.
pub fn default_setting() -> bool {
    true
}

/// Whether the process's memory is locked, as Settings › Audio shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemoryLock {
    /// Not on this system (macOS, Windows).
    Unsupported,
    /// Switched off in Settings › Audio (or not asked for yet).
    Off,
    /// Everything the process touches stays in RAM.
    Locked,
    /// `RLIMIT_MEMLOCK` is finite (`limit` bytes), so nothing was locked.
    LimitTooSmall { limit: u64 },
    /// The system refused.
    Failed { error: String },
}

impl MemoryLock {
    /// What a session starts with, before the app asks for the lock.
    pub fn initial() -> MemoryLock {
        if cfg!(target_os = "linux") {
            MemoryLock::Off
        } else {
            MemoryLock::Unsupported
        }
    }
}

/// What to do about the switch, given `RLIMIT_MEMLOCK`'s soft limit (None = unlimited): Ok to lock,
/// or the state to report without locking.
pub fn plan(on: bool, limit: Option<u64>) -> Result<(), MemoryLock> {
    match (on, limit) {
        (false, _) => Err(MemoryLock::Off),
        (true, None) => Ok(()),
        (true, Some(limit)) => Err(MemoryLock::LimitTooSmall { limit }),
    }
}

/// Locks or unlocks the process's memory to match the switch, logs the outcome and returns it.
pub fn apply(on: bool) -> MemoryLock {
    #[cfg(target_os = "linux")]
    {
        let state = match plan(on, memlock_limit()) {
            Ok(()) => lock(),
            Err(state) => {
                // Off, or a limit that keeps it from locking: let go of a lock taken earlier.
                unsafe { libc::munlockall() };
                state
            }
        };
        let mem = ProcessMemory::read();
        match &state {
            MemoryLock::Locked => {
                log::info!("memory locked (RLIMIT_MEMLOCK unlimited): the {} MB resident and what is touched later stays in RAM", mem.resident_kb / 1024)
            }
            MemoryLock::Off => log::info!("memory not locked: switched off in Settings › Audio"),
            MemoryLock::LimitTooSmall { limit } => log::warn!(
                "memory not locked: RLIMIT_MEMLOCK is {}, and only an unlimited one lets Marbble lock its {} MB; audio can drop out when the system swaps",
                size(*limit),
                mem.resident_kb / 1024
            ),
            MemoryLock::Failed { error } => log::warn!("memory not locked: {error}; audio can drop out when the system swaps"),
            MemoryLock::Unsupported => {}
        }
        state
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = on;
        MemoryLock::Unsupported
    }
}

#[cfg(target_os = "linux")]
fn lock() -> MemoryLock {
    let flags = libc::MCL_CURRENT | libc::MCL_FUTURE;
    if unsafe { libc::mlockall(flags | libc::MCL_ONFAULT) } == 0 {
        return MemoryLock::Locked;
    }
    // Kernels before 4.4 don't know MCL_ONFAULT; they lock everything at once.
    let mut err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EINVAL) {
        if unsafe { libc::mlockall(flags) } == 0 {
            return MemoryLock::Locked;
        }
        err = std::io::Error::last_os_error();
    }
    MemoryLock::Failed { error: format!("mlockall failed: {err}") }
}

/// `RLIMIT_MEMLOCK`'s soft limit in bytes; None when it is unlimited.
#[cfg(target_os = "linux")]
#[allow(clippy::unnecessary_cast)] // rlim_t is 32 bits on some 32-bit targets
pub fn memlock_limit() -> Option<u64> {
    let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    if unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut r) } != 0 {
        return Some(0);
    }
    (r.rlim_cur != libc::RLIM_INFINITY).then_some(r.rlim_cur as u64)
}

/// A limit for the log: "8 MB", "64 kB".
#[cfg(any(target_os = "linux", test))]
fn size(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{} MB", bytes >> 20)
    } else {
        format!("{} kB", bytes >> 10)
    }
}

/// The process's memory as `/proc/self/status` gives it, in kB (all zero where there is none).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessMemory {
    /// VmSize: every mapping, touched or not.
    pub mapped_kb: u64,
    /// VmRSS: what is in RAM.
    pub resident_kb: u64,
    /// VmLck: the size of the locked mappings (with `MCL_ONFAULT` their pages lock as they are
    /// touched, so this is close to `mapped_kb` and more than is resident).
    pub locked_kb: u64,
    /// VmSwap: what the system swapped out.
    pub swapped_kb: u64,
}

impl ProcessMemory {
    pub fn read() -> ProcessMemory {
        ProcessMemory::try_read().unwrap_or_default()
    }

    /// None where there is no `/proc/self/status` (macOS, Windows).
    pub fn try_read() -> Option<ProcessMemory> {
        std::fs::read_to_string("/proc/self/status").ok().map(|t| ProcessMemory::parse(&t))
    }

    fn parse(status: &str) -> ProcessMemory {
        let field = |name: &str| {
            status.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix(':')).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok()).unwrap_or(0)
        };
        ProcessMemory { mapped_kb: field("VmSize"), resident_kb: field("VmRSS"), locked_kb: field("VmLck"), swapped_kb: field("VmSwap") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_only_with_an_unlimited_limit() {
        assert_eq!(plan(true, None), Ok(()));
        assert_eq!(plan(true, Some(8 << 20)), Err(MemoryLock::LimitTooSmall { limit: 8 << 20 }));
        // Even a limit of many gigabytes: MCL_FUTURE would make an allocation past it fail.
        assert_eq!(plan(true, Some(64 << 30)), Err(MemoryLock::LimitTooSmall { limit: 64 << 30 }));
        assert_eq!(plan(false, None), Err(MemoryLock::Off));
        assert_eq!(plan(false, Some(0)), Err(MemoryLock::Off));
    }

    #[test]
    fn the_frontend_reads_the_state_by_name() {
        let j = |m: MemoryLock| serde_json::to_string(&m).unwrap();
        assert_eq!(j(MemoryLock::Locked), r#"{"state":"locked"}"#);
        assert_eq!(j(MemoryLock::LimitTooSmall { limit: 65536 }), r#"{"state":"limit_too_small","limit":65536}"#);
        assert_eq!(j(MemoryLock::Unsupported), r#"{"state":"unsupported"}"#);
    }

    #[test]
    fn reads_proc_status() {
        let m = ProcessMemory::parse("Name:\tdaw\nVmSize:\t 3145728 kB\nVmLck:\t     288 kB\nVmRSS:\t  716800 kB\nVmSwap:\t 1457664 kB\n");
        assert_eq!(m, ProcessMemory { mapped_kb: 3145728, resident_kb: 716800, locked_kb: 288, swapped_kb: 1457664 });
        assert_eq!(size(8 << 20), "8 MB");
        assert_eq!(size(64 << 10), "64 kB");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn this_process_has_its_status() {
        assert!(ProcessMemory::read().resident_kb > 0);
    }
}
