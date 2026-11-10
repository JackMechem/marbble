//! Crossfades on audio regions: types, the xfade drag mode, crossfade time, folders and loops, joining.

use super::*;
use crate::audio_io::AudioConfig;

/// A session on the null device with an audio track and a 10 s ramp clip.
fn session() -> (Session, Id, Id) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-crossfades-{}", std::process::id()));
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let frames = 480_000;
    let ramp: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
    let path = s.dir.join("Audio Files").join("ramp.wav");
    let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![ramp]));
    (s, t, clip)
}

fn add(s: &mut Session, t: Id, clip: Id, start: i64, length: i64, offset: i64) -> Id {
    let id = s.project.alloc_id();
    let content = RegionContent::Audio { clip, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() };
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

fn fade_out(s: &Session, id: Id) -> (i64, FadeOutType) {
    match &s.region(id).unwrap().content {
        RegionContent::Audio { fade_out, fade_shape, .. } => (*fade_out, fade_shape.out_type),
        _ => unreachable!(),
    }
}

#[test]
fn choosing_a_crossfade_type_gives_a_default_length() {
    let (mut s, t, clip) = session();
    let a = add(&mut s, t, clip, 48_000, 48_000, 10_000);
    let b = add(&mut s, t, clip, 96_000, 48_000, 100_000);
    let lone = add(&mut s, t, clip, 300_000, 48_000, 0);
    s.set_region_fade_shape(&[a, lone], None, Some(0.2), None, Some(FadeOutType::EqP));
    assert_eq!(fade_out(&s, a), (960, FadeOutType::EqP), "20 ms at 48 kHz");
    // Nothing follows: the type is kept but no fade appears.
    assert_eq!(fade_out(&s, lone), (0, FadeOutType::EqP));
    let x = s.crossfades();
    assert_eq!(x.len(), 1);
    assert_eq!((x[0].out_region, x[0].in_region, x[0].fade_out), (a, b, (96_000 - 480, 960)));
    // Remove it, then crossfade from the menu.
    s.crossfade_regions(&[a, b], 0, None);
    assert_eq!(fade_out(&s, a), (0, FadeOutType::Out));
    assert_eq!(fade_out(&s, b), (0, FadeOutType::Out), "b has nothing to crossfade into and is left alone");
    s.crossfade_regions(&[a], 4800, Some(FadeOutType::XS));
    assert_eq!(fade_out(&s, a), (4800, FadeOutType::XS));
}

#[test]
fn xfade_drag_mode_crossfades_overlaps() {
    let (mut s, t, clip) = session();
    let a = add(&mut s, t, clip, 48_000, 48_000, 10_000);
    let b = add(&mut s, t, clip, 200_000, 48_000, 100_000);
    // Overlap mode: moving over changes nothing.
    s.move_region(b, 90_000, None);
    assert_eq!(fade_out(&s, a), (0, FadeOutType::Out));
    s.set_drag_mode(DragMode::XFade);
    s.move_region(b, 86_000, None);
    assert_eq!(fade_out(&s, a), (10_000, FadeOutType::X), "the overlap");
    // Trimming the incoming region shortens the overlap.
    s.resize_region(b, 91_000, 43_000);
    assert_eq!(fade_out(&s, a), (5_000, FadeOutType::X));
    // Nudging apart removes it.
    s.nudge_regions(&[b], 10_000);
    assert_eq!(fade_out(&s, a), (0, FadeOutType::Out));
    // Pasting a copy over a's end crossfades too.
    let r = s.region(a).unwrap().clone();
    let made = s.paste_regions(&[(t, r)], 40_000);
    assert_eq!(fade_out(&s, a), (8_000, FadeOutType::X));
    assert_eq!(made.len(), 1);
}

#[test]
fn crossfade_time_sets_touching_xfades() {
    let (mut s, t, clip) = session();
    let a = add(&mut s, t, clip, 48_000, 48_000, 10_000);
    let b = add(&mut s, t, clip, 200_000, 48_000, 100_000);
    s.set_drag_mode(DragMode::XFade);
    s.prefs.editing.crossfade_ms = 50.0;
    s.prefs.editing.crossfade_curve = 0.0;
    // Butted against a: the Crossfade Time, an X with the Crossfade Curve.
    s.move_region(b, 96_000, None);
    assert_eq!(fade_out(&s, a), (2_400, FadeOutType::X));
    assert!(matches!(&s.region(a).unwrap().content, RegionContent::Audio { fade_shape, .. } if fade_shape.out_curve == 0.0));
    // Moving away and back with another time: the new one.
    s.nudge_regions(&[b], 10_000);
    s.prefs.editing.crossfade_ms = 20.0;
    s.move_region(b, 96_000, None);
    assert_eq!(fade_out(&s, a).0, 960);
    assert_eq!(s.crossfade_frames(), 960);
}

#[test]
fn take_folders_and_loops_crossfade_with_neighbours() {
    let (mut s, t, clip) = session();
    let a = add(&mut s, t, clip, 48_000, 24_000, 10_000);
    let b = add(&mut s, t, clip, 72_000, 24_000, 100_000);
    let c = add(&mut s, t, clip, 200_000, 48_000, 200_000);
    let folder = s.pack_take_folder(&[a, b]).unwrap();
    // The folder is followed by c once it's moved against it.
    s.move_region(c, 96_000, None);
    s.crossfade_regions(&[folder], 4_800, Some(FadeOutType::X));
    let x = s.crossfades();
    assert_eq!((x.len(), x[0].out_region, x[0].in_region), (1, folder, c));
    // A loop crossfades at its loop end.
    let d = add(&mut s, t, clip, 300_000, 10_000, 300_000);
    s.project.track_mut(t).unwrap().regions.iter_mut().find(|r| r.id == d).unwrap().loop_end = Some(340_000);
    let e = add(&mut s, t, clip, 340_000, 10_000, 400_000);
    s.crossfade_regions(&[d], 960, None);
    assert!(s.crossfades().iter().any(|x| x.out_region == d && x.in_region == e && x.fade_out == (339_520, 960)));
    s.set_loop_crossfade(&[d], true);
    assert!(matches!(&s.region(d).unwrap().content, RegionContent::Audio { fade_shape, .. } if fade_shape.loop_xfade));
}

#[test]
fn join_keeps_the_crossfade() {
    let (mut s, t, clip) = session();
    // Two pieces of the ramp, the second continuing where the first stops: a linear X
    // crossfade between them is seamless.
    let a = add(&mut s, t, clip, 0, 48_000, 0);
    let b = add(&mut s, t, clip, 48_000, 48_000, 48_000);
    s.crossfade_regions(&[a], 4_800, Some(FadeOutType::X));
    s.set_region_fade_shape(&[a], None, Some(0.0), None, None);
    let made = s.join_regions(&[a, b]).unwrap();
    let r = s.region(made[0]).unwrap().clone();
    let RegionContent::Audio { clip: joined, .. } = r.content else { panic!() };
    let c = s.clip_audio(joined).unwrap();
    for i in (0..96_000).step_by(997) {
        let want = i as f32 / 480_000.0;
        assert!((c.channels[0][i] - want).abs() < 1e-4, "frame {i}: {} vs {want}", c.channels[0][i]);
    }
}
