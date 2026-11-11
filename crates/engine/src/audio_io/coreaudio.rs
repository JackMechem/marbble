//! The audio device's IO thread on macOS, for the threads that play tracks beside it
//! (`dsp/pool/`): its `os_workgroup`, which they join so that the scheduler runs them on the
//! performance cores with it and sizes the CPU's clock for the whole group, and its time-constraint
//! policy, which they copy.
//!
//! `os_workgroup_join` and its kin are in libSystem from macOS 11 only, and the Intel build may
//! start on older systems, so they are looked up when first used: without them there is no
//! workgroup, as there is none on a device that doesn't offer one.

use std::ffi::{c_int, c_void, CStr};
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use mach2::thread_policy::{
    thread_policy_get, thread_policy_t, thread_time_constraint_policy_data_t, THREAD_TIME_CONSTRAINT_POLICY, THREAD_TIME_CONSTRAINT_POLICY_COUNT,
};
use objc2_core_audio::{
    kAudioDevicePropertyIOThreadOSWorkgroup, kAudioHardwarePropertyTranslateUIDToDevice, kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectSystemObject, AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress,
};
use objc2_core_foundation::CFString;

/// What the workers take from the device's IO thread. Empty for the null device, a device without a
/// workgroup, or before macOS 11.
#[derive(Debug, Clone, Default)]
pub struct IoThread {
    /// The IO thread's workgroup, for the workers to join.
    pub workgroup: Option<Workgroup>,
    /// The IO thread's time-constraint policy, for the workers to copy, as it was on the first
    /// callback.
    pub policy: Option<TimeConstraint>,
}

impl IoThread {
    /// Finds the IO thread of the output device `uid` (cpal's `DeviceId::id`), waiting for
    /// `probe` to see its first callback. Off the audio thread: this allocates and may wait.
    pub(crate) fn of(uid: &str, probe: &IoThreadProbe) -> IoThread {
        let workgroup = device_id(uid).and_then(io_workgroup);
        IoThread { workgroup, policy: probe.wait(Duration::from_millis(250)) }
    }

    /// How many threads the system suggests for work in the IO thread's workgroup
    /// (`os_workgroup_max_parallel_threads`), the IO thread included.
    pub fn suggested_threads(&self) -> Option<u32> {
        self.workgroup.as_ref()?.max_parallel_threads()
    }
}

/// A Mach time-constraint policy, in Mach absolute-time units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeConstraint {
    pub period: u32,
    pub computation: u32,
    pub constraint: u32,
}

/// The output callback's notes about its own thread, from its first call.
#[derive(Debug, Default)]
pub(crate) struct IoThreadProbe {
    period: AtomicU32,
    computation: AtomicU32,
    constraint: AtomicU32,
    /// Has a policy (the three above are set).
    real_time: AtomicBool,
    seen: AtomicBool,
}

impl IoThreadProbe {
    /// On the IO thread, once: reads its time-constraint policy. One Mach call that neither blocks
    /// nor allocates, and atomics.
    pub fn note(&self) {
        if let Some(p) = own_policy() {
            self.period.store(p.period, Ordering::Relaxed);
            self.computation.store(p.computation, Ordering::Relaxed);
            self.constraint.store(p.constraint, Ordering::Relaxed);
            self.real_time.store(true, Ordering::Relaxed);
        }
        self.seen.store(true, Ordering::Release);
    }

