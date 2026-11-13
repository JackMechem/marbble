//! Region playback parameters, Logic's Region inspector: non-destructive Quantize with Q-Swing,
//! Transpose, Velocity, Dynamics, Gate Time and Delay.
//!
//! The stored notes never change. What a region plays is worked out here, as pure functions
//! of its notes, its position and the tempo map, when the session builds the playback list
//! (`Session::regions_dsp`), so the audio thread does nothing new. The same functions bake the
//! parameters into the notes for Apply Region Parameters, Join and MIDI file export.
//!
//! The order is Logic's: quantize moves note starts (lengths are kept), gate time changes
//! lengths, delay shifts everything in time, then transpose, dynamics and velocity change the
//! notes themselves.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::model::tempo::TempoMap;
use crate::model::{CcEvent, Note, Region, RegionContent};

/// Ticks per quarter note, as the Region inspector's Delay counts them (Logic's 960).
pub const TICKS_PER_QUARTER: f64 = 960.0;

/// `gate` value for Logic's "Fixed": every note very short (a 1/64 note).
pub const GATE_FIXED: u16 = 0;
/// `gate` value for Logic's "Legato": every note lasts until the next one starts.
pub const GATE_LEGATO: u16 = 201;

/// Quantize values: the grid as a fraction of a whole note. Straight notes, then triplets
/// (1/3 is a half-note triplet, 1/12 an eighth-note triplet, as Logic names them).
pub const QUANTIZE_VALUES: [u16; 13] = [1, 2, 4, 8, 16, 32, 64, 3, 6, 12, 24, 48, 96];

/// Dynamics values (the ratio × 100, 0 = Fixed) and how much each scales a velocity's
/// distance from 64: 3:1 compresses it to a third, 0.5:1 expands it to double.
pub const DYNAMICS: [(u16, f64); 10] =
    [(0, 0.0), (300, 1.0 / 3.0), (200, 0.5), (150, 2.0 / 3.0), (125, 0.8), (100, 1.0), (80, 1.25), (67, 1.5), (50, 2.0), (33, 3.0)];

pub const TRANSPOSE_RANGE: (i16, i16) = (-96, 96);
pub const VELOCITY_RANGE: (i16, i16) = (-99, 127);
/// Delay limits: four quarter notes in ticks, two seconds in ms.
pub const DELAY_TICKS_MAX: f64 = 4.0 * TICKS_PER_QUARTER;
pub const DELAY_MS_MAX: f64 = 2000.0;

/// A region's playback parameters. The defaults change nothing. Audio regions use only the
/// delay.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionParams {
    /// Quantize grid as a fraction of a whole note (16 = 1/16 note, 12 = 1/12 note, an eighth
    /// triplet); 0 = off. See [`QUANTIZE_VALUES`].
    pub quantize: u16,
    /// Q-Swing in percent: where every second grid point sits between its neighbours. 50 is
    /// straight, above 50 delays it, below 50 pulls it earlier. Only used while quantizing.
    pub swing: u8,
    /// Semitones.
    pub transpose: i16,
    /// Added to every velocity; the result is kept within 1–127.
    pub velocity: i16,
    /// Velocity compression or expansion around 64: the ratio × 100 (300 = 3:1, 100 = off,
    /// 50 = 0.5:1), 0 = Fixed (every note at 64). See [`DYNAMICS`].
    pub dynamics: u16,
    /// Gate time: note lengths in percent (10–200, 100 = unchanged), or [`GATE_FIXED`] or
    /// [`GATE_LEGATO`].
    pub gate: u16,
    /// Delay, in ticks ([`TICKS_PER_QUARTER`] to a quarter note) or, with `delay_ms`, in
    /// milliseconds. Negative values play early.
    pub delay: f64,
    pub delay_ms: bool,
}

impl Default for RegionParams {
    fn default() -> Self {
        RegionParams { quantize: 0, swing: 50, transpose: 0, velocity: 0, dynamics: 100, gate: 100, delay: 0.0, delay_ms: false }
    }
}

impl RegionParams {
    pub fn is_default(&self) -> bool {
        *self == RegionParams::default()
    }

