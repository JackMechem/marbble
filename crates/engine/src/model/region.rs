//! Regions on a track and what they play: audio from a clip, MIDI notes and controllers, or a pattern.
//! Includes the time arithmetic of looped regions.
//!
//! All of this is saved. New fields go at the end of the struct with `#[serde(default)]`.

use super::*;

/// A MIDI note, in frames from its region's start.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Note {
    /// Offset from the region start, in samples.
    pub start: i64,
    pub length: i64,
    pub pitch: u8,
    pub velocity: u8,
    /// Muted notes stay in the region but don't play.
    #[serde(default)]
    pub muted: bool,
}

/// What a controller event drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "cc", rename_all = "snake_case")]
pub enum CcKind {
    /// A control change (CC 0–127), value 0–127.
    Controller(u8),
    /// Pitch bend, value 0–16383 with 8192 centred.
    PitchBend,
    /// Channel pressure (aftertouch), value 0–127.
    Aftertouch,
}

impl CcKind {
    /// A dense index (controllers 0–127, then pitch bend and aftertouch) for per-kind tables.
    pub fn index(self) -> usize {
        match self {
            CcKind::Controller(n) => (n & 127) as usize,
            CcKind::PitchBend => 128,
            CcKind::Aftertouch => 129,
        }
    }

    /// The inverse of `index`.
    pub fn from_index(i: usize) -> CcKind {
        match i {
            128 => CcKind::PitchBend,
            129 => CcKind::Aftertouch,
            n => CcKind::Controller((n & 127) as u8),
        }
    }

    /// The MIDI message that sets this controller to `value`.
    pub fn message(self, value: u16) -> [u8; 3] {
        match self {
            CcKind::Controller(n) => [0xB0, n & 127, value.min(127) as u8],
            CcKind::PitchBend => {
                let v = value.min(16383);
                [0xE0, (v & 127) as u8, (v >> 7) as u8]
            }
            CcKind::Aftertouch => [0xD0, value.min(127) as u8, 0],
        }
    }

    /// Reads a channel message back into a controller event, if it is one.
    pub fn from_message(d: [u8; 3]) -> Option<(CcKind, u16)> {
        match d[0] & 0xF0 {
            0xB0 => Some((CcKind::Controller(d[1] & 127), (d[2] & 127) as u16)),
            0xE0 => Some((CcKind::PitchBend, ((d[2] as u16 & 127) << 7) | (d[1] as u16 & 127))),
            0xD0 => Some((CcKind::Aftertouch, (d[1] & 127) as u16)),
            _ => None,
        }
    }
}

/// Number of distinct `CcKind::index` values.
pub const CC_KINDS: usize = 130;

/// A controller, pitch bend or aftertouch event in a MIDI region.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CcEvent {
    /// Offset from the region start, in samples.
    pub start: i64,
    pub kind: CcKind,
    /// 0–127, or 0–16383 for pitch bend.
    pub value: u16,
}

/// What a region plays.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RegionContent {
    Audio {
        clip: Id,
        /// Offset into the clip, in frames.
        offset: i64,
        #[serde(default)]
        gain_db: f32,
        /// Fade lengths in frames (0 = none).
        #[serde(default)]
        fade_in: i64,
        #[serde(default)]
        fade_out: i64,
        /// Fade curves and types (see `edit/fades/`).
        #[serde(default, skip_serializing_if = "FadeShape::is_default")]
        fade_shape: FadeShape,
    },
    Midi {
        notes: Vec<Note>,
        /// Controller events, sorted by start.
        #[serde(default)]
        cc: Vec<CcEvent>,
    },
}

