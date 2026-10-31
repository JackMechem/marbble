//! Markers, the Arrangement track, and the arranging commands of Edit › Section Between Locators,
//! Trim, Strip Silence and friends, which work across regions and tracks.

use super::*;

/// Create Marker at `pos` (the playhead, snapped by the UI). Returns the marker.
#[tauri::command]
pub fn add_marker(state: State<AppState>, pos: i64, name: Option<String>) -> R<Made> {
    edit_made(&state, "Create Marker", false, |s| Ok(vec![s.add_marker(pos, name)]))
}

#[tauri::command]
pub fn delete_markers(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, &plural(ids.len(), "Delete Marker", "Delete Markers"), None, false, |s| s.delete_markers(&ids))
}

#[tauri::command]
pub fn rename_marker(state: State<AppState>, id: Id, name: String) -> Snapshot {
    edit(&state, "Rename Marker", None, false, |s| s.rename_marker(id, name))
}

#[tauri::command]
pub fn move_marker(state: State<AppState>, id: Id, pos: i64) -> Snapshot {
    edit(&state, "Move Marker", None, false, |s| s.move_marker(id, pos))
}

#[tauri::command]
pub fn set_marker_color(state: State<AppState>, id: Id, color: Option<String>) -> Snapshot {
    edit(&state, "Marker Color", None, false, |s| s.set_marker_color(id, color))
}

/// Sets a marker's own length; None (or 0) makes it reach the next marker again.
#[tauri::command]
pub fn set_marker_length(state: State<AppState>, id: Id, length: Option<i64>) -> Snapshot {
    edit(&state, "Marker Length", None, false, |s| s.set_marker_length(id, length))
}

/// The Arrangement track's + button: a section after the last one, named `name` or from
/// Logic's list. Returns its id.
#[tauri::command]
pub fn add_arrangement_marker(state: State<AppState>, name: Option<String>) -> R<Made> {
    edit_made(&state, "Create Arrangement Marker", false, |s| Ok(vec![s.add_arrangement_marker(name)]))
}

#[tauri::command]
pub fn rename_arrangement_marker(state: State<AppState>, id: Id, name: String) -> Snapshot {
    edit(&state, "Rename Arrangement Marker", None, false, |s| s.rename_arrangement_marker(id, name))
}

/// None colours the section by its name.
#[tauri::command]
pub fn set_arrangement_marker_color(state: State<AppState>, id: Id, color: Option<String>) -> Snapshot {
    edit(&state, "Arrangement Marker Color", None, false, |s| s.set_arrangement_marker_color(id, color))
}

/// Resizes a section; the later ones move along (their content stays).
#[tauri::command]
pub fn resize_arrangement_marker(state: State<AppState>, id: Id, length: i64) -> Snapshot {
    edit(&state, "Resize Arrangement Marker", None, false, |s| s.resize_arrangement_marker(id, length))
}

/// Moves section `id` with its content before section `before` (null: to the end), or with
/// `copy` inserts a copy there. Returns the regions in the moved or copied section.
#[tauri::command]
pub fn move_arrangement_marker(state: State<AppState>, id: Id, before: Option<Id>, copy: bool) -> R<Made> {
    let label = if copy { "Copy Arrangement Marker" } else { "Move Arrangement Marker" };
    edit_made(&state, label, false, |s| s.move_arrangement_marker(id, before, copy))
}

/// Deletes a section: just the marker, or with `content` its time and everything in it.
#[tauri::command]
pub fn delete_arrangement_marker(state: State<AppState>, id: Id, content: bool) -> R<Snapshot> {
    let label = if content { "Delete Arrangement Marker and Content" } else { "Delete Arrangement Marker" };
    try_edit(&state, label, None, false, |s| s.delete_arrangement_marker(id, content))
}

/// Edit › Section Between Locators: `edit` is "insert_silence", "cut" or "repeat", between
/// `start` and `end`, on `tracks` (a marquee's) or across the project (null). Returns the
/// repeated regions, to select.
#[tauri::command]
pub fn section_edit(state: State<AppState>, edit: daw_engine::arrange_cmds::SectionEdit, start: i64, end: i64, tracks: Option<Vec<Id>>) -> R<Made> {
    edit_made(&state, edit.label(), false, |s| s.section_edit(edit, start, end, tracks.as_deref()))
}

/// Insert Cut Section: puts the section cut last back at `at`. Returns its regions.
#[tauri::command]
pub fn insert_cut_section(state: State<AppState>, at: i64) -> R<Made> {
    edit_made(&state, "Insert Cut Section", false, |s| s.insert_cut_section(at))
}

#[tauri::command]
pub fn remove_gaps(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Remove Gaps", None, false, |s| s.remove_gaps(&ids))
}

#[tauri::command]
pub fn remove_overlaps(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Remove Overlaps", None, false, |s| s.remove_overlaps(&ids))
}

