//! Commands that edit regions: moving, trimming, looping, gain and fades, MIDI content, patterns and
//! the Tracks area's marquee, the Audio Track Editor's range edits, and importing audio and MIDI files.

use super::*;

#[tauri::command]
pub fn paste_regions(state: State<AppState>, items: Vec<(Id, Region)>, offset: i64, label: Option<String>) -> R<Made> {
    edit_made(&state, label.as_deref().unwrap_or("Paste"), false, |s| Ok(s.drag_paste_regions(&items, offset)))
}

#[tauri::command]
pub fn repeat_regions(state: State<AppState>, ids: Vec<Id>, times: usize) -> R<Made> {
    edit_made(&state, "Repeat Regions", false, |s| Ok(s.repeat_regions(&ids, times)))
}

#[tauri::command]
pub fn set_region_gain(state: State<AppState>, ids: Vec<Id>, db: Option<f32>, delta: Option<f32>) -> Snapshot {
    let key = format!("gain:{ids:?}");
    edit(&state, "Region Gain", Some(key), false, |s| s.set_region_gain(&ids, db, delta))
}

#[tauri::command]
pub fn normalize_regions(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Normalize", None, false, |s| s.normalize_regions(&ids))
}

#[tauri::command]
pub fn set_region_fades(state: State<AppState>, ids: Vec<Id>, fade_in: Option<i64>, fade_out: Option<i64>) -> Snapshot {
    edit(&state, "Fade", None, false, |s| s.set_region_fades(&ids, fade_in, fade_out))
}

#[tauri::command]
pub fn set_region_color(state: State<AppState>, ids: Vec<Id>, color: Option<String>) -> Snapshot {
    edit(&state, "Region Color", None, false, |s| s.set_region_color(&ids, color))
}

#[tauri::command]
pub fn transpose_regions(state: State<AppState>, ids: Vec<Id>, semitones: i32) -> Snapshot {
    let key = format!("transpose:{ids:?}");
    edit(&state, "Transpose", Some(key), false, |s| s.transpose_regions(&ids, semitones))
}

#[tauri::command]
pub fn quantize_regions(state: State<AppState>, ids: Vec<Id>, grid: i64) -> Snapshot {
    edit(&state, "Quantize", None, false, |s| s.quantize_regions(&ids, grid))
}

/// `key` lets the edit that fills the new region (the Piano Roll's first note) join its undo
/// step, as `set_midi_content` with the same key.
#[tauri::command]
pub fn create_midi_region(state: State<AppState>, track: Id, start: i64, length: i64, key: Option<String>) -> R<Made> {
    with(&state, |s| {
        let pushed = s.checkpoint("Create MIDI Region", key, false);
        let ids = vec![s.create_midi_region(track, start, length).map_err(|e| {
            if pushed {
                s.abandon_checkpoint();
            }
            e.to_string()
        })?];
        s.frozen_guard().map_err(|e| e.to_string())?;
        Ok(Made { snapshot: snapshot(s), ids })
    })
}

#[tauri::command]
pub fn regions_to_new_track(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Move to New Track", false, |s| {
        let made = s.regions_to_new_track(&ids);
        if let Some(&t) = made.last() {
            s.select_track(Some(t));
        }
        Ok(ids.clone())
    })
}

#[tauri::command]
pub fn reverse_regions(state: State<AppState>, ids: Vec<Id>) -> R<Snapshot> {
    try_edit(&state, "Reverse", None, false, |s| s.reverse_regions(&ids))
}

#[tauri::command]
pub fn join_regions(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Join Regions", false, |s| s.join_regions(&ids))
}

#[tauri::command]
pub fn move_region(state: State<AppState>, id: Id, start: i64, track: Option<Id>, automation: Option<bool>) -> Snapshot {
    edit(&state, "Move Region", None, false, |s| s.drag_move_regions(&[daw_engine::session::RegionMove { id, start, track }], automation))
}

#[tauri::command]
pub fn resize_region(state: State<AppState>, id: Id, start: i64, length: i64) -> Snapshot {
    edit(&state, "Resize Region", None, false, |s| s.drag_resize_region(id, start, length))
}

#[tauri::command]
pub fn delete_regions(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, if ids.len() > 1 { "Delete Regions" } else { "Delete Region" }, None, false, |s| s.drag_delete_regions(&ids))
}

