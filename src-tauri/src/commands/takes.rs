//! Take folders and comps.

use super::*;

/// Quick Swipe Comping: `take` is heard over timeline `start..end` of the take folder
/// `region` (None silences that part).
#[tauri::command]
pub fn set_comp_range(state: State<AppState>, region: Id, take: Option<Id>, start: i64, end: i64) -> Snapshot {
    edit(&state, "Edit Comp", None, false, |s| s.set_comp_range(region, take, start, end))
}

#[tauri::command]
pub fn select_take(state: State<AppState>, region: Id, take: Id) -> Snapshot {
    edit(&state, "Select Take", None, false, |s| s.select_take(region, take))
}

/// Previous Take (`delta` -1) and Next Take (1) for the selected take folders.
#[tauri::command]
pub fn step_take(state: State<AppState>, regions: Vec<Id>, delta: i64) -> Snapshot {
    edit(&state, if delta < 0 { "Previous Take" } else { "Next Take" }, None, false, |s| s.step_take(&regions, delta))
}

/// Drags the comp edge at timeline `at` to `to` (a border moves both sections).
#[tauri::command]
pub fn move_comp_edge(state: State<AppState>, region: Id, at: i64, to: i64) -> Snapshot {
    edit(&state, "Move Comp Section Edge", None, false, |s| s.move_comp_edge(region, at, to))
}

/// Removes the comp section heard at timeline `at`.
#[tauri::command]
pub fn remove_comp_section(state: State<AppState>, region: Id, at: i64) -> Snapshot {
    edit(&state, "Remove Comp Section", None, false, |s| s.remove_comp_section(region, at))
}

/// Moves the comp section heard at timeline `at` to the take above (`delta` -1) or below (1).
#[tauri::command]
pub fn move_comp_section(state: State<AppState>, region: Id, at: i64, delta: i64) -> Snapshot {
    edit(&state, "Move Comp Section", None, false, |s| s.move_comp_section(region, at, delta))
}

#[tauri::command]
pub fn rename_take(state: State<AppState>, region: Id, take: Id, name: String) -> Snapshot {
    edit(&state, "Rename Take", None, false, |s| s.rename_take(region, take, name))
}

#[tauri::command]
pub fn delete_take(state: State<AppState>, region: Id, take: Id) -> Snapshot {
    edit(&state, "Delete Take", None, false, |s| s.delete_take(region, take))
}

#[tauri::command]
pub fn delete_unused_takes(state: State<AppState>, region: Id) -> Snapshot {
    edit(&state, "Delete Unused Takes", None, false, |s| s.delete_unused_takes(region))
}

/// Flatten: the comp as plain regions. Returns them.
#[tauri::command]
pub fn flatten_take_folder(state: State<AppState>, region: Id) -> R<Made> {
    edit_made(&state, "Flatten Take Folder", false, |s| Ok(s.flatten_take_folder(region)))
}

/// Flatten and Merge: the comp rendered to one new file. Returns the region.
#[tauri::command]
pub fn merge_take_folder(state: State<AppState>, region: Id) -> R<Made> {
    edit_made(&state, "Flatten and Merge", false, |s| s.merge_take_folder(region))
}

/// Unpack to New Tracks. Returns the new tracks.
#[tauri::command]
pub fn unpack_take_folder(state: State<AppState>, region: Id) -> R<Made> {
    edit_made(&state, "Unpack Take Folder", false, |s| Ok(s.unpack_take_folder(region)))
}

/// Unpack to Existing Tracks. Returns the tracks the takes went onto.
#[tauri::command]
pub fn unpack_to_existing_tracks(state: State<AppState>, region: Id) -> R<Made> {
    edit_made(&state, "Unpack Take Folder", false, |s| Ok(s.unpack_to_existing_tracks(region)))
}

/// Export Active Take to New Track, or with `remove` Move Active Take to New Track. Returns
/// the new track.
#[tauri::command]
pub fn export_active_take(state: State<AppState>, region: Id, remove: bool) -> R<Made> {
    let label = if remove { "Move Active Take" } else { "Export Active Take" };
    edit_made(&state, label, false, |s| Ok(s.export_active_take(region, remove)))
}

/// Pack Take Folder from audio regions on one track or several (onto the topmost). Returns
/// the folder.
#[tauri::command]
pub fn pack_take_folder(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Pack Take Folder", false, |s| s.pack_take_folder(&ids).map(|id| vec![id]))
}

/// Take folder menu › Create New Comp.
#[tauri::command]
pub fn create_comp(state: State<AppState>, region: Id) -> Snapshot {
    edit(&state, "Create New Comp", None, false, |s| s.create_comp(region))
}

/// Take folder menu › Duplicate Comp.
#[tauri::command]
pub fn duplicate_comp(state: State<AppState>, region: Id) -> Snapshot {
    edit(&state, "Duplicate Comp", None, false, |s| s.duplicate_comp(region))
}

/// Switches a take folder to its comp number `index` (from 0).
#[tauri::command]
pub fn select_comp(state: State<AppState>, region: Id, index: usize) -> Snapshot {
    edit(&state, "Select Comp", None, false, |s| s.select_comp(region, index))
}

#[tauri::command]
pub fn rename_comp(state: State<AppState>, region: Id, index: usize, name: String) -> Snapshot {
    edit(&state, "Rename Comp", None, false, |s| s.rename_comp(region, index, name))
}

#[tauri::command]
pub fn delete_comp(state: State<AppState>, region: Id, index: usize) -> Snapshot {
    edit(&state, "Delete Comp", None, false, |s| s.delete_comp(region, index))
}
