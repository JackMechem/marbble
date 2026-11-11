//! A track's MIDI settings against the session and the engine, counted at the instrument: what
//! the regions and live input play through them, their undo, the refusals, the tempo, and a
//! render left silent by a limit.

use crate::audio_io::NULL_OUTPUT;
use crate::dsp::Engine;
use crate::session::*;

const BLOCK: usize = 512;

fn session() -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

fn note(start: i64, pitch: u8, velocity: u8) -> Note {
    Note { start, length: 100_000, pitch, velocity, muted: false }
}

/// An instrument track, selected, with one region at 0 holding `notes`.
fn track_with(s: &mut Session, notes: Vec<Note>) -> Id {
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let r = s.create_midi_region(inst, 0, 400_000).unwrap();
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: vec![], start: None, length: None }]);
    s.select_track(Some(inst));
    inst
}

/// The note-ons the track's instrument gets in the next `blocks` blocks: (frame from the first
/// block's start, pitch, velocity).
fn note_ons(e: &mut Engine, track: Id, blocks: usize) -> Vec<(usize, u8, u8)> {
    let mut buf = vec![0.0f32; BLOCK];
    let mut got = Vec::new();
    for b in 0..blocks {
        e.process(&[], 0, &mut buf, 2);
        let ons = e.midi_sent(track).iter().filter(|m| m.data[0] & 0xF0 == 0x90 && m.data[2] > 0);
        got.extend(ons.map(|m| (b * BLOCK / 2 + m.offset as usize, m.data[1], m.data[2])));
    }
    got
}

/// The note-offs it gets in the next block, by pitch.
fn note_offs(e: &mut Engine, track: Id) -> Vec<u8> {
    let mut buf = vec![0.0f32; BLOCK];
    e.process(&[], 0, &mut buf, 2);
    e.midi_sent(track).iter().filter(|m| m.data[0] & 0xF0 == 0x80 || (m.data[0] & 0xF0 == 0x90 && m.data[2] == 0)).map(|m| m.data[1]).collect()
}

fn play_from_start(s: &mut Session) {
    s.locate(0);
    s.play();
}

/// Stops, and runs a block so the engine (and so `Session::is_playing`) has stopped.
fn stop(s: &mut Session, e: &mut Engine) {
    s.stop();
    note_offs(e, 0);
}

#[test]
fn regions_play_through_the_limits_transpose_and_velocity() {
    let mut s = session();
    let inst = track_with(&mut s, vec![note(0, 30, 100), note(10, 60, 100), note(20, 70, 10), note(30, 80, 127)]);
    let patch = TrackMidiPatch { transpose: Some(2), velocity: Some(10), key_limit: Some((40, 80)), velocity_limit: Some((20, 127)), ..Default::default() };
    s.set_track_midi(inst, &patch).unwrap();
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    play_from_start(&mut s);
    assert_eq!(note_ons(&mut e, inst, 2), vec![(10, 62, 110), (30, 82, 127)]);
    // The regions' notes are untouched.
    let RegionContent::Midi { notes, .. } = &s.project.track(inst).unwrap().regions[0].content else { panic!() };
    assert_eq!(notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![30, 60, 70, 80]);
}

#[test]
fn a_delay_in_ms_moves_the_notes_later() {
    let mut s = session();
    let inst = track_with(&mut s, vec![note(0, 60, 100)]);
    s.set_track_midi(inst, &TrackMidiPatch { delay: Some(5.0), delay_ms: Some(true), ..Default::default() }).unwrap();
    let late = (s.sample_rate() * 0.005).round() as usize;
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    play_from_start(&mut s);
    assert_eq!(note_ons(&mut e, inst, 4), vec![(late, 60, 100)]);
}

#[test]
fn live_input_is_transposed_and_its_note_off_follows_the_key() {
    let mut s = session();
    let inst = track_with(&mut s, vec![]);
    s.set_track_midi(inst, &TrackMidiPatch { transpose: Some(-12), key_limit: Some((48, 127)), ..Default::default() }).unwrap();
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    s.send_midi([0x90, 60, 90]);
    s.send_midi([0x90, 40, 90]);
    assert_eq!(note_ons(&mut e, inst, 1), vec![(0, 48, 90)]);
    // Changed while the key is down: its note-off still ends the note it started.
    s.set_track_midi(inst, &TrackMidiPatch { transpose: Some(5), ..Default::default() }).unwrap();
    s.send_midi([0x80, 60, 0]);
    s.send_midi([0x80, 40, 0]);
    assert_eq!(note_offs(&mut e, inst), vec![48]);
}

