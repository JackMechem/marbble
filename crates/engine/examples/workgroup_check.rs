//! The audio device's IO workgroup (macOS, `audio_io/coreaudio.rs`): opens the default output
//! device (never an input, which would ask for the microphone), plays silence on four tracks with
//! every block through three real-time workers, and checks that each worker joined the IO thread's
//! workgroup, that blocks went through them, and that reading the IO thread's policy (as the first
//! callback does) allocates nothing. Then it reopens the device, as a change in Settings › Audio
//! does, and checks the new workers join the new device's workgroup. SKIP where there is no output
//! device (CI runners), no workgroup (before macOS 11), or no macOS.
//! usage: workgroup_check
use std::sync::atomic::{AtomicBool, Ordering};

static FAILED: AtomicBool = AtomicBool::new(false);

#[allow(dead_code)]
fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("SKIP workgroups: the audio device's IO workgroup is macOS's");
}

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::check;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use daw_engine::audio_io::AudioConfig;
    use daw_engine::dsp::ParallelTest;
    use daw_engine::model::{InputSel, TrackKind};
    use daw_engine::Session;

    /// Counts the allocations made on a thread while its `COUNTING` flag is set.
    struct Counting;

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        static COUNTING: Cell<bool> = const { Cell::new(false) };
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            if COUNTING.with(|c| c.get()) {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            if COUNTING.with(|c| c.get()) {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.dealloc(p, l) }
        }
    }

    #[global_allocator]
    static A: Counting = Counting;

    /// Threads including the callback's: three workers.
    const THREADS: u32 = 4;

    pub fn run() {
        let cfg = AudioConfig { input_enabled: false, processing_threads: THREADS, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        if !s.audio_status.running {
            println!("SKIP workgroups: no output device ({:?})", s.audio_status.error);
            return;
        }
        println!("output: {:?}, {} frames at {} Hz", s.audio_status.output_name, s.audio_status.buffer_size, s.audio_status.sample_rate);
        let Some(io) = s.io_thread().cloned() else {
            println!("SKIP workgroups: the device stopped");
            return;
        };
        if io.workgroup.is_none() {
            println!("SKIP workgroups: the output device has no IO workgroup (before macOS 11?)");
            return;
        }
        println!("IO thread: time constraint {:?}, suggested threads {:?}", io.policy, io.suggested_threads());
        check("the IO thread's time-constraint policy was read on its first callback", io.policy.is_some());

        // What the first callback does, on a thread that counts what it allocates.
        let allocs = std::thread::spawn(|| {
            COUNTING.with(|c| c.set(true));
            let before = ALLOCS.load(Ordering::Relaxed);
            std::hint::black_box(daw_engine::audio_io::coreaudio::own_policy_for_checks());
            let n = ALLOCS.load(Ordering::Relaxed) - before;
            COUNTING.with(|c| c.set(false));
            n
        })
        .join()
        .unwrap();
        check(&format!("reading the IO thread's policy allocates nothing ({allocs} allocations)"), allocs == 0);

        for _ in 0..4 {
            s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        }
        s.set_threads_for_checks(THREADS, ParallelTest { always: true, renders: false, patient: true });
        joined_and_played(&mut s, "the device");

        // A new device (here the same one, reopened) brings a new workgroup, and new workers.
        s.restart_audio();
        s.sync_parallel();
        if s.audio_status.running {
            joined_and_played(&mut s, "the reopened device");
        } else {
            check(&format!("the device reopened ({:?})", s.audio_status.error), false);
        }
        drop(s);
        println!("workers left their workgroup and ended");
    }

    fn joined_and_played(s: &mut Session, what: &str) {
        let started = Instant::now();
        while s.threads_health_for_checks().is_none_or(|r| r.starting > 0) {
            assert!(started.elapsed() < Duration::from_secs(10), "the workers never started");
            s.idle();
            std::thread::sleep(Duration::from_millis(5));
        }
        let r = s.threads_health_for_checks().unwrap();
        check(&format!("{what}: {} of 3 workers real time", r.realtime), r.realtime == 3);
        check(&format!("{what}: {} of 3 workers in its IO workgroup", r.joined), r.joined == 3);
        let status = s.engine_for_checks().lock().status.clone();
        let pooled = status.parallel_blocks.load(Ordering::Relaxed);
        let errors = status.engine_errors.load(Ordering::Relaxed);
        s.locate(0);
        s.play();
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            s.idle();
            std::thread::sleep(Duration::from_millis(10));
        }
        s.stop();
        let pooled = status.parallel_blocks.load(Ordering::Relaxed) - pooled;
        let errors = status.engine_errors.load(Ordering::Relaxed) - errors;
        let r = s.threads_health_for_checks().unwrap();
        check(
            &format!("{what}: 500 ms of silence through the workers ({pooled} blocks on the pool, {errors} errors, {} playing)", r.playing),
            pooled > 0 && errors == 0 && r.playing == THREADS,
        );
    }
}
