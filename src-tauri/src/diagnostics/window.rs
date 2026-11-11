//! The Diagnostics window itself (Window › Diagnostics): opened or brought to the front from the
//! menu, labelled `diagnostics`. It loads the same frontend as every window, and main.tsx shows the
//! Diagnostics view for this label. It closes with the main window (windows.rs).

use tauri::window::Color;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// The window's label; `capabilities/default.json` lists it beside the main window.
pub const LABEL: &str = "diagnostics";

/// Opens the Diagnostics window, or brings it to the front if it is open.
///
/// Async because creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_diagnostics_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::default())
        .title("Diagnostics")
        .inner_size(920.0, 680.0)
        .min_inner_size(520.0, 320.0)
        .background_color(Color(0x1c, 0x1c, 0x1e, 0xff))
        .build()
        .map_err(|e| e.to_string())?;
    let _ = w.set_focus();
    Ok(())
}
