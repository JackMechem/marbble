//! The non-real-time side of the engine: owns the project document, turns edits into engine
//! commands, manages recording takes, plugin instances and project files.
//!
//! Methods that touch plugin controllers (instantiation, editors, `idle`) must be called on the
//! UI/main thread.
//!
//! This file holds [`Session`], [`Tick`] and `Session::new`. Each module in `session/` adds to the one
//! `impl Session`: `engine_sync.rs` is the wire to `crate::dsp`, `undo.rs` the history, `transport.rs`,
//! `tracks.rs`, `regions.rs`, `plugins.rs`, `periodic.rs` and `files.rs` the basics, and the rest one
//! feature each. Pure logic those call is in the crate's root modules, where it can be tested alone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use vst3_host::{PluginController, PluginDescriptor, PluginState};

use crate::audio_io::{self, AudioConfig, AudioIo, AudioStatus};
use crate::automation::lanes_dsp;
use crate::clip::Clip;
use crate::dsp::*;
use crate::fades::{DragMode, FadeInType, FadeLaw, FadeOutType, FadeShape};
use crate::midi_io::MidiIo;
use crate::model::*;
use crate::prefs::*;
use crate::recorder::{FinishedTake, Recorder, TakePreview};

mod engine_sync;
mod files;
mod groups;
mod left_out;
mod multi_out;
mod periodic;
mod plugins;
mod regions;
mod routing;
mod tracks;
mod transport;
mod undo;
pub use multi_out::{InstrumentOutputs, OutputInfo};
mod bounce;
pub use bounce::{AudioFormat, BipOptions, BipSource, BitDepth, BounceResult, InPlaceRender, RenderOptions};
mod range_edit;
pub use range_edit::{ClipInfo, RangeEdit};
mod options;
pub use options::{discard_autosave, recovery_info, AutosaveJob, Recovery};
mod midi_content;
pub use midi_content::MidiContent;
mod pdc;
pub use pdc::{PdcReport, StripLatency};
mod freeze;
pub use freeze::FreezeRender;
mod markers;
mod region_moves;
mod takes;
pub use region_moves::RegionMove;
mod builtin;
mod new_track_drag;
mod tempo;
pub use builtin::{builtin_effects, builtin_instruments, BuiltinInfo, BuiltinInstrument, FxTick};
mod presets;
pub use presets::EQ_SLOT;
mod crossfades;
mod insert_order;
mod region_params;
pub use region_params::{PlayedNotes, RegionParamsPatch};
mod drag_modes;
mod strip_meters;
pub use strip_meters::PeakHolds;
mod controllers;
mod marquee;
mod strip_settings;
pub use controllers::{controller_label, AssignmentRow};
mod vca;
mod vst_undo;
pub use vca::VcaPatch;
mod alternatives;
mod backups;
pub use backups::BackupInfo;
mod templates;
pub use templates::{delete_template, list_templates, rename_template, templates_dir, UserTemplate};
mod arrange_cmds;
mod arrangement;
mod auto_lanes;
mod patterns;
mod track_alternatives;
use arrange_cmds::SectionClip;

pub const BUILTIN_SYNTH: &str = "builtin:synth";
const PROJECT_FILE: &str = "project.json";
const AUDIO_DIR: &str = "Audio Files";
/// Written next to project.json every few minutes while there are unsaved changes, and offered
/// back after a crash.
pub const AUTOSAVE_FILE: &str = "project.autosave.json";

mod auto_write;
mod last_recording;
mod recovery;
pub use recovery::{discard_journals, read_journal, recording_journals, JournalTake, RecJournal, RecordingRecovery, BACKUP_FILE};
mod clip_loading;
mod drum_replace;
mod musical_moves;
mod region_gain;
mod sampler;
mod threads;
mod track_icons;
mod track_off;
mod vst_key;
mod zero_crossings;
pub use sampler::{prepare_files, PreparedFile, SamplerAdded, SamplerRecording, SamplerView, SamplerWave, SfzReport, ZoneView, SAMPLES_DIR};
#[cfg(test)]
mod comp_tests;
#[cfg(test)]
mod held_notes;
#[cfg(test)]
mod instrument_tests;
#[cfg(test)]
mod preset_tests;

/// A partial edit of a track, as the frontend sends it: fields left out don't change.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct TrackPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub volume_db: Option<f32>,
    pub pan: Option<f32>,
    pub mute: Option<bool>,
    pub solo: Option<bool>,
    pub armed: Option<bool>,
    pub monitor: Option<bool>,
    pub input: Option<InputSel>,
    pub collapsed: Option<bool>,
    pub off: Option<bool>,
}

