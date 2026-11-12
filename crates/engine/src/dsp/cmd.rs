//! The two message types between the session and the audio thread: [`Cmd`] goes in, and
//! [`Garbage`] comes back with whatever a command replaced, to be dropped off the audio thread.
//!
//! `apply.rs` handles each `Cmd`. See "Real-time rules" in `docs/ARCHITECTURE.md`.

use super::*;

// By value on purpose: commands sit inside the preallocated ring, so sending and applying one
// allocates nothing, and there is no box for the audio thread to free.
/// An edit on its way to the audio thread, applied at the start of the next block by `Engine::apply`.
/// Whatever it carries is built, and allocated, on the sender's thread.
#[allow(clippy::large_enum_variant)]
#[rustfmt::skip] // Rustfmt would stretch every struct variant over several lines.
pub enum Cmd {
    Play { from: i64 },
    Record { from: i64, punch: i64 },
    Stop,
    Locate(i64),
    /// The tempo map and time signatures (the old one comes back as garbage).
    Tempo(Arc<TempoMap>),
    Cycle { enabled: bool, start: i64, end: i64 },
    Metronome { on: bool, count_in: bool },
    AddTrack(Box<TrackDsp>, usize),
    RemoveTrack(Id),
    Order(Vec<Id>),
    Params(Id, TrackParams),
    MasterVolume(f32),
    Regions(Id, Arc<Vec<RegionDsp>>),
    Insert { track: Id, slot: usize, insert: Option<InsertDsp> },
    Instrument { track: Id, insert: Option<InsertDsp> },
    /// Rearranges a strip's inserts without reloading them: slot `i` takes what was in slot
    /// `order[i]`.
    ReorderInserts { track: Id, order: [u8; INSERT_SLOTS] },
    Bypass { track: Id, uid: Id, bypass: bool },
    RecordSink { track: Id, sink: Option<rtrb::Producer<f32>> },
    /// Copies an input to a recorder whatever the transport does, for recording into a sampler.
    InputTap { input: InputSel, sink: Option<rtrb::Producer<f32>> },
    /// Hands a built-in instrument data built off the audio thread (see `Instrument::swap_data`).
    InstrumentData { track: Id, uid: Id, data: crate::instrument::InstrumentData },
    MidiTarget(Option<Id>),
    /// Whether record-enabled tracks play their input without the I button.
    AutoMonitor(bool),
    /// Which insert (strip or MASTER_ID, instance id) feeds the Channel EQ editor's spectrum
    /// analyzer, from its output.
    Analyzer(Option<(Id, Id)>),
    /// A track's automation lanes.
    Automation(Id, Arc<Vec<LaneDsp>>),
    AutoMode(Id, AutoMode),
    /// The user holds (or lets go of) a parameter while automation is being written.
    AutoTouch { track: Id, target: AutoTarget, on: bool },
    /// Metronome sound, level and when it plays.
    Click(ClickParams),
    /// Whether live MIDI plays the target track (it is recorded either way).
    MidiThru(bool),
    /// A track's delay compensation, with a new delay line when the old one is too short (or
    /// None to keep it), and the same for the line its instrument output comes through.
    Pdc { track: Id, params: PdcParams, line: Option<Box<DelayLine>>, feed_line: Option<Box<DelayLine>> },
    PdcGlobal(PdcGlobal),
    /// Sent before `Record`: armed tracks keep playing outside `range` (None: they are silent
    /// throughout), and input is only captured between the `capture` positions (otherwise from
    /// the punch on). Cleared by `Stop`.
    Autopunch { range: Option<(i64, i64)>, capture: (i64, i64) },
    /// Sets a built-in effect's parameter (plain units) on a track (or MASTER_ID).
    FxParam { track: Id, uid: Id, id: u32, value: f32 },
    /// Which track keys an insert's side-chain (None: it listens to its input).
    Sidechain { track: Id, uid: Id, source: Option<Id> },
    /// Whether a built-in effect's side-chain key is taken before the source's fader.
    SidechainPre { track: Id, uid: Id, pre: bool },
    /// A VST3 stops or starts, around a change of its buses.
    VstGate { track: Id, uid: Id, gate: VstGate },
    /// Settings › MIDI › Chase: whether starting or jumping mid-region starts the notes held
    /// over the playhead, and sends the controller values in effect there.
    Chase { notes: bool, controllers: bool },
    /// The threads that play strips (None: the callback plays them alone).
    Workers(Option<Box<Workers>>),
    /// Render ahead's threads and lead in seconds (None: every strip plays live). See `ahead/`.
    Ahead(Option<Arc<ahead::AheadShared>>, f64),
    /// The tracks that play live whatever render ahead's on: the selected track, and those with a
    /// plug-in window open (see `ahead/`).
    KeepLive([Option<Id>; ahead::KEEP_LIVE]),
    /// What tracks need to go ahead (`TrackDsp::prepare_ahead`), made on the session's thread.
    AheadCtls(Vec<(Id, Arc<ahead::AheadCtl>)>),
    /// Which strips wait for which, for the workers. None drops it.
    Graph(Option<Box<TaskGraph>>),
    /// Makes checks go through the pool (see [`ParallelTest`]).
    ParallelTest(ParallelTest),
    /// A plug-in that stopped responding returned (`EngineStatus::take_back` gave its `token`):
    /// the track's new strip (None: it's gone from the project, its stand-in has gone, or another
    /// thread still holds one of its strips) takes the place of the stand-in for that token, if
    /// it's still there, and every strip sitting out comes back but for those in `still_out`
    /// (sorted), which are fed by one that hasn't returned yet. The new strip sits out too if it's
    /// one of those.
    Back { token: u64, track: Option<Box<TrackDsp>>, still_out: Vec<Id> },
}

/// What a [`Cmd`] replaced, going back to the session's thread so it is freed there.
pub enum Garbage {
    Track(Box<TrackDsp>),
    Insert(InsertDsp),
    Regions(Arc<Vec<RegionDsp>>),
    Sink(rtrb::Producer<f32>),
    Ids(Vec<Id>),
    Automation(Arc<Vec<LaneDsp>>),
    Delay(Box<DelayLine>),
    Tempo(Arc<TempoMap>),
    Workers(Box<Workers>),
    Ahead(Arc<ahead::AheadShared>),
    AheadCtls(Vec<(Id, Arc<ahead::AheadCtl>)>),
    Graph(Box<TaskGraph>),
    /// What an instrument's new data replaced. Its notes may still be reading it, so the session
    /// keeps hold of it until they're done (`Session::sampler_idle`).
    InstrumentData(crate::instrument::InstrumentData),
}
