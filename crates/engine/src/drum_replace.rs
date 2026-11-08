//! Drum Replacement and Doubling (Logic's Track › Drum Replacement/Doubling): finds the hits on a
//! recorded drum track and turns them into MIDI notes for a sampler on a new track, to replace
//! the drum or layer it. Pure; `session/drum_replace.rs` reads the track and makes the new one.
//!
//! - **Hits** come from [`crate::transients::hits`] at full sensitivity, on each part of the
//!   track's audio that is heard (a region, a loop pass, a comp segment of a take folder), with
//!   a little of the audio before it so a part that starts in a ringing drum doesn't count its
//!   tail as a hit. Every candidate goes to the sheet with its level, and the threshold chooses
//!   among them there, so the markers follow the slider without asking the engine again.
//! - **Timing**: a note goes at its hit's attack start by default (Logic's Relative), where a
//!   drum sample's own attack starts, so the sample lines up with the recording and stays in
//!   phase with the overheads when doubling. Peak (Logic's Absolute) puts it at the hit's
//!   loudest moment instead. The offset moves every note, for a sample with a slow start.
//! - **Velocity** is the hit's level against the loudest hit kept: the sampler plays velocity
//!   `v` at `v²` of full gain, so `127 · √(level / loudest)` plays each hit as loud, against the
//!   others, as it was recorded.
//! - **Samples**: Marbble ships none, so each instrument's is made here (a swept sine with a
//!   beater's click for the kick, and so on), written into the project once. Any sample dropped
//!   on the sampler's key takes over.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::clip::Clip;
use crate::model::Note;
use crate::transients;

/// How much audio before a part is analysed with it, in seconds.
const PREROLL_SECONDS: f64 = 0.05;
/// The shortest time between two notes, in seconds: a ringing drum's second wind, or the same
/// hit seen at the end of one part and the start of the next, is one hit.
pub const HOLD_SECONDS: f64 = 0.030;
/// The longest a note lasts, in seconds. The samples are one-shots, so this is only how it looks.
const NOTE_SECONDS: f64 = 0.1;

/// The drum the new track plays, with its General MIDI note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrumKind {
    Kick,
    Snare,
    TomHigh,
    TomMid,
    TomLow,
    HiHat,
}

impl DrumKind {
    pub fn name(self) -> &'static str {
        match self {
            DrumKind::Kick => "Kick",
            DrumKind::Snare => "Snare",
            DrumKind::TomHigh => "Tom High",
            DrumKind::TomMid => "Tom Mid",
            DrumKind::TomLow => "Tom Low",
            DrumKind::HiHat => "Hi-Hat",
        }
    }

    /// General MIDI's key: Bass Drum 1, Acoustic Snare, High Tom, Low-Mid Tom, High Floor Tom
    /// and Closed Hi-Hat.
    pub fn note(self) -> u8 {
        match self {
            DrumKind::Kick => 36,
            DrumKind::Snare => 38,
            DrumKind::TomHigh => 50,
            DrumKind::TomMid => 47,
            DrumKind::TomLow => 43,
            DrumKind::HiHat => 42,
        }
    }

    /// A one-shot of this drum at `rate`, mono, peaking at -1 dBFS.
    pub fn sample(self, rate: u32) -> Vec<f32> {
        let sr = rate.max(1) as f32;
        let mut noise = Noise(0x2545_f491);
        let secs = match self {
            DrumKind::Kick => 0.5,
            DrumKind::Snare => 0.35,
            DrumKind::HiHat => 0.15,
            _ => 0.7,
        };
        let n = (secs * sr) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut phase, mut prev, mut hp) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..n {
            let t = i as f32 / sr;
            let white = noise.next();
            // The noise's top half, for a snare's wires and a hat's sizzle.
            hp = 0.5 * (hp + white - prev);
            prev = white;
            let v = match self {
                DrumKind::Kick => {
                    phase += std::f32::consts::TAU * (48.0 + 120.0 * (-t / 0.03).exp()) / sr;
                    let click = if t < 0.002 { white * 0.35 * (1.0 - t / 0.002) } else { 0.0 };
                    phase.sin() * (-t / 0.16).exp() + click
                }
                DrumKind::Snare => {
                    phase += std::f32::consts::TAU * 185.0 / sr;
                    let body = (phase.sin() + 0.5 * (phase * 1.78).sin()) * (-t / 0.045).exp();
                    0.6 * body + 0.9 * hp * (-t / 0.09).exp()
                }
                DrumKind::HiHat => hp * (-t / 0.025).exp(),
                tom => {
                    let pitch = match tom {
                        DrumKind::TomHigh => 150.0,
                        DrumKind::TomMid => 112.0,
                        _ => 82.0,
                    };
                    phase += std::f32::consts::TAU * pitch * (1.0 + 0.4 * (-t / 0.05).exp()) / sr;
                    let stick = if t < 0.0015 { white * 0.25 } else { 0.0 };
                    phase.sin() * (-t / 0.22).exp() + stick
                }
            };
            out.push(v);
        }
        // Down to nothing over the last 5 ms, then to -1 dBFS.
        let fade = ((0.005 * sr) as usize).min(n);
        for (k, v) in out[n - fade..].iter_mut().enumerate() {
            *v *= 1.0 - (k + 1) as f32 / fade as f32;
        }
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if peak > 0.0 {
            let g = 0.891 / peak;
            out.iter_mut().for_each(|v| *v *= g);
        }
        out
    }
}

