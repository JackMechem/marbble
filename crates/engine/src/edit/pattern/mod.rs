//! Pattern regions, as edited in Logic's Step Sequencer: rows of steps, each row one pitch.
//!
//! A pattern region is a MIDI region with a [`Pattern`] (`Region::pattern`). Its notes are
//! made from the pattern when the session builds the playback list (`Session::regions_dsp`),
//! as pure functions of the pattern, the region's position and length, and the tempo map, so
//! the audio thread does nothing new and every render (bounce, export, freeze, Bounce in
//! Place) plays it. [`realize`] turns a pattern region into the plain MIDI region it plays,
//! which is also Convert to MIDI Region; [`from_notes`] goes the other way.
//!
//! Timing: every row counts steps of its own rate from the region start. A row of `n` steps
//! repeats every `n` steps (rows of different lengths drift against each other: polymeter),
//! and the pattern plays on to the region's end when it loops, or stops after its own length
//! when it doesn't. Swing delays every second step of a straight row. A tied step carries the
//! note before it on instead of starting one. A region cut off a longer pattern region by an
//! Arrangement move or a section edit starts partway into the pattern (`Pattern::offset`), so it
//! plays on from where it was cut.

use serde::{Deserialize, Serialize};

use crate::model::tempo::TempoMap;
use crate::model::{Note, Region, RegionContent};

/// Step rates as the steps per whole note: 1/4 to 1/32, then their triplets (6 = 1/4 triplet).
pub const RATES: [u16; 8] = [4, 8, 16, 32, 6, 12, 24, 48];
/// The most steps a pattern or a row has.
pub const MAX_STEPS: u16 = 64;
/// The most rows a pattern has.
pub const MAX_ROWS: usize = 64;
/// A new step's velocity.
pub const DEFAULT_VELOCITY: u8 = 100;

/// The rows of a new pattern region: the General MIDI drum map's kit pieces, from the kick up
/// (Logic's naming: C1 is 36).
pub const DEFAULT_KIT: [u8; 8] = [36, 38, 39, 42, 46, 45, 48, 49];

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_default_velocity(v: &u8) -> bool {
    *v == DEFAULT_VELOCITY
}

fn is_full_gate(g: &u8) -> bool {
    *g == 100
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// One step of a row. Off steps keep their values, so turning one on again brings them back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    #[serde(skip_serializing_if = "is_false")]
    pub on: bool,
    /// 1–127.
    #[serde(skip_serializing_if = "is_default_velocity")]
    pub velocity: u8,
    /// Note length in percent of the step, 1–100.
    #[serde(skip_serializing_if = "is_full_gate")]
    pub gate: u8,
    /// Carries the note of the step before on through this step (Logic's Tie).
    #[serde(skip_serializing_if = "is_false")]
    pub tie: bool,
}

impl Default for Step {
    fn default() -> Self {
        Step { on: false, velocity: DEFAULT_VELOCITY, gate: 100, tie: false }
    }
}

/// One row: a pitch and its steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternRow {
    pub pitch: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// The row's own step rate (see [`RATES`]); None follows the pattern's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<u16>,
    /// The row's own length in steps, after which it repeats; None follows the pattern's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u16>,
    /// The steps; missing ones at the end are off.
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl PatternRow {
    pub fn new(pitch: u8) -> Self {
        PatternRow { pitch, muted: false, solo: false, rate: None, length: None, steps: Vec::new() }
    }

    /// Step `i` (off when the list doesn't reach it).
    pub fn step(&self, i: usize) -> Step {
        self.steps.get(i).copied().unwrap_or_default()
    }
}

/// A pattern region's pattern (Logic's Step Sequencer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    /// Length in steps.
    pub length: u16,
    /// Steps per whole note (see [`RATES`]).
    pub rate: u16,
    /// Swing in percent: every second step of a straight row moves up to half a step later
    /// (0 = straight; about 66 is the triplet feel).
    #[serde(default)]
    pub swing: u8,
    /// Repeats to the region's end; off, the pattern plays once.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub looped: bool,
    pub rows: Vec<PatternRow>,
    /// Quarter notes into the pattern where the region starts: 0 for a region that starts the
    /// pattern; more for a piece an Arrangement move or a section edit cut off a longer pattern
    /// region, which plays on from where it was cut. Splitting by hand starts the pattern again.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
}

