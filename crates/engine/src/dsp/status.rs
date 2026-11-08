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
        (f32::from_bits(self.l.swap(0, Ordering::Relaxed)), f32::from_bits(self.r.swap(0, Ordering::Relaxed)))
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
    /// Commands the session gave up sending because the engine took none for a while with the
    /// queue full (see `Session::send`). The session counts these, not the audio thread.
    pub cmds_dropped: AtomicU64,
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
    /// For a stuck worker: whether it was left in a plug-in, its track and what that feeds left out
    /// and the rest playing on (see `Parallel::leave_out`, which reports that in `left_events`).
    pub overload_left_out: AtomicBool,
    /// Tracks sitting out since a plug-in stopped responding (see `TrackDsp::left_out`).
    pub left_out: AtomicU32,
    /// Times tracks were left out, and the track and the plug-in's uid (0: unknown) of the latest.
    pub left_events: AtomicU32,
    pub left_track: AtomicU64,
    pub left_plugin: AtomicU64,
    /// The strips threads are still holding in plug-ins that stopped responding.
    pub held: [Held; MAX_HELD],
    /// The last token `hold` handed out, and strips it couldn't list.
    held_tokens: AtomicU64,
    held_unlisted: AtomicU32,
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
            cmds_dropped: AtomicU64::new(0),
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
            overload_left_out: AtomicBool::new(false),
            left_out: AtomicU32::new(0),
            left_events: AtomicU32::new(0),
            left_track: AtomicU64::new(0),
            left_plugin: AtomicU64::new(0),
            held: std::array::from_fn(|_| Held::default()),
            held_tokens: AtomicU64::new(0),
            held_unlisted: AtomicU32::new(0),
            threads: AtomicU32::new(1),
            parallel_blocks: AtomicU64::new(0),
        }
    }
}

/// The most strips threads can hold at once: one per worker of a pool, and a pool replaced while one
/// is held keeps it.
pub const MAX_HELD: usize = 32;

/// A strip a thread holds in a plug-in that stopped responding: the token `hold` gave it (0: a free
/// entry), its track, the plug-in's uid (0: unknown), how many VST3 instances had been made by
/// then, whether the stand-in that took the strip's place has gone since (its track deleted), and
/// whether the thread has let go of it since, for the session to bring the track back
/// (`EngineStatus::take_back`). The token, never handed out twice, tells this strip from one a
/// thread holds for the same track later, and from one whose entry `release_all` freed.
#[derive(Default)]
pub struct Held {
    token: AtomicU64,
    track: AtomicU64,
    plugin: AtomicU64,
    before: AtomicU64,
    stand_in_gone: AtomicBool,
    back: AtomicBool,
}

/// What `EngineStatus::held` and `take_back` say of a strip a thread holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldStrip {
    pub token: u64,
    pub track: Id,
    /// The plug-in's uid (0: unknown, any of the track's).
    pub plugin: Id,
    /// `vst3_host::instances_made()` when it was held: only an instance numbered up to this can be
    /// the one held. A track's plug-ins made since (an undo bringing it back) are others.
    pub before: u64,
    /// The stand-in that took its place in the engine has gone: the track was deleted since, and
    /// whatever strip it has now isn't the one held.
    pub stand_in_gone: bool,
}

impl Held {
    fn read(&self, token: u64) -> HeldStrip {
        HeldStrip {
            token,
            track: self.track.load(Ordering::Relaxed),
            plugin: self.plugin.load(Ordering::Relaxed),
            before: self.before.load(Ordering::Relaxed),
            stand_in_gone: self.stand_in_gone.load(Ordering::Acquire),
        }
    }
}

impl EngineStatus {
    /// The strips threads still hold.
    pub fn held(&self) -> Vec<HeldStrip> {
        let held = self.held.iter().map(|h| (h, h.token.load(Ordering::Acquire))).filter(|(h, t)| *t != 0 && !h.back.load(Ordering::Acquire));
        held.map(|(h, t)| h.read(t)).collect()
    }

    /// The strips threads have let go of since the last call, which frees their entries. For the
    /// session only: only it frees entries, so none it reads changes under it.
    pub fn take_back(&self) -> Vec<HeldStrip> {
        let mut back = Vec::new();
        for h in &self.held {
            let token = h.token.load(Ordering::Acquire);
            if token != 0 && h.back.load(Ordering::Acquire) {
                back.push(h.read(token));
                h.back.store(false, Ordering::Relaxed);
                h.token.store(0, Ordering::Release);
            }
        }
        back
    }