    /// The policy `note` saw, waiting up to `most` for it to be called.
    fn wait(&self, most: Duration) -> Option<TimeConstraint> {
        let until = Instant::now() + most;
        while !self.seen.load(Ordering::Acquire) {
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.real_time.load(Ordering::Relaxed).then(|| TimeConstraint {
            period: self.period.load(Ordering::Relaxed),
            computation: self.computation.load(Ordering::Relaxed),
            constraint: self.constraint.load(Ordering::Relaxed),
        })
    }
}

/// The calling thread's time-constraint policy, or None while it has none.
pub(crate) fn own_policy() -> Option<TimeConstraint> {
    let mut info = thread_time_constraint_policy_data_t { period: 0, computation: 0, constraint: 0, preemptible: 0 };
    let mut count = THREAD_TIME_CONSTRAINT_POLICY_COUNT;
    // In: give the thread's own policy rather than the default. Out: it had none.
    let mut default = 0;
    // SAFETY: a query about this thread, into a struct of the size given. The port name is the
    // thread's while it lives, with no reference taken.
    let r = unsafe {
        let port = libc::pthread_mach_thread_np(libc::pthread_self());
        thread_policy_get(port, THREAD_TIME_CONSTRAINT_POLICY, &mut info as *mut _ as thread_policy_t, &mut count, &mut default)
    };
    (r == 0 && default == 0 && info.period > 0).then_some(TimeConstraint { period: info.period, computation: info.computation, constraint: info.constraint })
}

/// For checks: [`own_policy`] of the calling thread, to count what it allocates.
#[doc(hidden)]
pub fn own_policy_for_checks() -> Option<TimeConstraint> {
    let probe = IoThreadProbe::default();
    probe.note();
    probe.wait(Duration::ZERO)
}

/// The device whose UID is `uid`.
pub(crate) fn device_id(uid: &str) -> Option<AudioObjectID> {
    let uid = CFString::from_str(uid);
    let qualifier: *const CFString = &*uid;
    let mut device: AudioObjectID = 0;
    let mut size = size_of::<AudioObjectID>() as u32;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyTranslateUIDToDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: the property takes a CFStringRef as its qualifier and gives an AudioObjectID, of the
    // sizes given; `uid` outlives the call.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&address),
            size_of::<*const CFString>() as u32,
            &qualifier as *const *const CFString as *const c_void,
            NonNull::from(&mut size),
            NonNull::from(&mut device).cast(),
        )
    };
    // kAudioObjectUnknown (0) for a UID no device has.
    (status == 0 && device != 0).then_some(device)
}

/// The device's IO thread's workgroup, if it has one (macOS 11 and later).
fn io_workgroup(device: AudioObjectID) -> Option<Workgroup> {
    api()?;
    let mut wg: *mut c_void = std::ptr::null_mut();
    let mut size = size_of::<*mut c_void>() as u32;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyIOThreadOSWorkgroup,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: the property gives an os_workgroup_t, of the size given, with a reference the caller
    // owns (released by `Owned`'s drop).
    let status =
        unsafe { AudioObjectGetPropertyData(device, NonNull::from(&address), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::from(&mut wg).cast()) };
    if status != 0 {
        log::info!("the audio device has no IO workgroup (OSStatus {status})");
        return None;
    }
    NonNull::new(wg).map(|p| Workgroup(Arc::new(Owned(p))))
}

/// An `os_workgroup_t`. Any thread may join it.
#[derive(Clone)]
pub struct Workgroup(Arc<Owned>);

struct Owned(NonNull<c_void>);

// SAFETY: an os_workgroup is an os_object, whose reference count is atomic, and any thread may join
// or leave it.
unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the reference `io_workgroup` was given.
        unsafe { os_release(self.0.as_ptr()) };
    }
}

impl std::fmt::Debug for Workgroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Workgroup({:p})", self.0 .0)
    }
}

impl Workgroup {
    /// Joins the calling thread to the workgroup until the result is dropped, on this thread.
    /// Real-time safe (`os_workgroup_join`), but done once, as a worker starts. Fails with the
    /// errno: `EINVAL` once the device has cancelled it, `EALREADY` in another that doesn't nest.
    pub fn join(&self) -> Result<Joined, c_int> {
        let api = api().ok_or(libc::ENOTSUP)?;
        let mut token = JoinToken { sig: 0, opaque: [0; 36] };
        // SAFETY: a live workgroup, and a token of the size the header gives for 64-bit.
        match unsafe { (api.join)(self.0 .0.as_ptr(), &mut token) } {
            0 => Ok(Joined { wg: self.clone(), token, not_send: PhantomData }),
            e => Err(e),
        }
    }

    fn max_parallel_threads(&self) -> Option<u32> {
        // SAFETY: a live workgroup; the attributes are unused and must be null.
        let n = unsafe { (api()?.max_parallel_threads)(self.0 .0.as_ptr(), std::ptr::null_mut()) };
        u32::try_from(n).ok().filter(|&n| n > 0)
    }
}

