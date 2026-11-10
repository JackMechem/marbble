//! Section edits, Trim and Fill, Shuffle, Paste Replace, Move to Playhead, Strip Silence and names,
//! through the session.

use super::*;
use crate::audio_io::AudioConfig;
use crate::edit::automation::value_at;

/// A session on the silent ALSA null device with two audio tracks and a 200 000-frame
/// clip: a 100 Hz tone with silence at 50 000..80 000 and from 150 000 on.
fn session() -> (Session, Vec<Id>, Id) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-arrange-cmds-{}", std::process::id()));
    let path = s.dir.join("Audio Files").join("tone.wav");
    let tone: Vec<f32> = (0..200_000)
        .map(|i| if (50_000..80_000).contains(&i) || i >= 150_000 { 0.0 } else { 0.5 * (i as f32 * std::f32::consts::TAU * 100.0 / 48_000.0).sin() })
        .collect();
    let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48_000, vec![tone]));
    let tracks: Vec<Id> = (0..2).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
    (s, tracks, clip)
}

fn region(s: &mut Session, t: Id, clip: Id, start: i64, length: i64, offset: i64) -> Id {
    let id = s.project.alloc_id();
    let content = RegionContent::Audio { clip, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
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

fn pt(pos: i64, value: f32) -> AutoPoint {
    AutoPoint { pos, value, curve: 0.0 }
}

#[test]
fn insert_silence_cut_and_repeat_move_everything_with_its_time() {
    let (mut s, t, clip) = session();
    region(&mut s, t[0], clip, 1000, 4000, 0);
    region(&mut s, t[1], clip, 6000, 1000, 0);
    s.set_automation_lane(t[0], AutoParam::Volume, vec![pt(0, -10.0), pt(4000, 0.0)]);
    s.set_automation_lane(MASTER_ID, AutoParam::Volume, vec![pt(2000, -6.0), pt(3000, 0.0)]);
    s.add_marker(2500, Some("Hit".into()));
    s.add_marker(6000, Some("Late".into()));
    let sec = s.add_arrangement_marker(None);
    s.resize_arrangement_marker(sec, 5000);

    // Silence at 2000..3000: the region is cut at 2000 and its tail moves on.
    s.section_edit(SectionEdit::InsertSilence, 2000, 3000, None).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1000, 2000, 0), (3000, 6000, 1000)]);
    assert_eq!(spans(&s, t[1]), vec![(7000, 8000, 0)]);
    assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![3500, 7000]);
    assert_eq!((s.project.arrangement[0].start, s.project.arrangement[0].length), (0, 6000), "the section takes in the silence");
    let vol = &s.project.track(t[0]).unwrap().automation[0].points;
    assert!((value_at(vol, 1999).unwrap() - -5.0).abs() < 0.01 && (value_at(vol, 3000).unwrap() - -5.0).abs() < 0.01, "{vol:?}");
    let master = &s.project.master.automation[0].points;
    assert!((value_at(master, 2500).unwrap() - -6.0).abs() < 0.01 && (value_at(master, 3500).unwrap() - -3.0).abs() < 0.01, "{master:?}");

    // Cut 1500..3500 back out: the silence and 500 frames each side go.
    s.section_edit(SectionEdit::Cut, 1500, 3500, None).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 0), (1500, 4000, 1500)]);
    assert_eq!(spans(&s, t[1]), vec![(5000, 6000, 0)]);
    assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![1500, 5000]);

    // Repeat 1000..2000 (two regions): copies land at 2000..3000, the rest moves on.
    let made = s.section_edit(SectionEdit::Repeat, 1000, 2000, None).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 0), (1500, 2000, 1500), (2000, 2500, 0), (2500, 3000, 1500), (3000, 5000, 2000)]);
    assert_eq!(made.len(), 2);
    assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![1500, 2500, 6000], "the marker in the section is repeated");
}

#[test]
fn a_marquee_limits_a_section_edit_to_its_tracks() {
    let (mut s, t, clip) = session();
    region(&mut s, t[0], clip, 1000, 1000, 0);
    region(&mut s, t[1], clip, 1000, 1000, 0);
    s.add_marker(1500, None);
    s.section_edit(SectionEdit::InsertSilence, 500, 700, Some(&[t[1]])).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1000, 2000, 0)]);
    assert_eq!(spans(&s, t[1]), vec![(1200, 2200, 0)]);
    assert_eq!(s.project.markers[0].pos, 1500, "the global tracks stay");
    assert!(s.section_edit(SectionEdit::Cut, 700, 700, None).is_err());
}

