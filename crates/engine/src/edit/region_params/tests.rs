//! Region playback parameters on notes and audio, and baking them into the region.

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

fn params(f: impl FnOnce(&mut RegionParams)) -> RegionParams {
    let mut p = RegionParams::default();
    f(&mut p);
    p
}

fn starts(notes: &[Note]) -> Vec<i64> {
    notes.iter().map(|n| n.start).collect()
}

#[test]
fn defaults_change_nothing() {
    let notes = vec![note(13, 500, 60, 90), note(7001, 20, 72, 3)];
    let cc = vec![CcEvent { start: 5, kind: CcKind::Controller(1), value: 40 }];
    let (n, c) = apply(&RegionParams::default(), &notes, &cc, 12345, &map120());
    assert_eq!(starts(&n), vec![13, 7001]);
    assert_eq!(n.iter().map(|n| (n.length, n.pitch, n.velocity)).collect::<Vec<_>>(), vec![(500, 60, 90), (20, 72, 3)]);
    assert_eq!(c[0].start, 5);
    assert!(RegionParams::default().is_default());
    assert!(!RegionParams::default().follows_tempo());
}

#[test]
fn quantize_snaps_starts_and_keeps_lengths() {
    let map = map120();
    let p = params(|p| p.quantize = 16);
    let s = Q / 4;
    // The region starts mid-bar: the grid still counts from the bar line.
    let start = 3 * s + 100;
    let notes = vec![note(-100 + s + 1000, 777, 60, 100), note(-100 + 2 * s - 2000, 50, 62, 100)];
    let (n, _) = apply(&p, &notes, &[], start, &map);
    assert_eq!(starts(&n), vec![4 * s - start, 5 * s - start]);
    assert_eq!(n[0].length, 777);
    // A triplet grid: 1/12 note = a third of a quarter.
    let p = params(|p| p.quantize = 12);
    let (n, _) = apply(&p, &[note(Q / 3 + 500, 10, 60, 100)], &[], 0, &map);
    assert_eq!(n[0].start, Q / 3);
}

#[test]
fn swing_moves_every_second_grid_point() {
    let map = map120();
    let e = Q / 2;
    // 1/8 grid, 66 % swing: the off-beat eighths move 32 % of an eighth later (66.7 % would
    // be a third, the triplet feel).
    let p = params(|p| {
        p.quantize = 8;
        p.swing = 66;
    });
    let notes = vec![note(10, 100, 60, 100), note(e - 30, 100, 60, 100), note(2 * e + 5, 100, 60, 100), note(3 * e + 40, 100, 60, 100)];
    let (n, _) = apply(&p, &notes, &[], 0, &map);
    let late = (e as f64 * 0.32).round() as i64;
    assert_eq!(starts(&n), vec![0, e + late, 2 * e, 3 * e + late]);
    // Below 50 pulls them earlier; swing does nothing without quantize.
    let p = params(|p| {
        p.quantize = 8;
        p.swing = 25;
    });
    let (n, _) = apply(&p, &notes, &[], 0, &map);
    assert_eq!(n[1].start, e / 2);
    let p = params(|p| p.swing = 80);
    let (n, _) = apply(&p, &notes, &[], 0, &map);
    assert_eq!(n[1].start, e - 30);
}

#[test]
fn quantize_follows_the_tempo_map() {
    // 120 BPM for one bar, then 60 BPM: a quarter is 24000 samples, then 48000.
    let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 4 * Q, bpm: 60.0, ramp: false }], &[], 120.0, (4, 4));
    let p = params(|p| p.quantize = 4);
    let bar2 = 4 * Q;
    let notes = vec![note(bar2 + 2 * Q + 1000, 100, 60, 100), note(bar2 + 3 * Q, 100, 60, 100)];
    let (n, _) = apply(&p, &notes, &[], 0, &map);
    // Beat 2 of bar 2 is at bar2 + 48000; the second note (at 1.5 slow beats) rounds up to
    // beat 3 (bar2 + 96000).
    assert_eq!(starts(&n), vec![bar2 + 2 * Q, bar2 + 4 * Q]);
}

