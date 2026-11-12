//! A track's MIDI settings on notes, playback entries and render ranges.

use super::*;
use crate::model::tempo::TempoEvent;
use crate::model::CcKind;

const SR: f64 = 48000.0;
/// 120 BPM: 24000 samples per quarter.
const Q: i64 = 24000;

fn note(start: i64, length: i64, pitch: u8, velocity: u8) -> Note {
    Note { start, length, pitch, velocity, muted: false }
}

fn map120() -> TempoMap {
    TempoMap::constant(SR, 120.0, 4, 4)
}

/// 120 BPM, then 60 BPM from the second quarter on.
fn map_slowing() -> TempoMap {
    let events = vec![TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: Q, bpm: 60.0, ramp: false }];
    TempoMap::new(SR, &events, &[], 120.0, (4, 4))
}

fn settings(f: impl FnOnce(&mut TrackMidiSettings)) -> TrackMidiSettings {
    let mut s = TrackMidiSettings::default();
    f(&mut s);
    s
}

fn midi_region(start: i64, length: i64, notes: Vec<Note>) -> Region {
    Region {
        id: 1,
        name: "r".into(),
        start,
        length,
        muted: false,
        color: None,
        content: RegionContent::midi(notes, Vec::new()),
        loop_end: None,
        params: Default::default(),
        pattern: None,
        takes: None,
    }
}

#[test]
fn defaults_change_nothing() {
    let s = TrackMidiSettings::default();
    assert!(s.is_default());
    assert!(!s.follows_tempo());
    let ns = vec![note(0, 10, 0, 1), note(5, 10, 127, 127), note(9, 1, 60, 64)];
    let key = |n: &[Note]| n.iter().map(|n| (n.start, n.length, n.pitch, n.velocity)).collect::<Vec<_>>();
    assert_eq!(key(&notes(&s, &ns)), key(&ns));
    assert_eq!(s.constant_shift(&map_slowing()), Some(0));
}

#[test]
fn transpose_moves_pitches_and_stops_at_the_ends() {
    let s = settings(|s| s.transpose = 12);
    assert_eq!(s.pitch(60), 72);
    assert_eq!(s.pitch(120), 127);
    let down = settings(|s| s.transpose = -96);
    assert_eq!(down.pitch(100), 4);
    assert_eq!(down.pitch(30), 0);
}

#[test]
fn velocity_offset_is_kept_within_1_to_127() {
    let up = settings(|s| s.velocity = 30);
    assert_eq!(up.velocity(90), 120);
    assert_eq!(up.velocity(110), 127);
    let down = settings(|s| s.velocity = -99);
    assert_eq!(down.velocity(100), 1);
    assert_eq!(down.velocity(127), 28);
}

#[test]
fn limits_are_inclusive_and_look_at_the_note_as_played() {
    let s = settings(|s| {
        s.key_limit = (36, 47);
        s.velocity_limit = (64, 100);
        // Transposed out of the key range and pushed past the velocity range: still played,
        // because the limits look at what arrives.
        s.transpose = 24;
        s.velocity = 20;
    });
    assert!(s.plays(36, 64) && s.plays(47, 100));
    assert!(!s.plays(35, 80) && !s.plays(48, 80));
    assert!(!s.plays(40, 63) && !s.plays(40, 101));
    let ns = vec![note(0, 10, 35, 80), note(1, 10, 40, 80), note(2, 10, 47, 100), note(3, 10, 47, 101)];
    let played = notes(&s, &ns);
    assert_eq!(played.iter().map(|n| (n.start, n.pitch, n.velocity)).collect::<Vec<_>>(), vec![(1, 64, 100), (2, 71, 120)]);
}

#[test]
fn clamping_brings_values_into_range_and_orders_limits() {
    let s = settings(|s| {
        s.transpose = 200;
        s.velocity = -300;
        s.key_limit = (90, 20);
        s.velocity_limit = (0, 0);
        s.delay = 99999.0;
    })
    .clamped();
    assert_eq!((s.transpose, s.velocity), (96, -99));
    assert_eq!(s.key_limit, (20, 90));
    assert_eq!(s.velocity_limit, (1, 1));
    assert_eq!(s.delay, DELAY_TICKS_MAX);
    let ms = settings(|s| {
        s.delay_ms = true;
        s.delay = -5000.0;
    })
    .clamped();
    assert_eq!(ms.delay, -DELAY_MS_MAX);
    assert_eq!(settings(|s| s.delay = f64::NAN).clamped().delay, 0.0);
    assert_eq!(TrackMidiSettings::default().clamped(), TrackMidiSettings::default());
}

#[test]
fn a_delay_in_ms_or_at_one_tempo_moves_everything_alike() {
    let ms = settings(|s| {
        s.delay = -10.0;
        s.delay_ms = true;
    });
    assert_eq!(ms.constant_shift(&map_slowing()), Some(-480));
    assert!(!ms.follows_tempo());
    // A quarter of a quarter note at 120 BPM.
    let ticks = settings(|s| s.delay = 240.0);
    assert_eq!(ticks.constant_shift(&map120()), Some(Q / 4));
    assert!(ticks.follows_tempo());
    assert_eq!(ticks.constant_shift(&map_slowing()), None);
}

#[test]
fn a_delay_in_ticks_follows_tempo_changes_note_by_note() {
    // An eighth note late: 12000 samples at 120 BPM, 24000 at 60.
    let s = settings(|s| s.delay = 480.0);
    let ns = vec![note(0, Q / 2, 60, 100), note(2 * Q, Q, 62, 100)];
    let cc = vec![CcEvent { start: 2 * Q, kind: CcKind::Controller(1), value: 9 }];
    let e = delay_entry(&s, 0, 4 * Q, &ns, &cc, &map_slowing());
    assert_eq!(e.start, Q / 2);
    // The first note now ends where the tempo halves: its end moves by 24000 from there.
    assert_eq!((e.notes[0].start, e.notes[0].length), (0, Q / 2));
    // 1.5 quarters in, plus one eighth: 2 quarters, at sample 72000.
    assert_eq!(e.notes[1].start, 3 * Q - Q / 2);
    assert_eq!(e.notes[1].length, Q);
    assert_eq!(e.cc[0].start, e.notes[1].start);
    // The window moves with its end, so the region still cuts its notes where it ends.
    assert_eq!(e.start + e.length, 5 * Q);
}

#[test]
fn a_negative_delay_can_start_before_zero() {
    let s = settings(|s| s.delay = -960.0);
    let e = delay_entry(&s, 0, 2 * Q, &[note(0, Q, 60, 100)], &[], &map120());
    assert_eq!(e.start, -Q);
    assert_eq!((e.notes[0].start, e.notes[0].length), (0, Q));
    assert_eq!(e.length, 2 * Q);
}

#[test]
fn render_ranges_reach_as_far_as_the_delay() {
    let r = midi_region(Q, 2 * Q, vec![note(0, Q, 60, 100)]);
    let map = map120();
    assert_eq!(bounds(&TrackMidiSettings::default(), &r, &map), (Q, 3 * Q));
    assert_eq!(bounds(&settings(|s| s.delay = 960.0), &r, &map), (Q, 4 * Q));
    assert_eq!(bounds(&settings(|s| s.delay = -960.0), &r, &map), (0, 3 * Q));
    // Never before the project start.
    assert_eq!(bounds(&settings(|s| s.delay = -3840.0), &r, &map), (0, 3 * Q));
}
