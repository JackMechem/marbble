//! The metronome and the count-in: what they play ([`ClickParams`]) and the voice that plays it.
//!
//! `process.rs` runs them once per block, on the master, after the strips.

use super::*;

/// Metronome parameters, worked out from the project's [`ClickSettings`].
#[derive(Debug, Clone, Copy)]
pub struct ClickParams {
    pub sound: ClickSound,
    /// Linear level of an accented downbeat; other clicks are scaled from it.
    pub level: f32,
    pub accent: bool,
    pub bar_freq: f32,
    pub beat_freq: f32,
    /// Clicks per beat (1..=4).
    pub division: u32,
    pub while_playing: bool,
    pub while_recording: bool,
    pub only_count_in: bool,
}

impl ClickParams {
    pub fn from_settings(c: &ClickSettings) -> ClickParams {
        // Each sound's natural bar / beat pitches; the settings transpose them.
        let (bar, beat) = match c.sound {
            ClickSound::Classic => (1568.0, 1046.5),
            ClickSound::Click => (3135.0, 2349.0),
            ClickSound::Woodblock => (1245.0, 932.0),
            ClickSound::Beep => (1760.0, 880.0),
        };
        let st = |n: i32| 2f32.powf(n.clamp(-24, 24) as f32 / 12.0);
        ClickParams {
            sound: c.sound,
            level: 0.5 * db_to_gain(c.volume_db.clamp(-60.0, 12.0)),
            accent: c.accent,
            bar_freq: bar * st(c.bar_pitch),
            beat_freq: beat * st(c.beat_pitch),
            division: c.division.clamp(1, 4),
            while_playing: c.while_playing,
            while_recording: c.while_recording,
            only_count_in: c.only_count_in,
        }
    }
}

impl Default for ClickParams {
    fn default() -> Self {
        ClickParams::from_settings(&ClickSettings::default())
    }
}

/// The count-in's clicks: steps of `1 / div` beat counted back from the punch, at the tempo
/// and signature in effect there (as in Logic, whatever the tempo before the punch).
pub(super) struct CountIn {
    /// The grid point at or after the punch (step 0) and its index in its bar.
    anchor: f64,
    anchor_sub: i64,
    step: f64,
    div: i64,
    per_bar: i64,
}

impl CountIn {
    pub(super) fn at(map: &TempoMap, punch: i64, div: u32) -> CountIn {
        let at = punch as f64;
        let g = map.grid_at_or_after(map.beats_at(at), div);
        let (num, _) = map.sig_at(at);
        CountIn { anchor: g.pos, anchor_sub: g.sub as i64, step: map.samples_per_beat_at(at) / div as f64, div: div as i64, per_bar: num as i64 * div as i64 }
    }

    pub(super) fn pos(&self, k: i64) -> i64 {
        (self.anchor + k as f64 * self.step).round() as i64
    }

    pub(super) fn first_at_or_after(&self, pos: i64) -> i64 {
        ((pos as f64 - self.anchor) / self.step - 1e-9).ceil() as i64
    }

    pub(super) fn downbeat(&self, k: i64) -> bool {
        (self.anchor_sub + k).rem_euclid(self.per_bar) == 0
    }

    pub(super) fn on_beat(&self, k: i64) -> bool {
        (self.anchor_sub + k).rem_euclid(self.div) == 0
    }
}

/// One sounding metronome click. Synthesized sample by sample, without allocating.
#[derive(Default)]
pub(super) struct Click {
    pub(super) remaining: usize,
    len: usize,
    phase: f32,
    phase2: f32,
    freq: f32,
    amp: f32,
    sound: ClickSound,
    /// Exponential envelopes and their per-sample decay factors (Woodblock, Click).
    env1: f32,
    env2: f32,
    k1: f32,
    k2: f32,
    noise: u32,
    hp: f32,
}

impl Click {
    pub(super) fn start(sound: ClickSound, freq: f32, amp: f32, sr: f32) -> Click {
        let ms = match sound {
            ClickSound::Classic => 35.0,
            ClickSound::Click => 14.0,
            ClickSound::Woodblock => 70.0,
            ClickSound::Beep => 70.0,
        };
        let len = (ms * 0.001 * sr) as usize;
        let decay = |secs: f32| (-1.0 / (secs * sr)).exp();
        let (k1, k2) = match sound {
            ClickSound::Woodblock => (decay(0.016), decay(0.006)),
            ClickSound::Click => (decay(0.0025), decay(0.0012)),
            _ => (1.0, 1.0),
        };
        Click { remaining: len, len, freq, amp, sound, env1: 1.0, env2: 1.0, k1, k2, noise: 0x9E37_79B9, ..Click::default() }
    }

    pub(super) fn next(&mut self, sr: f32) -> f32 {
        use std::f32::consts::TAU;
        let age = self.len - self.remaining;
        let s = match self.sound {
            ClickSound::Classic => {
                let env = self.remaining as f32 / self.len.max(1) as f32;
                (self.phase * TAU).sin() * env * env
            }
            ClickSound::Click => {
                // High-passed noise for the transient plus a short pitched body.
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                let n = (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                let hi = n - self.hp;
                self.hp += 0.35 * hi;
                0.8 * hi * self.env2 + (self.phase * TAU).sin() * self.env1
            }
            ClickSound::Woodblock => {
                let attack = (age as f32 / (0.0008 * sr)).min(1.0);
                attack * 0.72 * ((self.phase * TAU).sin() * self.env1 + 0.45 * (self.phase2 * TAU).sin() * self.env2)
            }
            ClickSound::Beep => {
                // Rounded square (fundamental plus a little third harmonic), 3 ms edges.
                let edge = (0.003 * sr).max(1.0);
                let env = (age as f32 / edge).min(1.0).min(self.remaining as f32 / edge);
                let p = self.phase * TAU;
                0.8 * (p.sin() + (3.0 * p).sin() / 4.0) * env
            }
        };
        self.phase = (self.phase + self.freq / sr) % 1.0;
        self.phase2 = (self.phase2 + self.freq * 2.73 / sr) % 1.0;
        self.env1 *= self.k1;
        self.env2 *= self.k2;
        self.remaining -= 1;
        s * self.amp
    }
}