    /// Whether what the region plays changes with the tempo map (so a tempo edit has to
    /// rebuild its playback list).
    pub fn follows_tempo(&self) -> bool {
        self.quantize != 0 || self.gate == GATE_FIXED || (self.delay != 0.0 && !self.delay_ms)
    }

    /// The same parameters with every value brought into range (and unknown quantize and
    /// dynamics values to the nearest that exists).
    pub fn clamped(mut self) -> Self {
        if self.quantize != 0 && !QUANTIZE_VALUES.contains(&self.quantize) {
            self.quantize = *QUANTIZE_VALUES.iter().min_by_key(|v| v.abs_diff(self.quantize)).unwrap();
        }
        self.swing = self.swing.min(100);
        self.transpose = self.transpose.clamp(TRANSPOSE_RANGE.0, TRANSPOSE_RANGE.1);
        self.velocity = self.velocity.clamp(VELOCITY_RANGE.0, VELOCITY_RANGE.1);
        if !DYNAMICS.iter().any(|d| d.0 == self.dynamics) {
            self.dynamics = DYNAMICS.iter().min_by_key(|d| d.0.abs_diff(self.dynamics)).unwrap().0;
        }
        if self.gate != GATE_FIXED && self.gate != GATE_LEGATO {
            self.gate = self.gate.clamp(10, 200);
        }
        let max = if self.delay_ms { DELAY_MS_MAX } else { DELAY_TICKS_MAX };
        self.delay = if self.delay.is_finite() { self.delay.clamp(-max, max) } else { 0.0 };
        self
    }

    /// Where an event at timeline sample `at` plays once delayed.
    pub fn delayed(&self, at: f64, map: &TempoMap) -> f64 {
        delayed(self.delay, self.delay_ms, at, map)
    }

    /// The delay in the other unit, measured at timeline sample `at` (for switching units
    /// without changing what plays there).
    pub fn delay_in(&self, ms: bool, at: f64, map: &TempoMap) -> f64 {
        delay_in(self.delay, self.delay_ms, ms, at, map)
    }
}

/// Where an event at timeline sample `at` plays once delayed by `delay` ticks (following the
/// tempo map) or, with `ms`, milliseconds. The Track inspector's Delay uses it too.
pub fn delayed(delay: f64, ms: bool, at: f64, map: &TempoMap) -> f64 {
    if delay == 0.0 {
        at
    } else if ms {
        at + delay * map.sample_rate() / 1000.0
    } else {
        map.sample_at(map.beats_at(at) + delay / TICKS_PER_QUARTER)
    }
}

/// A delay of `delay` (in ms when `from_ms`, else ticks) in the unit `to_ms` asks for, measured
/// at timeline sample `at`, so switching units doesn't change what plays there.
pub fn delay_in(delay: f64, from_ms: bool, to_ms: bool, at: f64, map: &TempoMap) -> f64 {
    if to_ms == from_ms {
        return delay;
    }
    let shift = delayed(delay, from_ms, at, map) - at;
    if to_ms {
        shift * 1000.0 / map.sample_rate()
    } else {
        (map.beats_at(at + shift) - map.beats_at(at)) * TICKS_PER_QUARTER
    }
}

/// Where a note starting at timeline sample `at` quantizes to: the nearest point of a grid of
/// `div` steps per whole note, counted from its bar line (so odd meters line up), with every
/// second point moved by the swing. A note nearer the next bar line goes there.
pub fn quantize_target(at: f64, div: u16, swing: u8, map: &TempoMap) -> f64 {
    if div == 0 {
        return at;
    }
    let grid = 4.0 / div as f64;
    let q = map.beats_at(at);
    let bar = map.bar_at(at);
    let (start, end) = (map.bar_start_q(bar), map.bar_start_q(bar + 1));
    let k = ((q - start) / grid).round();
    let mut target = start + k * grid;
    if k as i64 % 2 == 1 {
        target += grid * (swing.min(100) as f64 - 50.0) / 50.0;
    }
    let target = if target >= end || end - q < (q - target).abs() { end } else { target };
    map.sample_at(target)
}