/// A stretch of content on a track's timeline: frames, as everything saved is. Its loop extends it past
/// `length` to `loop_end`. See [`Region::end`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Region {
    pub id: Id,
    pub name: String,
    /// Timeline position in samples.
    pub start: i64,
    pub length: i64,
    #[serde(default)]
    pub muted: bool,
    /// Overrides the track colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub content: RegionContent,
    /// Timeline sample where a looping region stops repeating (None = not looping). The
    /// region's own `start..start + length` is what repeats, as with Logic's Loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_end: Option<i64>,
    /// Makes the region a take folder (cycle recording, or Pack Take Folder): it plays its
    /// comp instead of `content`'s clip. See [`TakeFolder`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takes: Option<TakeFolder>,
    /// Playback parameters (Logic's Region inspector: quantize, transpose, velocity, delay
    /// and so on), applied as the region plays. See [`crate::edit::region_params`].
    #[serde(default, skip_serializing_if = "RegionParams::is_default")]
    pub params: RegionParams,
    /// Makes a MIDI region a pattern region (Logic's Step Sequencer): it plays the notes its
    /// pattern makes instead of `content`'s. See [`crate::edit::pattern`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Pattern>,
}

/// A loop can't repeat its region more often than this.
pub const MAX_LOOP_PASSES: i64 = 4096;

impl Region {
    /// A plain MIDI region of these notes and controllers (relative to `start`): not muted,
    /// looped, a take folder or a pattern.
    pub fn midi(id: Id, name: String, start: i64, length: i64, notes: Vec<Note>, cc: Vec<CcEvent>) -> Region {
        Region {
            id,
            name,
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::Midi { notes, cc },
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        }
    }

    /// The clips the region can play: an audio region's clip and every take in its folder,
    /// whether the comp plays it or not.
    pub fn clips(&self) -> Vec<Id> {
        let RegionContent::Audio { clip, .. } = self.content else { return Vec::new() };
        std::iter::once(clip).chain(self.takes.iter().flat_map(|f| f.takes.iter().map(|t| t.clip))).collect()
    }

    /// Where the region stops sounding on the timeline: its loop end, or its own end.
    pub fn end(&self) -> i64 {
        let own = self.start + self.length;
        match self.loop_end {
            Some(e) if self.length > 0 => e.clamp(own, self.start + self.length * MAX_LOOP_PASSES),
            _ => own,
        }
    }

    /// Whether it has looped repetitions past its own length.
    pub fn is_looped(&self) -> bool {
        self.end() > self.start + self.length
    }

    /// Timeline `(start, length)` of every pass through the region: the region itself, then
    /// each loop repetition, the last one cut short at the loop end.
    pub fn passes(&self) -> Vec<(i64, i64)> {
        let end = self.end();
        let mut out = vec![(self.start, self.length)];
        let mut at = self.start + self.length;
        while at < end {
            out.push((at, self.length.min(end - at)));
            at += self.length;
        }
        out
    }

    /// Moves the region to `start`, taking its loop end along.
    pub fn move_to(&mut self, start: i64) {
        let delta = start - self.start;
        self.start = start;
        self.loop_end = self.loop_end.map(|e| e + delta);
    }

    /// The loop as plain regions, one per pass (Logic's Convert Loops to Real Copies). Every
    /// copy keeps the original's id; the caller hands out new ones. Copies play exactly as
    /// the loop did: the fade-in stays on the first, the fade-out moves to the last, and the
    /// repetitions drop the notes that start past their end.
    pub fn real_copies(&self) -> Vec<Region> {
        let passes = self.passes();
        let last = passes.len() - 1;
        passes
            .into_iter()
            .enumerate()
            .map(|(k, (start, length))| {
                let mut r = Region { start, length, loop_end: None, ..self.clone() };
                match &mut r.content {
                    RegionContent::Audio { fade_in, fade_out, .. } if last > 0 => {
                        let fo = (*fade_out).min(length);
                        *fade_in = if k == 0 { *fade_in } else { 0 };
                        *fade_out = if k == last { fo } else { 0 };
                    }
                    RegionContent::Midi { notes, cc } if k > 0 => {
                        notes.retain(|n| n.start < length);
                        cc.retain(|c| c.start < length);
                    }
                    _ => {}
                }
                r
            })
            .collect()
    }
}