#[test]
fn insert_cut_section_puts_the_cut_back_elsewhere() {
    let (mut s, t, clip) = session();
    region(&mut s, t[0], clip, 1000, 2000, 0);
    s.set_automation_lane(t[0], AutoParam::Volume, vec![pt(1000, -20.0), pt(2000, 0.0), pt(10_000, 0.0)]);
    s.add_marker(1200, Some("In".into()));
    assert!(s.insert_cut_section(0).is_err(), "nothing cut yet");
    s.section_edit(SectionEdit::Cut, 1000, 1500, None).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1000, 2500, 500)]);
    // Paste it at 2000: room is made there, and the cut part comes back.
    let made = s.insert_cut_section(2000).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(1000, 2000, 500), (2000, 2500, 0), (2500, 3000, 1500)]);
    assert_eq!(made.len(), 1);
    assert_eq!(s.project.markers.iter().map(|m| (m.pos, m.name.as_str())).collect::<Vec<_>>(), vec![(2200, "In")]);
    let vol = &s.project.track(t[0]).unwrap().automation[0].points;
    assert!((value_at(vol, 2000).unwrap() - -20.0).abs() < 0.05 && (value_at(vol, 2250).unwrap() - -15.0).abs() < 0.05, "{vol:?}");
}

/// What Cut Section took out belongs to its project. Another one (here a new one, whose ids
/// start over, so that its first track and clip take the old ones' ids) has nothing to insert;
/// nor has a project opened, even the same one again.
#[test]
fn another_project_has_no_cut_section_to_insert() {
    let (mut s, t, clip) = session();
    region(&mut s, t[0], clip, 1000, 2000, 0);
    s.section_edit(SectionEdit::Cut, 1000, 1500, None).unwrap();
    s.new_project();
    s.dir = std::env::temp_dir().join(format!("marbble-arrange-cmds-other-{}", std::process::id()));
    let path = s.dir.join("Audio Files").join("other.wav");
    let other = s.add_clip(&path, Clip::from_channels(path.clone(), 48_000, vec![vec![0.25; 10_000]]));
    let track = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    assert_eq!((other, track), (clip, t[0]), "the new project's ids are the old one's");
    assert!(s.insert_cut_section(0).is_err(), "nothing to insert");
    assert!(spans(&s, track).is_empty());

    region(&mut s, track, other, 0, 4000, 0);
    s.section_edit(SectionEdit::Cut, 1000, 1500, None).unwrap();
    s.save(None).unwrap();
    let dir = s.dir.clone();
    s.open(&dir).unwrap();
    assert!(s.insert_cut_section(0).is_err(), "nothing to insert after opening");
    assert_eq!(spans(&s, track), vec![(0, 1000, 0), (1000, 3500, 1500)], "as saved");
    let _ = std::fs::remove_dir_all(&dir);
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

#[test]
fn section_edits_keep_what_a_pattern_region_plays() {
    let (mut s, _, _) = session();
    let bar = s.tempo_map().samples_per_bar_at(0.0).round() as i64;
    let s16 = bar / 16;
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let id = s.create_pattern_region(t, 0, 2 * bar).unwrap();
    // A kick every third sixteenth: the second bar starts on the row's second step.
    let mut p = s.region(id).unwrap().pattern.clone().unwrap();
    p.rows[0].length = Some(3);
    p.rows[0].steps = vec![crate::edit::pattern::Step { on: true, ..Default::default() }];
    s.set_pattern(id, p).unwrap();
    let before = heard(&s);
    let shifted = |from: i64, by: i64| -> Vec<(i64, u8)> { before.iter().map(|&(x, p)| (if x < from { x } else { x + by }, p)).collect() };

    // A bar of silence at bar 2: the second bar moves on and plays as it did.
    s.section_edit(SectionEdit::InsertSilence, bar, 2 * bar, None).unwrap();
    assert_eq!(heard(&s), shifted(bar, bar));
    s.section_edit(SectionEdit::Cut, bar, 2 * bar, None).unwrap();
    assert_eq!(heard(&s), before);

    // Cut from the sixth sixteenth (a kick) to bar 2, and Insert Cut Section back there.
    s.section_edit(SectionEdit::Cut, 6 * s16, bar, None).unwrap();
    assert_eq!(
        heard(&s),
        before.iter().filter(|n| n.0 < 6 * s16 || n.0 >= bar).map(|&(x, p)| (if x < bar { x } else { x - bar + 6 * s16 }, p)).collect::<Vec<_>>()
    );
    s.insert_cut_section(6 * s16).unwrap();
    assert_eq!(heard(&s), before);

    // Repeat the first bar: the copy plays it again, and the second bar moves on.
    s.section_edit(SectionEdit::Repeat, 0, bar, None).unwrap();
    let mut want = shifted(bar, bar);
    want.extend(before.iter().filter(|n| n.0 < bar).map(|&(x, p)| (x + bar, p)));
    want.sort();
    assert_eq!(heard(&s), want);
}

#[test]
fn section_edits_move_inactive_track_alternatives_and_put_them_back() {
    let (mut s, t, clip) = session();
    region(&mut s, t[0], clip, 1000, 2000, 0);
    // A waits with the region; B is empty, then has one of its own after the section.
    s.new_track_alternative(t[0], false).unwrap();
    region(&mut s, t[0], clip, 4000, 500, 0);
    s.section_edit(SectionEdit::InsertSilence, 1500, 2000, None).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(4500, 5000, 0)]);
    let a = |s: &Session| {
        let mut v: Vec<(i64, i64)> = s.project.track(t[0]).unwrap().alternatives[0].regions.iter().map(|r| (r.start, r.end())).collect();
        v.sort();
        v
    };
    assert_eq!(a(&s), vec![(1000, 1500), (2000, 3500)]);
    // Cut 1200..2400 and put it back: A gets its part back, B nothing.
    s.section_edit(SectionEdit::Cut, 1200, 2400, None).unwrap();
    assert_eq!(a(&s), vec![(1000, 1200), (1200, 2300)]);
    let made = s.insert_cut_section(1200).unwrap();
    assert!(made.is_empty(), "nothing went into the active alternative");
    assert_eq!(spans(&s, t[0]), vec![(4500, 5000, 0)]);
    assert_eq!(a(&s), vec![(1000, 1200), (1200, 1500), (2000, 2400), (2400, 3500)]);
    // Insert it again after switching: the part goes to A, now the active one.
    s.switch_track_alternative(t[0], "A").unwrap();
    s.section_edit(SectionEdit::Cut, 1200, 2400, None).unwrap();
    s.switch_track_alternative(t[0], "B").unwrap();
    s.switch_track_alternative(t[0], "A").unwrap();
    assert_eq!(s.insert_cut_section(1200).unwrap().len(), 2);
    assert_eq!(spans(&s, t[0]).len(), 4);
}

