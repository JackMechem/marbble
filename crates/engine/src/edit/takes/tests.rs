//! Take folder edits, comp pieces and cutting recordings into takes.

use std::sync::Arc;

use super::{midi::*, pieces::*, recording::*};
use crate::model::*;

fn folder(takes: &[(Id, i64, i64)]) -> TakeFolder {
    TakeFolder {
        takes: takes
            .iter()
            .map(|&(id, start, end)| Take {
                id,
                name: format!("Take {id}"),
                clip: id * 10,
                offset: 1000 * id as i64,
                start,
                end,
                gain_db: 0.0,
                ..Default::default()
            })
            .collect(),
        ..TakeFolder::new()
    }
}

fn region(start: i64, length: i64, offset: i64, f: TakeFolder) -> Region {
    Region {
        id: 99,
        name: "F".into(),
        start,
        length,
        muted: false,
        color: None,
        content: RegionContent::Audio { clip: 10, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() },
        loop_end: None,
        takes: Some(f),
        params: Default::default(),
        pattern: None,
    }
}

#[test]
fn swipes_replace_and_merge() {
    let mut f = folder(&[(1, 0, 1000), (2, 0, 1000), (3, 0, 600)]);
    f.default_comp();
    // Newest wherever it has audio, the one before it after that.
    assert_eq!(f.comp, vec![CompSegment { start: 0, end: 600, take: 3 }, CompSegment { start: 600, end: 1000, take: 2 }]);
    f.select(1, 200, 400);
    assert_eq!(f.comp.len(), 4);
    f.select(1, 400, 800);
    let spans: Vec<(i64, i64, Id)> = f.comp.iter().map(|s| (s.start, s.end, s.take)).collect();
    assert_eq!(spans, vec![(0, 200, 3), (200, 800, 1), (800, 1000, 2)]);
    // Clipped to the take's audio.
    f.select(3, 500, 900);
    assert_eq!(f.comp.iter().find(|s| s.take == 3 && s.start == 500).map(|s| s.end), Some(600));
    assert_eq!(f.whole_take(), None);
    f.select_whole(2);
    assert_eq!(f.whole_take(), Some(2));
    f.remove_unused();
    assert_eq!(f.takes.len(), 1);
    f.remove_take(2);
    assert!(f.comp.is_empty());
}

#[test]
fn grouped_folders_match_takes() {
    let passes = |f: &mut TakeFolder, p: &[Option<(Id, u32)>]| f.takes.iter_mut().zip(p).for_each(|(t, p)| t.pass = *p);
    let mut a = folder(&[(1, 0, 1000), (2, 0, 1000), (3, 0, 1000)]);
    let mut b = folder(&[(11, 0, 1000), (12, 0, 1000), (13, 0, 1000)]);
    // Packed by hand: by position.
    assert_eq!((a.matching_take(2, &b), a.matching_take(3, &b), a.matching_take(9, &b)), (Some(12), Some(13), None));
    // Recorded together: by pass, so b's own extra recording has no partner in a.
    passes(&mut a, &[Some((50, 0)), Some((50, 1)), Some((60, 0))]);
    passes(&mut b, &[Some((70, 0)), Some((50, 0)), Some((50, 1))]);
    assert_eq!((a.matching_take(1, &b), a.matching_take(2, &b), a.matching_take(3, &b)), (Some(12), Some(13), None));
    assert_eq!((b.matching_take(11, &a), b.matching_take(13, &a)), (None, Some(2)));
}

#[test]
fn comp_sections_move_resize_and_change_take() {
    let spans = |f: &TakeFolder| f.comp.iter().map(|s| (s.start, s.end, s.take)).collect::<Vec<_>>();
    let mut f = folder(&[(1, 0, 1000), (2, 0, 1000), (3, 200, 800)]);
    f.select(1, 0, 300);
    f.select(2, 300, 500);
    f.clear(500, 700);
    f.select(1, 700, 1000);
    // A border moves both sections, and can't swallow either.
    f.move_edge(300, 250);
    assert_eq!(spans(&f)[..2], [(0, 250, 1), (250, 500, 2)]);
    f.move_edge(250, 900);
    assert_eq!(spans(&f)[..2], [(0, 499, 1), (499, 500, 2)]);
    f.move_edge(499, 300);
    // An outer edge grows into the gap, up to the next section, and shrinks.
    f.move_edge(500, 900);
    assert_eq!(spans(&f)[1], (300, 700, 2));
    // Grown up to the next section, the edge is a border again.
    f.move_edge(700, 600);
    assert_eq!(spans(&f), vec![(0, 300, 1), (300, 600, 2), (600, 1000, 1)]);
    let before = spans(&f);
    f.move_edge(650, 640);
    assert_eq!(spans(&f), before, "no edge there");
    // A section of take 3 stops where take 3's audio does.
    f.select(3, 300, 600);
    f.move_edge(300, 0);
    assert_eq!(spans(&f)[..2], [(0, 200, 1), (200, 600, 3)]);
    // Between lanes: takes without audio there are skipped.
    let s = f.section_at(800).unwrap();
    assert_eq!((s.take, f.neighbour_take(&s, 1), f.neighbour_take(&s, -1)), (1, Some(2), None));
    let s = f.section_at(400).unwrap();
    assert_eq!((f.neighbour_take(&s, -1), f.neighbour_take(&s, 1), f.section_at(1000)), (Some(2), None, None));
}

