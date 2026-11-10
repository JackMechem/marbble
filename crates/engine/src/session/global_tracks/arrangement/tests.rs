//! Arrangement track sections, and moving, copying and deleting them with their content.

use super::*;
use crate::audio_file::clip::Clip;
use crate::audio_io::AudioConfig;

/// A session on the silent ALSA null device with two audio tracks, at 120 BPM in 4/4 (a
/// bar is 96 000 samples at 48 kHz), and three two-bar sections from bar 1.
fn session() -> (Session, Vec<Id>, Vec<Id>, Id) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-arrangement-{}", std::process::id()));
    let path = s.dir.join("Audio Files").join("tone.wav");
    let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![vec![0.25; 2_000_000]]));
    let tracks: Vec<Id> = (0..2).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
    let sections: Vec<Id> = (0..3)
        .map(|_| {
            let id = s.add_arrangement_marker(None);
            s.resize_arrangement_marker(id, 1000);
            id
        })
        .collect();
    (s, tracks, sections, clip)
}

fn region(s: &mut Session, t: Id, clip: Id, start: i64, length: i64) -> Id {
    let id = s.project.alloc_id();
    let content = RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
    s.project.track_mut(t).unwrap().regions.push(Region {
        id,
        name: "R".into(),
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
    s.push_regions(t);
    id
}

/// (start, end, clip offset) of a track's regions, by start.
fn spans(s: &Session, t: Id) -> Vec<(i64, i64, i64)> {
    let mut v: Vec<(i64, i64, i64)> = s
        .project
        .track(t)
        .unwrap()
        .regions
        .iter()
        .map(|r| match r.content {
            RegionContent::Audio { offset, .. } => (r.start, r.end(), offset),
            _ => (r.start, r.end(), 0),
        })
        .collect();
    v.sort();
    v
}

#[test]
fn adding_names_and_lays_out_eight_bars() {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let a = s.add_arrangement_marker(None);
    let b = s.add_arrangement_marker(Some("Chorus".into()));
    let bar = (4.0 * 60.0 / s.project.tempo * s.project.sample_rate as f64) as i64;
    let v = &s.project.arrangement;
    assert_eq!((v[0].id, v[0].name.as_str(), v[0].start, v[0].length), (a, "Intro", 0, 8 * bar));
    assert_eq!((v[1].id, v[1].name.as_str(), v[1].start, v[1].length), (b, "Chorus", 8 * bar, 8 * bar));
}

#[test]
fn moving_a_section_takes_its_regions_and_cuts_the_ones_across_its_edge() {
    let (mut s, t, sec, clip) = session();
    // Sections: 0..1000, 1000..2000, 2000..3000. A region across the 2|3 boundary on track
    // 0, one inside section 3 on track 1, and one after the arrangement.
    let across = region(&mut s, t[0], clip, 1500, 1000);
    let inside = region(&mut s, t[1], clip, 2200, 300);
    region(&mut s, t[1], clip, 3500, 100);
    let picked = s.move_arrangement_marker(sec[2], Some(sec[1]), false).unwrap();
    // Section 3 is now at 1000..2000 and section 2 at 2000..3000.
    assert_eq!(s.project.arrangement.iter().map(|x| (x.id, x.start)).collect::<Vec<_>>(), vec![(sec[0], 0), (sec[2], 1000), (sec[1], 2000)]);
    // The region's first half (section 2) moved to 2500..3000; its second half (section
    // 3, playing from offset 500) to 1000..1500.
    assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 500), (2500, 3000, 0)]);
    assert_eq!(spans(&s, t[1]), vec![(1200, 1500, 0), (3500, 3600, 0)]);
    let first = s.project.track(t[0]).unwrap().regions.iter().find(|r| r.start == 2500).unwrap().id;
    assert_eq!(first, across, "the first piece keeps the id");
    let mut picked = picked;
    picked.sort();
    let mut want: Vec<Id> = s.project.tracks.iter().flat_map(|x| x.regions.iter()).filter(|r| r.start >= 1000 && r.end() <= 2000).map(|r| r.id).collect();
    want.sort();
    assert_eq!(picked, want);
    assert!(picked.contains(&inside));
}