/// Deterministic white noise, so a made sample is the same every time.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
    }
}

/// Replace mutes the recorded track; Doubling leaves it playing with the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrumMode {
    Replace,
    Double,
}

/// Where a note goes against its hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrumTiming {
    /// Where the attack starts (Logic's Relative).
    Attack,
    /// At its loudest moment (Logic's Absolute).
    Peak,
}

/// The sheet's settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DrumSettings {
    pub instrument: DrumKind,
    pub mode: DrumMode,
    /// A hit counts when its level reaches this, in dBFS.
    pub threshold_db: f32,
    pub timing: DrumTiming,
    /// Moves every note, in milliseconds (negative is earlier).
    pub offset_ms: f64,
}

impl Default for DrumSettings {
    fn default() -> Self {
        DrumSettings { instrument: DrumKind::Kick, mode: DrumMode::Replace, threshold_db: -30.0, timing: DrumTiming::Attack, offset_ms: 0.0 }
    }
}

/// A hit on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DrumHit {
    /// Where its attack starts.
    pub pos: i64,
    /// Its loudest moment.
    pub peak: i64,
    /// How loud it is, linear (see [`transients::Hit::level`]), with the region's gain.
    pub level: f32,
}

/// A part of the track that is heard: `length` frames of `clip` from frame `from`, at timeline
/// `start`, scaled by `gain`.
#[derive(Clone)]
pub struct DrumSource {
    pub clip: Arc<Clip>,
    pub from: i64,
    pub start: i64,
    pub length: i64,
    pub gain: f32,
}