/// A velocity after dynamics (around 64) and the velocity offset, within 1–127.
pub fn velocity(v: u8, p: &RegionParams) -> u8 {
    let scale = DYNAMICS.iter().find(|d| d.0 == p.dynamics).map(|d| d.1).unwrap_or(1.0);
    let shaped = 64.0 + (v as f64 - 64.0) * scale;
    (shaped.round() as i64 + p.velocity as i64).clamp(1, 127) as u8
}

/// The notes and controller events of a region at timeline sample `start` as they play with
/// `p`, still relative to `start` and in the order given. Times can end up before 0 or past the
/// region's end (a negative delay, a note quantized back over the region start); see
/// [`window`]. Muted notes are transformed too and stay muted.
pub fn apply(p: &RegionParams, notes: &[Note], cc: &[CcEvent], start: i64, map: &TempoMap) -> (Vec<Note>, Vec<CcEvent>) {
    let s0 = start as f64;
    // Quantize and gate on timeline positions: (on, off) per note.
    let mut timed: Vec<(f64, f64)> = notes
        .iter()
        .map(|n| {
            let at = s0 + n.start as f64;
            let on = if p.quantize != 0 { quantize_target(at, p.quantize, p.swing, map) } else { at };
            (on, on + n.length as f64)
        })
        .collect();
    match p.gate {
        100 => {}
        GATE_FIXED => {
            for t in &mut timed {
                t.1 = t.0 + map.samples_per_quarter_at(t.0) / 16.0;
            }
        }
        GATE_LEGATO => {
            // Up to the next note that starts later (the notes of a chord end together).
            let mut ons: Vec<f64> = timed.iter().map(|t| t.0).collect();
            ons.sort_by(f64::total_cmp);
            for t in &mut timed {
                if let Some(&next) = ons.get(ons.partition_point(|&o| o <= t.0)) {
                    t.1 = next;
                }
            }
        }
        pct => {
            for t in &mut timed {
                t.1 = t.0 + (t.1 - t.0) * pct as f64 / 100.0;
            }
        }
    }
    let out_notes = notes
        .iter()
        .zip(timed)
        .map(|(n, (on, off))| {
            let (on, off) = (p.delayed(on, map), p.delayed(off, map));
            let start = (on - s0).round() as i64;
            Note {
                start,
                length: ((off - s0).round() as i64 - start).max(1),
                pitch: (n.pitch as i16 + p.transpose).clamp(0, 127) as u8,
                velocity: velocity(n.velocity, p),
                ..*n
            }
        })
        .collect();
    let out_cc = cc.iter().map(|c| CcEvent { start: (p.delayed(s0 + c.start as f64, map) - s0).round() as i64, ..*c }).collect();
    (out_notes, out_cc)
}

/// How far the playback window of a region at `start` with `length` has to reach beyond the
/// region so its transformed events still play: `(lead, tail)` in samples. `lead` covers events
/// moved before the region start; `tail` is a positive delay at the region end, so the end
/// moves with the notes.
pub fn window(p: &RegionParams, notes: &[Note], cc: &[CcEvent], start: i64, length: i64, map: &TempoMap) -> (i64, i64) {
    let first = notes.iter().map(|n| n.start).chain(cc.iter().map(|c| c.start)).min().unwrap_or(0);
    let end = (start + length) as f64;
    let tail = (p.delayed(end, map) - end).round() as i64;
    ((-first).max(0), tail.max(0))
}

/// One pass of a MIDI region as the engine plays it: the window starts `lead` samples before
/// the pass and lasts `length`; event times are relative to the window start, sorted.
#[derive(Debug, Clone)]
pub struct Pass {
    pub lead: i64,
    pub length: i64,
    pub notes: Vec<Note>,
    pub cc: Vec<CcEvent>,
}

