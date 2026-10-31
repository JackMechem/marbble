//! What the audio thread reports without locks: the level [`Meter`]s, the [`EngineStatus`] atomics
//! the session reads for the tick, and the load measurement behind the CPU meter ([`BlockLoad`]).
//!
//! `process.rs` writes these, one block at a time; `Session::tick` and the UI read them. Nothing
//! here allocates or blocks.

use super::*;

/// A stereo level meter the audio thread writes and the tick reads and resets, in atomics.
#[derive(Default)]
pub struct Meter {
    l: AtomicU32,
    r: AtomicU32,
}

impl Meter {
    /// Non-negative floats order the same as their bit patterns, so fetch_max works directly.
    pub fn feed(&self, l: f32, r: f32) {
        self.l.fetch_max(l.abs().to_bits(), Ordering::Relaxed);
        self.r.fetch_max(r.abs().to_bits(), Ordering::Relaxed);
    }

    /// Peak since the last call.
    pub fn take(&self) -> (f32, f32) {
        (
            f32::from_bits(self.l.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.r.swap(0, Ordering::Relaxed)),
        )
    }
}

/// Everything the audio thread reports, in atomics: position, meters, load, and counts of what went
/// wrong. Shared with the session, which reads it for the tick and to pace itself (`cmds_done`).
pub struct EngineStatus {
    pub pos: AtomicI64,
    pub playing: AtomicBool,
    pub recording: AtomicBool,
    pub master: Meter,
    pub inputs: Vec<Meter>,
    pub cpu_load: AtomicU32,
    /// The worst recent block's load, and how many blocks took longer than their own audio.
    /// See `BlockLoad`.
    pub cpu_peak: AtomicU32,
    pub late_blocks: AtomicU32,
    pub cmds_done: AtomicU64,
    pub midi_activity: AtomicU32,
    pub overruns: AtomicU32,
    /// Blocks in which `Engine::process` panicked; the audio callback silenced them (see
    /// `audio_io::build_output`).
    pub engine_errors: AtomicU32,
    /// Measured device latencies (µs) and how many input frames sit in the duplex ring.
    pub input_latency_us: AtomicU64,
    pub output_latency_us: AtomicU64,
    pub ring_frames: AtomicU64,
    /// Blocks in which a plug-in put out NaNs, infinities or a runaway level (see `contain`),
    /// and the uid of the last one that did.
    pub bad_audio: AtomicU32,
    pub bad_audio_uid: AtomicU64,
    /// Times the engine fell so far behind that it skipped processing for a moment (see
    /// `Engine::process`).
    pub overloads: AtomicU32,
    /// The latest overload, for its message: how long its callback took in seconds (f32 bits), the
    /// track that took longest or that a worker is stuck in (0: none), and whether it gave up on a
    /// stuck worker.
    pub overload_secs: AtomicU32,
    pub overload_track: AtomicU64,
    pub overload_stuck: AtomicBool,
    /// Threads playing the strips in the latest block, and how many blocks went through the pool
    /// (see `parallel.rs`).
    pub threads: AtomicU32,
    pub parallel_blocks: AtomicU64,
}

impl Default for EngineStatus {
    fn default() -> Self {
        EngineStatus {
            pos: AtomicI64::new(0),
            playing: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            master: Meter::default(),
            inputs: (0..MAX_INPUTS).map(|_| Meter::default()).collect(),
            cpu_load: AtomicU32::new(0),
            cpu_peak: AtomicU32::new(0),
            late_blocks: AtomicU32::new(0),
            cmds_done: AtomicU64::new(0),
            midi_activity: AtomicU32::new(0),
            overruns: AtomicU32::new(0),
            engine_errors: AtomicU32::new(0),
            input_latency_us: AtomicU64::new(0),
            output_latency_us: AtomicU64::new(0),
            ring_frames: AtomicU64::new(0),
            bad_audio: AtomicU32::new(0),
            bad_audio_uid: AtomicU64::new(0),
            overloads: AtomicU32::new(0),
            overload_secs: AtomicU32::new(0),
            overload_track: AtomicU64::new(0),
            overload_stuck: AtomicBool::new(false),
            threads: AtomicU32::new(1),
            parallel_blocks: AtomicU64::new(0),
        }
    }
}

/// How long the callbacks may run late back to back before the engine sheds load. Past 200 ms
/// the kernel kills a real-time thread that never sleeps (RLIMIT_RTTIME's hard limit; its soft
/// limit's SIGXCPU is ignored, see `audio_io::survive_slow_blocks`).
pub(super) const OVERLOAD_SECS: f64 = 0.06;
/// How long it then outputs silence without processing, so the thread catches up and sleeps.
pub(super) const SHED_SECS: f64 = 0.25;
/// How long the CPU meter's peak stays put after a block sets it, and the time constant of its
/// fall after that.
const PEAK_HOLD_SECS: f64 = 2.0;
const PEAK_FALL_SECS: f64 = 0.25;

/// Single blocks' load, which the smoothed average hides: one block over 1.0 is already a dropout.
/// The hold runs in audio time, counted from the blocks themselves.
#[derive(Default)]
pub(super) struct BlockLoad {
    pub(super) peak: f32,
    pub(super) held_secs: f64,
    pub(super) late: u32,
}

impl BlockLoad {
    /// Takes in a block that used `load` of its `secs` of audio.
    pub(super) fn block(&mut self, load: f32, secs: f64) {
        if load > 1.0 {
            self.late += 1;
        }
        if load >= self.peak {
            self.peak = load;
            self.held_secs = 0.0;
            return;
        }
        self.held_secs += secs;
        if self.held_secs > PEAK_HOLD_SECS {
            self.peak = (self.peak * (-secs / PEAK_FALL_SECS).exp() as f32).max(load);
        }
    }
}