/// Everything the UI redraws from about 30 times a second, built by [`Session::tick`] and emitted as
/// the `tick` event.
#[derive(Debug, Clone, Serialize)]
pub struct Tick {
    pub pos: i64,
    pub playing: bool,
    pub recording: bool,
    pub cpu: f32,
    pub cpu_peak: f32,
    pub late_blocks: u32,
    /// Threads playing tracks right now (1 unless the project needs more).
    pub threads: u32,
    pub meters: Vec<(Id, f32, f32)>,
    pub master: (f32, f32),
    pub inputs: Vec<f32>,
    pub previews: Vec<TakePreview>,
    pub midi_activity: u32,
    pub overruns: u32,
    pub latency: Latency,
    /// Post-EQ spectrum (dBFS per log band) of the strip shown in the EQ editor.
    pub spectrum: Option<(Id, Vec<f32>)>,
    /// Something went wrong out of sight (a recording couldn't be written); shown once.
    pub warning: Option<String>,
    /// Meters and live values of the built-in effects whose panels are open.
    pub fx: Vec<FxTick>,
    /// The clip indicators: each strip's highest peak (linear) since it was reset, MASTER_ID
    /// for Stereo Out (see `session/strip_meters.rs`).
    pub peak_holds: Vec<(Id, f32)>,
    /// Gain reduction (positive dB) of the strips with a built-in dynamics effect.
    pub gain_reduction: Vec<(Id, f32)>,
    /// The parameter a controller assignment is being learned for (Learn Assignment).
    pub learn: Option<String>,
    /// What the learn that just finished assigned; shown once.
    pub learned: Option<String>,
    /// Messages from hardware MIDI inputs since the last tick, oldest first (for Step Input).
    pub midi_in: Vec<[u8; 3]>,
}

/// The latencies the status line and Settings › Audio show.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Latency {
    pub input_ms: f32,
    pub output_ms: f32,
    /// How far recordings are moved earlier to line up with what was heard.
    pub compensation_ms: f32,
}

