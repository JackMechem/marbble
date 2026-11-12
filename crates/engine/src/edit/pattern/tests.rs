//! Step Sequencer patterns: the notes they make, timing and swing, and their JSON.

use super::*;
use crate::model::tempo::TempoEvent;

const SR: f64 = 48000.0;
/// 120 BPM: 24000 samples per quarter, 6000 per sixteenth.
const Q: i64 = 24000;
const S16: i64 = Q / 4;

fn map120() -> TempoMap {
    TempoMap::constant(SR, 120.0, 4, 4)
}

fn on(velocity: u8) -> Step {
    Step { on: true, velocity, ..Step::default() }
}

fn row(pitch: u8, on_steps: &[usize]) -> PatternRow {
    let mut r = PatternRow::new(pitch);
    for &k in on_steps {
        if r.steps.len() <= k {
            r.steps.resize(k + 1, Step::default());
        }
        r.steps[k] = on(DEFAULT_VELOCITY);
    }
    r
}

fn pattern(rows: Vec<PatternRow>) -> Pattern {
    Pattern { rows, ..Pattern::default() }
}

fn starts(notes: &[Note], pitch: u8) -> Vec<i64> {
    notes.iter().filter(|n| n.pitch == pitch).map(|n| n.start).collect()
}

#[test]
fn steps_become_notes_and_loop_to_the_region_end() {
    let p = pattern(vec![row(36, &[0, 4, 8, 12]), row(38, &[4, 12])]);
    // Two bars: the one-bar pattern plays twice.
    let n = p.notes(0, 32 * S16, &map120());
    assert_eq!(starts(&n, 36), (0..8).map(|k| k * 4 * S16).collect::<Vec<_>>());
    assert_eq!(starts(&n, 38), vec![4 * S16, 12 * S16, 20 * S16, 28 * S16]);
    assert!(n.iter().all(|n| n.length == S16 && n.velocity == DEFAULT_VELOCITY));
    // Sorted by start.
    assert!(n.windows(2).all(|w| w[0].start <= w[1].start));
    // Half a bar: only what starts inside the region.
    let n = p.notes(0, 8 * S16, &map120());
    assert_eq!(starts(&n, 36), vec![0, 4 * S16]);
    // Not looped: once, then silence.
    let p = Pattern { looped: false, ..p };
    assert_eq!(starts(&p.notes(0, 32 * S16, &map120()), 36), vec![0, 4 * S16, 8 * S16, 12 * S16]);
}

#[test]
fn timing_counts_from_the_region_start_through_the_tempo_map() {
    let p = pattern(vec![row(36, &[0, 1])]);
    let n = p.notes(1000, 16 * S16, &map120());
    assert_eq!(starts(&n, 36)[..2], [0, S16]);
    // 120 BPM for a bar, then 60: steps after the change are twice as long.
    let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 4 * Q, bpm: 60.0, ramp: false }], &[], 120.0, (4, 4));
    let p = pattern(vec![row(36, &[0, 1, 2])]);
    let n = p.notes(4 * Q, 32 * S16, &map);
    assert_eq!(starts(&n, 36)[..3], [0, 2 * S16, 4 * S16]);
    assert_eq!(n[0].length, 2 * S16);
}

#[test]
fn swing_delays_every_second_step_of_straight_rows() {
    let mut p = pattern(vec![row(42, &[0, 1, 2, 3])]);
    p.swing = 50;
    let n = p.notes(0, 16 * S16, &map120());
    // Half the swing range: a quarter of a step late.
    assert_eq!(starts(&n, 42), vec![0, S16 + S16 / 4, 2 * S16, 3 * S16 + S16 / 4]);
    // The swung note ends where the next step starts; the one before it keeps its length.
    assert_eq!(n[0].length, S16);
    assert_eq!(n[1].length, S16 - S16 / 4);
    // Triplet rows don't swing.
    p.rows[0].rate = Some(24);
    let n = p.notes(0, 16 * S16, &map120());
    assert_eq!(starts(&n, 42)[..2], [0, Q / 6]);
}

#[test]
fn rows_of_other_lengths_and_rates_drift() {
    let mut three = row(36, &[0]);
    three.length = Some(3);
    let mut eighths = row(38, &[0, 1]);
    eighths.rate = Some(8);
    eighths.length = Some(4);
    let p = pattern(vec![three, eighths]);
    let n = p.notes(0, 16 * S16, &map120());
    // Every 3 sixteenths, through the pattern's own 16.
    assert_eq!(starts(&n, 36), [0, 3, 6, 9, 12, 15].iter().map(|k| k * S16).collect::<Vec<_>>());
    // Eighths, 4 to a cycle: steps 0, 1 then 4, 5.
    assert_eq!(starts(&n, 38), vec![0, 2 * S16, 8 * S16, 10 * S16]);
    assert_eq!(n.iter().find(|n| n.pitch == 38).unwrap().length, 2 * S16);
}