#[test]
fn gaps_overlaps_and_trims() {
    let (mut s, t, clip) = session();
    let a = region(&mut s, t[0], clip, 0, 1000, 0);
    let b = region(&mut s, t[0], clip, 3000, 1000, 0);
    let c = region(&mut s, t[0], clip, 6000, 1000, 0);
    s.remove_gaps(&[a, b, c]);
    assert_eq!(spans(&s, t[0]), vec![(0, 1000, 0), (1000, 2000, 0), (2000, 3000, 0)]);
    // Overlaps: b moved back over a.
    s.move_region(b, 500, None);
    s.remove_overlaps(&[a, b]);
    assert_eq!(spans(&s, t[0]), vec![(0, 500, 0), (500, 1500, 0), (2000, 3000, 0)]);
    // b's end to the next region (lengthened), then a's start and end to a playhead.
    s.trim_to_next_region(&[b]);
    assert_eq!(spans(&s, t[0])[1], (500, 2000, 0));
    s.trim_regions_to(&[a], 200, false);
    s.trim_regions_to(&[c], 2600, true);
    assert_eq!(spans(&s, t[0]), vec![(200, 500, 200), (500, 2000, 0), (2000, 2600, 0)]);
    // Same length as the first: c grows to b's 1500.
    s.resize_to_same_length(&[b, c]);
    assert_eq!(spans(&s, t[0])[2], (2000, 3500, 0));
}

#[test]
fn fill_within_locators_lengthens_up_to_the_next_region_or_the_right_locator() {
    let (mut s, t, clip) = session();
    let a = region(&mut s, t[0], clip, 1000, 500, 0);
    let b = region(&mut s, t[0], clip, 3000, 500, 0);
    let out = region(&mut s, t[0], clip, 9000, 100, 0);
    s.fill_within_locators(&[a, b, out], 1000, 5000);
    assert_eq!(spans(&s, t[0]), vec![(1000, 3000, 0), (3000, 5000, 0), (9000, 9100, 0)]);
}