#[tauri::command]
pub fn set_region_muted(state: State<AppState>, id: Id, muted: bool) -> Snapshot {
    edit(&state, if muted { "Mute Region" } else { "Unmute Region" }, None, false, |s| s.set_region_muted(id, muted))
}

#[tauri::command]
pub fn rename_region(state: State<AppState>, id: Id, name: String) -> Snapshot {
    edit(&state, "Rename Region", None, false, |s| s.rename_region(id, name))
}

#[tauri::command]
pub fn split_regions(state: State<AppState>, pos: i64, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Split Regions", None, false, |s| s.split_at(pos, &ids))
}

#[tauri::command]
pub fn import_audio(state: State<AppState>, path: String, track: Id, start: i64) -> R<Snapshot> {
    try_edit(&state, "Import Audio", None, false, |s| s.import_audio(&PathBuf::from(path), track, start).map(|_| ()))
}

/// The clip's waveform overview as raw bytes (an ArrayBuffer in JS); empty if it isn't loaded.
#[tauri::command(async)]
pub fn clip_waveform(state: State<AppState>, id: Id) -> tauri::ipc::Response {
    tauri::ipc::Response::new(with(&state, |s| s.clip_waveform(id)).unwrap_or_default())
}

#[derive(Serialize)]
pub struct ImportResult {
    snapshot: Snapshot,
    errors: Vec<String>,
}

/// Imports dropped audio files: onto `track` at `start` (extra files on new tracks below), or
/// onto new tracks inserted at `index`.
#[tauri::command]
pub fn import_audio_files(state: State<AppState>, paths: Vec<String>, track: Option<Id>, start: i64, index: Option<usize>) -> ImportResult {
    with(&state, |s| {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        s.checkpoint(if paths.len() > 1 { "Import Audio Files" } else { "Import Audio" }, None, false);
        let errors = s.import_audio_files(&paths, track, start, index);
        ImportResult { snapshot: snapshot(s), errors }
    })
}

/// Drags a region's loop to `loop_end` (None, or the region's own end, turns it off).
#[tauri::command]
pub fn set_region_loop(state: State<AppState>, id: Id, loop_end: Option<i64>) -> Snapshot {
    edit(&state, "Loop Region", None, false, |s| s.set_region_loop(id, loop_end))
}

/// The Loop switch (L): on loops up to the next region on the track, off removes the loop.
#[tauri::command]
pub fn set_regions_looped(state: State<AppState>, ids: Vec<Id>, on: bool) -> Snapshot {
    let label = if on { plural(ids.len(), "Loop Region", "Loop Regions") } else { "Turn Off Loop".into() };
    edit(&state, &label, None, false, |s| s.set_regions_looped(&ids, on))
}

/// Convert Loops to Real Copies. Returns the regions now standing for the loops (originals
/// and copies), for the UI to select.
#[tauri::command]
pub fn convert_loops(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Convert Loops to Real Copies", false, |s| {
        let mut all = ids.clone();
        all.extend(s.convert_loops(&ids));
        Ok(all)
    })
}

/// Splits the regions at every position given, as one undo step. Returns all the pieces.
#[tauri::command]
pub fn split_regions_at(state: State<AppState>, ids: Vec<Id>, positions: Vec<i64>) -> R<Made> {
    edit_made(&state, "Split Regions", false, |s| Ok(s.split_regions_at(&ids, &positions)))
}

/// Applies a range action (`action`: "split", "trim", "silence", "delete", "normalize",
/// "reverse", "fade_in" or "fade_out") to the parts of `ids` between `start` and `end`.
/// Returns the regions that now hold the range.
#[tauri::command]
pub fn edit_range(state: State<AppState>, ids: Vec<Id>, start: i64, end: i64, action: RangeEdit) -> R<Made> {
    edit_made(&state, action.label(), false, |s| s.edit_range(&ids, start, end, action))
}

/// Onset positions in the clip (frames from its start). Runs off the main thread, without
/// holding the session while it analyses.
#[tauri::command(async)]
pub fn clip_transients(state: State<AppState>, clip: Id, sensitivity: f32) -> Vec<i64> {
    let audio = with(&state, |s| s.clip_audio(clip));
    audio.map(|c| transients::detect(&c.channels, c.sample_rate, sensitivity)).unwrap_or_default()
}