/// Every hit in the sources, in time order, at least [`HOLD_SECONDS`] apart.
pub fn find_hits(sources: &[DrumSource], rate: u32) -> Vec<DrumHit> {
    let preroll = (PREROLL_SECONDS * rate as f64) as i64;
    let mut all = Vec::new();
    for s in sources {
        let frames = s.clip.frames() as i64;
        let (a, b) = ((s.from - preroll).clamp(0, frames), (s.from + s.length).clamp(0, frames));
        if b <= a {
            continue;
        }
        let part: Vec<&[f32]> = s.clip.channels.iter().map(|c| &c[a as usize..b as usize]).collect();
        for h in transients::hits(&part, s.clip.sample_rate, 1.0) {
            let (at, peak_at) = (a + h.at, a + h.peak_at);
            // Only what's heard: a hit whose peak is before the part belongs to the audio before it.
            if peak_at < s.from {
                continue;
            }
            let to_timeline = |f: i64| s.start + (f - s.from).max(0);
            all.push(DrumHit { pos: to_timeline(at), peak: to_timeline(peak_at), level: h.level * s.gain });
        }
    }
    all.sort_by_key(|h| h.pos);
    let hold = (HOLD_SECONDS * rate as f64) as i64;
    let mut out: Vec<DrumHit> = Vec::with_capacity(all.len());
    for h in all {
        match out.last_mut() {
            // The same hit twice: keep the louder reading, at the earlier attack.
            Some(prev) if h.pos - prev.pos < hold => {
                if h.level > prev.level {
                    *prev = DrumHit { pos: prev.pos, ..h };
                }
            }
            _ => out.push(h),
        }
    }
    out
}

/// The hits at or above the threshold.
pub fn kept(hits: &[DrumHit], threshold_db: f32) -> Vec<DrumHit> {
    let thr = 10f32.powf(threshold_db / 20.0);
    hits.iter().copied().filter(|h| h.level >= thr).collect()
}

/// A hit's velocity against the loudest, for a sampler that plays velocity `v` at `v²` gain.
pub fn velocity(level: f32, loudest: f32) -> u8 {
    if loudest <= 0.0 {
        return 127;
    }
    (127.0 * (level / loudest).clamp(0.0, 1.0).sqrt()).round().clamp(1.0, 127.0) as u8
}