#[test]
fn copy_and_delete_with_content_ripple_the_rest() {
    let (mut s, t, sec, clip) = session();
    region(&mut s, t[0], clip, 100, 800);
    region(&mut s, t[1], clip, 2500, 1000);
    s.add_marker(500, Some("Hit".into()));
    // A copy of section 1 at the end of the arrangement.
    let made = s.move_arrangement_marker(sec[0], None, true).unwrap();
    assert_eq!(s.project.arrangement.len(), 4);
    assert_eq!(spans(&s, t[0]), vec![(100, 900, 0), (3100, 3900, 0)]);
    // The region crossing the arrangement's end: its tail is after the arrangement, so it
    // moves along with the time after it.
    assert_eq!(spans(&s, t[1]), vec![(2500, 3000, 0), (4000, 4500, 500)]);
    assert_eq!(made.len(), 1);
    assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![500, 3500]);
    // Delete section 1 and its content: everything after it moves 1000 earlier.
    s.delete_arrangement_marker(sec[0], true).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(2100, 2900, 0)]);
    assert_eq!(spans(&s, t[1]), vec![(1500, 2000, 0), (3000, 3500, 500)]);
    assert_eq!(s.project.arrangement[0].start, 0);
    assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![2500]);
}

#[test]
fn stereo_out_automation_moves_with_its_section() {
    let (mut s, _, sec, _) = session();
    let pt = |pos: i64, value: f32| crate::model::AutoPoint { pos, value, curve: 0.0 };
    // A dip in the middle section's Stereo Out volume.
    s.set_automation_lane(MASTER_ID, AutoParam::Volume, vec![pt(1200, 0.0), pt(1500, -20.0), pt(1800, 0.0)]);
    s.move_arrangement_marker(sec[1], None, false).unwrap();
    let pos: Vec<i64> = s.project.master.automation[0].points.iter().map(|p| p.pos).collect();
    assert!(pos.contains(&2500), "the dip moved with section 2 to 2000..3000: {pos:?}");
    assert!(!pos.contains(&1500));
}

#[test]
fn a_frozen_track_in_the_way_refuses_the_move() {
    let (mut s, t, sec, clip) = session();
    region(&mut s, t[0], clip, 1200, 100);
    s.project.track_mut(t[0]).unwrap().frozen = Some(FreezeInfo { clip, start: 0 });
    assert!(s.move_arrangement_marker(sec[1], None, false).is_err());
    assert_eq!(spans(&s, t[0]), vec![(1200, 1300, 0)]);
    assert_eq!(s.project.arrangement[1].start, 1000, "nothing moved");
}

/// Every note the project plays: (timeline start, pitch), sorted.
fn heard(s: &Session) -> Vec<(i64, u8)> {
    let mut v: Vec<(i64, u8)> = s
        .played_notes()
        .iter()
        .flat_map(|p| {
            let start = s.region(p.region).unwrap().start;
            p.notes.iter().map(move |n| (start + n.start, n.pitch))
        })
        .collect();
    v.sort();
    v
}

/// A pattern region over `bars` bars from 0 whose kick plays every third sixteenth, so
/// each bar starts on another step of the row. Returns it, the track and the bar length.
fn pattern_region(s: &mut Session, bars: i64) -> (Id, Id, i64) {
    let bar = s.tempo_map().samples_per_bar_at(0.0).round() as i64;
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let id = s.create_pattern_region(t, 0, bars * bar).unwrap();
    let mut p = s.region(id).unwrap().pattern.clone().unwrap();
    p.rows[0].length = Some(3);
    p.rows[0].steps = vec![crate::edit::pattern::Step { on: true, ..Default::default() }];
    s.set_pattern(id, p).unwrap();
    (id, t, bar)
}

