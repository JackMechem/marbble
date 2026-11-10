//! The Audio Track editor's edits: splits, and silence, delete, trim, fades, normalize and reverse on a selection.

use super::*;
use crate::audio_io::AudioConfig;

/// A session on the silent ALSA null device, with one audio track holding one region of a
/// synthetic clip (frame i has the value i / frames, so offsets are easy to check).
fn session(frames: usize) -> (Session, Id, Id) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-range-edit-{}", std::process::id()));
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let ramp: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
    let path = s.dir.join("Audio Files").join("ramp.wav");
    let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![ramp]));
    let id = s.project.alloc_id();
    let content = RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 1000, fade_out: 2000, fade_shape: Default::default() };
    s.project.track_mut(t).unwrap().regions.push(Region {
        id,
        name: "Ramp".into(),
        start: 10_000,
        length: frames as i64,
        muted: false,
        color: None,
        content,
        loop_end: None,
        takes: None,
        params: Default::default(),
        pattern: None,
    });
    s.push_regions(t);
    (s, t, id)
}

fn regions(s: &Session, t: Id) -> Vec<Region> {
    let mut v = s.project.track(t).unwrap().regions.clone();
    v.sort_by_key(|r| r.start);
    v
}

fn audio(r: &Region) -> (i64, i64, i64, f32) {
    match r.content {
        RegionContent::Audio { offset, fade_in, fade_out, gain_db, .. } => (offset, fade_in, fade_out, gain_db),
        _ => panic!("not audio"),
    }
}

#[test]
fn split_at_many_positions_keeps_order_and_outer_fades() {
    let (mut s, t, id) = session(48_000);
    // Positions outside the region are ignored; duplicates count once.
    let pieces = s.split_regions_at(&[id, id], &[30_000, 20_000, 5_000, 20_000, 70_000]);
    let rs = regions(&s, t);
    assert_eq!(pieces.len(), 3);
    assert_eq!(rs.iter().map(|r| (r.start, r.length)).collect::<Vec<_>>(), vec![(10_000, 10_000), (20_000, 10_000), (30_000, 28_000)]);
    assert_eq!(rs[0].id, id, "the first piece keeps the id");
    assert_eq!(rs.iter().map(|r| audio(r).0).collect::<Vec<_>>(), vec![0, 10_000, 20_000]);
    assert_eq!(rs.iter().map(|r| (audio(r).1, audio(r).2)).collect::<Vec<_>>(), vec![(1000, 0), (0, 0), (0, 2000)]);
    // Nothing to cut: the region comes back unchanged.
    assert_eq!(s.split_regions_at(&[id], &[10_000, 1]), vec![id]);
}

#[test]
fn silence_and_delete_cut_out_the_range() {
    let (mut s, t, id) = session(48_000);
    let inside = s.edit_range(&[id], 20_000, 30_000, RangeEdit::Silence).unwrap();
    let rs = regions(&s, t);
    assert_eq!(rs.len(), 3);
    assert_eq!(inside, vec![rs[1].id]);
    assert!(rs[1].muted && !rs[0].muted && !rs[2].muted);
    assert_eq!((rs[1].start, rs[1].length), (20_000, 10_000));

    let (mut s, t, id) = session(48_000);
    assert!(s.edit_range(&[id], 30_000, 20_000, RangeEdit::Delete).unwrap().is_empty());
    let rs = regions(&s, t);
    assert_eq!(rs.iter().map(|r| (r.start, r.start + r.length)).collect::<Vec<_>>(), vec![(10_000, 20_000), (30_000, 58_000)]);
    assert_eq!(audio(&rs[1]).0, 20_000, "the right part still plays the same audio");
}

