//! A track's MIDI settings, Logic's Track inspector on a software instrument track: Transpose,
//! Velocity, Key Limit, Velocity Limit and Delay.
//!
//! They change what the track plays, never what its regions hold. Region notes meet them when the
//! session builds the playback list (`session/tracks/midi_settings.rs`), after the regions' own
//! playback parameters, so every render plays them too; live input meets them on the audio thread
//! (`dsp/live_keys.rs`). Recording keeps what was played, as Logic does.
//!
//! The order: the limits look at a note as it arrives, so two tracks can split one keyboard (or
//! one drum pad's soft and hard hits) between them by what is played; then Transpose and Velocity
//! change it; then Delay moves it in time.

use serde::{Deserialize, Serialize};

use crate::edit::region_params::{self as rp, DELAY_MS_MAX, DELAY_TICKS_MAX, TRANSPOSE_RANGE};
use crate::model::tempo::TempoMap;
use crate::model::{CcEvent, Note, Region, RegionContent};

/// Logic's Track inspector offers ±99 (the Region inspector goes up to +127).
pub const VELOCITY_RANGE: (i16, i16) = (-99, 99);

/// A track's MIDI settings. The defaults change nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackMidiSettings {
    /// Semitones; notes stop at 0 and 127.
    pub transpose: i16,
    /// Added to every velocity; the result is kept within 1–127.
    pub velocity: i16,
    /// The lowest and highest note played (inclusive); others are left out.
    pub key_limit: (u8, u8),
    /// The lowest and highest velocity played (inclusive); others are left out.
    pub velocity_limit: (u8, u8),
    /// Delay, in ticks (960 to a quarter note) or, with `delay_ms`, in milliseconds, as the
    /// Region inspector's Delay. Negative values play early.
    pub delay: f64,
    pub delay_ms: bool,
}

impl Default for TrackMidiSettings {
    fn default() -> Self {
        TrackMidiSettings { transpose: 0, velocity: 0, key_limit: (0, 127), velocity_limit: (1, 127), delay: 0.0, delay_ms: false }
    }
}

impl TrackMidiSettings {
    pub fn is_default(&self) -> bool {
        *self == TrackMidiSettings::default()
    }

    /// Whether what the track plays changes with the tempo map (a delay in ticks).
    pub fn follows_tempo(&self) -> bool {
        self.delay != 0.0 && !self.delay_ms
    }

    /// The same settings with every value in range. A limit given high end first is turned
    /// round, and a velocity limit starts at 1 at the lowest (0 is a note-off).
    pub fn clamped(mut self) -> Self {
        let ordered = |(a, b): (u8, u8), low: u8| {
            let (a, b) = (a.clamp(low, 127), b.clamp(low, 127));
            (a.min(b), a.max(b))
        };
        self.transpose = self.transpose.clamp(TRANSPOSE_RANGE.0, TRANSPOSE_RANGE.1);
        self.velocity = self.velocity.clamp(VELOCITY_RANGE.0, VELOCITY_RANGE.1);
        self.key_limit = ordered(self.key_limit, 0);
        self.velocity_limit = ordered(self.velocity_limit, 1);
        let max = if self.delay_ms { DELAY_MS_MAX } else { DELAY_TICKS_MAX };
        self.delay = if self.delay.is_finite() { self.delay.clamp(-max, max) } else { 0.0 };
        self
    }

    /// Whether a note of `pitch` played at `velocity` gets through the Key and Velocity Limits.
    pub fn plays(&self, pitch: u8, velocity: u8) -> bool {
        let within = |v: u8, (low, high): (u8, u8)| (low..=high).contains(&v);
        within(pitch, self.key_limit) && within(velocity, self.velocity_limit)
    }

    /// A pitch transposed, kept within 0–127.
    pub fn pitch(&self, pitch: u8) -> u8 {
        (pitch as i16 + self.transpose).clamp(0, 127) as u8
    }

    /// A velocity with the offset added, kept within 1–127.
    pub fn velocity(&self, velocity: u8) -> u8 {
        (velocity as i16 + self.velocity).clamp(1, 127) as u8
    }

    /// A note as the track plays it, or None when a limit leaves it out. Its timing is left to
    /// [`delay_entry`].
    pub fn note(&self, n: &Note) -> Option<Note> {
        self.plays(n.pitch, n.velocity).then(|| Note { pitch: self.pitch(n.pitch), velocity: self.velocity(n.velocity), ..*n })
    }

    /// Where an event at timeline sample `at` plays once delayed.
    pub fn delayed(&self, at: f64, map: &TempoMap) -> f64 {
        rp::delayed(self.delay, self.delay_ms, at, map)
    }

    /// The number of samples the delay moves everything by, when that is the same everywhere:
    /// always in ms, and in ticks while the tempo never changes. None when it varies with the
    /// tempo, and every event has to be moved by [`delay_entry`].
    pub fn constant_shift(&self, map: &TempoMap) -> Option<i64> {
        (self.delay == 0.0 || self.delay_ms || map.is_constant()).then(|| self.delayed(0.0, map).round() as i64)
    }
}

/// The notes of a playback entry as the track plays them: limits, then Transpose and Velocity.
/// Their order is kept.
pub fn notes(s: &TrackMidiSettings, notes: &[Note]) -> Vec<Note> {
    notes.iter().filter_map(|n| s.note(n)).collect()
}

/// One MIDI entry of a playback list: where its window starts and how long it lasts, with event
/// times relative to that start.
#[derive(Debug, Clone)]
pub struct Entry {
    pub start: i64,
    pub length: i64,
    pub notes: Vec<Note>,
    pub cc: Vec<CcEvent>,
}

/// A playback entry moved by the track's Delay, event by event, for a delay in ticks while the
/// tempo changes (otherwise [`TrackMidiSettings::constant_shift`] moves the whole entry). The
/// window moves with its events, so notes are still cut where the region ends, and the delay
/// keeps their order.
pub fn delay_entry(s: &TrackMidiSettings, start: i64, length: i64, notes: &[Note], cc: &[CcEvent], map: &TempoMap) -> Entry {
    let at = |offset: i64| s.delayed((start + offset) as f64, map).round() as i64;
    let new_start = at(0);
    let notes = notes
        .iter()
        .map(|n| {
            let on = at(n.start);
            Note { start: on - new_start, length: (at(n.start + n.length) - on).max(1), ..*n }
        })
        .collect();
    let cc = cc.iter().map(|c| CcEvent { start: at(c.start) - new_start, ..*c }).collect();
    Entry { start: new_start, length: (at(length) - new_start).max(1), notes, cc }
}

/// Where a region on a track with settings `s` starts and stops sounding on the timeline, its own
/// playback parameters and the track's Delay included (for render ranges). Never before 0.
pub fn bounds(s: &TrackMidiSettings, r: &Region, map: &TempoMap) -> (i64, i64) {
    let (from, to) = rp::bounds(r, map);
    if s.delay == 0.0 || !matches!(r.content, RegionContent::Midi { .. }) {
        return (from, to);
    }
    let moved = |at: i64| s.delayed(at as f64, map).round() as i64;
    (from.min(moved(from)).max(0), to.max(moved(to)))
}

#[cfg(test)]
mod tests;