/// The non-real-time controller, and the only way in for edits: it owns the document, loaded clips,
/// plug-in controllers, undo history and recorder, and turns edits into [`Cmd`]s for the engine. The app
/// keeps it behind a mutex. Its methods are spread over the modules of `session/`, one per feature.
/// Those that touch plug-in controllers must run on the main thread.
pub struct Session {
    pub project: Project,
    pub dir: PathBuf,
    pub saved: bool,
    pub dirty: bool,
    /// The clips decoded into memory: those the project's tracks can play, and any it has used
    /// since it opened (see `session/clip_loading.rs`).
    clips: HashMap<Id, Arc<Clip>>,
    /// Clips whose file couldn't be read, not tried again until a project opens.
    clips_unreadable: std::collections::HashSet<Id>,
    /// The real-time engine. The audio callback only `try_lock`s it and outputs silence when
    /// that fails, so nothing may hold this lock on the main thread's regular paths (ticks,
    /// edits): talk to the engine through `cmd_tx`. The exceptions are offline renders (the
    /// engine isn't playing then), the graph swap in `rebuild_graph`, and draining the
    /// command queue while no audio device runs (`send`).
    engine: Arc<Mutex<Engine>>,
    cmd_tx: rtrb::Producer<Cmd>,
    cmds_sent: u64,
    garbage_rx: rtrb::Consumer<Garbage>,
    sampler: sampler::SamplerHost,
    pub status: Arc<EngineStatus>,
    audio: Option<AudioIo>,
    pub audio_status: AudioStatus,
    pub audio_config: AudioConfig,
    /// Whether the process's memory is locked (`apply_memory_lock`).
    pub memory_lock: crate::memlock::MemoryLock,
    midi: MidiIo,
    midi_rec_rx: rtrb::Consumer<(i64, [u8; 3])>,
    recorder: Recorder,
    controllers: HashMap<Id, PluginController>,
    meters: HashMap<Id, Arc<Meter>>,
    pub plugins: Vec<PluginDescriptor>,
    pub selected: Option<Id>,
    rec_punch: Option<i64>,
    analyzer_rx: rtrb::Consumer<f32>,
    analyzer_buf: Vec<f32>,
    analyzer_target: Option<Id>,
    rec_midi_track: Option<Id>,
    last_play_from: i64,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    auto: auto_write::AutoWriter,
    /// Settings › General, Recording, MIDI and Editing (see [`Session::set_prefs`]).
    pub prefs: Prefs,
    autosaved_at: Instant,
    /// Hash of the last autosaved document, to skip writing an unchanged one.
    autosave_hash: u64,
    /// Plug-in delay compensation last sent to the engine: per track its settings and the
    /// longest delay its line holds (see `pdc.rs`).
    pdc_sent: HashMap<Id, (PdcParams, usize, usize)>,
    pdc_global: PdcGlobal,
    pdc_report: PdcReport,
    /// How the recording in progress is cut into takes (see `session/takes.rs`).
    rec_plan: Option<crate::takes::RecPlan>,
    /// Where playback started for the recording in progress, the rating marked for its take,
    /// and what the last recording made (see `session/last_recording.rs`).
    rec_play_from: i64,
    rec_rating: Option<TakeRating>,
    last_recording: Option<last_recording::LastRecording>,
    /// Files of discarded recordings, deleted once nothing can need them.
    discarded: Vec<(Id, PathBuf)>,
    /// The engine's sample rate, kept here so reading it needn't lock the engine (see
    /// `engine`: the audio callback only try_locks it, and loses its block when that fails).
    /// Set where the graph is replaced, in `rebuild_graph`.
    engine_rate: f64,
    /// For the next tick's `warning`.
    warning: Option<String>,
    /// Engine panics already reported (see `engine_error_warning`).
    engine_errors_seen: u32,
    engine_error_at: Instant,
    bad_audio_seen: u32,
    bad_audio_at: Instant,
    overloads_seen: u32,
    /// Times tracks were left out already reported (see `session/left_out.rs`).
    left_events_seen: u32,
    /// Some plug-in controller is marked as not to be called (see `session/left_out.rs`).
    marked_unresponsive: bool,
    /// Strips left in plug-ins that `EngineStatus::held` couldn't list, already logged.
    held_unlisted_seen: u32,
    /// What each loaded built-in effect shares with the session, by instance id (see
    /// `session/builtin.rs`), and the instances whose panels are open.
    fx: HashMap<Id, Arc<FxShared>>,
    fx_watch: std::collections::HashSet<Id>,
    /// The last transport jump sent (locate, or record from a pre-roll): the `cmds_sent` count
    /// that includes it and the position it moves to (see [`Session::position`]).
    pending_pos: Option<(u64, i64)>,
    /// Autosave at the next chance rather than after the interval: a recording, bounce or
    /// freeze just finished (see `session/recovery.rs`).
    autosave_soon: bool,
    /// The crash-recovery journal of the recording in progress.
    rec_journal: Option<PathBuf>,
    /// Journals of finished recordings whose takes aren't in a file on disk yet; the next
    /// autosave or save deletes them.
    unsaved_journals: Vec<PathBuf>,
    /// Plug-in states as of the last autosave or save, by instance id, for an emergency
    /// autosave that mustn't call into plug-ins.
    captured_states: HashMap<Id, String>,
    /// The clip indicators' held peaks (see `session/strip_meters.rs`).
    peak_holds: PeakHolds,
    /// The tempo map the engine's playback lists were last built for.
    regions_tempo: Option<crate::tempo::TempoMap>,
    /// Controller assignments (see `session/controllers.rs`).
    controls: controllers::Controls,
    /// The sequence number of the next MIDI input message the ticks haven't reported.
    midi_seen: u64,
    /// What Cut Section Between Locators took out last, for Insert Cut Section.
    section_clip: Option<SectionClip>,
    /// Plug-ins whose editor is open, watched for undo steps (see `session/vst_undo.rs`).
    vst_watches: HashMap<Id, vst_undo::Watch>,
    /// Processing Threads (see `session/threads.rs`), and whether an edit may have changed the
    /// task graph or the setting since it was last looked at.
    threads: threads::ThreadState,
    graph_dirty: bool,
}

/// A project snapshot taken before an edit.
struct UndoEntry {
    project: Project,
    label: String,
    /// Consecutive edits with the same key within a moment (a fader drag) share one entry.
    key: Option<String>,
    at: Instant,
    /// A VST3 and its state to go with the snapshot: an edit in its own editor, which the
    /// snapshot can't undo, since `restore` leaves a plug-in that's still there alone.
    plugin: Option<(Id, Arc<PluginState>)>,
}

/// `folder/<src's name>` inside `target`, numbered ("take 2.wav") if that name is already in
/// `taken` or on disk. The result is added to `taken`.
fn unique_name(target: &Path, folder: &Path, src: &Path, taken: &mut std::collections::HashSet<PathBuf>) -> PathBuf {
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "audio".into());
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut rel = folder.join(format!("{stem}{ext}"));
    let mut n = 2;
    while taken.contains(&rel) || target.join(&rel).exists() {
        rel = folder.join(format!("{stem} {n}{ext}"));
        n += 1;
    }
    taken.insert(rel.clone());
    rel
}