#[test]
fn crop_and_delete_and_shuffle() {
    let (mut s, t, clip) = session();
    let a = region(&mut s, t[0], clip, 0, 1000, 0);
    let b = region(&mut s, t[0], clip, 1000, 1000, 0);
    let c = region(&mut s, t[0], clip, 5000, 1000, 0);
    s.crop_regions(&[a, b, c], 500, 1500).unwrap();
    assert_eq!(spans(&s, t[0]), vec![(500, 1000, 500), (1000, 1500, 0)]);
    let d = region(&mut s, t[0], clip, 1500, 500, 0);
    let e = region(&mut s, t[0], clip, 3000, 500, 0);
    s.delete_and_shuffle(&[d]);
    assert_eq!(spans(&s, t[0]), vec![(500, 1000, 500), (1000, 1500, 0), (2500, 3000, 0)]);
    assert!(s.region(e).is_some());
}

#[test]
fn paste_replace_clears_the_pasted_range_first() {
    let (mut s, t, clip) = session();
    region(&mut s, t[0], clip, 0, 10_000, 0);
    let item = s.project.track(t[0]).unwrap().regions[0].clone();
    let small = Region { start: 0, length: 2000, ..item };
    let made = s.paste_replace(&[(t[1], small.clone()), (t[0], small)], 4000);
    assert_eq!(made.len(), 2);
    assert_eq!(spans(&s, t[0]), vec![(0, 4000, 0), (4000, 6000, 0), (6000, 10_000, 6000)]);
    assert_eq!(spans(&s, t[1]), vec![(4000, 6000, 0)]);
    assert_eq!(s.project.track(t[0]).unwrap().regions.len(), 3, "the part under the paste went");
}

#[test]
fn move_to_playhead_keeps_the_spacing() {
    let (mut s, t, clip) = session();
    let a = region(&mut s, t[0], clip, 1000, 100, 0);
    let b = region(&mut s, t[1], clip, 1500, 100, 0);
    s.move_regions_to(&[a, b], 4000);
    assert_eq!((spans(&s, t[0]), spans(&s, t[1])), (vec![(4000, 4100, 0)], vec![(4500, 4600, 0)]));
}

#[test]
fn strip_silence_cuts_out_the_quiet_parts() {
    let (mut s, t, clip) = session();
    let id = region(&mut s, t[0], clip, 10_000, 200_000, 0);
    let set = StripSettings { threshold: 0.04, min_time: 0.1, pre_attack: 0.0, post_release: 0.0 };
    let preview = s.strip_silence_preview(id, &set);
    assert_eq!(preview.len(), 2, "{preview:?}");
    let made = s.strip_silence(&[id], &set);
    assert_eq!(made.len(), 2);
    assert_eq!(made[0], id, "the first part keeps the id");
    let got = spans(&s, t[0]);
    assert_eq!(got.len(), 2);
    assert!((got[0].0 - 10_000).abs() < 10 && (got[0].1 - 60_000).abs() < 10, "{got:?}");
    assert!((got[1].0 - 90_000).abs() < 10 && (got[1].1 - 160_000).abs() < 10 && (got[1].2 - 80_000).abs() < 10, "{got:?}");
    assert_eq!(got.iter().map(|g| (g.0, g.1)).collect::<Vec<_>>(), preview, "the preview shows the cuts");
    // A silent region goes.
    let quiet = region(&mut s, t[1], clip, 0, 10_000, 160_000);
    assert!(s.strip_silence(&[quiet], &set).is_empty() && spans(&s, t[1]).is_empty());
}

#[test]
fn names_and_colours_go_between_tracks_and_regions() {
    let (mut s, t, clip) = session();
    let a = region(&mut s, t[0], clip, 0, 100, 0);
    let b = region(&mut s, t[0], clip, 200, 100, 0);
    s.rename_region(a, "Lead".into());
    s.set_region_color(&[a], Some("#ff0000".into()));
    s.name_tracks_by_region(&[b, a]);
    s.color_tracks_by_region(&[a, b]);
    let tr = s.project.track(t[0]).unwrap();
    assert_eq!((tr.name.as_str(), tr.color.as_str()), ("Lead", "#ff0000"));
    s.update_track(t[0], TrackPatch { name: Some("Vox".into()), ..Default::default() });
    s.name_regions_by_track(&[a, b]);
    s.color_regions_by_track(&[a]);
    let tr = s.project.track(t[0]).unwrap();
    assert!(tr.regions.iter().all(|r| r.name == "Vox"));
    assert!(tr.regions.iter().all(|r| r.color.is_none()));
}