impl Default for Pattern {
    /// The pattern of a new pattern region: a bar of sixteenth notes on the default kit.
    fn default() -> Self {
        Pattern { length: 16, rate: 16, swing: 0, looped: true, rows: DEFAULT_KIT.iter().map(|&p| PatternRow::new(p)).collect(), offset: 0.0 }
    }
}

/// Quarter notes per step at `rate` steps per whole note.
pub fn step_quarters(rate: u16) -> f64 {
    4.0 / rate.max(1) as f64
}

/// Whether a rate is a triplet (triplet rows don't swing).
pub fn is_triplet(rate: u16) -> bool {
    rate.is_multiple_of(3)
}

fn nearest_rate(rate: u16) -> u16 {
    if RATES.contains(&rate) {
        rate
    } else {
        *RATES.iter().min_by_key(|r| r.abs_diff(rate)).unwrap()
    }
}

impl Pattern {
    /// The rate row `r` plays at.
    pub fn row_rate(&self, r: &PatternRow) -> u16 {
        r.rate.unwrap_or(self.rate)
    }

    /// The steps row `r` has before it repeats.
    pub fn row_length(&self, r: &PatternRow) -> u16 {
        r.length.unwrap_or(self.length).max(1)
    }

    /// The pattern's length in quarter notes.
    pub fn quarters(&self) -> f64 {
        self.length as f64 * step_quarters(self.rate)
    }

    /// The same pattern with every value brought into range: rates to the nearest that
    /// exists, lengths 1–[`MAX_STEPS`], velocities 1–127, gates 1–100, at most [`MAX_ROWS`]
    /// rows, and no steps stored past a row's length.
    pub fn clamped(mut self) -> Self {
        self.rate = nearest_rate(self.rate);
        self.length = self.length.clamp(1, MAX_STEPS);
        self.swing = self.swing.min(100);
        self.rows.truncate(MAX_ROWS);
        for r in &mut self.rows {
            r.pitch = r.pitch.min(127);
            r.rate = r.rate.map(nearest_rate);
            r.length = r.length.map(|l| l.clamp(1, MAX_STEPS));
            let len = r.length.unwrap_or(MAX_STEPS) as usize;
            r.steps.truncate(len.max(self.length as usize).min(MAX_STEPS as usize));
            for s in &mut r.steps {
                s.velocity = s.velocity.clamp(1, 127);
                s.gate = s.gate.clamp(1, 100);
            }
        }
        self
    }

    /// The notes the pattern plays in a region at timeline sample `start` lasting `length`,
    /// relative to the region start and sorted. Muted rows are left out; when any row is
    /// soloed, only soloed rows play. With an offset the region plays the pattern from there:
    /// the notes that start before it are left out, as a cut MIDI region leaves them out.
    pub fn notes(&self, start: i64, length: i64, map: &TempoMap) -> Vec<Note> {
        // Pattern time (quarters from the pattern's start) `from` is at timeline beat `q0`.
        let from = self.offset.max(0.0);
        let q0 = map.beats_at(start as f64) - from;
        let span = map.beats_at((start + length) as f64) - q0;
        let limit = if self.looped { span } else { span.min(self.quarters()) };
        let solo = self.rows.iter().any(|r| r.solo);
        let mut out = Vec::new();
        for row in self.rows.iter().filter(|r| if solo { r.solo } else { !r.muted }) {
            let rate = self.row_rate(row);
            let step = step_quarters(rate);
            let len = self.row_length(row) as usize;
            let swing = if is_triplet(rate) { 0.0 } else { step * self.swing as f64 / 200.0 };
            // Where step k starts sounding, swing included.
            let on_at = |k: usize| k as f64 * step + if k % 2 == 1 { swing } else { 0.0 };
            let count = ((limit / step) - 1e-9).ceil().max(0.0) as usize;
            let mut push = |on: f64, off: f64, velocity: u8| {
                let s = map.sample_at(q0 + on).round() as i64 - start;
                let e = map.sample_at(q0 + off).round() as i64 - start;
                if (0..length).contains(&s) {
                    out.push(Note { start: s, length: (e - s).max(1), pitch: row.pitch, velocity, muted: false });
                }
            };
            // The note being played: (on, off, velocity) in quarters from the region start.
            let mut held: Option<(f64, f64, u8)> = None;
            for k in 0..count {
                let s = row.step(k % len);
                let on = on_at(k);
                // A note ends at its gate, but never after the next step starts.
                let off = (on + step * s.gate as f64 / 100.0).min(on_at(k + 1));
                match &mut held {
                    Some(h) if s.tie => h.1 = off,
                    _ => {
                        if let Some((a, b, v)) = held.take() {
                            push(a, b, v);
                        }
                        if s.on {
                            held = Some((on, off, s.velocity));
                        }
                    }
                }
            }
            if let Some((a, b, v)) = held {
                push(a, b, v);
            }
        }
        out.sort_by_key(|n| (n.start, n.pitch));
        out
    }
}

