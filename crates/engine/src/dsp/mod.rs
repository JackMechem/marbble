//! The real-time mixing graph. Everything in `Engine::process` runs on the audio thread and must
//! not block or allocate; structural changes arrive as `Cmd`s and replaced objects are shipped
//! back as `Garbage` to be dropped elsewhere.
//!
//! This file holds [`Engine`] and how it's made. A block runs through `process.rs`, which plays each
//! strip with `strip.rs` (on the worker pool through `parallel/` and `pool/`, when `graph/` has a
//! graph); `apply.rs` handles commands; `track.rs` holds a strip's state and `block.rs` what a block
//! shares; `live_keys.rs` plays live input through a track's MIDI settings; `idle/` sleeps plug-ins fed silence; `ahead/` renders the source of tracks that aren't live ahead of the playhead; `strip_load.rs` notes which thread played each strip, for the Diagnostics window. `offline.rs` renders without a device. `crate::session` is the other side of every message.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use std::cell::RefCell;
use vst3_host::{MidiEvent, PluginProcessor, TransportInfo};

use crate::audio_file::clip::Clip;
use crate::edit::automation::{value_at, AutoTarget, LaneDsp};
use crate::edit::fades::FadeLaw;
use crate::model::tempo::{GridPoint, TempoMap};
use crate::model::{AutoMode, CcEvent, CcKind, Id, InputSel, InstrumentOutput, Note, TrackKind, CC_KINDS, INSERT_SLOTS, MASTER_ID, SEND_SLOTS};
use crate::prefs::{ClickSettings, ClickSound};

pub mod ahead;
mod apply;
mod block;
mod denormals;
pub use block::*;
mod click;
pub use click::*;
mod cmd;
pub use cmd::*;
mod process;
pub use process::{interleave, BlockIo};
mod status;
pub use status::*;
mod strip;
use strip::*;
mod strip_load;
pub use strip_load::{StripLoad, StripLoadReading};
mod track;
pub use track::*;
mod offline;
pub use offline::Isolate;
use offline::Role;
mod pdc;
pub use pdc::{DelayLine, PdcGlobal, PdcParams, INSTRUMENT_BIT};
mod builtin;
pub use builtin::{clamp as builtin_clamp, to_normalized, to_plain, BuiltinDsp, FxShared, InstrumentDsp};
pub(crate) mod auto;
use auto::MasterAuto;
pub mod graph;
mod idle;
mod live_keys;
mod multi_out;
pub mod parallel;
pub mod pool;
mod track_off;
pub use graph::TaskGraph;
pub use parallel::{ParallelTest, Workers};

pub const MAX_BLOCK: usize = 1024;
pub const MAX_INPUTS: usize = 32;
const MAX_MIDI: usize = 512;
/// Parameters a track can have held (touched) at once while writing automation.
const MAX_TOUCHED: usize = 16;
/// Plug-in lanes per track whose last sent value is remembered (later ones resend every block).
const MAX_AUTO_SENT: usize = 64;

/// The real-time graph: every strip, the transport, the master and the click. Lives behind a mutex that
/// the audio callback only `try_lock`s, and the session reaches it through [`Cmd`]s. `process.rs` is the
/// block loop and `apply.rs` the command handling.
pub struct Engine {
    pub sample_rate: f64,
    // Boxed so a strip arrives in a `Cmd`, and goes back as `Garbage`, by moving a pointer, and
    // the audio thread never copies one or frees it.
    #[allow(clippy::vec_box)]
    tracks: Vec<Box<TrackDsp>>,
    master_inserts: Vec<Option<InsertDsp>>,
    master_volume: f32,
    master_gain: f32,
    playing: bool,
    recording: bool,
    pos: i64,
    punch: i64,
    tempo: Arc<TempoMap>,
    cycle: (bool, i64, i64),
    metronome: bool,
    count_in: bool,
    click: Click,
    click_params: ClickParams,
    midi_thru: bool,
    midi_target: Option<Id>,
    auto_monitor: bool,
    /// Stereo Out's automation lanes.
    master_auto: MasterAuto,
    /// The insert (strip, instance id) whose output feeds the Channel EQ editor's analyzer.
    analyzer_target: Option<(Id, Id)>,
    analyzer_tx: rtrb::Producer<f32>,
    /// Commands from the session. A wait-free single-producer ring: the audio thread must never
    /// spin waiting on a sender (a crossbeam channel can, while a preempted sender finishes a
    /// write, which with real-time priority starves the sender and trips RLIMIT_RTTIME).
    cmd_rx: rtrb::Consumer<Cmd>,
    /// Replaced objects back to the session, to be dropped off the audio thread.
    garbage: RefCell<rtrb::Producer<Garbage>>,
    pub status: Arc<EngineStatus>,
    live_midi: rtrb::Consumer<[u8; 3]>,
    midi_rec: rtrb::Producer<(i64, [u8; 3])>,
    live: Vec<MidiEvent>,
    bus_l: Vec<f32>,
    bus_r: Vec<f32>,
    inputs: Vec<Vec<f32>>,
    load: f32,
    block_load: BlockLoad,
    /// How long the callbacks have been taking longer than the audio they produce, and how
    /// many callbacks are left to skip after an overload.
    behind_secs: f64,
    shedding: u32,
    /// Render ahead (`ahead/`).
    ahead: ahead::Ahead,
    /// An offline render is running: it may take as long as it needs.
    offline: bool,
    /// Set while an offline render plays only some tracks (see `offline.rs`).
    isolate: Option<Box<Isolate>>,
    /// Jump faders straight to their targets on the next block instead of gliding, so a
    /// render starts (and live playback resumes) at the right levels.
    snap_gains: bool,
    pdc: PdcGlobal,
    /// The serial loop's scratch (each of the pool's threads has one of its own).
    scratch: Scratch,
    /// Playing strips on several threads (see `parallel/`).
    parallel: parallel::Parallel,
    /// See `Cmd::Autopunch`.
    punch_range: Option<(i64, i64)>,
    /// Where the input goes for a recording into a sampler (see `Cmd::InputTap`).
    input_tap: Option<(InputSel, rtrb::Producer<f32>)>,
    capture: Option<(i64, i64)>,
    /// See `Cmd::Chase`: (notes, controllers).
    chase: (bool, bool),
    /// The end of an offline render, while the audio still in the delay-compensation lines
    /// comes out: the transport runs on (so automation and Stereo Out's lanes keep following
    /// that audio), but regions, notes and the click start nothing new.
    draining: bool,
    /// After a Stop while playing with delay compensation: where the transport stopped and the
    /// frames still to come out of the lines. They drain as at the end of a render, and the
    /// session is shown the transport stopped there meanwhile (see `Engine::finish_stop`).
    stopping: Option<(i64, i64)>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.settle();
    }
}

