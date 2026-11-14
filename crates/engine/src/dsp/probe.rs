//! A lock-free event trace for investigating render ahead misses (used by `examples/ahead_probe.rs`;
//! nothing calls `ev` in the engine unless you add the calls while investigating).
//! `init` allocates before playing; `ev` stores an event with relaxed atomics; `dump` writes CSV.
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

pub const CALLBACK: u64 = 1; // a = pos, b = dur_us, c = misses so far, d = pooled(1)/lent count<<1
pub const TAKE: u64 = 2; // a = strip id, b = buffered frames at entry, c = waited_us, d = 1 hit / 0 miss / 2 other pos
pub const RENDER: u64 = 3; // a = strip id, b = pos, c = dur_us, d = thread<<16 | cpu
pub const LEND: u64 = 4; // a = strip id, b = pos
pub const STRIP: u64 = 5; // a = strip id, b = spent_us (live strips, callback/pool), c = pos, d = cpu
pub const MARK: u64 = 6; // a = code from the driver

struct Trace {
    epoch: Instant,
    ev: Box<[[AtomicU64; 6]]>,
    at: AtomicUsize,
}

static T: OnceLock<Trace> = OnceLock::new();
static ON: AtomicBool = AtomicBool::new(false);

pub fn init(n: usize) {
    let _ = T.set(Trace { epoch: Instant::now(), ev: (0..n).map(|_| Default::default()).collect(), at: AtomicUsize::new(0) });
    ON.store(true, Ordering::SeqCst);
}

#[inline]
pub fn now_us() -> u64 {
    T.get().map_or(0, |t| t.epoch.elapsed().as_micros() as u64)
}

#[inline]
pub fn ev(kind: u64, a: u64, b: u64, c: u64, d: u64) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let Some(t) = T.get() else { return };
    let i = t.at.fetch_add(1, Ordering::Relaxed);
    if i >= t.ev.len() {
        return;
    }
    let e = &t.ev[i];
    let vals = [kind, t.epoch.elapsed().as_micros() as u64, a, b, c, d];
    for (slot, v) in e.iter().zip(vals) {
        slot.store(v, Ordering::Relaxed);
    }
}

/// The CPU this thread is on.
#[cfg(target_os = "linux")]
pub fn cpu() -> u64 {
    // SAFETY: plain libc call.
    unsafe { libc::sched_getcpu() as u64 }
}

/// The CPU this thread is on (macOS 11 and later; on Apple silicon the efficiency cores come first).
#[cfg(target_os = "macos")]
pub fn cpu() -> u64 {
    let mut n: libc::size_t = 0;
    // SAFETY: plain libc call into a local.
    unsafe { libc::pthread_cpu_number_np(&mut n) };
    n as u64
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn cpu() -> u64 {
    0
}

pub fn dump(path: &std::path::Path) {
    let Some(t) = T.get() else { return };
    let n = t.at.load(Ordering::SeqCst).min(t.ev.len());
    let mut s = String::from("kind,t_us,a,b,c,d\n");
    for e in &t.ev[..n] {
        let v: Vec<u64> = e.iter().map(|x| x.load(Ordering::Relaxed)).collect();
        s += &format!("{},{},{},{},{},{}\n", v[0], v[1], v[2] as i64, v[3] as i64, v[4], v[5]);
    }
    std::fs::write(path, s).unwrap();
}

/// Minor + major page faults of this thread so far: (minor, major).
#[cfg(target_os = "linux")]
pub fn faults() -> (u64, u64) {
    // SAFETY: plain libc call into a zeroed struct.
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_THREAD, &mut u);
        (u.ru_minflt as u64, u.ru_majflt as u64)
    }
}

/// Minor + major page faults so far, of the whole process: macOS counts them per process only, so
/// a thread's difference includes every other thread's faults meanwhile.
#[cfg(target_os = "macos")]
pub fn faults() -> (u64, u64) {
    // SAFETY: plain libc call into a zeroed struct.
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        (u.ru_minflt as u64, u.ru_majflt as u64)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn faults() -> (u64, u64) {
    (0, 0)
}