#[test]
fn transpose_clamps_to_the_midi_range() {
    let p = params(|p| p.transpose = 24);
    let (n, _) = apply(&p, &[note(0, 1, 60, 100), note(1, 1, 120, 100)], &[], 0, &map120());
    assert_eq!(n.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![84, 127]);
    let p = params(|p| p.transpose = -96);
    let (n, _) = apply(&p, &[note(0, 1, 60, 100)], &[], 0, &map120());
    assert_eq!(n[0].pitch, 0);
    assert_eq!(params(|p| p.transpose = 500).clamped().transpose, 96);
}

#[test]
fn velocity_and_dynamics() {
    let off = |v: i16| params(|p| p.velocity = v);
    assert_eq!(velocity(100, &off(20)), 120);
    assert_eq!(velocity(100, &off(50)), 127);
    assert_eq!(velocity(30, &off(-99)), 1);
    // 2:1 halves the distance from 64; 0.5:1 doubles it; Fixed puts everything at 64.
    let dynamics = |d: u16, v: i16| params(|p| (p.dynamics, p.velocity) = (d, v));
    assert_eq!(velocity(100, &dynamics(200, 0)), 82);
    assert_eq!(velocity(40, &dynamics(200, 0)), 52);
    assert_eq!(velocity(80, &dynamics(50, 0)), 96);
    assert_eq!(velocity(10, &dynamics(50, 0)), 1);
    assert_eq!(velocity(127, &dynamics(0, 0)), 64);
    assert_eq!(velocity(1, &dynamics(0, 10)), 74);
    assert_eq!(velocity(100, &dynamics(300, 0)), 76);
    assert_eq!(velocity(70, &dynamics(33, 0)), 82);
    // Unknown values go to the nearest one there is.
    assert_eq!(dynamics(140, 0).clamped().dynamics, 150);
}

#[test]
fn gate_time() {
    let map = map120();
    let notes = vec![note(0, 1000, 60, 100), note(0, 400, 64, 100), note(Q, 2000, 60, 100), note(3 * Q, 10, 60, 100)];
    let lengths = |p: RegionParams| apply(&p, &notes, &[], 0, &map).0.iter().map(|n| n.length).collect::<Vec<_>>();
    assert_eq!(lengths(params(|p| p.gate = 50)), vec![500, 200, 1000, 5]);
    assert_eq!(lengths(params(|p| p.gate = 200)), vec![2000, 800, 4000, 20]);
    assert_eq!(lengths(params(|p| p.gate = GATE_FIXED)), vec![Q / 16; 4]);
    // Legato: up to the next later start; the last note keeps its length.
    assert_eq!(lengths(params(|p| p.gate = GATE_LEGATO)), vec![Q, Q, 2 * Q, 10]);
    assert_eq!(params(|p| p.gate = 5).clamped().gate, 10);
    assert_eq!(params(|p| p.gate = GATE_LEGATO).clamped().gate, GATE_LEGATO);
}

#[test]
fn delay_in_ticks_and_ms() {
    let map = map120();
    let notes = vec![note(1000, 500, 60, 100)];
    let cc = vec![CcEvent { start: 0, kind: CcKind::PitchBend, value: 9000 }];
    // 480 ticks = an eighth = 12000 samples at 120 BPM.
    let p = params(|p| p.delay = 480.0);
    let (n, c) = apply(&p, &notes, &cc, 0, &map);
    assert_eq!((n[0].start, n[0].length, c[0].start), (13000, 500, 12000));
    let p = params(|p| (p.delay, p.delay_ms) = (-10.0, true));
    let (n, c) = apply(&p, &notes, &cc, 0, &map);
    assert_eq!((n[0].start, c[0].start), (520, -480));
    assert_eq!(window(&p, &n, &c, 0, Q, &map), (480, 0));
    let p = params(|p| p.delay = 960.0);
    assert_eq!(window(&p, &[], &[], 0, Q, &map), (0, Q));
    assert!(p.follows_tempo());
    assert!(!params(|p| (p.delay, p.delay_ms) = (5.0, true)).follows_tempo());
}

