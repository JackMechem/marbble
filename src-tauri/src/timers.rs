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
    #[cfg(not(target_os = "linux"))]
    {
        let s = session.clone();
        let h = handle.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(30));
            let s = s.clone();
            let h2 = h.clone();
            let _ = h.run_on_main_thread(move || {
                if let Some(mut g) = s.try_lock() {
                    if g.idle() {
                        let _ = h2.emit("project", commands::snapshot(&g));
                    }
                    autosave(&mut g);
                }
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