impl Engine {
    pub fn new(
        sample_rate: f64,
        cmd_rx: rtrb::Consumer<Cmd>,
        garbage: rtrb::Producer<Garbage>,
        status: Arc<EngineStatus>,
        live_midi: rtrb::Consumer<[u8; 3]>,
        midi_rec: rtrb::Producer<(i64, [u8; 3])>,
        analyzer_tx: rtrb::Producer<f32>,
    ) -> Engine {
        Engine {
            sample_rate,
            tracks: Vec::with_capacity(256),
            master_inserts: (0..INSERT_SLOTS).map(|_| None).collect(),
            master_volume: 1.0,
            master_gain: 1.0,
            playing: false,
            recording: false,
            pos: 0,
            punch: 0,
            tempo: Arc::new(TempoMap::constant(sample_rate, 120.0, 4, 4)),
            cycle: (false, 0, 0),
            metronome: false,
            count_in: false,
            click: Click::default(),
            click_params: ClickParams::default(),
            midi_thru: true,
            midi_target: None,
            auto_monitor: false,
            master_auto: MasterAuto::default(),
            analyzer_target: None,
            analyzer_tx,
            cmd_rx,
            garbage: RefCell::new(garbage),
            status,
            live_midi,
            midi_rec,
            live: Vec::with_capacity(MAX_MIDI),
            bus_l: vec![0.0; MAX_BLOCK],
            bus_r: vec![0.0; MAX_BLOCK],
            inputs: (0..MAX_INPUTS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            load: 0.0,
            block_load: BlockLoad::default(),
            behind_secs: 0.0,
            shedding: 0,
            ahead: Default::default(),
            offline: false,
            isolate: None,
            snap_gains: false,
            pdc: PdcGlobal::default(),
            scratch: Scratch::default(),
            parallel: Default::default(),
            punch_range: None,
            input_tap: None,
            capture: None,
            chase: (true, true),
            draining: false,
            stopping: None,
        }
    }

    /// Replaces the whole graph (project load, sample-rate change). Called with the engine lock
    /// held from a non-audio thread; returns the old graph for the caller to drop. Commands still
    /// queued were sent for the old graph, so they are applied to it first: taken afterwards, an
    /// old track's strip or another project's would land in the new one.
    pub fn replace_graph(
        &mut self,
        sample_rate: f64,
        tracks: Vec<Box<TrackDsp>>,
        master_inserts: Vec<Option<InsertDsp>>,
    ) -> (Vec<Box<TrackDsp>>, Vec<Option<InsertDsp>>) {
        self.take_queued();
        self.reclaim_all();
        // New strips throughout: none sits out, and what threads still hold is no track's.
        self.status.release_all();
        self.sample_rate = sample_rate;
        self.playing = false;
        self.recording = false;
        self.status.playing.store(false, Ordering::Relaxed);
        self.status.recording.store(false, Ordering::Relaxed);
        let old_t = std::mem::replace(&mut self.tracks, tracks);
        let old_m = std::mem::replace(&mut self.master_inserts, master_inserts);
        self.resolve_routes();
        (old_t, old_m)
    }
}

#[cfg(test)]
pub(crate) mod tests;
