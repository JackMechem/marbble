//! Track automation: the lanes as the engine plays them, evaluating a lane at a position, and
//! the curve editing behind automation writing (splicing a recorded pass into a lane and
//! thinning out redundant points). Evaluation is allocation-free for the audio thread.

use std::sync::Arc;

use crate::model::{AutoLane, AutoParam, AutoPoint, Id};

/// What a lane drives, without the display name, so the audio thread can compare it cheaply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTarget {
    Volume,
    Pan,
    Mute,
    Send(u8),
    Plugin { uid: Id, param: u32 },
}

impl From<&AutoParam> for AutoTarget {
    fn from(p: &AutoParam) -> AutoTarget {
        match p {
            AutoParam::Volume => AutoTarget::Volume,
            AutoParam::Pan => AutoTarget::Pan,
            AutoParam::Mute => AutoTarget::Mute,
            AutoParam::Send(s) => AutoTarget::Send(*s),
            AutoParam::Plugin { uid, param, .. } => AutoTarget::Plugin { uid: *uid, param: *param },
        }
    }
}

/// A lane as the engine plays it. Shipped to the audio thread inside an `Arc` and never
/// changed there; an edit sends a whole new list.
pub struct LaneDsp {
    pub target: AutoTarget,
    pub points: Vec<AutoPoint>,
}

/// The engine's copy of a track's lanes (empty lanes are left out).
pub fn lanes_dsp(lanes: &[AutoLane]) -> Arc<Vec<LaneDsp>> {
    Arc::new(
        lanes
            .iter()
            .filter(|l| !l.points.is_empty())
            .map(|l| {
                let mut points = l.points.clone();
                points.sort_by_key(|p| p.pos);
                LaneDsp { target: AutoTarget::from(&l.param), points }
            })
            .collect(),
    )
}

/// The lane's value at `pos`: lines between points (straight, or bent by the earlier point's
/// `curve`), the first point's value before it and the last point's value after it. Two
/// points at the same position make a step (the later one applies from there on). None for
/// an empty lane.
pub fn value_at(points: &[AutoPoint], pos: i64) -> Option<f32> {
    let first = points.first()?;
    if pos < first.pos {
        return Some(first.value);
    }
    let i = points.partition_point(|p| p.pos <= pos);
    let Some(b) = points.get(i) else { return points.last().map(|p| p.value) };
    let a = points[i - 1];
    // a.pos <= pos < b.pos, so the span is never empty.
    let t = (pos - a.pos) as f64 / (b.pos - a.pos) as f64;
    // A straight segment takes exactly the arithmetic it always has, so projects without
    // curves play bit for bit as before.
    let t = if a.curve == 0.0 { t as f32 } else { shape(t as f32, a.curve) };
    Some(a.value + (b.value - a.value) * t)
}

/// The steepest a curve gets: at `curve` ±1 the segment follows `t^(1/8)` or `t^8`.
const CURVE_RANGE: f32 = 3.0;

/// How far along a curved segment's change the value is at `t` (0..1 of the way in time):
/// a power curve, `t^(2^(-3·curve))`. It starts at exactly 0 and ends at exactly 1, rises
/// steadily in between, and is the straight line at `curve` 0. Positive curves make the
/// change early (a rising segment bulges upwards), negative ones late.
pub fn shape(t: f32, curve: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if curve == 0.0 {
        return t;
    }
    t.powf((-CURVE_RANGE * curve.clamp(-1.0, 1.0)).exp2())
}

/// How far a written value may stray from the thinned curve, in the parameter's units.
pub fn tolerance(param: &AutoParam) -> f32 {
    match param {
        AutoParam::Volume | AutoParam::Send(_) => 0.05,
        AutoParam::Pan => 0.004,
        AutoParam::Mute => 0.0,
        AutoParam::Plugin { .. } => 0.002,
    }
}

