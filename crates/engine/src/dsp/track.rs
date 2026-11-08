//! One strip's state on the audio thread: [`TrackDsp`], with its regions, inserts, instrument and
//! parameters, and what those are made of ([`Plugin`], [`InsertDsp`], [`RegionDsp`]).
//!
//! The session builds a `TrackDsp` on its own thread (`build_track`) and sends it over in a `Cmd`,
//! with every buffer allocated. `strip.rs` plays one for a block. A plug-in that puts out NaNs or
//! runaway levels is contained here (`check_output`).

use super::*;

/// What an insert or instrument slot runs: a VST3's processor, or a built-in.
pub enum Plugin {
    Vst(Box<PluginProcessor>),
    /// A built-in instrument (see `crate::instrument`).
    Instrument(Box<InstrumentDsp>),
    /// A built-in effect (see `crate::fx`).
    Builtin(Box<BuiltinDsp>),
}

/// The loudest sample a plug-in may hand on, +60 dBFS: anything past it is a plug-in blowing up.
pub(super) const RUNAWAY: f32 = 1000.0;

/// Silences NaNs and clamps infinite or runaway samples, so one unstable plug-in can't poison
/// everything after it (a NaN never leaves a filter's state, and huge values overflow
/// detectors into NaN) or reach the device. Returns whether anything had to be fixed.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // The test below must also catch NaN, so it can't be `>`.
pub(super) fn contain(l: &mut [f32], r: &mut [f32]) -> bool {
    let mut bad = false;
    for s in l.iter_mut().chain(r.iter_mut()) {
        // NaN fails the comparison too.
        if !(s.abs() <= RUNAWAY) {
            bad = true;
            *s = if s.is_nan() { 0.0 } else { s.clamp(-RUNAWAY, RUNAWAY) };
        }
    }
    bad
}

/// Contains what a plug-in just produced, and reports it. A built-in effect or instrument that
/// went unstable is also cleared, so it recovers.
pub(super) fn check_output(status: &EngineStatus, ins: &mut InsertDsp, l: &mut [f32], r: &mut [f32]) {
    if contain(l, r) {
        status.bad_audio_uid.store(ins.uid, Ordering::Relaxed);
        status.bad_audio.fetch_add(1, Ordering::Relaxed);
        match &mut ins.plugin {
            Plugin::Builtin(b) => b.reset(),
            Plugin::Instrument(i) => i.reset(),
            Plugin::Vst(_) => {}
        }
    }
}

/// What a built-in effect is told about the transport.
pub(super) fn fx_context(t: &TransportInfo) -> crate::fx::FxContext {
    crate::fx::FxContext { sample_rate: t.sample_rate as f32, tempo: t.tempo, playing: t.playing, position: t.position_quarters }
}

impl Plugin {
    pub(super) fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], t: &TransportInfo) {
        match self {
            Plugin::Vst(p) => p.process(l, r, midi, t),
            Plugin::Instrument(i) => i.process(l, r, midi, &fx_context(t)),
            Plugin::Builtin(b) => b.process(l, r, None, &fx_context(t)),
        }
    }

    /// Queues an automation value (normalized 0..1) for the next block. A built-in effect or
    /// instrument takes the block's first value and glides on its own.
    pub(super) fn set_param(&mut self, id: u32, offset: i32, value: f64) {
        match self {
            Plugin::Vst(p) => p.queue_param(id, offset, value),
            Plugin::Builtin(b) if offset == 0 => b.set_normalized(id, value as f32),
            Plugin::Instrument(i) if offset == 0 => i.set_normalized(id, value as f32),
            _ => {}
        }
    }
}

/// A plug-in in a slot, with its uid, bypass state and the side-chain key it reads.
pub struct InsertDsp {
    pub uid: Id,
    pub bypass: bool,
    pub plugin: Plugin,
    /// Where its side-chain key comes from.
    pub key: KeyRoute,
}

