//! Commands that add, remove and arrange tracks and change their settings: stacks, groups, sends and
//! outputs, VCAs, track alternatives, MIDI settings and Freeze.

use super::*;

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn add_tracks(
    state: State<AppState>,
    kind: TrackKind,
    count: usize,
    input: InputSel,
    ascending: bool,
    instrument: Option<String>,
    armed: bool,
    monitor: bool,
) -> R<Snapshot> {
    try_edit(&state, if count > 1 { "New Tracks" } else { "New Track" }, None, false, |s| {
        let after = s.selected.and_then(|id| s.project.tracks.iter().position(|t| t.id == id)).map(|i| i + 1);
        let mut last = None;
        for n in 0..count.clamp(1, 64) {
            let input = match input {
                InputSel::Mono(c) if ascending => InputSel::Mono(c + n as u16),
                InputSel::Stereo(c) if ascending => InputSel::Stereo(c + 2 * n as u16),
                other => other,
            };
            let idx = after.map(|i| i + n);
            let id = s.add_track(kind, input, instrument.clone(), idx)?;
            if armed || monitor {
                s.update_track(id, TrackPatch { armed: Some(armed), monitor: Some(monitor), ..Default::default() });
            }
            last = Some(id);
        }
        s.select_track(last);
        Ok(())
    })
}

#[tauri::command]
pub fn delete_track(state: State<AppState>, id: Id) -> Snapshot {
    edit(&state, "Delete Track", None, true, |s| s.delete_track(id))
}

#[tauri::command]
pub fn update_track(state: State<AppState>, id: Id, patch: TrackPatch) -> Snapshot {
    if patch.off.is_some() {
        if let Some(refused) = with(&state, |s| s.off_refused()) {
            return Snapshot { refused: Some(refused), ..snap(&state, |_| {}) };
        }
    }
    // Mute, solo, record-enable and monitoring are performance switches, not edits (as in Logic).
    let label = if patch.volume_db.is_some() {
        Some(("Volume", "volume"))
    } else if patch.pan.is_some() {
        Some(("Pan", "pan"))
    } else if patch.name.is_some() {
        Some(("Rename Track", "name"))
    } else if patch.color.is_some() {
        Some(("Track Color", "color"))
    } else if patch.input.is_some() {
        Some(("Track Input", "input"))
    } else {
        None
    };
    // Moves recorded as automation belong to the pass's undo step.
    let label = label.filter(|_| !with(&state, |s| s.automation_writing(id)));
    with(&state, |s| s.touched_patch(id, &patch));
    match label {
        Some((label, field)) => edit(&state, label, Some(format!("track:{id}:{field}")), false, |s| s.update_track_linked(id, patch)),
        None => snap(&state, |s| s.update_track_linked(id, patch)),
    }
}

/// Logic's Track Icon for `ids` (None: back to the default the UI picks).
#[tauri::command]
pub fn set_track_icon(state: State<AppState>, ids: Vec<Id>, icon: Option<String>) -> Snapshot {
    edit(&state, "Track Icon", None, false, |s| s.set_track_icon(&ids, icon))
}

#[tauri::command]
pub fn delete_tracks(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, &plural(ids.len(), "Delete Track", "Delete Tracks"), None, true, |s| s.delete_tracks(&ids))
}

#[tauri::command]
pub fn delete_unused_tracks(state: State<AppState>) -> Made {
    with(&state, |s| {
        s.checkpoint("Delete Unused Tracks", None, true);
        let n = s.delete_unused_tracks();
        Made { snapshot: snapshot(s), ids: vec![n as Id] }
    })
}

/// `content` copies the regions too (Duplicate Track); without it, New Track with Same Settings.
#[tauri::command]
pub fn duplicate_tracks(state: State<AppState>, ids: Vec<Id>, content: bool) -> R<Made> {
    let label = if content { plural(ids.len(), "Duplicate Track", "Duplicate Tracks") } else { "New Track with Same Settings".into() };
    edit_made(&state, &label, false, |s| {
        let made = s.duplicate_tracks(&ids, content);
        if let Some(&last) = made.last() {
            s.select_track(Some(last));
        }
        Ok(made)
    })
}

#[tauri::command]
pub fn create_stack(state: State<AppState>, ids: Vec<Id>, summing: bool) -> R<Made> {
    edit_made(&state, "Create Track Stack", false, |s| {
        let id = s.create_stack(&ids, summing).ok_or_else(|| anyhow::anyhow!("select the tracks to put in a stack first"))?;
        s.select_track(Some(id));
        Ok(vec![id])
    })
}

#[tauri::command]
pub fn flatten_stack(state: State<AppState>, id: Id) -> Snapshot {
    edit(&state, "Flatten Stack", None, true, |s| s.flatten_stack(id))
}