/// What a pass of `pass_len` samples through MIDI region `r` plays (None for audio): the notes
/// that start inside the pass and aren't muted, and the controllers before its end, with the
/// region's parameters applied.
pub fn pass(r: &Region, pass_len: i64, map: &TempoMap) -> Option<Pass> {
    let RegionContent::Midi { notes, cc } = &r.content else { return None };
    let notes: Vec<Note> = notes.iter().filter(|n| !n.muted && n.start >= 0 && n.start < pass_len).copied().collect();
    let cc: Vec<CcEvent> = cc.iter().filter(|c| c.start >= 0 && c.start < pass_len).copied().collect();
    let (mut notes, mut cc) = apply(&r.params, &notes, &cc, r.start, map);
    let (lead, tail) = window(&r.params, &notes, &cc, r.start, pass_len, map);
    for n in &mut notes {
        n.start += lead;
    }
    for c in &mut cc {
        c.start += lead;
    }
    notes.sort_by_key(|n| (n.start, n.pitch));
    cc.sort_by_key(|c| c.start);
    Some(Pass { lead, length: pass_len + lead + tail, notes, cc })
}

/// The region with its parameters written into it and reset (Apply Region Parameters).
///
/// A MIDI region gets the transformed notes, and grows to cover notes a negative delay or
/// quantize moved before its start and the end a positive delay moved later, so it plays
/// exactly as before. A looping region keeps its length (so the loop keeps its period) and its
/// notes are kept inside it instead. Notes outside the region, which didn't play, stay where
/// they are on the timeline and are left out if they would now play. Neither kind starts before
/// the project start: a MIDI region grows back to 0 at most, and an audio region moves by its
/// delay, losing whatever would fall before 0.
pub fn bake(r: &Region, map: &TempoMap) -> Region {
    // A pattern region bakes as the notes its pattern plays.
    if r.pattern.is_some() {
        return bake(&crate::edit::pattern::realize(r, map), map);
    }
    let mut out = Region { params: RegionParams::default(), ..r.clone() };
    if r.params.is_default() {
        return out;
    }
    match &mut out.content {
        RegionContent::Audio { offset, .. } => {
            let to = r.params.delayed(r.start as f64, map).round() as i64;
            // Trim the part before 0: nothing plays there.
            *offset -= to.min(0);
            out.length = (out.length + to.min(0)).max(1);
            out.move_to(to.max(0));
        }
        RegionContent::Midi { notes, cc } => {
            let inside = |start: i64| start >= 0 && start < r.length;
            let played: Vec<bool> = notes.iter().map(|n| inside(n.start)).collect();
            let (mut new_notes, mut new_cc) = apply(&r.params, notes, cc, r.start, map);
            let looped = r.is_looped();
            let (lead, tail) = if looped {
                (0, 0)
            } else {
                let playing: Vec<Note> = new_notes.iter().zip(&played).filter(|p| *p.1).map(|p| *p.0).collect();
                let early: Vec<CcEvent> = new_cc.iter().zip(cc.iter()).filter(|(_, c)| inside(c.start)).map(|p| *p.0).collect();
                window(&r.params, &playing, &early, r.start, r.length, map)
            };
            // Never before the project start: a note moved there stays before the region's
            // start, where it doesn't play (nothing plays before 0).
            let lead = lead.min(r.start.max(0));
            let length = r.length + lead + tail;
            let mut kept = Vec::with_capacity(new_notes.len());
            for ((n, old), was) in new_notes.iter_mut().zip(notes.iter()).zip(played) {
                if was {
                    n.start = if looped { n.start.clamp(0, r.length - 1) } else { n.start + lead };
                    kept.push(*n);
                } else {
                    // Didn't play: unchanged on the timeline, and still not playing.
                    let start = old.start + lead;
                    if start < 0 || start >= length {
                        kept.push(Note { start, ..*old });
                    }
                }
            }
            for c in &mut new_cc {
                c.start = if looped { c.start.max(0) } else { c.start + lead };
            }
            new_cc.sort_by_key(|c| c.start);
            kept.sort_by_key(|n| (n.start, n.pitch));
            *notes = Arc::new(kept);
            *cc = Arc::new(new_cc);
            out.start = r.start - lead;
            out.length = length;
        }
    }
    out
}

/// Where region `r` starts and stops sounding on the timeline, its parameters included (for
/// render ranges). Never before 0.
pub fn bounds(r: &Region, map: &TempoMap) -> (i64, i64) {
    if r.params.is_default() {
        return (r.start, r.end());
    }
    let b = bake(r, map);
    (b.start.max(0), b.end().max(r.end()))
}

#[cfg(test)]
mod tests;