#[test]
fn previous_and_next_take_step_from_the_take_heard() {
    let mut f = folder(&[(1, 0, 1000), (2, 0, 1000), (3, 0, 1000)]);
    assert_eq!((f.stepped_take(1), f.stepped_take(-1)), (Some(1), Some(3)), "nothing heard");
    f.select_whole(2);
    assert_eq!((f.heard_take(), f.stepped_take(-1), f.stepped_take(1)), (Some(2), Some(1), Some(3)));
    f.select(3, 0, 600);
    assert_eq!((f.heard_take(), f.stepped_take(1), f.stepped_take(-1)), (Some(3), None, Some(2)));
}

#[test]
fn pieces_crossfade_at_boundaries() {
    let mut f = folder(&[(1, 0, 1000), (2, 0, 1000)]);
    f.select(1, 0, 500);
    f.select(2, 500, 1000);
    let r = region(5000, 1000, 0, f);
    let p = pass_pieces(&r, r.takes.as_ref().unwrap(), 5000, 1000, 100);
    assert_eq!(p.len(), 2);
    // Take 1 runs 50 frames past the boundary at 5500 and fades out across it; take 2
    // starts 50 early and fades in over the same 100 frames.
    assert_eq!((p[0].start, p[0].end, p[0].offset), (5000, 5550, 1000));
    assert_eq!(p[0].fade_out, (5450, 100));
    assert_eq!((p[1].start, p[1].end, p[1].offset), (5450, 6000, 2450));
    assert_eq!(p[1].fade_in, (5450, 100));
    // Trimming the folder's front (offset 200) shifts the take-time window.
    let mut r2 = r.clone();
    r2.start = 5200;
    r2.length = 800;
    r2.content = RegionContent::Audio { clip: 10, offset: 200, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
    let p = pass_pieces(&r2, r2.takes.as_ref().unwrap(), 5200, 800, 100);
    assert_eq!((p[0].start, p[0].offset), (5200, 1200));
    assert_eq!(flat_pieces(&r2, r2.takes.as_ref().unwrap()), vec![(5200, 300, 1, 1200), (5500, 500, 2, 2500)]);
}

#[test]
fn pieces_next_to_silence_fade_inside() {
    let mut f = folder(&[(1, 0, 1000)]);
    f.select(1, 300, 600);
    let r = region(0, 1000, 0, f);
    let p = pass_pieces(&r, r.takes.as_ref().unwrap(), 0, 1000, 100);
    assert_eq!((p[0].start, p[0].end, p[0].fade_in, p[0].fade_out), (300, 600, (300, 50), (550, 50)));
}

#[test]
fn comps_switch_and_keep_their_edits() {
    let mut f = folder(&[(1, 0, 1000), (2, 0, 1000)]);
    f.default_comp();
    assert_eq!((f.comp_names(), f.comp_name()), (vec!["Comp 1".to_string()], "Comp 1".to_string()));
    // Create New Comp starts from the newest take; edits stay with the comp.
    f.create_comp();
    assert_eq!((f.comp_names(), f.current), (vec!["Comp 1".to_string(), "Comp 2".to_string()], 1));
    f.select(1, 0, 500);
    f.store_comp();
    assert_eq!(f.comps[0].comp, vec![CompSegment { start: 0, end: 1000, take: 2 }]);
    f.switch_comp(0);
    assert_eq!(f.comp, vec![CompSegment { start: 0, end: 1000, take: 2 }]);
    f.switch_comp(1);
    assert_eq!(f.comp.len(), 2);
    // Duplicate copies the comp heard.
    f.duplicate_comp();
    assert_eq!((f.current, f.comp_name(), f.comp.len()), (2, "Comp 3".to_string(), 2));
    f.rename_comp(2, "  Lead  ".into());
    f.rename_comp(0, "".into());
    assert_eq!(f.comp_names(), vec!["Comp 1".to_string(), "Comp 2".to_string(), "Lead".to_string()]);
    // Deleting one before the current keeps the current one heard; the last stays.
    f.delete_comp(0);
    assert_eq!((f.current, f.comp_name()), (1, "Lead".to_string()));
    f.delete_comp(1);
    assert_eq!((f.current, f.comp_name(), f.comp.len()), (0, "Comp 2".to_string(), 2));
    f.delete_comp(0);
    assert_eq!(f.comps.len(), 1);
    // A deleted take drops out of every comp it is switched back to.
    f.create_comp();
    f.remove_take(2);
    f.switch_comp(0);
    assert!(f.comp.iter().all(|s| s.take == 1));
}

#[test]
fn midi_takes_window_and_store() {
    let note = |start, pitch| Note { start, length: 10, pitch, velocity: 100, muted: false };
    let mut f = TakeFolder::new();
    f.takes.push(Take {
        id: 1,
        name: "Take 1".into(),
        clip: 0,
        offset: 0,
        start: 0,
        end: 1000,
        gain_db: 0.0,
        notes: Arc::new(vec![note(50, 60), note(500, 62)]),
        ..Default::default()
    });
    f.takes.push(Take {
        id: 2,
        name: "Take 2".into(),
        clip: 0,
        offset: 0,
        start: 0,
        end: 1000,
        gain_db: 0.0,
        notes: Arc::new(vec![note(300, 72)]),
        ..Default::default()
    });
    assert_eq!(f.midi_take(), Some(2), "the newest without a comp");
    f.select_whole(1);
    assert_eq!(f.midi_take(), Some(1));
    let key = |n: &[Note]| n.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>();
    let (n, _) = f.midi_window(1, 100, 900);
    assert_eq!(key(&n), vec![(400, 62)]);
    // An edit inside the window replaces it; what's outside stays.
    f.store_midi(1, 100, 900, &[note(10, 64)], &[]);
    assert_eq!(key(&f.take(1).unwrap().notes), vec![(50, 60), (110, 64)]);
    // A region over the folder plays the take it selects, and trimming loses nothing.
    let mut r = Region {
        id: 9,
        name: "M".into(),
        start: 2000,
        length: 1000,
        muted: false,
        color: None,
        content: RegionContent::midi(Vec::new(), Vec::new()),
        loop_end: None,
        takes: Some(f),
        params: Default::default(),
        pattern: None,
    };
    load_midi_take(&mut r);
    retrim_midi_folder(&mut r, 2100, 900);
    assert!(matches!(&r.content, RegionContent::Midi { notes, .. } if key(notes) == vec![(10, 64)]));
    retrim_midi_folder(&mut r, 2000, 1000);
    assert!(matches!(&r.content, RegionContent::Midi { notes, .. } if notes.len() == 2));
    assert_eq!(r.takes.as_ref().unwrap().offset, 0);
}

#[test]
fn recorded_midi_splits_at_pass_marks() {
    let on = |p| (p, [0x90, 60, 100]);
    let events = vec![on(10), (0, PASS_MARK), (0, PASS_MARK), on(20), (0, PASS_MARK), on(30)];
    assert_eq!(midi_passes(&events), vec![vec![on(10)], vec![on(20)], vec![on(30)]], "an empty pass is dropped");
    assert_eq!(midi_passes(&[]).len(), 0);
}

#[test]
fn edged_pieces_reach_into_a_neighbour_crossfade() {
    let mut f = folder(&[(1, 0, 2000)]);
    f.select(1, 0, 2000);
    let r = region(5000, 1000, 500, f);
    let edges = PassEdges { lead: 100, tail: 50, fade_in: Some((4900, 200)), fade_out: Some((5950, 100)) };
    let p = pass_pieces_edged(&r, r.takes.as_ref().unwrap(), 5000, 1000, 100, edges);
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].start, p[0].end, p[0].offset), (4900, 6050, 1400));
    assert_eq!((p[0].fade_in, p[0].fade_out), ((4900, 200), (5950, 100)));
}