/// An insert's side-chain source (`PluginSlot::sidechain`), for built-in effects and VST3s alike.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyRoute {
    /// The track whose output keys the plug-in.
    pub source: Option<Id>,
    /// The source's index in the engine's list, kept up to date by `Engine::resolve_routes`.
    pub(super) idx: Option<usize>,
    /// The key is the source's signal before its fader (`PluginSlot::sidechain_pre`).
    pub pre: bool,
}

impl KeyRoute {
    pub fn new(source: Option<Id>, pre: bool) -> KeyRoute {
        KeyRoute { source, idx: None, pre }
    }
}

/// What `Session::sync_vst_keys` tells a VST3's processor while it changes the plug-in's buses.
#[derive(Debug, Clone, Copy)]
pub enum VstGate {
    /// Fade to dry and stop.
    Close,
    /// Stop now, for a plug-in that isn't running or didn't finish fading.
    Shut,
    /// Run again, fading in after this many frames (its latency).
    Open(u32),
}

/// A region as the engine plays it: positions in frames, with its clip or notes shared by `Arc`. The
/// session makes these from the document (`regions_dsp`), expanding loops, comps and patterns.
pub enum RegionDsp {
    Audio {
        start: i64,
        length: i64,
        offset: i64,
        gain: f32,
        clip: Arc<Clip>,
        /// Timeline (start, length) of the region's fade-in and fade-out; length 0 = none.
        fade_in: (i64, i64),
        fade_out: (i64, i64),
        /// The gain laws of the fade-in and fade-out (see `fades.rs`).
        laws: [FadeLaw; 2],
    },
    Midi {
        start: i64,
        length: i64,
        notes: Arc<Vec<Note>>,
        cc: Arc<Vec<CcEvent>>,
    },
}

/// A strip's parameters that change without rebuilding it, in plain values. Small and `Copy`, so a fader
/// drag is one small `Cmd`.
#[derive(Debug, Clone, Copy)]
pub struct TrackParams {
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub monitor: bool,
    pub input: InputSel,
    /// The summing stack or aux this track plays through (None = Stereo Out).
    pub output: Option<Id>,
    /// Per send slot: the aux it feeds and at what gain.
    pub sends: [Option<SendParams>; SEND_SLOTS],
    /// The part of `volume` and `mute` that comes from the folder stacks the track is in and
    /// its VCA, which still applies when automation drives the track's own volume and mute.
    pub stack_gain: f32,
    pub stack_mute: bool,
    /// The nearest folder stack the track is in. Automation on it (or on a folder above it)
    /// replaces `stack_gain` and `stack_mute` while playing.
    pub folder: Option<Id>,
    /// The track's own fader gain and mute, and the mute of the summing stacks it is in:
    /// what `volume` and `mute` are made of again when a folder's automation plays.
    pub own_gain: f32,
    pub own_mute: bool,
    pub bus_mute: bool,
    /// The track's VCA offset (gain) and mute, which a folder's automation leaves in place.
    pub vca_gain: f32,
    pub vca_mute: bool,
    /// The track or a stack it is in is switched off (`Track::off`): it runs nothing at all.
    pub off: bool,
    /// For an aux: the instrument output that feeds it.
    pub instrument_output: Option<InstrumentOutput>,
}

/// A send as the engine plays it. A muted send keeps its slot at gain 0, so it fades out
/// instead of clicking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SendParams {
    pub bus: Id,
    pub gain: f32,
    pub pre_fader: bool,
}