#[tauri::command]
pub fn clip_info(state: State<AppState>, id: Id) -> Option<ClipInfo> {
    with(&state, |s| s.clip_info(id))
}

/// Moves regions by `delta` samples (Nudge Left / Right). With `from` and `to` (a nudge by a
/// bar, beat or note value), `from` goes to `to` and the others keep their bars and beats.
#[tauri::command]
pub fn nudge_regions(state: State<AppState>, ids: Vec<Id>, delta: i64, automation: Option<bool>, from: Option<i64>, to: Option<i64>) -> Snapshot {
    let key = format!("nudge:{ids:?}");
    edit(&state, "Nudge", Some(key), false, |s| match (from, to) {
        (Some(from), Some(to)) => s.drag_nudge_musical(&ids, from, to, automation),
        _ => s.drag_nudge_regions(&ids, delta, automation),
    })
}

/// Replaces the notes and controllers of MIDI regions (the Piano Roll's edits). `key`
/// merges a burst of edits (nudges, a drag sent in parts) into one undo step.
#[tauri::command]
pub fn set_midi_content(state: State<AppState>, edits: Vec<MidiContent>, label: String, key: Option<String>) -> Snapshot {
    edit(&state, &label, key, false, |s| s.set_midi_content(edits))
}

/// Imports a Standard MIDI File onto `track` (an instrument track) or new tracks at `index`.
#[tauri::command]
pub fn import_midi(state: State<AppState>, path: String, track: Option<Id>, start: i64, index: Option<usize>) -> R<Made> {
    edit_made(&state, "Import MIDI File", false, |s| s.import_midi(&PathBuf::from(path), track, start, index))
}

/// Writes MIDI regions to a Standard MIDI File.
#[tauri::command]
pub fn export_midi(state: State<AppState>, ids: Vec<Id>, path: String) -> R<()> {
    with(&state, |s| s.export_midi(&ids, &PathBuf::from(path))).map_err(|e| e.to_string())
}

/// Moves several regions as one undo step (a multi-region drag).
#[tauri::command]
pub fn move_regions(state: State<AppState>, moves: Vec<daw_engine::session::RegionMove>, automation: Option<bool>) -> Snapshot {
    edit(&state, if moves.len() == 1 { "Move Region" } else { "Move Regions" }, None, false, |s| s.drag_move_regions(&moves, automation))
}

/// Sets fade curves (-1..1) and types for the Region inspector; omitted settings stay. Curve
/// drags of the same regions share an undo step.
#[tauri::command]
pub fn set_region_fade_shape(
    state: State<AppState>,
    ids: Vec<Id>,
    in_curve: Option<f32>,
    out_curve: Option<f32>,
    in_type: Option<FadeInType>,
    out_type: Option<FadeOutType>,
) -> Snapshot {
    let curve = in_curve.is_some() || out_curve.is_some();
    let key = curve.then(|| format!("fade-curve:{ids:?}"));
    edit(&state, if curve { "Fade Curve" } else { "Fade Type" }, key, false, |s| s.set_region_fade_shape(&ids, in_curve, out_curve, in_type, out_type))
}

/// Crossfades each region with the one that follows it over `length` frames (0 removes it;
/// none: Settings › Editing › Crossfade Time).
#[tauri::command]
pub fn crossfade_regions(state: State<AppState>, ids: Vec<Id>, length: Option<i64>, kind: Option<FadeOutType>) -> Snapshot {
    edit(&state, if length != Some(0) { "Crossfade" } else { "Remove Crossfade" }, None, false, |s| {
        let length = length.unwrap_or_else(|| s.crossfade_frames());
        s.crossfade_regions(&ids, length, kind)
    })
}

/// The Tracks area's drag mode (not an undo step).
#[tauri::command]
pub fn set_drag_mode(state: State<AppState>, mode: DragMode) -> Snapshot {
    snap(&state, |s| s.set_drag_mode(mode))
}