/// A thread's place in a workgroup, which it leaves when this is dropped. Stays on the thread that
/// joined, as `os_workgroup_leave` must be called there.
pub struct Joined {
    wg: Workgroup,
    token: JoinToken,
    not_send: PhantomData<*const ()>,
}

impl Drop for Joined {
    fn drop(&mut self) {
        if let Some(api) = api() {
            // SAFETY: the token its join filled in, on the thread that joined.
            unsafe { (api.leave)(self.wg.0 .0.as_ptr(), &mut self.token) };
        }
    }
}

/// `os_workgroup_join_token_opaque_s` (`os/workgroup_base.h`, 64-bit). It holds no pointer to
/// itself, so it may move between the join and the leave.
#[repr(C)]
struct JoinToken {
    sig: u32,
    opaque: [u8; 36],
}

#[cfg(not(target_pointer_width = "64"))]
compile_error!("JoinToken's size is the 64-bit one");

extern "C" {
    /// libSystem's, since macOS 10.12.
    fn os_release(object: *mut c_void);
}

type JoinFn = unsafe extern "C" fn(*mut c_void, *mut JoinToken) -> c_int;
type LeaveFn = unsafe extern "C" fn(*mut c_void, *mut JoinToken);
type MaxParallelFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;

struct Api {
    join: JoinFn,
    leave: LeaveFn,
    max_parallel_threads: MaxParallelFn,
}

/// The workgroup functions, if this macOS has them (11 and later).
fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        let find = |name: &CStr| {
            // SAFETY: a lookup in the images already loaded.
            let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
            (!p.is_null()).then_some(p)
        };
        let (join, leave, max) = (find(c"os_workgroup_join")?, find(c"os_workgroup_leave")?, find(c"os_workgroup_max_parallel_threads")?);
        // SAFETY: the three have these signatures (`os/workgroup_object.h`).
        unsafe {
            Some(Api {
                join: std::mem::transmute::<*mut c_void, JoinFn>(join),
                leave: std::mem::transmute::<*mut c_void, LeaveFn>(leave),
                max_parallel_threads: std::mem::transmute::<*mut c_void, MaxParallelFn>(max),
            })
        }
    })
    .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workgroup_functions_are_found() {
        // Every macOS this is built and tested on is 11 or later.
        assert!(api().is_some());
    }

    #[test]
    fn an_unknown_uid_is_no_device() {
        assert_eq!(device_id("no such device, surely"), None);
    }

    #[test]
    fn a_thread_without_a_policy_has_none() {
        assert_eq!(std::thread::spawn(own_policy).join().unwrap(), None);
    }

    #[test]
    fn a_probe_that_never_saw_its_callback_gives_nothing() {
        assert_eq!(IoThreadProbe::default().wait(Duration::from_millis(2)), None);
    }

    #[test]
    fn a_probe_reads_its_threads_policy() {
        use mach2::thread_policy::thread_policy_set;
        // About 10 ms, 2 ms and 10 ms on Apple silicon's 24 MHz clock (Intel's counts nanoseconds).
        let set = thread_time_constraint_policy_data_t { period: 240_000, computation: 48_000, constraint: 240_000, preemptible: 1 };
        let got = std::thread::spawn(move || {
            let mut p = set;
            // SAFETY: a valid policy of the flavour and size given, for this thread's own port.
            let r = unsafe {
                thread_policy_set(
                    libc::pthread_mach_thread_np(libc::pthread_self()),
                    THREAD_TIME_CONSTRAINT_POLICY,
                    &mut p as *mut _ as thread_policy_t,
                    THREAD_TIME_CONSTRAINT_POLICY_COUNT,
                )
            };
            assert_eq!(r, 0);
            let probe = IoThreadProbe::default();
            probe.note();
            probe.wait(Duration::ZERO)
        })
        .join()
        .unwrap();
        let got = got.expect("a policy");
        // The scheduler may raise a short computation (xnu does to half the constraint).
        assert_eq!((got.period, got.constraint), (set.period, set.constraint));
        assert!(got.computation >= set.computation && got.computation <= set.constraint, "{got:?}");
    }
}
