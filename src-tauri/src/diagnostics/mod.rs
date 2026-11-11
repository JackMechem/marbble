//! The Diagnostics window's side of the app: the window (`window.rs`), the logger that keeps the
//! log in memory for it (`logger.rs`, `log_ring.rs`), and the command it polls a few times a second
//! for a [`DiagnosticsSnapshot`]: the session's `Diagnostics` (crates/engine/src/session/diagnostics/),
//! the log lines it hasn't seen and what Marbble is running on. The window is src/diagnostics/.

mod log_ring;
pub mod logger;
pub mod window;

use std::time::Duration;

use daw_engine::session::Diagnostics;
use serde::Serialize;
use tauri::State;

use crate::AppState;
use log_ring::LogLine;

/// The longest a snapshot waits for the session, which edits and the tick hold for moments. Past it
/// the window skips a refresh rather than hold up anything.
const SESSION_WAIT: Duration = Duration::from_millis(5);

/// What the Diagnostics window gets each time it asks.
#[derive(Serialize)]
pub struct DiagnosticsSnapshot {
    pub version: String,
    pub os: &'static str,
    pub arch: &'static str,
    /// None when the session was busy; the window keeps what it had.
    pub engine: Option<Diagnostics>,
    /// The log lines after the one the window last had, oldest first.
    pub log: Vec<LogLine>,
}

/// A snapshot, with the log lines after `log_after` (0: every line still kept). Async, so it runs
/// off the main thread, and it waits for the session only briefly.
#[tauri::command]
pub async fn diagnostics(app: tauri::AppHandle, state: State<'_, AppState>, log_after: u64) -> Result<DiagnosticsSnapshot, String> {
    let engine = state.session.try_lock_for(SESSION_WAIT).map(|s| s.diagnostics());
    Ok(DiagnosticsSnapshot {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        engine,
        log: logger::ring().since(log_after),
    })
}

/// Starts the block-load figures afresh (the window's reset; it counts the rest from where they
/// stand itself).
#[tauri::command]
pub async fn reset_diagnostics(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(s) = state.session.try_lock_for(SESSION_WAIT * 4) {
        s.reset_load_statistics();
    }
    Ok(())
}
