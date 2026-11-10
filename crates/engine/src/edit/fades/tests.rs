//! Fade curves and crossfades: shapes, gains, zones, folders and loops, old projects.

use super::*;

#[test]
fn curves_run_from_silence_to_full_and_rise() {
    for c in [-1.0, -0.7, -0.5, -0.2, 0.0, 0.2, 0.5, 0.8, 1.0] {
        assert_eq!(curve(0.0, c), 0.0, "c = {c}");
        assert!((curve(1.0, c) - 1.0).abs() < 1e-6, "c = {c}");
        let mut prev = 0.0;
        for k in 1..=1000 {
            let g = curve(k as f32 / 1000.0, c);
            assert!(g >= prev - 1e-6, "c = {c} falls at {k}");
            prev = g;
        }
    }
    // Out of range clamps.
    assert_eq!(curve(-0.5, 0.3), 0.0);
    assert_eq!(curve(1.5, 0.3), 1.0);
}

#[test]
fn curve_shapes() {
    for k in 0..=100 {
        let x = k as f32 / 100.0;
        assert_eq!(curve(x, 0.0), x, "0 is linear");
        // The default reproduces the fixed quarter sine fades had before, exactly.
        assert_eq!(curve(x, DEFAULT_CURVE), (x * FRAC_PI_2).sin());
        // Negative curves mirror positive ones through the centre.
        assert!((curve(x, -0.6) - (1.0 - curve(1.0 - x, 0.6))).abs() < 1e-6);
    }
    // Positive bows out, negative in, more as the value grows.
    assert!(curve(0.25, 1.0) > curve(0.25, 0.8) && curve(0.25, 0.8) > curve(0.25, 0.5));
    assert!(curve(0.25, 0.5) > curve(0.25, 0.2) && curve(0.25, 0.2) > 0.25);
    assert!(curve(0.25, -0.5) < 0.25);
    // Continuous across the joins of the formula.
    assert!((curve(0.3, 0.5001) - curve(0.3, 0.5)).abs() < 1e-3);
    assert!((curve(0.3, 1e-3) - 0.3).abs() < 1e-3);
}

#[test]
fn crossfade_gains_sum() {
    let mk = |kind| Crossfade { out_region: 1, in_region: 2, kind, curve: 0.0, fade_out: (0, 100), fade_in: (0, 100) };
    for k in 0..=200 {
        let p = k as f32 / 200.0;
        // The outgoing side is at 1 - p of the way to full level, the incoming at p.
        let (o, i) = mk(FadeOutType::EqP).laws();
        let (go, gi) = (o.gain(1.0 - p), i.gain(p));
        assert!((go * go + gi * gi - 1.0).abs() < 1e-5, "equal power at {p}");
        for c in [0.0, 0.5, -0.8] {
            let x = Crossfade { curve: c, ..mk(FadeOutType::X) };
            let (o, i) = x.laws();
            assert!((o.gain(1.0 - p) + i.gain(p) - 1.0).abs() < 1e-6, "X sums to unity at {p}, curve {c}");
        }
        let (o, i) = mk(FadeOutType::XS).laws();
        assert!((o.gain(1.0 - p) + i.gain(p) - 1.0).abs() < 1e-6, "X S sums to unity at {p}");
    }
    // Linear X is a straight line; X S is flat at both ends.
    let (_, i) = mk(FadeOutType::X).laws();
    assert!((i.gain(0.3) - 0.3).abs() < 1e-6);
    assert!(s_curve(0.05) < 0.01 && s_curve(0.95) > 0.99);
}

#[test]
fn speed_table_integrates_the_lost_time() {
    // Linear speed ramp: D(x) = ∫ₓ¹ (1 - u) du = (1 - x)² / 2.
    let t = speed_table(0.0);
    let law = FadeLaw::Speed(0.0, Arc::new(t));
    for k in 0..=10 {
        let x = k as f32 / 10.0;
        assert!((law.lag(x) - (1.0 - x).powi(2) / 2.0).abs() < 1e-4, "at {x}");
    }
    assert_eq!(law.lag(1.0), 0.0);
    assert_eq!(FadeLaw::Curve(0.0).lag(0.5), 0.0);
}

fn region(id: Id, start: i64, length: i64, offset: i64, fade_out: i64, out_type: FadeOutType) -> Region {
    Region {
        id,
        name: format!("R{id}"),
        start,
        length,
        muted: false,
        color: None,
        content: RegionContent::Audio { clip: 1, offset, gain_db: 0.0, fade_in: 0, fade_out, fade_shape: FadeShape { out_type, ..FadeShape::default() } },
        loop_end: None,
        takes: None,
        params: Default::default(),
        pattern: None,
    }
}

