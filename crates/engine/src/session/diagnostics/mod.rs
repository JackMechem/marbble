//! The Diagnostics window's view of the session (Window › Diagnostics): the audio device, the
//! engine's load, the threads and cores, each track's thread and share of the block, plug-ins,
//! memory and MIDI, as one [`Diagnostics`] snapshot the window asks for a few times a second.
//!
//! This file holds the snapshot's types; `gather.rs` fills them from the session, reading only
//! what the session keeps and what the audio thread left in atomics (`EngineStatus`, each strip's
//! `StripLoad`), so taking one never touches the engine's lock. The app adds its log and sends it
//! (`src-tauri/src/diagnostics/`); the window is `src/diagnostics/`. `watch.rs` is what the
//! window's activity log looks at, and the lines it writes for what changed.

mod gather;
#[cfg(test)]
mod tests;
mod watch;

pub use watch::{Watch, WatchKind};

use serde::Serialize;

use crate::audio_io::memlock::MemoryLock;
use crate::midi::input::MidiLogEntry;
use crate::model::{Id, TrackKind};

/// Everything the Diagnostics window shows but the log.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostics {
    pub audio: AudioDiagnostics,
    pub load: LoadDiagnostics,
    pub threads: ThreadDiagnostics,
    /// In the project's order.
    pub tracks: Vec<TrackDiagnostics>,
    pub memory: MemoryDiagnostics,
    pub midi: MidiDiagnostics,
}

/// The device: what was asked, what opened, and what it has reported since.
#[derive(Debug, Clone, Serialize)]
pub struct AudioDiagnostics {
    pub running: bool,
    pub host: String,
    pub input: Option<String>,
    pub output: Option<String>,
    pub input_channels: u16,
    pub output_channels: u16,
    /// What opened, and what Settings › Audio asked for.
    pub sample_rate: u32,
    pub sample_rate_asked: u32,
    pub buffer_size: u32,
    pub buffer_size_asked: u32,
    /// As the device last reported them.
    pub input_latency_ms: f32,
    pub output_latency_ms: f32,
    /// Input frames waiting in the duplex ring.
    pub ring_frames: u64,
    /// Output callbacks that came after the previous one's audio ran out (see `EngineStatus`).
    pub device_gaps: u32,
    /// Since the device opened: events that call for reopening it, and other stream errors, with
    /// the kind of the last of those.
    pub device_events: u32,
    pub stream_errors: u32,
    pub last_stream_error: Option<String>,
    /// Times Marbble reopened the device by itself since the user last did, and whether it gave up.
    pub restarts: u32,
    pub gave_up_restarting: bool,
    /// Why the device didn't open.
    pub error: Option<String>,
    /// macOS: whether the output device has an IO workgroup for the workers to join, and the
    /// threads it suggests. None elsewhere.
    pub workgroup: Option<bool>,
    pub workgroup_threads: Option<u32>,
}

/// How long blocks take against their deadline, and the counts of what went wrong.
#[derive(Debug, Clone, Serialize)]
pub struct LoadDiagnostics {
    /// The CPU meter's average and recent peak (1.0: a whole buffer's time).
    pub cpu: f32,
    pub cpu_peak: f32,
    /// Every block since launch (or since the window's reset): how many, and the load that half
    /// and 99 % of them stayed under, and the worst.
    pub blocks: u64,
    pub p50: f32,
    pub p99: f32,
    pub max: f32,
    pub late_blocks: u32,
    /// Times the engine shed load (silence for a moment) because callbacks kept running late, and
    /// the latest one's callback time and the track blamed.
    pub overloads: u32,
    pub overload_ms: Option<f32>,
    pub overload_track: Option<String>,
    /// Blocks silenced after a panic, and blocks a plug-in put out NaNs or a runaway level in.
    pub engine_errors: u32,
    /// Blocks render ahead hadn't rendered in time, whose track played live.
    pub ahead_misses: u32,
    pub bad_audio: u32,
    pub cmds_sent: u64,
    pub cmds_done: u64,
    pub cmds_dropped: u64,
    /// Recording: audio the recorder thread couldn't keep up with, MIDI that found no room.
    pub overruns: u32,
    pub midi_rec_lost: u32,
    /// Callbacks in which the strips played, and blocks that went through the worker pool.
    pub callbacks: u64,
    pub parallel_blocks: u64,
}