#[tauri::command]
pub fn trim_to_next_region(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Trim Region End to Next Region", None, false, |s| s.trim_to_next_region(&ids))
}

#[tauri::command]
pub fn fill_within_locators(state: State<AppState>, ids: Vec<Id>, start: i64, end: i64) -> Snapshot {
    edit(&state, "Fill Within Locators", None, false, |s| s.fill_within_locators(&ids, start, end))
}

#[tauri::command]
pub fn resize_to_same_length(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Resize to Same Length", None, false, |s| s.resize_to_same_length(&ids))
}

/// Trim Region Start (`end` false) or End to the playhead at `pos`.
#[tauri::command]
pub fn trim_regions_to(state: State<AppState>, ids: Vec<Id>, pos: i64, end: bool) -> Snapshot {
    let label = if end { "Trim Region End to Playhead" } else { "Trim Region Start to Playhead" };
    edit(&state, label, None, false, |s| s.trim_regions_to(&ids, pos, end))
}

/// Crop Regions Outside Locators (or the marquee). Returns what is left, to select.
#[tauri::command]
pub fn crop_regions(state: State<AppState>, ids: Vec<Id>, start: i64, end: i64) -> R<Made> {
    edit_made(&state, "Crop Regions", false, |s| s.crop_regions(&ids, start, end))
}

#[tauri::command]
pub fn delete_and_shuffle(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Delete and Shuffle", None, false, |s| s.delete_and_shuffle(&ids))
}

/// Paste Replace: as `paste_regions`, deleting what lies under the pasted range first.
#[tauri::command]
pub fn paste_replace(state: State<AppState>, items: Vec<(Id, Region)>, offset: i64) -> R<Made> {
    edit_made(&state, "Paste Replace", false, |s| Ok(s.paste_replace(&items, offset)))
}

/// Move Region to Playhead (Pickup Clock): the first region starts at `pos`.
#[tauri::command]
pub fn move_regions_to(state: State<AppState>, ids: Vec<Id>, pos: i64) -> Snapshot {
    edit(&state, "Move Region to Playhead", None, false, |s| s.move_regions_to(&ids, pos))
}

/// The timeline ranges Strip Silence would keep of region `id`. Not an edit.
#[tauri::command]
pub fn strip_silence_preview(state: State<AppState>, id: Id, settings: daw_engine::strip_silence::StripSettings) -> Vec<(i64, i64)> {
    with(&state, |s| s.strip_silence_preview(id, &settings))
}

/// Strip Silence: splits the audio regions around the parts that sound. Returns the parts.
#[tauri::command]
pub fn strip_silence(state: State<AppState>, ids: Vec<Id>, settings: daw_engine::strip_silence::StripSettings) -> R<Made> {
    edit_made(&state, "Strip Silence", false, |s| Ok(s.strip_silence(&ids, &settings)))
}

#[tauri::command]
pub fn name_regions_by_track(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Name Regions by Track", None, false, |s| s.name_regions_by_track(&ids))
}

#[tauri::command]
pub fn name_tracks_by_region(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Name Tracks by Region", None, false, |s| s.name_tracks_by_region(&ids))
}

#[tauri::command]
pub fn color_regions_by_track(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Color Regions by Track", None, false, |s| s.color_regions_by_track(&ids))
}

#[tauri::command]
pub fn color_tracks_by_region(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Color Tracks by Region", None, false, |s| s.color_tracks_by_region(&ids))
}

/// The undo label of a drag past the last track: `verb` is "Move" or "Copy".
fn new_track_label(verb: &str, regions: usize, tracks: usize) -> String {
    format!("{verb} {} to New {}", plural(regions, "Region", "Regions"), plural(tracks, "Track", "Tracks"))
}

/// A drag past the last track: `lanes` are (source track, moves) for the regions that go onto
/// a new track each, `moves` the regions landing on existing tracks. One undo step.
#[tauri::command]
pub fn move_regions_to_new_tracks(state: State<AppState>, moves: Vec<daw_engine::session::RegionMove>, lanes: Vec<(Id, Vec<daw_engine::session::RegionMove>)>, automation: Option<bool>) -> R<Made> {
    let regions = moves.len() + lanes.iter().map(|l| l.1.len()).sum::<usize>();
    edit_made(&state, &new_track_label("Move", regions, lanes.len()), false, |s| Ok(s.drag_regions_to_new_tracks(&moves, &lanes, automation)))
}

/// An Alt-drag past the last track: as `move_regions_to_new_tracks`, with copies in place of
/// moves. Returns the copies.
#[tauri::command]
pub fn copy_regions_to_new_tracks(state: State<AppState>, items: Vec<(Id, Region)>, lanes: Vec<(Id, Vec<Region>)>) -> R<Made> {
    let regions = items.len() + lanes.iter().map(|l| l.1.len()).sum::<usize>();
    edit_made(&state, &new_track_label("Copy", regions, lanes.len()), false, |s| Ok(s.drag_copy_regions_to_new_tracks(&items, &lanes)))
}