    /// Strips that couldn't be listed because the list was full (see `hold`), for the log.
    pub fn held_unlisted(&self) -> u32 {
        self.held_unlisted.load(Ordering::Relaxed)
    }

    /// Lists a strip a thread now holds, and returns its token for `release` (0 when the list is
    /// full, which takes more pools replaced while threads hold strips than there are: the track
    /// just isn't listed, and `held_unlisted` counts it).
    pub(super) fn hold(&self, track: Id, plugin: Id) -> u64 {
        let Some(h) = self.held.iter().find(|h| h.token.load(Ordering::Acquire) == 0) else {
            self.held_unlisted.fetch_add(1, Ordering::Relaxed);
            return 0;
        };
        let token = self.held_tokens.fetch_add(1, Ordering::Relaxed) + 1;
        h.track.store(track, Ordering::Relaxed);
        h.plugin.store(plugin, Ordering::Relaxed);
        h.before.store(vst3_host::instances_made(), Ordering::Relaxed);
        h.stand_in_gone.store(false, Ordering::Relaxed);
        h.back.store(false, Ordering::Relaxed);
        h.token.store(token, Ordering::Release);
        token
    }

    fn entry(&self, token: u64) -> Option<&Held> {
        (token != 0).then(|| self.held.iter().find(|h| h.token.load(Ordering::Relaxed) == token)).flatten()
    }

    /// The thread has let go of the strip `hold` gave `token` for. Nothing if its entry was freed
    /// meanwhile (`release_all`).
    pub(super) fn release(&self, token: u64) {
        if let Some(h) = self.entry(token) {
            h.back.store(true, Ordering::Release);
        }
    }

    /// The stand-in for the strip `hold` gave `token` for has left the engine (its track deleted).
    pub(super) fn stand_in_gone(&self, token: u64) {
        if let Some(h) = self.entry(token) {
            h.stand_in_gone.store(true, Ordering::Release);
        }
    }

    /// The strips were all rebuilt: whatever threads still hold is no track's any more.
    pub(super) fn release_all(&self) {
        for h in &self.held {
            h.back.store(false, Ordering::Relaxed);
            h.token.store(0, Ordering::Release);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[HeldStrip]) -> Vec<(Id, Id)> {
        v.iter().map(|h| (h.track, h.plugin)).collect()
    }

    /// A track left, deleted and brought back by undo, then left again: two threads hold strips of
    /// it, and the one that lets go is the one marked.
    #[test]
    fn a_strip_let_go_of_is_told_from_another_held_for_the_same_track() {
        let s = EngineStatus::default();
        let first = s.hold(2, 99);
        let second = s.hold(2, 99);
        assert_ne!(first, second);
        // The first one's stand-in went with its track.
        s.stand_in_gone(first);
        s.release(second);
        assert_eq!(ids(&s.held()), [(2, 99)], "the first is still held");
        assert_eq!(s.held()[0].token, first);
        let back = s.take_back();
        assert_eq!((back.len(), back[0].token, back[0].stand_in_gone), (1, second, false));
        assert_eq!(ids(&s.held()), [(2, 99)]);
        assert!(s.take_back().is_empty());
        s.release(first);
        assert!(s.held().is_empty());
        let back = s.take_back();
        assert_eq!((back.len(), back[0].token, back[0].stand_in_gone), (1, first, true));
    }

    /// Entries freed when the strips are rebuilt aren't marked when their threads let go later, even
    /// once the entry holds another strip.
    #[test]
    fn a_strip_let_go_of_after_a_rebuild_marks_nothing() {
        let s = EngineStatus::default();
        let old = s.hold(2, 99);
        s.release_all();
        assert!(s.held().is_empty());
        let new = s.hold(3, 98);
        s.release(old);
        assert_eq!(ids(&s.held()), [(3, 98)]);
        assert!(s.take_back().is_empty());
        s.release(new);
        assert_eq!(ids(&s.take_back()), [(3, 98)]);
        // And a full list hands out no token, which marks nothing.
        let tokens: Vec<u64> = (0..MAX_HELD as Id + 1).map(|t| s.hold(t + 10, 0)).collect();
        assert_eq!(tokens.iter().filter(|&&t| t == 0).count(), 1);
        assert_eq!(s.held_unlisted(), 1, "and it's counted");
        s.release(0);
        assert!(s.take_back().is_empty());
    }
}
