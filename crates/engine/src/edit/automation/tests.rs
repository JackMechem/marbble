//! Automation lanes: evaluating them, curves, thinning, and splicing a written pass.

use super::*;

fn pts(v: &[(i64, f32)]) -> Vec<AutoPoint> {
    v.iter().map(|&(pos, value)| AutoPoint { pos, value, curve: 0.0 }).collect()
}

#[test]
fn empty_lane_has_no_value() {
    assert_eq!(value_at(&[], 100), None);
}

#[test]
fn holds_outside_the_points() {
    let p = pts(&[(100, -6.0), (200, 0.0)]);
    assert_eq!(value_at(&p, 0), Some(-6.0));
    assert_eq!(value_at(&p, 100), Some(-6.0));
    assert_eq!(value_at(&p, 200), Some(0.0));
    assert_eq!(value_at(&p, 10_000), Some(0.0));
}

#[test]
fn interpolates_linearly() {
    let p = pts(&[(0, 0.0), (100, 1.0), (300, -1.0)]);
    assert!((value_at(&p, 50).unwrap() - 0.5).abs() < 1e-6);
    assert!((value_at(&p, 200).unwrap() - 0.0).abs() < 1e-6);
    assert!((value_at(&p, 250).unwrap() + 0.5).abs() < 1e-6);
}

#[test]
fn straight_segments_match_the_plain_line_exactly() {
    // The formula as it was before curves: a project without them must play bit for bit.
    let old = |p: &[AutoPoint], pos: i64| {
        let i = p.partition_point(|q| q.pos <= pos);
        let (a, b) = (p[i - 1], p[i]);
        let t = (pos - a.pos) as f64 / (b.pos - a.pos) as f64;
        a.value + (b.value - a.value) * t as f32
    };
    let p = pts(&[(0, -96.0), (48_000, 6.0), (100_001, -13.7)]);
    for pos in (0..100_001).step_by(37) {
        assert_eq!(value_at(&p, pos).unwrap().to_bits(), old(&p, pos).to_bits(), "at {pos}");
    }
}

fn curved(a: (i64, f32, f32), b: (i64, f32)) -> Vec<AutoPoint> {
    vec![AutoPoint { pos: a.0, value: a.1, curve: a.2 }, AutoPoint { pos: b.0, value: b.1, curve: 0.0 }]
}

#[test]
fn curves_meet_their_points_exactly() {
    for c in [-1.0, -0.5, -0.01, 0.3, 1.0] {
        let p = curved((100, -40.0, c), (1100, 3.0));
        assert_eq!(value_at(&p, 100), Some(-40.0));
        assert_eq!(value_at(&p, 1100), Some(3.0));
        assert_eq!(value_at(&p, 99), Some(-40.0));
        assert_eq!(value_at(&p, 5000), Some(3.0));
        assert_eq!(shape(0.0, c), 0.0);
        assert_eq!(shape(1.0, c), 1.0);
    }
}

#[test]
fn curves_are_monotonic() {
    for c in [-1.0, -0.7, -0.2, 0.2, 0.7, 1.0] {
        for (from, to) in [(-40.0, 3.0), (0.8, 0.1)] {
            let p = curved((0, from, c), (1000, to));
            let mut last = from;
            for pos in 0..=1000 {
                let v = value_at(&p, pos).unwrap();
                assert!(if to > from { v >= last } else { v <= last }, "curve {c} at {pos}: {v} after {last}");
                assert!(v >= from.min(to) && v <= from.max(to));
                last = v;
            }
        }
    }
}

#[test]
fn curve_sign_moves_the_change_earlier_or_later() {
    let mid = |c: f32| value_at(&curved((0, 0.0, c), (1000, 1.0)), 500).unwrap();
    assert!((mid(0.0) - 0.5).abs() < 1e-6);
    assert!(mid(0.5) > 0.7 && mid(1.0) > mid(0.5));
    assert!(mid(-0.5) < 0.3 && mid(-1.0) < mid(-0.5));
    // Falling segments bend the same way in time: the change comes early.
    let fall = value_at(&curved((0, 1.0, 0.5), (1000, 0.0)), 500).unwrap();
    assert!(fall < 0.3);
}

#[test]
fn a_curve_only_bends_its_own_segment() {
    let p = vec![AutoPoint { pos: 0, value: 0.0, curve: 0.8 }, AutoPoint { pos: 100, value: 1.0, curve: 0.0 }, AutoPoint { pos: 200, value: 0.0, curve: 0.0 }];
    assert!((value_at(&p, 150).unwrap() - 0.5).abs() < 1e-6);
    assert!(value_at(&p, 50).unwrap() > 0.8);
}

