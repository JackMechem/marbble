//! File › Project Alternatives and File › Revert to, rebuilt whenever the project, its alternative
//! or its saved state changes.

use serde::Deserialize;
use tauri::menu::{CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Runtime};

use super::window_menu::refresh_windows;
use super::MenuBar;

/// A backup in File › Revert to: its file (the item's id is `revert:<file>`) and its label in
/// local time, which the frontend formats.
#[derive(Deserialize)]
pub struct BackupItem {
    file: String,
    label: String,
}

pub(super) fn fill_project_menus<R: Runtime>(
    app: &AppHandle<R>,
    alternatives: &Submenu<R>,
    revert: &Submenu<R>,
    names: &[String],
    active: &str,
    backups: &[BackupItem],
) -> tauri::Result<()> {
    for it in alternatives.items()? {
        alternatives.remove(&it)?;
    }
    for name in names {
        alternatives.append(&CheckMenuItem::with_id(app, format!("alternative:{name}"), name, true, name == active, None::<&str>)?)?;
    }
    if !names.is_empty() {
        alternatives.append(&PredefinedMenuItem::separator(app)?)?;
    }
    alternatives.append(&MenuItem::with_id(app, "alt-new", "New Alternative…", true, None::<&str>)?)?;
    alternatives.append(&MenuItem::with_id(app, "alt-edit", "Edit Alternatives…", true, None::<&str>)?)?;
    for it in revert.items()? {
        revert.remove(&it)?;
    }
    if backups.is_empty() {
        revert.append(&MenuItem::with_id(app, "revert-none", "No Backups", false, None::<&str>)?)?;
    }
    for b in backups {
        revert.append(&MenuItem::with_id(app, format!("revert:{}", b.file), &b.label, true, None::<&str>)?)?;
    }
    Ok(())
}

/// Rebuilds File › Project Alternatives (`alternatives`, `active` ticked) and File › Revert to
/// (`backups`, newest first). The frontend calls it when the project, its alternative or its
/// saved state changes (src/project/projectFiles.ts).
#[tauri::command]
pub fn set_project_menus<R: Runtime>(app: AppHandle<R>, alternatives: Vec<String>, active: String, backups: Vec<BackupItem>) -> Result<(), String> {
    let Some(menus) = app.try_state::<MenuBar<R>>() else { return Ok(()) };
    fill_project_menus(&app, &menus.alternatives, &menus.revert, &alternatives, &active, &backups).map_err(|e| e.to_string())?;
    // The alternatives' new items are drawn as radio buttons, and the main window's title (the
    // project's name) may have changed.
    refresh_windows(&app, None);
    Ok(())
}