#[test]
fn delay_in_ticks_across_a_tempo_change() {
    // 120 BPM, then 60 BPM from sample 4Q: a quarter of delay is 24000 samples before the
    // change and 48000 after it; a note just before the change gets a mix of both.
    let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 4 * Q, bpm: 60.0, ramp: false }], &[], 120.0, (4, 4));
    let p = params(|p| p.delay = 960.0);
    let notes = vec![note(0, 100, 60, 100), note(4 * Q - Q / 2, 100, 60, 100), note(5 * Q, 100, 60, 100)];
    let (n, _) = apply(&p, &notes, &[], 0, &map);
    // Half a quarter at 120 (12000) then half a quarter at 60 (24000).
    assert_eq!(starts(&n), vec![Q, 4 * Q + 24000, 5 * Q + 48000]);
    // Switching units keeps what plays at the position.
    assert!((p.delay_in(true, 0.0, &map) - 500.0).abs() < 1e-6);
    assert!((p.delay_in(true, (5 * Q) as f64, &map) - 1000.0).abs() < 1e-6);
    let ms = params(|p| (p.delay, p.delay_ms) = (1000.0, true));
    assert!((ms.delay_in(false, (5 * Q) as f64, &map) - 960.0).abs() < 1e-6);
}

#[test]
fn everything_together_in_order() {
    // Quantize, then gate on the quantized start, then delay, then the note values.
    let map = map120();
    let p = params(|p| {
        p.quantize = 4;
        p.gate = 50;
        p.delay = 240.0;
        p.transpose = -12;
        p.velocity = 10;
        p.dynamics = 200;
    });
    let (n, _) = apply(&p, &[note(Q + 3000, 8000, 60, 100), note(Q / 2 - 10, 400, 70, 20)], &[], 0, &map);
    // In the order given.
    assert_eq!((n[0].start, n[0].length, n[0].pitch, n[0].velocity), (Q + Q / 4, 4000, 48, 92));
    assert_eq!((n[1].start, n[1].length, n[1].pitch, n[1].velocity), (Q / 4, 200, 58, 52));
}

fn midi_region(start: i64, length: i64, notes: Vec<Note>, p: RegionParams) -> Region {
    Region {
        id: 1,
        name: "M".into(),
        start,
        length,
        muted: false,
        color: None,
        content: RegionContent::midi(notes, vec![CcEvent { start: 0, kind: CcKind::Controller(7), value: 100 }]),
        loop_end: None,
        takes: None,
        params: p,
        pattern: None,
    }
}

/// Timeline (on, off, pitch, velocity) of what a region plays in its first pass.
fn heard(r: &Region, map: &TempoMap) -> Vec<(i64, i64, u8, u8)> {
    let ps = match r.params.is_default() {
        true => {
            let RegionContent::Midi { notes, .. } = &r.content else { unreachable!() };
            let mut v: Vec<_> = notes
                .iter()
                .filter(|n| !n.muted && n.start >= 0 && n.start < r.length)
                .map(|n| (r.start + n.start, r.start + (n.start + n.length).min(r.length), n.pitch, n.velocity))
                .collect();
            v.sort();
            return v;
        }
        false => pass(r, r.length, map).unwrap(),
    };
    let ws = r.start - ps.lead;
    ps.notes.iter().filter(|n| n.start < ps.length).map(|n| (ws + n.start, ws + (n.start + n.length).min(ps.length), n.pitch, n.velocity)).collect()
}

