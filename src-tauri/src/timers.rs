//! The threads and timers that drive the session while the app runs: the `tick` event (~30 Hz),
//! `idle` on the main thread with the autosave it may start, and picking up MIDI devices
//! plugged in after launch.

use std::sync::Arc;
use std::time::Duration;

use daw_engine::Session;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

use crate::commands;

/// Starts them all.
pub fn start(handle: &AppHandle, session: &Arc<Mutex<Session>>) -> std::io::Result<()> {
    // Meters, playhead and recording previews at ~30 Hz.
    let s = session.clone();
    let h = handle.clone();
    std::thread::Builder::new().name("ui-tick".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_millis(33));
        let tick = s.lock().tick();
        let _ = h.emit("tick", tick);
    })?;

    // Plug-in controller idle + freeing replaced DSP objects, on the main thread.
    #[cfg(target_os = "linux")]
    {
        let s = session.clone();
        let h = handle.clone();
        glib::timeout_add_local(Duration::from_millis(30), move || {
            if let Some(mut g) = s.try_lock() {
                if vst3_host::editor::gl_isolated(|| g.idle()) {
                    let _ = h.emit("project", commands::snapshot(&g));
                }
                autosave(&mut g);
            }
            glib::ControlFlow::Continue
        });
    }
    #[cfg(target_os = "macos")]
    macos::start_idle_timer(session.clone(), handle.clone());
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // No main-loop timer to hook into here, so a helper thread posts the idle work. Only one
        // post is in flight at a time: while the main thread is busy the helper skips ticks
        // rather than queueing them up.
        let s = session.clone();
        let h = handle.clone();
        let in_flight = Arc::new(InFlight::default());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(30));
            let Some(claim) = in_flight.claim() else { continue };
            let s = s.clone();
            let h2 = h.clone();
            // If the post never runs (the event loop is gone), dropping the closure frees the claim.
            let _ = h.run_on_main_thread(move || {
                let _claim = claim;
                idle(&s, &h2);
            });
        });
    }

    // Pick up MIDI devices plugged in after launch.
    let s = session.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        s.lock().refresh_midi();
    });
    Ok(())
}

/// One round of main-thread idle work: plug-in idle, a fresh `project` event when something
/// changed, and a due autosave. Skipped when the session is busy elsewhere.
#[cfg(not(target_os = "linux"))]
fn idle(s: &Mutex<Session>, h: &AppHandle) {
    if let Some(mut g) = s.try_lock() {
        if g.idle() {
            let _ = h.emit("project", commands::snapshot(&g));
        }
        autosave(&mut g);
    }
}

/// Lets a ticking helper thread keep at most one post to the main thread outstanding.
#[cfg(any(test, not(any(target_os = "linux", target_os = "macos"))))]
#[derive(Default)]
struct InFlight(std::sync::atomic::AtomicBool);

#[cfg(any(test, not(any(target_os = "linux", target_os = "macos"))))]
impl InFlight {
    /// Claims the slot, or `None` while the last claim hasn't been dropped yet.
    fn claim(self: &Arc<Self>) -> Option<Claim> {
        use std::sync::atomic::Ordering;
        self.0.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok()?;
        Some(Claim(self.clone()))
    }
}

/// Frees its slot when dropped, whether the post ran or was thrown away.
#[cfg(any(test, not(any(target_os = "linux", target_os = "macos"))))]
struct Claim(Arc<InFlight>);

#[cfg(any(test, not(any(target_os = "linux", target_os = "macos"))))]
impl Drop for Claim {
    fn drop(&mut self) {
        self.0 .0.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// On macOS idle runs from a `CFRunLoopTimer` on the main run loop. Added in the common modes, it
/// also fires while a menu is open or a window is being resized, and a busy main thread just
/// makes it fire late, never queue up.
#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::sync::Arc;

    use core_foundation::date::CFAbsoluteTimeGetCurrent;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext, CFRunLoopTimerRef};
    use daw_engine::Session;
    use parking_lot::Mutex;
    use tauri::AppHandle;

    struct Ctx {
        session: Arc<Mutex<Session>>,
        handle: AppHandle,
    }

    extern "C" fn fire(_timer: CFRunLoopTimerRef, info: *mut c_void) {
        // SAFETY: `info` is the leaked `Ctx` below, which lives as long as the app, and the
        // timer only fires on the main thread.
        let ctx = unsafe { &*(info as *const Ctx) };
        // A panic must not unwind into Core Foundation.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::idle(&ctx.session, &ctx.handle)));
    }

    pub fn start_idle_timer(session: Arc<Mutex<Session>>, handle: AppHandle) {
        let ctx = Box::into_raw(Box::new(Ctx { session, handle }));
        let mut context = CFRunLoopTimerContext { version: 0, info: ctx.cast(), retain: None, release: None, copyDescription: None };
        const INTERVAL: f64 = 0.030;
        // SAFETY: plain Core Foundation calls; the context is copied by the timer.
        let first = unsafe { CFAbsoluteTimeGetCurrent() } + INTERVAL;
        let timer = CFRunLoopTimer::new(first, INTERVAL, 0, 0, fire, &mut context);
        // SAFETY: `kCFRunLoopCommonModes` is an immutable Core Foundation constant.
        let mode = unsafe { kCFRunLoopCommonModes };
        // The run loop retains the timer, which then runs for the life of the app.
        CFRunLoop::get_main().add_timer(&timer, mode);
    }
}

/// Takes a due autosave on the main thread (plug-in states are read there) and writes it on
/// a worker thread, so the disk never holds up the interface.
fn autosave(s: &mut Session) {
    if let Some(job) = s.autosave_job() {
        std::thread::spawn(move || match job.write() {
            Ok(p) => log::info!("autosaved {}", p.display()),
            Err(e) => log::warn!("autosave failed: {e}"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_claim_at_a_time() {
        let f = Arc::new(InFlight::default());
        let a = f.claim().expect("free slot");
        assert!(f.claim().is_none(), "second claim while the first is in flight");
        assert!(f.claim().is_none());
        drop(a);
        let b = f.claim().expect("slot freed once the post ran or was dropped");
        assert!(f.claim().is_none());
        drop(b);
    }

    #[test]
    fn a_post_dropped_unrun_frees_the_slot() {
        let f = Arc::new(InFlight::default());
        let claim = f.claim().unwrap();
        let post = move || drop(claim);
        drop(post);
        assert!(f.claim().is_some());
    }
}
