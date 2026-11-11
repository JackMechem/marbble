//! The Window menu: Minimize, and the list of open windows (the main window, then the panel
//! windows), kept in step as windows open, close and come to the front.

use tauri::menu::{MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Manager, Runtime};

use super::MenuBar;

/// Window › Minimize (the window in front) and the list of windows (brings one to the front).
/// False for other items.
pub(super) fn window_command<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    if id == "minimize" {
        let windows = app.webview_windows();
        let front = windows.values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| windows.get("main"));
        if let Some(w) = front {
            let _ = w.minimize();
        }
        return true;
    }
    let Some(label) = id.strip_prefix("window:") else { return false };
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    true
}

/// The windows Window lists, in its order: the main window, then the panel windows and the
/// Diagnostics window by title.
/// `gone` is a window being destroyed.
pub(super) fn window_list<R: Runtime>(app: &AppHandle<R>, gone: Option<&str>) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| {
            Some(label.as_str()) != gone && (label == "main" || label == crate::diagnostics::window::LABEL || crate::windows::is_panel_label(label))
        })
        .map(|(label, w)| {
            let title = w.title().unwrap_or_default();
            (label, if title.is_empty() { "Marbble".to_string() } else { title })
        })
        .collect();
    list.sort_by(|a, b| (a.0 != "main", &a.1).cmp(&(b.0 != "main", &b.1)));
    list
}

/// Brings Window's list of windows up to date, later on the main thread (a window may be in the
/// middle of opening or closing now), and draws the radio items in each window's menu bar.
pub(super) fn refresh_windows<R: Runtime>(app: &AppHandle<R>, gone: Option<String>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(bar) = handle.try_state::<MenuBar<R>>() else { return };
        let list = window_list(&handle, gone.as_deref());
        let changed = {
            let mut shown = bar.shown.lock();
            let changed = shown.windows != list;
            shown.windows = list.clone();
            changed
        };
        if changed {
            if let Err(e) = bar.fill_window_list(&handle, &list) {
                log::warn!("Window menu: {e}");
            }
        }
        #[cfg(target_os = "linux")]
        for w in handle.webview_windows().values() {
            bar.draw_radios_in(w);
        }
    });
}

/// A window came to the front: a panel window opened since the keys last changed gets them
/// in its menu bar, and Window lists it under its current title (windows.rs `on_window_event`).
pub fn on_window_focused<R: Runtime>(window: &tauri::Window<R>) {
    #[cfg(target_os = "linux")]
    if let (Some(bar), Some(w)) = (window.app_handle().try_state::<MenuBar<R>>(), window.app_handle().get_webview_window(window.label())) {
        bar.show_keys_in(&w);
    }
    refresh_windows(window.app_handle(), None);
}

/// A window closed: one opened later under its label (a panel window) has a new menu bar, and
/// Window stops listing it.
pub fn on_window_destroyed<R: Runtime>(window: &tauri::Window<R>) {
    if let Some(bar) = window.app_handle().try_state::<MenuBar<R>>() {
        bar.shown.lock().applied.remove(window.label());
    }
    refresh_windows(window.app_handle(), Some(window.label().to_string()));
}

impl<R: Runtime> MenuBar<R> {
    /// Window's list of windows: a separator, then one item per window.
    pub(super) fn fill_window_list(&self, app: &AppHandle<R>, list: &[(String, String)]) -> tauri::Result<()> {
        let (menu, fixed) = &self.window;
        for it in menu.items()?.iter().skip(*fixed) {
            menu.remove(it)?;
        }
        if !list.is_empty() {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
        for (label, title) in list {
            menu.append(&MenuItem::with_id(app, format!("window:{label}"), title, true, None::<&str>)?)?;
        }
        Ok(())
    }
}