fn plan(cycle: Option<(i64, i64)>, punch: Option<(i64, i64)>, latency: i64) -> RecPlan {
    RecPlan { from: 0, cycle, punch, latency, midi_latency: 0, min_take: 10 }
}

#[test]
fn cycle_recordings_split_into_passes() {
    // A 1000-frame cycle at 2000, 30 frames of latency, three and a half passes recorded.
    let p = plan(Some((2000, 3000)), None, 30);
    assert_eq!(p.capture(2500), Some((2000, i64::MAX)));
    let (start, len, takes) = p.cycle_takes(3530).unwrap();
    assert_eq!((start, len), (2000, 1000));
    assert_eq!(takes, vec![(30, 0, 1000), (1030, 0, 1000), (2030, 0, 1000), (3030, 0, 500)]);
    // Stopped just after a pass boundary: the scrap is dropped.
    assert_eq!(p.cycle_takes(2035).unwrap().2.len(), 2);
}

#[test]
fn punch_inside_the_cycle_gates_each_pass() {
    let p = plan(Some((2000, 3000)), Some((2200, 2600)), 30);
    assert_eq!(p.cycle_window(), Some((2200, 2600, 2630)));
    assert_eq!(p.capture(0), Some((2200, 2630)));
    let (start, len, takes) = p.cycle_takes(3 * 430).unwrap();
    assert_eq!((start, len), (2200, 400));
    assert_eq!(takes, vec![(30, 0, 400), (460, 0, 400), (890, 0, 400)]);
}

#[test]
fn punch_without_cycle_trims_to_the_range() {
    let p = plan(None, Some((1000, 2000)), 30);
    assert_eq!(p.capture(400), Some((1000, 2030)));
    assert_eq!(p.capture(2500), None);
    // Captured from 1000 for 1030 frames: heard from 970, trimmed to 1000..2000.
    assert_eq!(p.punch_span(1000, 1030), Some((1000, 1000, 30)));
    // Recording started inside the range: placed as heard, like any recording.
    assert_eq!(p.punch_span(1500, 530), Some((1470, 530, 0)));
}