fn same<T: Serialize>(a: &T, b: &T) -> bool {
    serde_json::to_string(a).ok() == serde_json::to_string(b).ok()
}

/// ~/Music/Marbble, where new projects go unless Settings › General names another folder. Until
/// it exists, the projects folder from before the rename (~/Music/Fermata) stays the default.
pub fn default_projects_root() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let root = home.join("Music").join("Marbble");
    if root.exists() {
        return root;
    }
    crate::old_name::old_projects_root().unwrap_or(root)
}

fn untitled_dir(root: &Path) -> PathBuf {
    for i in 1.. {
        let name = if i == 1 { "Untitled.marbble".to_string() } else { format!("Untitled {i}.marbble") };
        let p = root.join(name);
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

fn encode_state(st: &PluginState) -> String {
    let b = base64::engine::general_purpose::STANDARD;
    format!("{}:{}", b.encode(&st.component), b.encode(&st.controller))
}

fn decode_state(s: &str) -> PluginState {
    let b = base64::engine::general_purpose::STANDARD;
    let (a, c) = s.split_once(':').unwrap_or((s, ""));
    PluginState { component: b.decode(a).unwrap_or_default(), controller: b.decode(c).unwrap_or_default() }
}

impl Session {
    pub fn new(audio_config: AudioConfig, plugins: Vec<PluginDescriptor>) -> Session {
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(4096);
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(4096);
        let status = Arc::new(EngineStatus::default());
        let (live_tx, live_rx) = rtrb::RingBuffer::new(1024);
        let (rec_tx, rec_rx) = rtrb::RingBuffer::new(8192);
        let (an_tx, an_rx) = rtrb::RingBuffer::new(1 << 16);
        let engine = Arc::new(Mutex::new(Engine::new(audio_config.sample_rate as f64, cmd_rx, garbage_tx, status.clone(), live_rx, rec_tx, an_tx)));
        let mut midi = MidiIo::new(live_tx);
        midi.refresh();
        let engine_rate = audio_config.sample_rate as f64;
        let mut s = Session {
            project: Project::new("Untitled", audio_config.sample_rate),
            dir: untitled_dir(&default_projects_root()),
            saved: false,
            dirty: false,
            clips: HashMap::new(),
            clips_unreadable: Default::default(),
            engine,
            cmd_tx,
            cmds_sent: 0,
            garbage_rx,
            status,
            audio: None,
            audio_status: AudioStatus::default(),
            audio_config,
            memory_lock: crate::memlock::MemoryLock::initial(),
            midi,
            midi_rec_rx: rec_rx,
            recorder: Recorder::new(),
            sampler: Default::default(),
            controllers: HashMap::new(),
            meters: HashMap::new(),
            plugins,
            selected: None,
            rec_punch: None,
            analyzer_rx: an_rx,
            analyzer_buf: vec![0.0; crate::eq::FFT_SIZE],
            analyzer_target: None,
            rec_midi_track: None,
            last_play_from: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            auto: Default::default(),
            prefs: Prefs::default(),
            autosaved_at: Instant::now(),
            autosave_hash: 0,
            pdc_sent: HashMap::new(),
            pdc_global: PdcGlobal::default(),
            pdc_report: PdcReport::default(),
            rec_plan: None,
            rec_play_from: 0,
            rec_rating: None,
            last_recording: None,
            discarded: Vec::new(),
            engine_rate,
            warning: None,
            engine_errors_seen: 0,
            engine_error_at: Instant::now() - Duration::from_secs(60),
            bad_audio_seen: 0,
            overloads_seen: 0,
            left_events_seen: 0,
            marked_unresponsive: false,
            held_unlisted_seen: 0,
            bad_audio_at: Instant::now() - Duration::from_secs(60),
            fx: HashMap::new(),
            fx_watch: Default::default(),
            pending_pos: None,
            autosave_soon: false,
            rec_journal: None,
            unsaved_journals: Vec::new(),
            captured_states: HashMap::new(),
            peak_holds: PeakHolds::default(),
            regions_tempo: None,
            controls: Default::default(),
            midi_seen: 0,
            section_clip: None,
            vst_watches: HashMap::new(),
            threads: threads::ThreadState { topology: crate::threads::Topology::detect(), ..Default::default() },
            graph_dirty: true,
        };
        s.restart_audio();
        s.sync_globals();
        s
    }
}
