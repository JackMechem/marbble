//! The saved form of a take folder: takes, comps and comp segments. The comping logic that works on
//! them is in `crate::takes`.

use super::*;

/// A take folder, Logic's container for the passes of a cycle recording.
///
/// Takes and the comp are laid out in *take time*: frames from the start of the recorded
/// passes. The folder region's `content` is still `RegionContent::Audio`, but its `offset` is
/// the take time at the region's start (so moving, trimming and splitting the folder work like
/// any audio region), its `clip` is the newest take's file, and its gain and fades apply to the
/// whole comp. At take time `t`, a take plays frame `take.offset + t` of its own clip.
///
/// A MIDI take folder (MIDI cycle recording with "Create take folders") holds whole takes
/// only, as in Logic: its comp is one take throughout, its `offset` is the take time at the
/// region's start, and its content is the selected take's notes inside the region, so it
/// plays like any MIDI region. Edits to the content belong to that take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeFolder {
    /// Oldest first, so the newest take is last.
    pub takes: Vec<Take>,
    /// Which take is heard where: sorted, non-overlapping take-time ranges. Gaps are silent.
    #[serde(default)]
    pub comp: Vec<CompSegment>,
    /// The folder's comps (Logic's Create New Comp, Duplicate Comp and the comp list in the
    /// take folder menu). `comp` is the one at `current`, kept in step with it. Empty: the
    /// folder has the one comp, "Comp 1".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comps: Vec<Comp>,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub current: usize,
    /// A MIDI take folder's take time at the region's start (an audio folder keeps it in its
    /// content's `offset`).
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub offset: i64,
}

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

/// A saved comp of a take folder: its name and which take is heard where.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comp {
    pub name: String,
    pub comp: Vec<CompSegment>,
}

/// One pass of a cycle recording (or a region packed into a folder).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Take {
    pub id: Id,
    pub name: String,
    pub clip: Id,
    /// Clip frame at take time 0 (negative when the take's file starts later).
    pub offset: i64,
    /// The take-time range the take has audio for.
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub gain_db: f32,
    /// A MIDI take's notes and controller events, in take time (an audio take has none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<CcEvent>,
    /// The recording the take came from and which of its cycle passes it is (from 0). Every
    /// track recorded at once gets its takes with the same, so grouped folders can match
    /// their takes pass by pass. None: packed by hand, or recorded before this was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<(Id, u32)>,
}

/// Part of a comp: take `take` is heard over take time `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompSegment {
    pub start: i64,
    pub end: i64,
    pub take: Id,
}
