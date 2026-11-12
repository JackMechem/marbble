//! How much RAM the process holds (its resident size), for the status line and the Diagnostics
//! window, on each system: `/proc/self/status` on Linux (memlock.rs's [`ProcessMemory`]), the task
//! info on macOS, the working set on Windows. Cheap enough to ask every few seconds; never asked
//! on the audio thread.

#[cfg(target_os = "linux")]
use super::memlock::ProcessMemory;

/// The resident size in bytes; None where the system doesn't say.
pub fn resident_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        ProcessMemory::try_read().map(|m| m.resident_kb * 1024)
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: proc_taskinfo is plain integers, and the call writes at most its size.
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        let got = unsafe { libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, (&mut info as *mut libc::proc_taskinfo).cast(), size) };
        (got == size).then_some(info.pti_resident_size)
    }
    #[cfg(target_os = "windows")]
    {
        windows::working_set()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// `K32GetProcessMemoryInfo`, in kernel32 since Windows 7, declared here rather than pulling in a
/// bindings crate for one call.
#[cfg(target_os = "windows")]
mod windows {
    /// PROCESS_MEMORY_COUNTERS (psapi.h).
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
    }

    pub fn working_set() -> Option<u64> {
        let cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        let mut c = ProcessMemoryCounters { cb, ..Default::default() };
        // SAFETY: the current process's pseudo-handle, and a struct of the size passed.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, cb) } != 0;
        ok.then_some(c.working_set_size as u64)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[test]
    fn this_process_has_a_resident_size() {
        let bytes = super::resident_bytes().expect("the system gives a resident size");
        // More than a megabyte, less than a terabyte: a size in bytes, not pages or kB.
        assert!(bytes > 1 << 20 && bytes < 1 << 40, "{bytes}");
    }
}
