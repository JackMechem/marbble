//! Region edits in each drag mode: overlap, no overlap, shuffle, and automation moving along.

use super::*;
use crate::audio_io::AudioConfig;

fn session() -> (Session, Id) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-drag-modes-{}", std::process::id()));
    let t = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
    (s, t)
}

fn midi(s: &mut Session, track: Id, start: i64, length: i64) -> Id {
    let id = s.project.alloc_id();
    let content = RegionContent::midi(vec![Note { start: 0, length: 100, pitch: 60, velocity: 100, muted: false }], Vec::new());
    s.project.track_mut(track).unwrap().regions.push(Region {
        id,
        name: "M".into(),
        start,
        length,
        muted: false,
        color: None,
        content,
        loop_end: None,
        takes: None,
        params: Default::default(),
        pattern: None,
    });
    s.push_regions(track);
    id
}

/// (id, start, length) by start.
fn lane(s: &Session, track: Id) -> Vec<(Id, i64, i64)> {
    let mut v: Vec<(Id, i64, i64)> = s.project.track(track).unwrap().regions.iter().map(|r| (r.id, r.start, r.length)).collect();
    v.sort_by_key(|x| x.1);
    v
}

#[test]
fn overlap_and_xfade_modes_are_the_plain_edits() {
    let (mut s, t) = session();
    let a = midi(&mut s, t, 0, 1000);
    let b = midi(&mut s, t, 1000, 1000);
    s.drag_move_regions(&[RegionMove { id: b, start: 500, track: None }], None);
    assert_eq!(lane(&s, t), vec![(a, 0, 1000), (b, 500, 1000)]);
    s.drag_delete_regions(&[a]);
    assert_eq!(lane(&s, t), vec![(b, 500, 1000)]);
}

#[test]
fn no_overlap_trims_the_left_region() {
    let (mut s, t) = session();
    s.set_drag_mode(DragMode::NoOverlap);
    let a = midi(&mut s, t, 0, 1000);
    let b = midi(&mut s, t, 2000, 1000);
    s.drag_move_regions(&[RegionMove { id: b, start: 600, track: None }], None);
    assert_eq!(lane(&s, t), vec![(a, 0, 600), (b, 600, 1000)]);
    // Lengthening a's end is stopped by b.
    s.drag_resize_region(a, 0, 5000);
    assert_eq!(lane(&s, t), vec![(a, 0, 600), (b, 600, 1000)]);
    // A copy pasted exactly over b replaces it.
    let r = s.region(b).unwrap().clone();
    let made = s.drag_paste_regions(&[(t, r)], 0);
    assert_eq!(lane(&s, t), vec![(a, 0, 600), (made[0], 600, 1000)]);
}

#[test]
fn trimming_several_regions_trims_each_once() {
    let (mut s, t) = session();
    let a = midi(&mut s, t, 0, 1000);
    let b = midi(&mut s, t, 2000, 1000);
    // Both ends 200 later, then both starts 100 later: the plain edits.
    s.drag_resize_regions(&[(a, 0, 1200), (b, 2000, 1200)]);
    assert_eq!(lane(&s, t), vec![(a, 0, 1200), (b, 2000, 1200)]);
    s.drag_resize_regions(&[(a, 100, 1100), (b, 2100, 1100)]);
    assert_eq!(lane(&s, t), vec![(a, 100, 1100), (b, 2100, 1100)]);
}

#[test]
fn trimming_several_regions_shuffles_one_trim_after_another() {
    let (mut s, t) = session();
    s.set_drag_mode(DragMode::ShuffleL);
    let a = midi(&mut s, t, 0, 1000);
    let b = midi(&mut s, t, 2000, 1000);
    let notes = |s: &Session, id: Id| match &s.region(id).unwrap().content {
        RegionContent::Midi { notes, .. } => notes.iter().map(|n| (n.start, n.length)).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let before = notes(&s, b);
    assert!(!before.is_empty());
    // a's longer end pushes b along, and b's own trim still lengthens its end without
    // touching its notes: it isn't taken for a trim of its start.
    s.drag_resize_regions(&[(a, 0, 1200), (b, 2000, 1200)]);
    assert_eq!(lane(&s, t), vec![(a, 0, 1200), (b, 2200, 1200)]);
    assert_eq!(notes(&s, b), before);
    // Shorter again, in the other order: the lane is as after one trim and then the other.
    s.drag_resize_regions(&[(b, 2200, 1000), (a, 0, 1000)]);
    assert_eq!(lane(&s, t), vec![(a, 0, 1000), (b, 2000, 1000)]);
    assert_eq!(notes(&s, b), before);
}

#[test]
fn shuffle_l_closes_and_opens_gaps() {
    let (mut s, t) = session();
    s.set_drag_mode(DragMode::ShuffleL);
    let a = midi(&mut s, t, 0, 1000);
    let b = midi(&mut s, t, 1000, 500);
    let c = midi(&mut s, t, 1500, 1000);
    // Deleting b pulls c left.
    s.drag_delete_regions(&[b]);
    assert_eq!(lane(&s, t), vec![(a, 0, 1000), (c, 1000, 1000)]);
    // Dragging a onto c's second half swaps them.
    s.drag_move_regions(&[RegionMove { id: a, start: 1700, track: None }], None);
    assert_eq!(lane(&s, t), vec![(c, 0, 1000), (a, 1000, 1000)]);
    // Trimming c's end shorter pulls a along.
    s.drag_resize_region(c, 0, 400);
    assert_eq!(lane(&s, t), vec![(c, 0, 400), (a, 400, 1000)]);
    // Trimming c's start keeps it at 0 and pulls a further.
    s.drag_resize_region(c, 100, 300);
    assert_eq!(lane(&s, t), vec![(c, 0, 300), (a, 300, 1000)]);
    // Pasting a copy of c onto a's first half inserts it before a.
    let r = s.region(c).unwrap().clone();
    let made = s.drag_paste_regions(&[(t, r)], 310);
    assert_eq!(lane(&s, t), vec![(c, 0, 300), (made[0], 300, 300), (a, 600, 1000)]);
}

#[test]
fn automation_moves_with_the_region() {
    let (mut s, t) = session();
    let a = midi(&mut s, t, 1000, 1000);
    s.set_automation_lane(
        t,
        AutoParam::Volume,
        vec![
            AutoPoint { pos: 0, value: -10.0, curve: 0.0 },
            AutoPoint { pos: 1200, value: 0.0, curve: 0.0 },
            AutoPoint { pos: 1800, value: 0.0, curve: 0.0 },
            AutoPoint { pos: 3000, value: -10.0, curve: 0.0 },
        ],
    );
    let curve = |s: &Session, x: i64| crate::edit::automation::value_at(&s.project.track(t).unwrap().automation[0].points, x).unwrap();
    // Never (the default): the curve stays.
    s.drag_move_regions(&[RegionMove { id: a, start: 5000, track: None }], None);
    assert_eq!(curve(&s, 1500), 0.0);
    // Back, then Always.
    s.drag_move_regions(&[RegionMove { id: a, start: 1000, track: None }], None);
    s.prefs.editing.move_automation = MoveAutomation::Always;
    s.drag_move_regions(&[RegionMove { id: a, start: 5000, track: None }], None);
    assert_eq!(curve(&s, 5500), 0.0);
    assert!((curve(&s, 1500) - curve(&s, 1000)).abs() < 0.5, "the old place is bridged");
    // An explicit answer (Ask) overrides the setting.
    s.drag_nudge_regions(&[a], 1000, Some(false));
    assert_eq!(curve(&s, 5500), 0.0);
    assert!(curve(&s, 6900) < -1.0);
}