/// One strip on the audio thread: everything it plays and the buffers it plays into, all allocated up
/// front. See `strip.rs` for a block's work and `track_off.rs` for switching it off.
pub struct TrackDsp {
    pub id: Id,
    pub kind: TrackKind,
    pub params: TrackParams,
    pub meter: Arc<Meter>,
    pub(super) regions: Arc<Vec<RegionDsp>>,
    pub(super) inserts: Vec<Option<InsertDsp>>,
    pub(super) instrument: Option<InsertDsp>,
    pub(super) gains: (f32, f32),
    pub(super) l: Vec<f32>,
    pub(super) r: Vec<f32>,
    /// What subtracks sent this block, for summing stacks.
    pub(super) sum_l: Vec<f32>,
    pub(super) sum_r: Vec<f32>,
    /// Index in the engine's list of the track `params.output` names.
    pub(super) out_idx: Option<usize>,
    pub(super) midi: Vec<MidiEvent>,
    pub(super) rec: Option<rtrb::Producer<f32>>,
    pub(super) sounding: [bool; 128],
    pub(super) flush_notes: bool,
    pub(super) eq: EqCoeffs,
    pub(super) eq_state: EqState,
    /// Index in the engine's list of each send's aux, kept up to date by `resolve_routes()`.
    pub(super) send_idx: [Option<usize>; SEND_SLOTS],
    /// Smoothed send gains, like `gains` for the fader.
    pub(super) send_gains: [f32; SEND_SLOTS],
    pub(super) automation: Arc<Vec<LaneDsp>>,
    pub(super) auto_mode: AutoMode,
    /// Parameters held by the user while writing in Touch or Latch: their live value wins.
    pub(super) touched: [Option<AutoTarget>; MAX_TOUCHED],
    /// The value last queued for each plug-in lane, so unchanged values aren't resent.
    pub(super) auto_sent: [f32; MAX_AUTO_SENT],
    /// Send the controller values in effect at the playhead before the next block plays, so a
    /// region started in the middle sounds as it would from its start.
    pub(super) chase_cc: bool,
    /// Controllers were played since the last flush, so the flush resets sustain and bend.
    pub(super) cc_sent: bool,
    /// Plug-in delay compensation (see `pdc.rs`).
    pub(super) pdc: PdcParams,
    pub(super) delay: Option<Box<DelayLine>>,
    /// Another strip's insert is keyed by this track: its post-fader output is kept
    /// here for the block (set by `resolve_routes`).
    pub(super) keyed: bool,
    pub(super) key_l: Vec<f32>,
    pub(super) key_r: Vec<f32>,
    /// Like `keyed`, for side-chains that take the signal before the fader.
    pub(super) keyed_pre: bool,
    pub(super) pre_key_l: Vec<f32>,
    pub(super) pre_key_r: Vec<f32>,
    /// Index in the engine's list of `params.folder` (kept up to date by `resolve_routes()`).
    pub(super) fold_idx: Option<usize>,
    /// Where the output and each send read the delay line, gliding when a delay changes.
    pub(super) pdc_taps: pdc::PdcTaps,
    /// Switched off and faded out: nothing of the strip runs (see `track_off.rs`).
    pub(super) dormant: bool,
    /// Frames of the fade-in left since it was switched back on.
    pub(super) resume_left: u32,
    /// Notes the player is holding through MIDI Thru, released if the track goes off meanwhile.
    pub(super) live_held: [bool; 128],
    /// The instrument played this block, so its outputs hold this block's audio.
    pub(super) instrument_ran: bool,
    /// For an aux on an instrument's extra output: where that comes from.
    pub(super) feed: multi_out::OutputFeed,
    /// What the strip played this block when it ran on the pool (see `parallel.rs`).
    pub(super) outs: Outlets,
    /// How long the strip's last block took, in seconds.
    pub(super) spent: f32,
    /// What its folder does to it this block, read before the pool started.
    pub(super) fold_now: auto::FoldState,
    /// It sits out every block: its plug-in stopped responding on a worker, and this is the stand-in
    /// for it, or it's fed by one (see `Parallel::leave_out`). Until the plug-in returns
    /// (`Cmd::Back`) or the strips are rebuilt.
    pub(super) left_out: bool,
    /// It's such a stand-in, which only ever sits out, and the token `EngineStatus::hold` gave the
    /// strip it stands in for.
    pub(super) stand_in: bool,
    pub(super) held_token: u64,
}