/// The machine's cores and the threads that play tracks.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadDiagnostics {
    pub logical_cpus: usize,
    /// Physical cores of the fastest kind, their logical CPUs (empty where they can't be told
    /// apart, and on macOS), and the slower cores counted (macOS).
    pub fast_cores: usize,
    pub fast_cpus: Vec<usize>,
    pub other_cores: usize,
    /// Settings › Audio › Processing Threads (0: Automatic) and what it means here.
    pub setting: u32,
    pub resolved: usize,
    /// Threads that played the latest block's strips.
    pub playing_now: u32,
    /// The worker pool, while there is one.
    pub pool: Option<PoolDiagnostics>,
    /// Tracks sitting out since a plug-in stopped responding, and strips threads still hold.
    pub left_out_tracks: u32,
    pub held_strips: u32,
}

/// The worker pool's `HealthReport`, with the first refusal's reason.
#[derive(Debug, Clone, Serialize)]
pub struct PoolDiagnostics {
    /// Asked for and playing, the audio callback included.
    pub threads: u32,
    pub playing: u32,
    pub starting: u32,
    pub refused: u32,
    pub stuck: u32,
    pub realtime: u32,
    /// Joined the device's IO workgroup (macOS).
    pub joined: u32,
    pub refusal: Option<String>,
}

/// One track: where and how long its strip played, its delay compensation and its plug-ins.
#[derive(Debug, Clone, Serialize)]
pub struct TrackDiagnostics {
    pub id: Id,
    pub name: String,
    pub kind: TrackKind,
    /// Track Off (itself or a stack it's in), frozen.
    pub off: bool,
    pub frozen: bool,
    /// The thread that played its latest block (0: the audio callback, n: `marbble-dsp-n`), or None
    /// before any.
    pub thread: Option<u32>,
    /// Its latest block's time as a share of the block's audio, and the worst since the last
    /// snapshot.
    pub share: f32,
    pub peak: f32,
    /// Its plug-ins' latency and its compensation delay, in frames.
    pub latency: u32,
    pub delay: u32,
    /// A thread is held up in one of its plug-ins.
    pub held: bool,
    pub plugins: Vec<PluginDiagnostics>,
    /// Its inserts that slept through its latest block, fed silence.
    pub asleep: u32,
    /// Its source is rendered ahead (Settings › Audio › Render Ahead).
    pub ahead: bool,
}

/// A plug-in in a track's instrument or insert slot.
#[derive(Debug, Clone, Serialize)]
pub struct PluginDiagnostics {
    pub uid: Id,
    pub name: String,
    pub instrument: bool,
    pub bypass: bool,
    pub builtin: bool,
    /// Its own latency in frames, when loaded.
    pub latency: Option<u32>,
    /// Not called from the main thread while a processing thread is held up in it.
    pub unresponsive: bool,
}

/// The process's memory and the audio it holds.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryDiagnostics {
    pub lock: MemoryLock,
    /// In kB, from `/proc/self/status` (Linux); None where the system doesn't say.
    pub resident_kb: Option<u64>,
    pub locked_kb: Option<u64>,
    pub swapped_kb: Option<u64>,
    pub mapped_kb: Option<u64>,
    /// Decoded clips in memory, and their samples' size in bytes.
    pub clips: usize,
    pub clip_bytes: u64,
}

/// MIDI ports in use and what came in.
#[derive(Debug, Clone, Serialize)]
pub struct MidiDiagnostics {
    pub inputs: Vec<String>,
    /// By key (the name without ALSA's numbers).
    pub outputs: Vec<String>,
    /// Live messages the engine has taken since launch.
    pub activity: u32,
    /// The track live MIDI plays.
    pub target: Option<String>,
    /// The latest messages from hardware inputs, oldest first.
    pub recent: Vec<MidiLogEntry>,
}