/// The plain MIDI region a pattern region plays: its notes made from the pattern, the pattern
/// gone (Convert to MIDI Region). Controllers and playback parameters stay. Any other region
/// comes back unchanged.
pub fn realize(r: &Region, map: &TempoMap) -> Region {
    let mut out = r.clone();
    if let (Some(p), RegionContent::Midi { notes, .. }) = (out.pattern.take(), &mut out.content) {
        *notes = p.notes(r.start, r.length, map);
    }
    out
}

/// A pattern that plays MIDI notes as closely as sixteenth steps can (Convert to Pattern
/// Region): a row per pitch, lowest first, each note on the nearest step with its velocity, and
/// notes longer than a step tied on. The pattern covers the region up to [`MAX_STEPS`] steps.
pub fn from_notes(notes: &[Note], start: i64, length: i64, map: &TempoMap) -> Pattern {
    let rate = 16;
    let step = step_quarters(rate);
    let q0 = map.beats_at(start as f64);
    let span = map.beats_at((start + length) as f64) - q0;
    let steps = ((span / step - 1e-6).ceil() as i64).clamp(1, MAX_STEPS as i64) as usize;
    let mut pitches: Vec<u8> = notes.iter().filter(|n| !n.muted).map(|n| n.pitch).collect();
    pitches.sort_unstable();
    pitches.dedup();
    let mut rows: Vec<PatternRow> = pitches.iter().take(MAX_ROWS).map(|&p| PatternRow { steps: vec![Step::default(); steps], ..PatternRow::new(p) }).collect();
    for n in notes.iter().filter(|n| !n.muted && n.start >= 0 && n.start < length) {
        let Some(row) = rows.iter_mut().find(|r| r.pitch == n.pitch) else { continue };
        let on = map.beats_at((start + n.start) as f64) - q0;
        let k = (on / step).round() as usize;
        if k >= steps {
            continue;
        }
        let dur = (map.beats_at((start + n.start + n.length) as f64) - q0 - on) / step;
        // Whole steps it covers after its own, then what is left of the last one.
        let more = ((dur - 1e-6).ceil() as usize).saturating_sub(1);
        let last = ((dur - more as f64) * 100.0).round().clamp(1.0, 100.0) as u8;
        row.steps[k] = Step { on: true, velocity: n.velocity.clamp(1, 127), gate: if more == 0 { last } else { 100 }, tie: false };
        for j in 1..=more {
            let Some(s) = row.steps.get_mut(k + j) else { break };
            if s.on {
                break;
            }
            *s = Step { tie: true, gate: if j == more { last } else { 100 }, ..*s };
        }
    }
    Pattern { length: steps as u16, rate, swing: 0, looped: true, rows, offset: 0.0 }
}

#[cfg(test)]
mod tests;