#[test]
fn ties_gates_velocity_mute_and_solo() {
    let mut r = row(36, &[0, 4]);
    r.steps.resize(8, Step::default());
    r.steps[0].velocity = 77;
    r.steps[1].tie = true;
    r.steps[2] = Step { tie: true, gate: 50, ..Step::default() };
    // A tie after an off step does nothing.
    r.steps[6].tie = true;
    r.steps[4].gate = 25;
    let p = pattern(vec![r.clone(), row(38, &[0])]);
    let n = p.notes(0, 16 * S16, &map120());
    let kick: Vec<&Note> = n.iter().filter(|n| n.pitch == 36).collect();
    assert_eq!(kick.len(), 2);
    assert_eq!((kick[0].start, kick[0].length, kick[0].velocity), (0, 2 * S16 + S16 / 2, 77));
    assert_eq!((kick[1].start, kick[1].length), (4 * S16, S16 / 4));
    // Muted rows are silent; a soloed row silences the others.
    let mut p2 = p.clone();
    p2.rows[1].muted = true;
    assert!(starts(&p2.notes(0, 16 * S16, &map120()), 38).is_empty());
    p2.rows[1].solo = true;
    let n = p2.notes(0, 16 * S16, &map120());
    assert!(starts(&n, 36).is_empty());
    assert_eq!(starts(&n, 38), vec![0]);
    // A tie at step 0 carries the last step of the cycle before on, across the loop.
    let mut wrap = row(40, &[3]);
    wrap.length = Some(4);
    wrap.steps.resize(4, Step::default());
    wrap.steps[0].tie = true;
    let n = pattern(vec![wrap]).notes(0, 8 * S16, &map120());
    assert_eq!(n.iter().map(|n| (n.start, n.length)).collect::<Vec<_>>(), vec![(3 * S16, 2 * S16), (7 * S16, S16)]);
}

#[test]
fn an_offset_plays_on_from_partway_in() {
    let mut three = row(36, &[0]);
    three.length = Some(3);
    let mut tied = row(38, &[4, 9, 12]);
    tied.steps[10].tie = true;
    let p = pattern(vec![three, tied]);
    let whole = p.notes(0, 32 * S16, &map120());
    // From ten sixteenths in, as a piece somewhere else: what the whole played from there,
    // less the tied note that started before it.
    let cut = Pattern { offset: 10.0 / 4.0, ..p.clone() };
    let piece = cut.notes(50_000, 22 * S16, &map120());
    let key = |n: &[Note]| n.iter().map(|n| (n.start, n.length, n.pitch)).collect::<Vec<_>>();
    let want: Vec<Note> = whole.iter().filter(|n| n.start >= 10 * S16).map(|n| Note { start: n.start - 10 * S16, ..*n }).collect();
    assert_eq!(key(&piece), key(&want));
    assert!(!starts(&piece, 38).contains(&0));
    // Not looped: the rest of its one pass.
    let once = Pattern { looped: false, ..cut };
    assert_eq!(starts(&once.notes(0, 32 * S16, &map120()), 38), vec![2 * S16]);
    // Saved only when set.
    assert!(!serde_json::to_string(&p).unwrap().contains("offset"));
    let back: Pattern = serde_json::from_str(&serde_json::to_string(&once).unwrap()).unwrap();
    assert_eq!(back.offset, 2.5);
}

#[test]
fn clamped_keeps_values_in_range() {
    let mut p = pattern(vec![row(200, &[0])]);
    p.rate = 15;
    p.length = 0;
    p.swing = 250;
    p.rows[0].rate = Some(100);
    p.rows[0].length = Some(500);
    p.rows[0].steps[0].velocity = 0;
    p.rows[0].steps[0].gate = 0;
    p.rows[0].steps.resize(200, Step::default());
    let c = p.clamped();
    assert_eq!((c.rate, c.length, c.swing), (16, 1, 100));
    assert_eq!((c.rows[0].pitch, c.rows[0].rate, c.rows[0].length), (127, Some(48), Some(MAX_STEPS)));
    assert_eq!((c.rows[0].steps[0].velocity, c.rows[0].steps[0].gate), (1, 1));
    assert_eq!(c.rows[0].steps.len(), MAX_STEPS as usize);
}

#[test]
fn realize_and_from_notes_round_trip() {
    let map = map120();
    let mut r = row(36, &[0, 5, 10]);
    r.steps[5].velocity = 64;
    r.steps.resize(16, Step::default());
    r.steps[11].tie = true;
    r.steps[11].gate = 50;
    let p = pattern(vec![row(42, &[2, 6]), r]);
    let region = Region {
        id: 1,
        name: "Pattern".into(),
        start: 7 * Q,
        length: 16 * S16,
        muted: false,
        color: None,
        content: RegionContent::midi(Vec::new(), Vec::new()),
        loop_end: None,
        takes: None,
        params: Default::default(),
        pattern: Some(p),
    };
    let plain = realize(&region, &map);
    assert!(plain.pattern.is_none());
    let RegionContent::Midi { notes, .. } = &plain.content else { unreachable!() };
    assert_eq!(notes.len(), 5);
    let back = from_notes(notes, region.start, region.length, &map);
    assert_eq!((back.length, back.rate), (16, 16));
    assert_eq!(back.rows.iter().map(|r| r.pitch).collect::<Vec<_>>(), vec![36, 42]);
    // Lowest pitch first, so the rows come back in a different order; the notes match.
    let key = |n: &[Note]| n.iter().map(|n| (n.start, n.length, n.pitch, n.velocity)).collect::<Vec<_>>();
    assert_eq!(key(&back.notes(region.start, region.length, &map)), key(notes));
    // A plain region is left alone.
    assert!(realize(&plain, &map).pattern.is_none());
}

#[test]
fn old_and_compact_json() {
    // Off steps with default values are stored as {}; `looped` only when off.
    let p = pattern(vec![row(36, &[1])]);
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains(r#""steps":[{},{"on":true}]"#), "{json}");
    assert!(!json.contains("looped"));
    let back: Pattern = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}