/// Drops points the curve doesn't need: every removed point lies within `tol` of the line the
/// remaining points draw (Ramer-Douglas-Peucker on the vertical error).
pub fn thin(points: &[AutoPoint], tol: f32) -> Vec<AutoPoint> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut spans = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = spans.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (points[a], points[b]);
        let mut worst = (0.0f32, a);
        for (k, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let line = if pb.pos == pa.pos {
                // A vertical step: anything in between is a third value.
                if (p.value - pa.value).abs() < (p.value - pb.value).abs() {
                    pa.value
                } else {
                    pb.value
                }
            } else {
                pa.value + (pb.value - pa.value) * ((p.pos - pa.pos) as f64 / (pb.pos - pa.pos) as f64) as f32
            };
            let err = (p.value - line).abs();
            if err > worst.0 {
                worst = (err, k);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            spans.push((a, worst.1));
            spans.push((worst.1, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Puts a recorded pass into a lane, replacing the lane's points from `start` to `end`.
///
/// The old curve carries on around the pass: it holds up to just before `start`, and when
/// `return_to_curve` is set (Touch), or the lane has points after the pass, it comes back
/// `glide` samples after `end`. Otherwise the last written value holds (Latch, Write). An
/// empty lane counts as holding `before`, the value the parameter had before writing. The
/// written stretch is thinned to within `tol`.
#[allow(clippy::too_many_arguments)]
pub fn splice(lane: &[AutoPoint], recorded: &[AutoPoint], start: i64, end: i64, before: f32, return_to_curve: bool, glide: i64, tol: f32) -> Vec<AutoPoint> {
    if recorded.is_empty() {
        return lane.to_vec();
    }
    let base: Vec<AutoPoint> = if lane.is_empty() { vec![AutoPoint { pos: 0, value: before, curve: 0.0 }] } else { lane.to_vec() };
    let back = end + glide.max(1);
    let returns = return_to_curve || lane.iter().any(|p| p.pos > end);
    let mut out: Vec<AutoPoint> = base.iter().filter(|p| p.pos < start).copied().collect();
    let written_from = out.len();
    if start > 0 {
        let v = value_at(&base, start - 1).unwrap_or(before);
        if out.last().map(|p| p.pos < start - 1 || p.value != v).unwrap_or(true) {
            out.push(AutoPoint { pos: start - 1, value: v, curve: 0.0 });
        }
    }
    out.extend(recorded.iter().filter(|p| p.pos >= start && p.pos <= end));
    if returns {
        let v = value_at(&base, back).unwrap_or(before);
        out.push(AutoPoint { pos: back, value: v, curve: 0.0 });
    }
    let written = thin(&out[written_from..], tol);
    out.truncate(written_from);
    out.extend(written);
    if returns {
        out.extend(base.iter().filter(|p| p.pos > back));
    }
    // An empty lane that ends up holding one value everywhere needs a single point.
    if lane.is_empty() && out.iter().all(|p| p.value == before) {
        out.truncate(1);
    }
    out
}

/// Gives plug-in lanes the new instance ids of copied plug-ins (`map` is old -> new).
pub fn remap_plugins(lanes: &mut [AutoLane], map: &[(Id, Id)]) {
    for l in lanes {
        if let AutoParam::Plugin { uid, .. } = &mut l.param {
            if let Some((_, new)) = map.iter().find(|(old, _)| old == uid) {
                *uid = *new;
            }
        }
    }
}

/// Takes a plug-in instance's lanes (`old`) out of a track's list, re-keyed to `new`: the
/// lanes of a plug-in moved to another track, as that track's instance. The rest keep their
/// order.
pub fn take_plugin_lanes(lanes: &mut Vec<AutoLane>, old: Id, new: Id) -> Vec<AutoLane> {
    let (mut taken, kept): (Vec<AutoLane>, Vec<AutoLane>) =
        std::mem::take(lanes).into_iter().partition(|l| matches!(l.param, AutoParam::Plugin { uid, .. } if uid == old));
    *lanes = kept;
    remap_plugins(&mut taken, &[(old, new)]);
    taken
}

#[cfg(test)]
mod tests;