#[test]
fn curves_are_saved_only_when_set() {
    let j = serde_json::to_string(&AutoPoint { pos: 5, value: 1.0, curve: 0.0 }).unwrap();
    assert_eq!(j, r#"{"pos":5,"value":1.0}"#);
    let back: AutoPoint = serde_json::from_str(r#"{"pos":5,"value":1.0}"#).unwrap();
    assert_eq!(back.curve, 0.0);
    let c: AutoPoint = serde_json::from_str(r#"{"pos":5,"value":1.0,"curve":-0.25}"#).unwrap();
    assert_eq!(c.curve, -0.25);
}

#[test]
fn same_position_is_a_step() {
    let p = pts(&[(0, 0.0), (100, 0.0), (100, 1.0), (200, 1.0)]);
    assert_eq!(value_at(&p, 99), Some(0.0));
    assert_eq!(value_at(&p, 100), Some(1.0));
    assert_eq!(value_at(&p, 150), Some(1.0));
}

#[test]
fn single_point_holds_everywhere() {
    let p = pts(&[(500, 0.25)]);
    assert_eq!(value_at(&p, 0), Some(0.25));
    assert_eq!(value_at(&p, 9999), Some(0.25));
}

#[test]
fn thinning_keeps_corners_only() {
    // A ramp up, a hold, a ramp down, sampled densely.
    let mut v = Vec::new();
    for i in 0..=10 {
        v.push((i * 10, i as f32));
    }
    for i in 11..=20 {
        v.push((i * 10, 10.0));
    }
    for i in 21..=30 {
        v.push((i * 10, 10.0 - (i - 20) as f32));
    }
    let t = thin(&pts(&v), 0.01);
    assert_eq!(t, pts(&[(0, 0.0), (100, 10.0), (200, 10.0), (300, 0.0)]));
}

#[test]
fn thinning_respects_tolerance() {
    let p = pts(&[(0, 0.0), (10, 0.03), (20, 0.0)]);
    assert_eq!(thin(&p, 0.05).len(), 2);
    assert_eq!(thin(&p, 0.01).len(), 3);
}

#[test]
fn touch_returns_to_the_old_curve() {
    let lane = pts(&[(0, 0.0), (1000, 0.0)]);
    let rec = pts(&[(200, -10.0), (400, -10.0)]);
    let out = splice(&lane, &rec, 200, 400, 0.0, true, 50, 0.0);
    assert_eq!(value_at(&out, 100), Some(0.0));
    assert_eq!(value_at(&out, 300), Some(-10.0));
    assert_eq!(value_at(&out, 450), Some(0.0));
    assert_eq!(value_at(&out, 1000), Some(0.0));
}

#[test]
fn latch_holds_the_last_value() {
    let lane = pts(&[(0, 0.0)]);
    let rec = pts(&[(200, -3.0), (400, -12.0)]);
    let out = splice(&lane, &rec, 200, 400, 0.0, false, 50, 0.0);
    assert_eq!(value_at(&out, 100), Some(0.0));
    assert_eq!(value_at(&out, 5000), Some(-12.0));
}

#[test]
fn latch_keeps_later_points() {
    let lane = pts(&[(0, 0.0), (2000, 6.0)]);
    let rec = pts(&[(200, -3.0), (400, -3.0)]);
    let out = splice(&lane, &rec, 200, 400, 0.0, false, 50, 0.0);
    assert_eq!(value_at(&out, 2000), Some(6.0));
    assert_eq!(value_at(&out, 300), Some(-3.0));
}

#[test]
fn writing_an_empty_lane_keeps_the_value_before() {
    let rec = pts(&[(1000, -6.0), (2000, -6.0)]);
    let out = splice(&[], &rec, 1000, 2000, -2.0, false, 50, 0.0);
    assert_eq!(value_at(&out, 0), Some(-2.0));
    assert_eq!(value_at(&out, 1500), Some(-6.0));
    assert_eq!(value_at(&out, 9000), Some(-6.0));
}

#[test]
fn params_serialize_as_the_ui_expects() {
    let j = |p: &AutoParam| serde_json::to_string(p).unwrap();
    assert_eq!(j(&AutoParam::Volume), r#"{"type":"volume"}"#);
    assert_eq!(j(&AutoParam::Send(2)), r#"{"type":"send","value":2}"#);
    let p = AutoParam::Plugin { uid: 7, param: 42, name: "Cutoff".into() };
    assert_eq!(j(&p), r#"{"type":"plugin","value":{"uid":7,"param":42,"name":"Cutoff"}}"#);
    let back: AutoParam = serde_json::from_str(&j(&p)).unwrap();
    assert!(back.same(&p) && !back.same(&AutoParam::Plugin { uid: 7, param: 43, name: "Cutoff".into() }));
}

#[test]
fn replaces_points_inside_the_pass() {
    let lane = pts(&[(0, 0.0), (300, 5.0), (600, 0.0)]);
    let rec = pts(&[(200, 1.0), (400, 1.0)]);
    let out = splice(&lane, &rec, 200, 400, 0.0, true, 10, 0.0);
    assert!(!out.iter().any(|p| p.pos == 300));
}

#[test]
fn taking_a_plugins_lanes_rekeys_them() {
    let lane = |param: AutoParam, v: f32| AutoLane { param, points: pts(&[(0, v)]) };
    let plug = |uid: Id, param: u32| AutoParam::Plugin { uid, param, name: format!("P{param}") };
    let mut lanes = vec![lane(AutoParam::Volume, -3.0), lane(plug(7, 1), 0.2), lane(plug(8, 1), 0.4), lane(plug(7, 2), 0.6)];
    let taken = take_plugin_lanes(&mut lanes, 7, 30);
    assert_eq!(taken.len(), 2);
    assert!(taken[0].param.same(&plug(30, 1)) && taken[0].points == pts(&[(0, 0.2)]));
    assert!(taken[1].param.same(&plug(30, 2)) && taken[1].points == pts(&[(0, 0.6)]));
    assert!(matches!(&taken[1].param, AutoParam::Plugin { name, .. } if name == "P2"));
    assert_eq!(lanes.len(), 2);
    assert!(lanes[0].param.same(&AutoParam::Volume) && lanes[1].param.same(&plug(8, 1)));
    assert!(take_plugin_lanes(&mut lanes, 7, 31).is_empty());
    assert_eq!(lanes.len(), 2);
}
