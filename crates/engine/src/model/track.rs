//! A track and what hangs off it: its kind and input, plug-in slots, Channel EQ, sends, freeze and
//! alternatives, plus the groups and VCAs tracks belong to.
//!
//! All of this is saved. New fields go at the end of the struct with `#[serde(default)]`.

use super::*;

/// What a track is. Folder and Summing are the two kinds of stack; only Audio and Instrument hold
/// regions of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Audio,
    Instrument,
    /// Folder stack main track: groups its subtracks; its fader, mute and solo apply to them.
    Folder,
    /// Summing stack main track: its subtracks play through it like a bus, with its own EQ,
    /// plug-ins and fader.
    Summing,
    /// Aux channel strip, as in Logic: no regions of its own. It plays what sends and routed
    /// outputs deliver to its bus, through its own EQ, plug-ins and fader.
    Aux,
}

impl TrackKind {
    pub fn is_stack(self) -> bool {
        matches!(self, TrackKind::Folder | TrackKind::Summing)
    }

    /// Tracks that can be record-enabled.
    pub fn can_record(self) -> bool {
        matches!(self, TrackKind::Audio | TrackKind::Instrument)
    }
}

/// Which hardware input channels feed an audio track (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "ch", rename_all = "snake_case")]
pub enum InputSel {
    None,
    Mono(u16),
    Stereo(u16),
}

impl InputSel {
    pub fn channels(&self) -> usize {
        match self {
            InputSel::None => 0,
            InputSel::Mono(_) => 1,
            InputSel::Stereo(_) => 2,
        }
    }
}

/// A plug-in in an insert or instrument slot, with its state saved as base64.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSlot {
    /// Instance id, unique within the project.
    pub uid: Id,
    /// Plugin descriptor id, or `builtin:<name>`.
    pub plugin_id: String,
    pub name: String,
    #[serde(default)]
    pub bypass: bool,
    /// Base64 plugin state. Only populated in saved files, never sent to the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// A built-in effect's parameter values: (parameter id, value in its plain units). Ids
    /// missing here play at their defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<(u32, f32)>,
    /// The track whose post-fader output keys the plug-in's side-chain (None: a built-in
    /// effect's detector listens to its own input, a VST3's aux input stays inactive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<Id>,
    /// The side-chain key is taken before the source's fader (still silenced by its mute)
    /// instead of after it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sidechain_pre: bool,
    /// The preset last loaded into this built-in plug-in or saved from it, which its panel's
    /// preset box shows. The parameters may have changed since; the panel compares them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<PresetRef>,
}

/// A preset by name: one of the factory presets or one of the user's (see `crate::presets`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetRef {
    pub name: String,
    pub factory: bool,
}

/// The shape of a Channel EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandKind {
    LowCut,
    LowShelf,
    Bell,
    HighShelf,
    HighCut,
}

/// One band of the Channel EQ, in plain units.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EqBand {
    pub kind: BandKind,
    pub on: bool,
    /// Hz
    pub freq: f32,
    /// dB (ignored by the cut filters)
    pub gain: f32,
    pub q: f32,
    /// dB/octave for the cut filters: 12, 24 or 48.
    #[serde(default = "default_slope")]
    pub slope: u8,
}

fn default_slope() -> u8 {
    24
}

/// The built-in eight-band channel EQ every strip has, modelled on Logic's Channel EQ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEq {
    pub on: bool,
    pub bands: Vec<EqBand>,
    #[serde(default)]
    pub output_db: f32,
}

impl Default for ChannelEq {
    fn default() -> Self {
        let b = |kind, on, freq, q| EqBand { kind, on, freq, gain: 0.0, q, slope: 24 };
        ChannelEq {
            on: true,
            bands: vec![
                b(BandKind::LowCut, false, 30.0, 0.71),
                b(BandKind::LowShelf, true, 80.0, 0.71),
                b(BandKind::Bell, true, 160.0, 1.1),
                b(BandKind::Bell, true, 500.0, 1.1),
                b(BandKind::Bell, true, 1500.0, 1.1),
                b(BandKind::Bell, true, 5000.0, 1.1),
                b(BandKind::HighShelf, true, 10000.0, 0.71),
                b(BandKind::HighCut, false, 18000.0, 0.71),
            ],
            output_db: 0.0,
        }
    }
}

impl ChannelEq {
    /// True when the EQ would leave the signal untouched, so it can be skipped entirely.
    pub fn is_flat(&self) -> bool {
        !self.on
            || (self.output_db.abs() < 0.01
                && self.bands.iter().all(|b| !b.on || (matches!(b.kind, BandKind::Bell | BandKind::LowShelf | BandKind::HighShelf) && b.gain.abs() < 0.01)))
    }
}