impl TrackDsp {
    pub fn new(id: Id, kind: TrackKind, params: TrackParams, meter: Arc<Meter>) -> TrackDsp {
        TrackDsp {
            id,
            kind,
            params,
            meter,
            regions: Arc::new(Vec::new()),
            inserts: (0..INSERT_SLOTS).map(|_| None).collect(),
            instrument: None,
            gains: (0.0, 0.0),
            l: vec![0.0; MAX_BLOCK],
            r: vec![0.0; MAX_BLOCK],
            sum_l: vec![0.0; MAX_BLOCK],
            sum_r: vec![0.0; MAX_BLOCK],
            out_idx: None,
            midi: Vec::with_capacity(MAX_MIDI),
            rec: None,
            sounding: [false; 128],
            flush_notes: false,
            eq: EqCoeffs::default(),
            eq_state: EqState::default(),
            send_idx: [None; SEND_SLOTS],
            send_gains: [0.0; SEND_SLOTS],
            automation: Arc::new(Vec::new()),
            auto_mode: AutoMode::Read,
            touched: [None; MAX_TOUCHED],
            auto_sent: [f32::NAN; MAX_AUTO_SENT],
            chase_cc: true,
            cc_sent: false,
            pdc: PdcParams::default(),
            delay: None,
            keyed: false,
            key_l: vec![0.0; MAX_BLOCK],
            key_r: vec![0.0; MAX_BLOCK],
            keyed_pre: false,
            pre_key_l: vec![0.0; MAX_BLOCK],
            pre_key_r: vec![0.0; MAX_BLOCK],
            fold_idx: None,
            pdc_taps: pdc::PdcTaps::default(),
            dormant: false,
            resume_left: 0,
            live_held: [false; 128],
            instrument_ran: false,
            feed: Default::default(),
            outs: Outlets::new(),
            spent: 0.0,
            fold_now: Default::default(),
            left_out: false,
            stand_in: false,
            held_token: 0,
        }
    }

    /// Leaves the strip out from now on (`left_out`). What effects keyed from it read goes silent.
    pub(super) fn sit_out(&mut self) {
        self.left_out = true;
        for key in [&mut self.key_l, &mut self.key_r, &mut self.pre_key_l, &mut self.pre_key_r] {
            key.fill(0.0);
        }
    }

    /// Back from sitting out, as a track switched back on comes back: notes and controllers
    /// chased afresh, automation resent, and a fade-in of `fade` frames.
    pub(super) fn come_back(&mut self, fade: u32) {
        self.left_out = false;
        self.resume_left = fade;
        self.flush_notes = true;
        self.chase_cc = true;
        self.auto_sent = [f32::NAN; MAX_AUTO_SENT];
    }

    pub fn set_automation(&mut self, lanes: Arc<Vec<LaneDsp>>, mode: AutoMode) {
        self.automation = lanes;
        self.auto_mode = mode;
        self.auto_sent = [f32::NAN; MAX_AUTO_SENT];
    }

    pub(super) fn set_touched(&mut self, target: AutoTarget, on: bool) {
        let at = self.touched.iter().position(|t| *t == Some(target));
        match (on, at) {
            (true, None) => {
                if let Some(free) = self.touched.iter_mut().find(|t| t.is_none()) {
                    *free = Some(target);
                }
            }
            (false, Some(i)) => self.touched[i] = None,
            _ => {}
        }
        self.auto_sent = [f32::NAN; MAX_AUTO_SENT];
    }

    pub fn set_eq(&mut self, c: EqCoeffs) {
        if !c.active {
            self.eq_state.reset();
        }
        self.eq = c;
    }

    pub fn set_regions(&mut self, r: Arc<Vec<RegionDsp>>) {
        self.regions = r;
    }

    pub fn set_insert(&mut self, slot: usize, ins: Option<InsertDsp>) {
        if slot < self.inserts.len() {
            self.inserts[slot] = ins;
        }
    }

    pub fn set_instrument(&mut self, ins: Option<InsertDsp>) {
        self.instrument = ins;
    }
}