#[test]
fn trim_and_split_at_selection() {
    let (mut s, t, id) = session(48_000);
    assert_eq!(s.edit_range(&[id], 15_000, 25_000, RangeEdit::Trim).unwrap(), vec![id]);
    let rs = regions(&s, t);
    assert_eq!((rs.len(), rs[0].start, rs[0].length, audio(&rs[0]).0), (1, 15_000, 10_000, 5_000));
    assert_eq!((audio(&rs[0]).1, audio(&rs[0]).2), (0, 0), "trimmed-off ends lose their fades");

    let (mut s, t, id) = session(48_000);
    // A range over the start: only one cut falls inside.
    let inside = s.edit_range(&[id], 0, 12_000, RangeEdit::Split).unwrap();
    let rs = regions(&s, t);
    assert_eq!(rs.len(), 2);
    assert_eq!(inside, vec![id]);
    assert_eq!((rs[0].start, rs[0].length), (10_000, 2_000));
}

#[test]
fn fades_to_selection() {
    let (mut s, t, id) = session(48_000);
    // From the region start: just a longer fade-in, no split.
    s.edit_range(&[id], 10_000, 16_000, RangeEdit::FadeIn).unwrap();
    let rs = regions(&s, t);
    assert_eq!((rs.len(), audio(&rs[0]).1), (1, 6_000));
    // From inside: split at the start and fade the rest in over the range.
    let faded = s.edit_range(&[id], 30_000, 34_000, RangeEdit::FadeIn).unwrap();
    let rs = regions(&s, t);
    assert_eq!(rs.len(), 2);
    assert_eq!(faded, vec![rs[1].id]);
    assert_eq!((rs[1].start, audio(&rs[1]).1, audio(&rs[1]).2), (30_000, 4_000, 2_000));
    // Fade out over a range in the middle of the last piece.
    let faded = s.edit_range(&[rs[1].id], 40_000, 50_000, RangeEdit::FadeOut).unwrap();
    let rs = regions(&s, t);
    assert_eq!(rs.len(), 3);
    assert_eq!(faded, vec![rs[1].id]);
    assert_eq!((rs[1].start + rs[1].length, audio(&rs[1]).2, audio(&rs[1]).1), (50_000, 10_000, 4_000));
    assert_eq!(audio(&rs[2]).2, 2_000, "the original fade-out stays on the last piece");
}

#[test]
fn normalize_selection_uses_the_peak_inside() {
    let (mut s, t, id) = session(48_000);
    // The ramp peaks at the range's end: (30_000 - 10_000) / 48_000.
    let inside = s.edit_range(&[id], 20_000, 30_000, RangeEdit::Normalize).unwrap();
    let rs = regions(&s, t);
    let r = rs.iter().find(|r| r.id == inside[0]).unwrap();
    let want = -20.0 * (19_999.0f32 / 48_000.0).log10();
    assert!((audio(r).3 - want).abs() < 0.01, "{} vs {want}", audio(r).3);
    assert_eq!(audio(&rs[0]).3, 0.0);
}

#[test]
fn reverse_selection_writes_only_the_inside() {
    let (mut s, t, id) = session(4_800);
    let inside = s.edit_range(&[id], 11_000, 12_000, RangeEdit::Reverse).unwrap();
    let rs = regions(&s, t);
    assert_eq!(rs.len(), 3);
    let RegionContent::Audio { clip, offset, .. } = rs[1].content else { panic!() };
    assert_eq!((rs[1].id, offset), (inside[0], 0));
    let c = s.clip_audio(clip).unwrap();
    assert_eq!(c.frames(), 1_000);
    // Reversed: starts with the ramp value at the range's end.
    assert!((c.channels[0][0] - 1_999.0 / 4_800.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(&s.dir);
}

#[test]
fn ranges_outside_every_region_change_nothing() {
    let (mut s, t, id) = session(48_000);
    assert!(s.edit_range(&[id], 0, 5_000, RangeEdit::Delete).unwrap().is_empty());
    assert!(s.edit_range(&[id], 20_000, 20_000, RangeEdit::Silence).unwrap().is_empty());
    assert_eq!(regions(&s, t).len(), 1);
}