#[test]
fn pattern_regions_cut_by_a_move_play_on_from_the_cut() {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let (id, t, bar) = pattern_region(&mut s, 3);
    let sec: Vec<Id> = (0..3)
        .map(|_| {
            let id = s.add_arrangement_marker(None);
            s.resize_arrangement_marker(id, bar);
            id
        })
        .collect();
    let before = heard(&s);
    assert!(before.contains(&(0, 36)) && !before.contains(&(bar, 36)), "bar 2 starts on the row's second step");
    // Bars 2 and 3 trade places, each playing what it played before.
    s.move_arrangement_marker(sec[2], Some(sec[1]), false).unwrap();
    let mut want: Vec<(i64, u8)> = before
        .iter()
        .map(|&(x, p)| {
            (
                if x < bar {
                    x
                } else if x < 2 * bar {
                    x + bar
                } else {
                    x - bar
                },
                p,
            )
        })
        .collect();
    want.sort();
    assert_eq!(heard(&s), want);
    let regions = &s.project.track(t).unwrap().regions;
    assert_eq!(regions.len(), 3);
    assert!(regions.iter().any(|r| r.id == id && r.pattern.as_ref().unwrap().offset == 0.0));
    // Splitting by hand still starts the pattern again.
    let piece = regions.iter().find(|r| r.start == 0).unwrap().id;
    s.split_regions_at(&[piece], &[bar / 2]);
    let second = s.project.track(t).unwrap().regions.iter().find(|r| r.start == bar / 2).unwrap();
    assert_eq!(second.pattern.as_ref().unwrap().offset, 0.0);
    assert!(heard(&s).contains(&(bar / 2, 36)));
}

#[test]
fn inactive_track_alternatives_move_with_their_sections() {
    let (mut s, t, sec, clip) = session();
    region(&mut s, t[0], clip, 1500, 1000);
    // Alternative B is a copy, then loses its region for one in section 3 alone; A waits.
    s.new_track_alternative(t[0], true).unwrap();
    s.project.track_mut(t[0]).unwrap().regions.clear();
    region(&mut s, t[0], clip, 2200, 300);
    s.move_arrangement_marker(sec[2], Some(sec[1]), false).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1200, 1500, 0)]);
    // Switching to A finds its region cut and moved as the active one would have been.
    s.switch_track_alternative(t[0], "A").unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 500), (2500, 3000, 0)]);
    let ids: Vec<Id> =
        s.project.tracks.iter().flat_map(|x| x.regions.iter().chain(x.alternatives.iter().flat_map(|a| a.regions.iter()))).map(|r| r.id).collect();
    assert!(ids.iter().all(|id| ids.iter().filter(|x| *x == id).count() == 1), "every piece has an id of its own: {ids:?}");
    // A frozen track's alternatives can't change: a move that would cut one is refused.
    s.delete_track_alternative(t[0], "B").unwrap();
    s.new_track_alternative(t[0], false).unwrap();
    s.project.track_mut(t[0]).unwrap().frozen = Some(FreezeInfo { clip, start: 0 });
    assert!(s.move_arrangement_marker(sec[2], None, false).is_err());
    s.switch_track_alternative(t[0], "A").unwrap_err();
    assert_eq!(s.project.track(t[0]).unwrap().alternatives[0].regions.len(), 2, "nothing moved");
}

#[test]
fn looped_regions_across_an_edge_become_real_copies() {
    let (mut s, t, sec, clip) = session();
    let id = region(&mut s, t[0], clip, 600, 300);
    s.project.track_mut(t[0]).unwrap().regions[0].loop_end = Some(1500);
    s.move_arrangement_marker(sec[1], Some(sec[0]), false).unwrap();
    // Passes 600..900, 900..1200, 1200..1500; section 2 (1000..2000) goes to 0..1000 and
    // section 1 to 1000..2000.
    assert_eq!(spans(&s, t[0]), vec![(0, 200, 100), (200, 500, 0), (1600, 1900, 0), (1900, 2000, 0)]);
    assert!(s.project.track(t[0]).unwrap().regions.iter().all(|r| r.loop_end.is_none()));
    assert!(s.project.track(t[0]).unwrap().regions.iter().any(|r| r.id == id));
}