/// Reorders tracks; `parents` moves some into a stack (Some) or out of one (None).
#[tauri::command]
pub fn move_tracks(state: State<AppState>, ids: Vec<Id>, parents: Vec<(Id, Option<Id>)>) -> Snapshot {
    edit(&state, "Move Tracks", None, false, |s| s.move_tracks(ids, &parents))
}

#[tauri::command]
pub fn create_group(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Create Group", false, |s| Ok(vec![s.create_group(&ids)]))
}

#[tauri::command]
pub fn update_group(state: State<AppState>, group: Group) -> Snapshot {
    let key = format!("group:{}", group.id);
    edit(&state, "Group Settings", Some(key), false, |s| s.update_group(group))
}

#[tauri::command]
pub fn delete_group(state: State<AppState>, id: Id) -> Snapshot {
    edit(&state, "Delete Group", None, false, |s| s.delete_group(id))
}

#[tauri::command]
pub fn set_track_group(state: State<AppState>, ids: Vec<Id>, group: Option<Id>) -> Snapshot {
    edit(&state, if group.is_some() { "Assign Group" } else { "Remove from Group" }, None, false, |s| s.set_track_group(&ids, group))
}

#[tauri::command]
pub fn select_track(state: State<AppState>, id: Option<Id>) -> Snapshot {
    snap(&state, |s| s.select_track(id))
}

#[tauri::command]
pub fn reorder_tracks(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Reorder Tracks", None, false, |s| s.reorder_tracks(ids))
}

/// Feeds the Channel EQ editor's analyzer from the output of the insert `uid` (None: off).
#[tauri::command]
pub fn set_analyzer(state: State<AppState>, uid: Option<Id>) {
    with(&state, |s| s.set_analyzer(uid))
}

#[tauri::command]
pub fn set_tracks_hidden(state: State<AppState>, ids: Vec<Id>, hidden: bool) -> Snapshot {
    edit(&state, if hidden { "Hide Tracks" } else { "Show Tracks" }, None, false, |s| s.set_tracks_hidden(&ids, hidden))
}

/// A new aux at the end of the track list. `send_from` (track, slot) fills that send slot
/// with a 0 dB post-fader send to it, and `output_from` routes that track's output to it, both
/// in the same undo step.
#[tauri::command]
pub fn create_aux(state: State<AppState>, name: Option<String>, send_from: Option<(Id, usize)>, output_from: Option<Id>) -> R<Made> {
    edit_made(&state, "New Aux", false, |s| {
        let id = s.create_aux(name)?;
        if let Some((track, slot)) = send_from {
            s.set_send(track, slot, Some(TrackSend { bus: id, level_db: 0.0, pre_fader: false, muted: false }))?;
        }
        if let Some(track) = output_from {
            s.set_output(track, Some(id))?;
        }
        Ok(vec![id])
    })
}

/// Fills (`send`) or clears (null) a send slot; refuses feedback loops.
#[tauri::command]
pub fn set_send(state: State<AppState>, track: Id, slot: usize, send: Option<TrackSend>) -> R<Snapshot> {
    let label = if send.is_some() { "Send" } else { "Remove Send" };
    try_edit(&state, label, None, false, |s| s.set_send(track, slot, send))
}

/// A send knob drag; consecutive changes to one send make one undo step.
#[tauri::command]
pub fn set_send_level(state: State<AppState>, track: Id, slot: usize, level_db: f32) -> Snapshot {
    with(&state, |s| s.touched(CtlTarget::Send { track, slot: slot as u8 }));
    // Moves recorded as automation belong to the pass's undo step.
    if with(&state, |s| s.automation_writing(track)) {
        return snap(&state, |s| s.set_send_level_linked(track, slot, level_db));
    }
    edit(&state, "Send Level", Some(format!("send:{track}:{slot}")), false, |s| s.set_send_level_linked(track, slot, level_db))
}

/// Routes a track to an aux, to Stereo Out (0) or back to its default (null).
#[tauri::command]
pub fn set_output(state: State<AppState>, track: Id, output: Option<Id>) -> R<Snapshot> {
    try_edit(&state, "Track Output", None, false, |s| s.set_output(track, output))
}