/// A track and its channel strip. Saved. Stacks hang off `parent`, and the order of `Project::tracks`
/// is the order on screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub kind: TrackKind,
    pub name: String,
    pub color: String,
    pub volume_db: f32,
    /// -1 (left) ..= 1 (right)
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub monitor: bool,
    pub input: InputSel,
    #[serde(default)]
    pub instrument: Option<PluginSlot>,
    #[serde(default)]
    pub inserts: Vec<Option<PluginSlot>>,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub eq: ChannelEq,
    /// The track stack this track belongs to.
    #[serde(default)]
    pub parent: Option<Id>,
    /// For stack main tracks: subtracks hidden.
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub group: Option<Id>,
    /// Hidden from the tracks area and mixer (Logic's Hide Track). Hidden tracks still play.
    #[serde(default)]
    pub hidden: bool,
    /// Send slots (`SEND_SLOTS` of them; older projects have none). A send is addressed by
    /// track id + slot index.
    #[serde(default)]
    pub sends: Vec<Option<TrackSend>>,
    /// Explicit output: an aux, or `MASTER_ID` for Stereo Out. None is the default route: the
    /// summing stack the track sits in, else Stereo Out. An explicit output overrides the stack.
    #[serde(default)]
    pub output: Option<Id>,
    /// For auxes: the bus number feeding them ("Bus 3"). 0 for other tracks.
    #[serde(default)]
    pub bus: u32,
    /// Automation lanes, one per parameter.
    #[serde(default)]
    pub automation: Vec<AutoLane>,
    #[serde(default)]
    pub auto_mode: AutoMode,
    /// Set while the track is frozen: it plays this render of its instrument and plug-ins
    /// (pre-fader), and its plug-ins are unloaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<FreezeInfo>,
    /// The VCA whose level offsets this track's fader (see `vca.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vca: Option<Id>,
    /// Logic's Track Alternatives: the track's other sets of regions, kept while `regions` plays
    /// the active one (see `session/track_alternatives.rs`). Empty: the track has just one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<TrackAlternative>,
    /// The name of the active track alternative ("A" when empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative: String,
    /// Extra automation lanes shown below the track's main lane (Logic's sub-lanes), by the
    /// parameter each shows. The main lane's parameter is a view setting of the UI.
    #[serde(default)]
    pub auto_lanes: Vec<AutoParam>,
    /// Logic's Track On/Off, switched off: the track plays nothing and runs nothing, though its
    /// plug-ins stay loaded (see `dsp/track_off.rs`). A stack's subtracks go off with it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
    /// For auxes: the extra output of an instrument that feeds it, besides anything sent to its
    /// bus (see `session/multi_out.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_output: Option<InstrumentOutput>,
    /// The preset last loaded into the Channel EQ or saved from it (see `PluginSlot::preset`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eq_preset: Option<PresetRef>,
    /// The icon the user chose for the track, by the UI's id for it ("kick", "mic"). None shows
    /// the icon its name suggests, else its kind's (see `src/trackIcons.ts`). Not part of channel
    /// strip settings: the icon belongs to the track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// An output of a multi-output instrument, as an aux's input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentOutput {
    /// The instrument track.
    pub track: Id,
    /// The plug-in's output bus (1 and up; 0 is the main output, which plays through the strip).
    pub bus: u32,
}

/// An inactive track alternative: a named set of regions that replaces the track's regions
/// when it is switched to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackAlternative {
    pub name: String,
    #[serde(default)]
    pub regions: Vec<Region>,
}

/// A frozen track's freeze file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreezeInfo {
    /// The clip holding the render (in the project's Freeze Files folder).
    pub clip: Id,
    /// Where on the timeline the render starts.
    pub start: i64,
}

/// A send: a copy of a strip's signal, at its own level, into an aux's bus. (Not called `Send`,
/// which would shadow the marker trait wherever the model is glob-imported.)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackSend {
    /// The aux track the send feeds.
    pub bus: Id,
    pub level_db: f32,
    /// Taps the signal after the plug-ins but before the fader and pan.
    #[serde(default)]
    pub pre_fader: bool,
    #[serde(default)]
    pub muted: bool,
}

/// A track group: what its members share, as in Logic's Group Settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: Id,
    pub name: String,
    pub color: String,
    /// Off works like Logic's group clutch: members act alone.
    pub enabled: bool,
    pub volume: bool,
    pub pan: bool,
    pub mute: bool,
    pub solo: bool,
    pub record: bool,
    /// Selecting one member's track selects all of them.
    pub select: bool,
    /// Selecting a region selects the regions at the same place on the other members.
    pub editing: bool,
}

/// A VCA fader (Logic's VCA strip). It passes no audio: its level offsets the faders of the
/// strips assigned to it without moving them, and its mute mutes them. A VCA can itself be
/// assigned to another (`parent`), whose level and mute then reach its strips too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vca {
    pub id: Id,
    pub name: String,
    pub color: String,
    /// -96 (silence) ..= +6 dB.
    pub level_db: f32,
    pub mute: bool,
    #[serde(default)]
    pub parent: Option<Id>,
}
