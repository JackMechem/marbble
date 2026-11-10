//! The saved form of automation: what a lane controls, its points and the track's mode. Evaluating
//! lanes is in `crate::edit::automation`.

use super::*;

/// What an automation lane drives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum AutoParam {
    Volume,
    Pan,
    Mute,
    /// A send level, by send slot.
    Send(u8),
    /// A plug-in parameter: instance id, VST3 parameter id, and its name for display.
    Plugin {
        uid: Id,
        param: u32,
        name: String,
    },
}

impl AutoParam {
    /// Same parameter (a plug-in parameter's name is only a label).
    pub fn same(&self, other: &AutoParam) -> bool {
        match (self, other) {
            (AutoParam::Volume, AutoParam::Volume) | (AutoParam::Pan, AutoParam::Pan) | (AutoParam::Mute, AutoParam::Mute) => true,
            (AutoParam::Send(a), AutoParam::Send(b)) => a == b,
            (AutoParam::Plugin { uid: a, param: p, .. }, AutoParam::Plugin { uid: b, param: q, .. }) => a == b && p == q,
            _ => false,
        }
    }

    /// The range values are clamped to, in the parameter's native units.
    pub fn range(&self) -> (f32, f32) {
        match self {
            AutoParam::Volume => (-96.0, 6.0),
            AutoParam::Pan => (-1.0, 1.0),
            AutoParam::Send(_) => (-96.0, 6.0),
            AutoParam::Mute | AutoParam::Plugin { .. } => (0.0, 1.0),
        }
    }
}

/// One automation breakpoint.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    /// Timeline position in samples.
    pub pos: i64,
    /// In the parameter's native units: dB for volume and sends, -1..1 for pan, 0/1 for mute,
    /// normalized 0..1 for plug-in parameters.
    pub value: f32,
    /// The shape of the segment from this point to the next: 0 is a straight line, towards 1
    /// the change comes early, towards -1 late (see `automation::shape`).
    #[serde(default, skip_serializing_if = "is_linear")]
    pub curve: f32,
}

fn is_linear(c: &f32) -> bool {
    *c == 0.0
}

/// One parameter's automation points on a track, sorted by position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoLane {
    pub param: AutoParam,
    /// Sorted by position. Values are joined by lines (curved by each point's `curve`); before
    /// the first point its value holds, and after the last point the last value does.
    #[serde(default)]
    pub points: Vec<AutoPoint>,
}

/// Logic's automation modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMode {
    /// Lanes are ignored.
    Off,
    /// Lanes play back.
    #[default]
    Read,
    /// Writes while a control is held, then returns to the existing curve.
    Touch,
    /// Writes from the first touch until playback stops, holding the last value.
    Latch,
    /// Overwrites every lane from where playback started.
    Write,
}

impl AutoMode {
    /// Whether moving a control while playing records automation.
    pub fn writes(self) -> bool {
        matches!(self, AutoMode::Touch | AutoMode::Latch | AutoMode::Write)
    }
}
