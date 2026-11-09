//! Automation lanes and modes, and controller assignments (MIDI Learn).

use super::*;

/// Replaces an automation lane's points. Drags of one lane share an undo step.
#[tauri::command]
pub fn set_automation_lane(state: State<AppState>, track: Id, param: AutoParam, points: Vec<AutoPoint>) -> Snapshot {
    let key = format!("automation:{track}:{}", serde_json::to_string(&param).unwrap_or_default());
    edit(&state, "Edit Automation", Some(key), false, |s| s.set_automation_lane(track, param, points))
}

#[tauri::command]
pub fn set_auto_mode(state: State<AppState>, track: Id, mode: AutoMode) -> Snapshot {
    edit(&state, "Automation Mode", None, false, |s| s.set_auto_mode(track, mode))
}

/// Deletes one lane, or all of the track's automation without `param`.
#[tauri::command]
pub fn clear_automation(state: State<AppState>, track: Id, param: Option<AutoParam>) -> Snapshot {
    edit(&state, "Delete Automation", None, false, |s| s.clear_automation(track, param))
}

/// The UI grabbed or let go of a fader, knob or slider. Writing in Touch and Latch follows it.
#[tauri::command]
pub fn automation_touch(state: State<AppState>, track: Id, param: AutoParam, touching: bool) -> Snapshot {
    if touching {
        with(&state, |s| s.touched_auto(track, &param));
    }
    snap(&state, |s| s.automation_touch(track, param, touching))
}

/// The Controller Assignments window's list.
#[tauri::command]
pub fn controller_assignments(state: State<AppState>) -> Vec<AssignmentRow> {
    with(&state, |s| s.controller_assignments())
}

/// Learn Assignment: the next controller moved is assigned to `target`, or (null) to the last
/// touched parameter. Returns the parameter's name.
#[tauri::command]
pub fn start_learn(state: State<AppState>, target: Option<CtlTarget>) -> R<String> {
    with(&state, |s| s.start_learn(target)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_learn(state: State<AppState>) {
    with(&state, |s| s.cancel_learn())
}

/// Changes an assignment's takeover mode, range or button behaviour (global settings, not an
/// undo step).
#[tauri::command]
pub fn update_controller_assignment(state: State<AppState>, index: usize, patch: AssignmentPatch) -> R<Vec<AssignmentRow>> {
    with(&state, |s| s.update_controller_assignment(index, patch).map(|_| s.controller_assignments())).map_err(|e| e.to_string())
}

/// Turns an assignment's feedback to its controller on or off (a global setting).
#[tauri::command]
pub fn set_controller_feedback(state: State<AppState>, index: usize, on: bool) -> R<Vec<AssignmentRow>> {
    with(&state, |s| s.set_controller_feedback(index, on).map(|_| s.controller_assignments())).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_controller_assignment(state: State<AppState>, index: usize) -> R<Vec<AssignmentRow>> {
    with(&state, |s| s.delete_controller_assignment(index).map(|_| s.controller_assignments())).map_err(|e| e.to_string())
}

/// Sets the extra automation lanes shown under tracks: `[track, parameters]` pairs, with
/// `MASTER_ID` for the Output track. One undo step.
#[tauri::command]
pub fn set_auto_lanes(state: State<AppState>, lanes: Vec<(Id, Vec<AutoParam>)>) -> Snapshot {
    edit(&state, "Automation Lanes", None, false, |s| s.set_auto_lanes(lanes))
}
