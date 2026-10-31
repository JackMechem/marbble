//! Transport commands, and the project-wide values that go with it: cycle, tempo and the tempo
//! map, signature, metronome, Autopunch, and MIDI from the on-screen keyboard. See `Session` in
//! `session/transport.rs`.

use super::*;

#[tauri::command]
pub fn play(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.play())
}

#[tauri::command]
pub fn stop(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.stop())
}

#[tauri::command]
pub fn toggle_play(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.toggle_play())
}

#[tauri::command]
pub fn record(state: State<AppState>) -> R<Snapshot> {
    try_snap(&state, |s| {
        if s.is_recording() {
            s.stop();
            Ok(())
        } else {
            s.record()
        }
    })
}

#[tauri::command]
pub fn locate(state: State<AppState>, pos: i64) {
    with(&state, |s| s.locate(pos))
}

#[tauri::command]
pub fn set_cycle(state: State<AppState>, enabled: bool, start: i64, end: i64) -> Snapshot {
    snap(&state, |s| s.set_cycle(enabled, start, end))
}

#[tauri::command]
pub fn set_tempo(state: State<AppState>, bpm: f64) -> Snapshot {
    edit(&state, "Tempo", Some("tempo".into()), false, |s| s.set_tempo(bpm))
}

#[tauri::command]
pub fn set_signature(state: State<AppState>, num: u32, den: u32) -> Snapshot {
    edit(&state, "Time Signature", None, false, |s| s.set_signature(num, den))
}

#[tauri::command]
pub fn set_key(state: State<AppState>, key: String) -> Snapshot {
    edit(&state, "Key", None, false, |s| s.set_key(key))
}

#[tauri::command]
pub fn set_metronome(state: State<AppState>, on: bool) -> Snapshot {
    snap(&state, |s| s.set_metronome(on))
}

#[tauri::command]
pub fn set_count_in(state: State<AppState>, on: bool) -> Snapshot {
    snap(&state, |s| s.set_count_in(on))
}

#[tauri::command]
pub fn set_master_volume(state: State<AppState>, db: f32) -> Snapshot {
    with(&state, |s| s.touched(CtlTarget::MasterVolume));
    // Moves recorded as automation belong to the pass's undo step.
    if with(&state, |s| s.automation_writing(MASTER_ID)) {
        return snap(&state, |s| s.set_master_volume_linked(db));
    }
    edit(&state, "Stereo Out Volume", Some("master-volume".into()), false, |s| s.set_master_volume_linked(db))
}

/// A MIDI message from the on-screen keyboard / Musical Typing.
#[tauri::command(async)]
pub fn midi_event(state: State<AppState>, data: [u8; 3]) {
    state.session.lock().send_midi(data);
}

#[tauri::command]
pub fn set_click(state: State<AppState>, click: ClickSettings) -> Snapshot {
    edit(&state, "Metronome Settings", Some("click".into()), false, |s| s.set_click(click))
}

#[tauri::command]
pub fn set_record_bit_depth(state: State<AppState>, depth: Option<BitDepth>) -> Snapshot {
    edit(&state, "Recording Format", None, false, |s| s.set_record_bit_depth(depth))
}

/// The Autopunch range and switch (not an undo step, like the cycle).
#[tauri::command]
pub fn set_punch(state: State<AppState>, enabled: bool, start: i64, end: i64) -> Snapshot {
    snap(&state, |s| s.set_punch(enabled, start, end))
}

/// The Tempo track. With `moveMidi`, MIDI regions keep their bars and beats.
#[tauri::command]
pub fn set_tempo_map(state: State<AppState>, events: Vec<TempoEvent>, move_midi: Option<bool>) -> Snapshot {
    edit(&state, "Tempo Change", None, false, |s| s.set_tempo_map(events, move_midi.unwrap_or(false)))
}

/// The Signature track.
#[tauri::command]
pub fn set_signatures(state: State<AppState>, events: Vec<SigEvent>) -> Snapshot {
    edit(&state, "Signature Change", None, false, |s| s.set_signatures(events))
}

/// The LCD's tempo: the tempo event in effect at `pos`.
#[tauri::command]
pub fn set_tempo_at(state: State<AppState>, pos: i64, bpm: f64, move_midi: Option<bool>) -> Snapshot {
    edit(&state, "Tempo", Some("tempo".into()), false, |s| s.set_tempo_at(pos, bpm, move_midi.unwrap_or(false)))
}

/// The LCD's signature: the change in effect at `bar`.
#[tauri::command]
pub fn set_signature_at(state: State<AppState>, bar: u32, num: u32, den: u32) -> Snapshot {
    edit(&state, "Time Signature", None, false, |s| s.set_signature_at(bar, num, den))
}

/// Moves MIDI regions to keep their bars and beats from one Tempo track to another.
#[tauri::command]
pub fn move_midi_with_tempo(state: State<AppState>, old_map: Vec<TempoEvent>, new_map: Vec<TempoEvent>) -> Snapshot {
    edit(&state, "Move MIDI Regions with Tempo", None, false, |s| s.move_midi_between(&old_map, &new_map))
}