#[test]
fn crossfade_zones() {
    let frames = |_| Some(100_000);
    // Adjacent, with plenty of audio either side: centred on the junction.
    let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::EqP), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
    let x = crossfades(&rs, frames);
    assert_eq!(x.len(), 1);
    assert_eq!((x[0].out_region, x[0].in_region, x[0].fade_out, x[0].fade_in), (1, 2, (1900, 200), (1900, 200)));
    // Overlapping: centred on the overlap.
    let rs = vec![region(1, 1000, 1000, 5000, 400, FadeOutType::X), region(2, 1800, 1000, 8000, 0, FadeOutType::Out)];
    assert_eq!(crossfades(&rs, frames)[0].fade_out, (1700, 400));
    // The incoming region's file starts right at it: the crossfade slides after the junction.
    let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::X), region(2, 2000, 1000, 0, 0, FadeOutType::Out)];
    assert_eq!(crossfades(&rs, frames)[0].fade_in, (2000, 200));
    // Neither file has audio to spare: butt fades of half the length each.
    let rs = vec![region(1, 1000, 1000, 99_000, 200, FadeOutType::X), region(2, 2000, 1000, 0, 0, FadeOutType::Out)];
    let x = crossfades(&rs, frames);
    assert_eq!((x[0].fade_out, x[0].fade_in), ((1900, 100), (2000, 100)));
    // A gap, an Out type, or no fade length: no crossfade.
    let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::X), region(2, 2001, 1000, 8000, 0, FadeOutType::Out)];
    assert!(crossfades(&rs, frames).is_empty());
    let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::Out), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
    assert!(crossfades(&rs, frames).is_empty());
    let rs = vec![region(1, 1000, 1000, 5000, 0, FadeOutType::X), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
    assert!(crossfades(&rs, frames).is_empty());
    // A region inside another isn't the following one.
    let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::X), region(2, 1200, 300, 8000, 0, FadeOutType::Out)];
    assert!(crossfades(&rs, frames).is_empty());
}

#[test]
fn folders_and_loops_crossfade() {
    use crate::model::{CompSegment, Take, TakeFolder};
    let frames = |_| Some(100_000);
    // A take folder whose comp has audio either side of its ends, then a plain region.
    let mut folder = region(1, 1000, 1000, 500, 200, FadeOutType::EqP);
    let take =
        Take { id: 7, name: "Take 1".into(), clip: 3, offset: 0, start: 0, end: 5000, gain_db: 0.0, notes: Vec::new(), cc: Vec::new(), ..Default::default() };
    folder.takes = Some(TakeFolder { takes: vec![take], comp: vec![CompSegment { start: 0, end: 5000, take: 7 }], ..TakeFolder::new() });
    let rs = vec![folder.clone(), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
    assert_eq!(crossfades(&rs, frames)[0].fade_out, (1900, 200));
    // The comp ends right at the folder's end: the crossfade slides before the junction,
    // the next region playing from earlier in its file.
    folder.takes.as_mut().unwrap().comp[0].end = 1500;
    let rs = vec![folder, region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
    assert_eq!(crossfades(&rs, frames)[0].fade_out, (1800, 200));
    // A loop crossfades at its loop end, into a region starting there.
    let mut looped = region(1, 1000, 1000, 5000, 200, FadeOutType::X);
    looped.loop_end = Some(3500);
    let rs = vec![looped.clone(), region(2, 3500, 1000, 8000, 0, FadeOutType::Out)];
    let x = crossfades(&rs, frames);
    assert_eq!((x[0].in_region, x[0].fade_out), (2, (3400, 200)));
    assert!(loop_seams(&looped, frames, 100).is_empty(), "seams only when asked for");
    if let RegionContent::Audio { fade_shape, .. } = &mut looped.content {
        fade_shape.loop_xfade = true;
    }
    // Two seams (at 2000 and 3000), each centred.
    assert_eq!(loop_seams(&looped, frames, 100), vec![((1950, 100), (1950, 100)), ((2950, 100), (2950, 100))]);
    // At the start of its file: no audio before, so the seam slides after it.
    let mut at_zero = looped.clone();
    if let RegionContent::Audio { offset, .. } = &mut at_zero.content {
        *offset = 0;
    }
    assert_eq!(loop_seams(&at_zero, frames, 100)[0].1, (2000, 100));
}

#[test]
fn reversed_shape_swaps_ends() {
    let s = FadeShape { in_curve: 0.2, out_curve: -0.4, in_type: FadeInType::SpeedUp, out_type: FadeOutType::EqP, loop_xfade: false };
    let r = s.reversed();
    assert_eq!((r.in_curve, r.out_curve, r.in_type, r.out_type), (-0.4, 0.2, FadeInType::Fade, FadeOutType::SlowDown));
}

#[test]
fn old_projects_load_with_the_default_shape() {
    let c: RegionContent = serde_json::from_str(r#"{"type":"audio","clip":3,"offset":0,"gain_db":0,"fade_in":10,"fade_out":20}"#).unwrap();
    let RegionContent::Audio { fade_shape, .. } = c else { panic!() };
    assert!(fade_shape.is_default());
    // And an unchanged shape isn't written out.
    let back = serde_json::to_string(&RegionContent::Audio { clip: 3, offset: 0, gain_db: 0.0, fade_in: 10, fade_out: 20, fade_shape }).unwrap();
    assert!(!back.contains("fade_shape"));
    let s: FadeShape = serde_json::from_str(r#"{"out_type":"xs"}"#).unwrap();
    assert_eq!((s.out_type, s.in_curve), (FadeOutType::XS, DEFAULT_CURVE));
}