/// Sets region playback parameters (Region inspector) on every region in `ids`. Steps on one
/// value of the same regions share an undo step.
#[tauri::command]
pub fn set_region_params(state: State<AppState>, ids: Vec<Id>, patch: RegionParamsPatch) -> Snapshot {
    let stepped = patch.swing.is_some() || patch.transpose.is_some() || patch.velocity.is_some() || patch.delay.is_some();
    let key = stepped.then(|| format!("region-params:{}:{ids:?}", patch.label()));
    edit(&state, patch.label(), key, false, |s| s.set_region_params(&ids, &patch))
}

/// Apply Region Parameters: writes the playback parameters into the regions and resets them.
#[tauri::command]
pub fn apply_region_params(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Apply Region Parameters", None, false, |s| s.apply_region_params(&ids))
}

/// Splits the regions on `tracks` at the marquee's edges, `start` and `end` (a Pointer click
/// inside the marquee). Returns the pieces inside it.
#[tauri::command]
pub fn marquee_split(state: State<AppState>, tracks: Vec<Id>, start: i64, end: i64) -> R<Made> {
    edit_made(&state, "Split at Marquee", false, |s| Ok(s.marquee_split(&tracks, start, end)))
}

/// Deletes the parts of the regions on `tracks` between `start` and `end`, in the drag mode.
#[tauri::command]
pub fn marquee_delete(state: State<AppState>, tracks: Vec<Id>, start: i64, end: i64) -> Snapshot {
    edit(&state, "Delete Marquee Selection", None, false, |s| s.marquee_delete(&tracks, start, end))
}

/// The parts of the regions on `tracks` between `start` and `end`, each with its track's
/// index, for the region clipboard. Not an edit.
#[tauri::command]
pub fn marquee_copy(state: State<AppState>, tracks: Vec<Id>, start: i64, end: i64) -> Vec<(usize, Region)> {
    with(&state, |s| s.marquee_copy(&tracks, start, end))
}

/// Track › New Pattern Region, or Create Pattern Region in a lane's menu: a pattern region with
/// the default drum kit on an instrument track.
#[tauri::command]
pub fn create_pattern_region(state: State<AppState>, track: Id, start: i64, length: i64) -> R<Made> {
    edit_made(&state, "Create Pattern Region", false, |s| s.create_pattern_region(track, start, length).map(|id| vec![id]))
}

/// The Step Sequencer's edits: replaces a pattern region's pattern. `key` merges a burst of
/// edits (the steps a drag paints) into one undo step.
#[tauri::command]
pub fn set_pattern(state: State<AppState>, id: Id, pattern: daw_engine::pattern::Pattern, label: String, key: Option<String>) -> R<Snapshot> {
    try_edit(&state, &label, key, false, |s| s.set_pattern(id, pattern))
}

/// Convert to MIDI Region: pattern regions become the MIDI regions they play.
#[tauri::command]
pub fn convert_patterns_to_midi(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Convert to MIDI Region", None, false, |s| s.convert_patterns_to_midi(&ids))
}

/// Convert to Pattern Region: MIDI regions become pattern regions on a sixteenth-note grid.
#[tauri::command]
pub fn convert_midi_to_patterns(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Convert to Pattern Region", None, false, |s| s.convert_midi_to_patterns(&ids))
}

/// Edit › Snap Edits to Zero Crossings (a project setting, not an undo step).
#[tauri::command]
pub fn set_snap_zero_crossings(state: State<AppState>, on: bool) -> Snapshot {
    snap(&state, |s| s.set_snap_zero_crossings(on))
}

/// A region gain drag in the Tracks area: `gains` holds each region's new gain in dB. The
/// drag's `first` update records the undo step and the rest join it, so a drag is one step
/// however long it is held.
#[tauri::command]
pub fn set_region_gains(state: State<AppState>, gains: Vec<(Id, f32)>, first: bool) -> Snapshot {
    if first {
        edit(&state, "Region Gain", None, false, |s| s.set_region_gains(&gains))
    } else {
        snap(&state, |s| s.set_region_gains(&gains))
    }
}

/// Crossfades a looping region's repetitions into each other, or stops doing so.
#[tauri::command]
pub fn set_loop_crossfade(state: State<AppState>, ids: Vec<Id>, on: bool) -> Snapshot {
    edit(&state, "Crossfade Loop Repetitions", None, false, |s| s.set_loop_crossfade(&ids, on))
}
