//! The values that pass between the engine and a hosted plug-in.

/// A side-chain signal for one block: left and right.
pub type KeyInput<'a> = Option<(&'a [f32], &'a [f32])>;

/// One of a plug-in's audio buses, as it reports it.
#[derive(Debug, Clone)]
pub struct BusLayout {
    pub name: String,
    pub input: bool,
    pub aux: bool,
    pub default_active: bool,
    pub channels: usize,
}

/// An audio output bus of a plug-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputBus {
    /// The bus's index in the plug-in; 0 is the main output, which plays through its strip.
    pub index: u32,
    pub name: String,
    pub channels: u32,
    /// The plug-in plays this bus now.
    pub active: bool,
}

/// A three-byte MIDI message and the sample it lands on in the block (`offset`).
#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    pub offset: u32,
    pub data: [u8; 3],
}

/// The transport as a plug-in sees it for one block.
#[derive(Debug, Clone, Copy, Default)]
pub struct TransportInfo {
    pub playing: bool,
    pub recording: bool,
    pub sample_rate: f64,
    pub tempo: f64,
    pub sig_num: i32,
    pub sig_den: i32,
    pub position_samples: i64,
    pub position_quarters: f64,
    pub bar_start_quarters: f64,
    pub cycle: Option<(f64, f64)>,
}

/// A parameter as the plug-in describes it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub units: String,
    /// Normalized 0..1.
    pub default: f64,
    /// 0 = continuous, 1 = on/off, n = n+1 discrete values.
    pub steps: i32,
}

/// A plug-in's saved state: a VST3's component and controller blobs, or a CLAP plug-in's one
/// blob in `component`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginState {
    pub component: Vec<u8>,
    pub controller: Vec<u8>,
}

/// One step of a parameter gesture in the plug-in's own editor, as the host sees it (VST3's
/// `beginEdit` / `performEdit` / `endEdit`, CLAP's gesture and value events). Values are
/// normalized. The session uses these to write automation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    Begin(u32),
    Perform(u32, f64),
    End(u32),
}