/// Freezes tracks: renders each through its instrument and plug-ins (off the main thread, like
/// a bounce, reporting `bounce-progress`), then plays the renders and unloads the plug-ins on
/// the main thread, as one undo step.
#[tauri::command(async)]
pub fn freeze_tracks(app: tauri::AppHandle, state: State<AppState>, ids: Vec<Id>) -> R<Snapshot> {
    let mut p = progress(&app);
    let renders = with(&state, |s| s.render_freeze(&ids, &mut p)).map_err(|e| e.to_string())?;
    let session = state.session.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let mut s = session.lock();
        s.checkpoint(if ids.len() > 1 { "Freeze Tracks" } else { "Freeze Track" }, None, true);
        let res = s.place_freeze(renders).map(|_| snapshot(&s));
        if res.is_err() {
            s.abandon_checkpoint();
        }
        let _ = tx.send(res.map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// Unfreezes tracks: their plug-ins load again and their freeze files leave the project.
#[tauri::command]
pub fn unfreeze_tracks(state: State<AppState>, ids: Vec<Id>) -> R<Snapshot> {
    try_edit(&state, if ids.len() > 1 { "Unfreeze Tracks" } else { "Unfreeze Track" }, None, true, |s| ids.iter().try_for_each(|&id| s.unfreeze_track(id)))
}

/// Mixer › Options › Create New VCA (or a strip's VCA slot › New VCA…): a new VCA, with
/// `tracks` assigned to it. Returns its id.
#[tauri::command]
pub fn create_vca(state: State<AppState>, tracks: Vec<Id>) -> R<Made> {
    edit_made(&state, "Create New VCA", false, |s| Ok(vec![s.create_vca(&tracks)]))
}

#[tauri::command]
pub fn delete_vca(state: State<AppState>, id: Id) -> Snapshot {
    edit(&state, "Delete VCA", None, false, |s| s.delete_vca(id))
}

/// Changes a VCA's name, colour, level or mute. A fader drag is one undo step; mute is a
/// performance switch, not an edit, as on a track.
#[tauri::command]
pub fn update_vca(state: State<AppState>, id: Id, patch: daw_engine::session::VcaPatch) -> Snapshot {
    let label = match (&patch.level_db, &patch.name, &patch.color) {
        (Some(_), ..) => Some(("VCA Level", Some(format!("vca:{id}:level")))),
        (_, Some(_), _) => Some(("Rename VCA", None)),
        (_, _, Some(_)) => Some(("VCA Color", None)),
        _ => None,
    };
    match label {
        Some((label, key)) => edit(&state, label, key, false, |s| {
            // A fader drag: Learn Assignment learns the VCA's level.
            if patch.level_db.is_some() {
                s.touched(CtlTarget::VcaLevel { vca: id });
            }
            s.update_vca(id, patch)
        }),
        None => snap(&state, |s| {
            if patch.mute.is_some() {
                s.touched(CtlTarget::VcaMute { vca: id });
            }
            s.update_vca(id, patch)
        }),
    }
}

/// Assigns tracks to a VCA (null: No VCA).
#[tauri::command]
pub fn assign_vca(state: State<AppState>, tracks: Vec<Id>, vca: Option<Id>) -> R<Snapshot> {
    try_edit(&state, if vca.is_some() { "Assign VCA" } else { "Remove from VCA" }, None, false, |s| s.assign_vca(&tracks, vca))
}

/// Assigns a VCA to another VCA (null: none); refuses loops.
#[tauri::command]
pub fn set_vca_parent(state: State<AppState>, id: Id, parent: Option<Id>) -> R<Snapshot> {
    try_edit(&state, "Assign VCA", None, false, |s| s.set_vca_parent(id, parent))
}

/// Track › Track Alternatives › New (empty) or Duplicate (`duplicate`).
#[tauri::command]
pub fn new_track_alternative(state: State<AppState>, track: Id, duplicate: bool) -> R<Snapshot> {
    let label = if duplicate { "Duplicate Track Alternative" } else { "New Track Alternative" };
    try_edit(&state, label, None, false, |s| s.new_track_alternative(track, duplicate).map(|_| ()))
}

#[tauri::command]
pub fn switch_track_alternative(state: State<AppState>, track: Id, name: String) -> R<Snapshot> {
    try_edit(&state, "Switch Track Alternative", None, false, |s| s.switch_track_alternative(track, &name))
}

#[tauri::command]
pub fn rename_track_alternative(state: State<AppState>, track: Id, name: String, new_name: String) -> R<Snapshot> {
    try_edit(&state, "Rename Track Alternative", None, false, |s| s.rename_track_alternative(track, &name, &new_name))
}

#[tauri::command]
pub fn delete_track_alternative(state: State<AppState>, track: Id, name: String) -> R<Snapshot> {
    try_edit(&state, "Delete Track Alternative", None, false, |s| s.delete_track_alternative(track, &name))
}

/// A new aux on the next free output of the track's multi-output instrument.
#[tauri::command]
pub fn add_output_aux(state: State<AppState>, track: Id) -> R<Made> {
    edit_made(&state, "New Aux", false, |s| Ok(vec![s.add_output_aux(track)?]))
}

/// Sets an instrument track's MIDI settings (Track inspector). Steps on one value of the same
/// track share an undo step; switching the delay's unit is a step of its own.
#[tauri::command]
pub fn set_track_midi(state: State<AppState>, id: Id, patch: TrackMidiPatch) -> R<Snapshot> {
    let key = patch.delay_ms.is_none().then(|| format!("track-midi:{id}:{}", patch.label()));
    try_edit(&state, patch.label(), key, false, |s| s.set_track_midi(id, &patch))
}