#[test]
fn pass_widens_the_window_for_moved_notes() {
    let map = map120();
    let p = params(|p| (p.delay, p.delay_ms) = (-20.0, true));
    let r = midi_region(Q, 2 * Q, vec![note(0, 100, 60, 100), note(Q, 100, 62, 100), note(2 * Q + 5, 100, 64, 100)], p);
    let ps = pass(&r, r.length, &map).unwrap();
    // 20 ms = 960 samples early; the note past the end is left out.
    assert_eq!((ps.lead, ps.length), (960, 2 * Q + 960));
    assert_eq!(starts(&ps.notes), vec![0, Q]);
    assert_eq!(ps.cc[0].start, 0);
    // A shorter (last loop) pass only plays the notes that start inside it.
    assert_eq!(pass(&r, Q, &map).unwrap().notes.len(), 1);
}

#[test]
fn bake_plays_exactly_as_before() {
    let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 2 * Q, bpm: 90.0, ramp: false }], &[], 120.0, (4, 4));
    let notes = vec![note(30, 5000, 60, 100), note(Q - 700, 9000, 64, 70), note(3 * Q + 11, 3000, 67, 40), note(-500, 900, 50, 50)];
    for p in [
        params(|p| (p.quantize, p.swing, p.delay) = (8, 60, -120.0)),
        params(|p| (p.transpose, p.velocity, p.dynamics, p.gate) = (5, -10, 150, 140)),
        params(|p| (p.delay, p.gate) = (300.0, GATE_LEGATO)),
        params(|p| (p.quantize, p.delay, p.delay_ms) = (12, 15.0, true)),
    ] {
        let r = midi_region(Q / 3, 4 * Q, notes.clone(), p);
        let b = bake(&r, &map);
        assert!(b.params.is_default());
        assert_eq!(heard(&r, &map), heard(&b, &map), "{p:?}");
    }
    // Looping regions keep their length; their notes stay inside.
    let mut r = midi_region(0, Q, vec![note(10, 100, 60, 100)], params(|p| (p.delay, p.delay_ms) = (-10.0, true)));
    r.loop_end = Some(4 * Q);
    let b = bake(&r, &map);
    assert_eq!((b.start, b.length, b.loop_end), (0, Q, Some(4 * Q)));
    let RegionContent::Midi { notes, .. } = &b.content else { unreachable!() };
    assert_eq!(notes[0].start, 0);
}

#[test]
fn bake_moves_audio_by_its_delay() {
    let map = map120();
    let audio = |start: i64, delay: f64| Region {
        content: RegionContent::Audio { clip: 1, offset: 100, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() },
        ..midi_region(start, Q, Vec::new(), params(|p| (p.delay, p.delay_ms) = (delay, true)))
    };
    let b = bake(&audio(Q, 10.0), &map);
    assert_eq!((b.start, b.length), (Q + 480, Q));
    let b = bake(&audio(100, -10.0), &map);
    let RegionContent::Audio { offset, .. } = b.content else { unreachable!() };
    assert_eq!((b.start, b.length, offset), (0, Q - 380, 480));
}

#[test]
fn bake_never_starts_a_midi_region_before_the_project_start() {
    // 10 ms early is 480 samples, more than the region's 100 from the start.
    let map = map120();
    let r = midi_region(100, Q, vec![note(0, 1000, 60, 100), note(Q / 2, 100, 62, 100)], params(|p| (p.delay, p.delay_ms) = (-10.0, true)));
    let b = bake(&r, &map);
    assert_eq!((b.start, b.end()), (0, r.end()));
    // What can play, from 0 on, is unchanged; the note moved before 0 stays there.
    let from_0 = |r: &Region| heard(r, &map).into_iter().filter(|n| n.0 >= 0).collect::<Vec<_>>();
    assert_eq!(from_0(&b), from_0(&r));
    let RegionContent::Midi { notes, .. } = &b.content else { unreachable!() };
    assert_eq!(starts(notes), vec![-380, Q / 2 - 380]);
}

#[test]
fn serde_defaults() {
    let p: RegionParams = serde_json::from_str("{\"transpose\": 5}").unwrap();
    assert_eq!(p, params(|p| p.transpose = 5));
}