/// The notes for `hits` (already past the threshold) in a MIDI region at timeline `start`,
/// `length` long. Notes that would land outside it are moved to its edge.
pub fn notes(hits: &[DrumHit], s: &DrumSettings, start: i64, length: i64, rate: u32) -> Vec<Note> {
    let loudest = hits.iter().fold(0.0f32, |m, h| m.max(h.level));
    let offset = (s.offset_ms * rate as f64 / 1000.0).round() as i64;
    let longest = (NOTE_SECONDS * rate as f64) as i64;
    let at = |h: &DrumHit| {
        let pos = match s.timing {
            DrumTiming::Attack => h.pos,
            DrumTiming::Peak => h.peak,
        };
        (pos + offset - start).clamp(0, (length - 1).max(0))
    };
    let pitch = s.instrument.note();
    hits.iter()
        .enumerate()
        .map(|(i, h)| {
            let pos = at(h);
            let next = hits.get(i + 1).map_or(length, at);
            Note { start: pos, length: (next - pos).min(longest).max(1), pitch, velocity: velocity(h.level, loudest), muted: false }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;

    fn clip(channels: Vec<Vec<f32>>) -> Arc<Clip> {
        Arc::new(Clip::from_channels("t.wav".into(), RATE, channels))
    }

    /// The made kick at each of `at`, scaled.
    fn kicks(frames: usize, at: &[(usize, f32)]) -> Vec<f32> {
        let k = DrumKind::Kick.sample(RATE);
        let mut buf = vec![0.0f32; frames];
        for &(a, g) in at {
            for (i, v) in k.iter().enumerate().take(frames - a) {
                buf[a + i] += v * g;
            }
        }
        buf
    }

    #[test]
    fn hits_land_on_the_timeline_with_the_region_gain() {
        let at = [(4800, 1.0), (28800, 0.5), (52800, 0.25)];
        let c = clip(vec![kicks(RATE as usize * 2, &at)]);
        // The region starts 2400 frames into the file, at bar 2 (96000), with -6 dB of gain.
        let src = DrumSource { clip: c, from: 2400, start: 96000, length: 90000, gain: 0.5 };
        let hits = find_hits(&[src], RATE);
        assert_eq!(hits.len(), 3, "{hits:?}");
        for (h, &(a, g)) in hits.iter().zip(&at) {
            assert!((h.pos - (96000 + a as i64 - 2400)).abs() <= 2, "{h:?}");
            assert!((h.level - 0.891 * g * 0.5).abs() < 0.03, "{h:?}");
            assert!(h.peak > h.pos);
        }
    }

    #[test]
    fn a_part_starting_in_a_ringing_drum_does_not_count_the_tail() {
        let c = clip(vec![kicks(RATE as usize * 2, &[(4800, 1.0), (52800, 1.0)])]);
        // Starts 30 ms after the first kick, as a region cut there would.
        let src = DrumSource { clip: c, from: 4800 + 1440, start: 0, length: 80000, gain: 1.0 };
        let hits = find_hits(&[src], RATE);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!((hits[0].pos - (52800 - 6240)).abs() <= 2);
    }

    #[test]
    fn the_same_hit_seen_by_two_parts_is_one_note() {
        let c = clip(vec![kicks(RATE as usize, &[(24000, 1.0)])]);
        // Two parts that meet 2 ms into the hit.
        let a = DrumSource { clip: c.clone(), from: 0, start: 0, length: 24100, gain: 1.0 };
        let b = DrumSource { clip: c, from: 24100, start: 24100, length: 20000, gain: 1.0 };
        let hits = find_hits(&[a, b], RATE);
        assert_eq!(hits.len(), 1, "{hits:?}");
    }

    #[test]
    fn the_threshold_and_velocities() {
        let hits =
            [DrumHit { pos: 1000, peak: 1200, level: 0.8 }, DrumHit { pos: 9000, peak: 9300, level: 0.2 }, DrumHit { pos: 20000, peak: 20100, level: 0.01 }];
        assert_eq!(kept(&hits, -30.0).len(), 2);
        assert_eq!(kept(&hits, -60.0).len(), 3);
        assert!(kept(&hits, 0.0).is_empty());
        // A quarter of the loudest is half its velocity: the sampler squares it back.
        assert_eq!(velocity(0.8, 0.8), 127);
        assert_eq!(velocity(0.2, 0.8), 64);
        assert_eq!(velocity(0.0, 0.8), 1);
        let s = DrumSettings::default();
        let n = notes(&kept(&hits, -30.0), &s, 0, 48000, RATE);
        assert_eq!((n[0].start, n[0].pitch, n[0].velocity), (1000, 36, 127));
        assert_eq!((n[1].start, n[1].velocity), (9000, 64));
        assert_eq!(n[0].length, 4800);
        // Peak timing, a -2 ms offset, a region from 500, another drum.
        let s = DrumSettings { timing: DrumTiming::Peak, offset_ms: -2.0, instrument: DrumKind::Snare, ..s };
        let n = notes(&kept(&hits, -30.0), &s, 500, 48000, RATE);
        assert_eq!((n[0].start, n[1].start, n[0].pitch), (1200 - 96 - 500, 9300 - 96 - 500, 38));
        // Never before the region.
        let s = DrumSettings { offset_ms: -50.0, ..s };
        assert_eq!(notes(&kept(&hits, -30.0), &s, 500, 48000, RATE)[0].start, 0);
    }

    #[test]
    fn made_samples_start_at_once_and_end_in_silence() {
        for k in [DrumKind::Kick, DrumKind::Snare, DrumKind::TomHigh, DrumKind::TomMid, DrumKind::TomLow, DrumKind::HiHat] {
            let s = k.sample(44100);
            let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - 0.891).abs() < 1e-3, "{k:?} {peak}");
            // Loud within the first 10 ms, so a note at the hit's attack lines up with it.
            assert!(s[..441].iter().any(|v| v.abs() > 0.3), "{k:?}");
            assert_eq!(*s.last().unwrap(), 0.0, "{k:?}");
            assert_eq!(s, k.sample(44100), "{k:?} is the same every time");
        }
    }
}