#[test]
fn undo_puts_back_what_the_regions_play() {
    let mut s = session();
    let inst = track_with(&mut s, vec![note(0, 60, 100)]);
    s.checkpoint("Track Transpose", None, false);
    s.set_track_midi(inst, &TrackMidiPatch { transpose: Some(7), ..Default::default() }).unwrap();
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    play_from_start(&mut s);
    assert_eq!(note_ons(&mut e, inst, 1), vec![(0, 67, 100)]);
    stop(&mut s, &mut e);
    s.undo();
    assert!(s.project.track(inst).unwrap().midi_settings.is_default());
    play_from_start(&mut s);
    assert_eq!(note_ons(&mut e, inst, 1), vec![(0, 60, 100)]);
    stop(&mut s, &mut e);
    s.redo();
    play_from_start(&mut s);
    assert_eq!(note_ons(&mut e, inst, 1), vec![(0, 67, 100)]);
}

#[test]
fn only_unfrozen_instrument_tracks_take_settings_and_values_are_kept_in_range() {
    let mut s = session();
    let inst = track_with(&mut s, vec![]);
    let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    assert!(s.set_track_midi(audio, &TrackMidiPatch { transpose: Some(1), ..Default::default() }).is_err());
    s.set_track_midi(inst, &TrackMidiPatch { transpose: Some(500), key_limit: Some((90, 10)), ..Default::default() }).unwrap();
    let m = s.project.track(inst).unwrap().midi_settings;
    assert_eq!((m.transpose, m.key_limit), (96, (10, 90)));
    s.project.track_mut(inst).unwrap().frozen = Some(FreezeInfo { clip: 0, start: 0 });
    assert!(s.set_track_midi(inst, &TrackMidiPatch { transpose: Some(1), ..Default::default() }).is_err());
}

#[test]
fn switching_the_delay_unit_keeps_the_delay() {
    let mut s = session();
    s.set_tempo(120.0);
    let inst = track_with(&mut s, vec![]);
    // An eighth note at 120 BPM is 250 ms.
    s.set_track_midi(inst, &TrackMidiPatch { delay: Some(480.0), ..Default::default() }).unwrap();
    s.set_track_midi(inst, &TrackMidiPatch { delay_ms: Some(true), ..Default::default() }).unwrap();
    let m = s.project.track(inst).unwrap().midi_settings;
    assert_eq!((m.delay, m.delay_ms), (250.0, true));
}

#[test]
fn a_tempo_change_moves_a_delay_in_ticks() {
    let mut s = session();
    s.set_tempo(120.0);
    let inst = track_with(&mut s, vec![note(0, 60, 100)]);
    // A 64th note late: 1/16 of a quarter.
    s.set_track_midi(inst, &TrackMidiPatch { delay: Some(60.0), ..Default::default() }).unwrap();
    let quarter = |bpm: f64| s.sample_rate() * 60.0 / bpm;
    let at_120 = (quarter(120.0) / 16.0).round() as usize;
    let at_60 = (quarter(60.0) / 16.0).round() as usize;
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    play_from_start(&mut s);
    let blocks = at_60 * 2 / BLOCK + 2;
    assert_eq!(note_ons(&mut e, inst, blocks), vec![(at_120, 60, 100)]);
    stop(&mut s, &mut e);
    s.set_tempo(60.0);
    play_from_start(&mut s);
    assert_eq!(note_ons(&mut e, inst, blocks), vec![(at_60, 60, 100)]);
}

#[test]
fn a_render_plays_the_settings_too() {
    let mut s = session();
    let inst = track_with(&mut s, vec![note(0, 60, 100)]);
    let loud = |a: &[Vec<f32>]| a.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
    let before = s.render(0, 24_000, 0, None, &mut |_| {}).unwrap();
    assert!(loud(&before) > 0.01);
    s.set_track_midi(inst, &TrackMidiPatch { key_limit: Some((61, 127)), ..Default::default() }).unwrap();
    let after = s.render(0, 24_000, 0, None, &mut |_| {}).unwrap();
    assert_eq!(loud(&after), 0.0);
}
