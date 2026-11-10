//! Marbble Onda: a modern wavetable synth of the kind that Serum made the standard (this project isn't
//! affiliated with Xfer Records, and the product name stays clear of theirs). It isn't a copy of any
//! one instrument's panel: it's the architecture: two wavetable oscillators with position scan and
//! unison, a sub and noise, a choice of filters, three envelopes, two LFOs and a modulation matrix.
//! Its wavetables are generated in code (`tables.rs`).
//!
//! **Signal path, per voice, at twice the output rate below 64 kHz:** each oscillator is up to 7
//! detuned copies playing a mip level of two neighbouring frames, crossfaded by the position. The
//! level is picked so nothing lies past the voice's Nyquist, and what lands above the audio band is
//! taken out by the decimator: so aliasing is measured rather than assumed. The copies pan into two
//! buses, each with its own filter (stereo for real, per voice).
//!
//! **Modulation** is worked out every [`CONTROL_TICKS`] ticks and ramped between, so a scanning
//! position has no steps in it: the matrix's slots each take one source, one destination and an
//! amount. Env 1 is the VCA. Env 2, Env 3 and the LFOs do nothing until a slot routes them.
//!
//! **Free-running LFOs** count ticks from the last reset, and keep doing so at rest, without work.
//! **Tempo-synced LFOs** read the song position at the start of a block (`crate::synth_dsp::lfo`'s
//! way), and advance sample by sample between, so their values may differ across block sizes by
//! rounding (measured under 1e-5 in the output), not bit for bit. All else is bit-identical.

use vst3_host::MidiEvent;

use super::common::{dc_pole, hz_to_note, OutStage, Rest};
use super::{FxContext, Instrument, InstrumentInfo, Midi};
use crate::synth_dsp::alloc::{Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::analog::sub_seed;
use crate::synth_dsp::env::EnvShape;
use crate::synth_dsp::filter::cutoff_g;
use crate::synth_dsp::smooth::Smoothed;

pub mod params;
mod presets;
pub mod tables;
mod voice;

use params::param::*;
pub use params::{PARAMS, SECTIONS};
pub use presets::PRESETS;
use tables::{bank, Bank};
use voice::*;

pub const VOICES: usize = 16;
const UNISON_MAX: usize = 7;
/// Ticks between modulation updates.
pub const CONTROL_TICKS: u32 = 16;
const FILTER_REF_NOTE: f32 = 60.0;
/// Detune at 100 %, the outermost copy from the centre, in semitones. A judgement, wide as the
/// genre likes it.
const DETUNE_ST: f32 = 0.6;
/// A slot's reach at amount 1, for each destination.
const POS_REACH: f32 = 1.0;
const PITCH_REACH_ST: f32 = 48.0;
const CUTOFF_REACH_ST: f32 = 84.0;
/// FM from B to A at 100 %, in cycles of A's phase. A judgement.
const FM_CYCLES: f32 = 2.0;
const OSC_GAIN: f32 = 0.45;
const NOISE_GAIN: f32 = 0.4;
const SUB_GAIN: f32 = 0.5;
const OUTPUT_GAIN: f32 = 1.25;
const DC_HZ: f32 = 5.0;
const HOLD_SECS: f32 = 0.05;
const SMOOTH_MS: f32 = 8.0;

/// A source's value this control tick, by the slot's source index.
#[derive(Debug, Clone, Copy, Default)]
struct Sources([f32; 9]);

/// What the panel's slots do, in the voice's terms.
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    source: usize,
    dest: usize,
    amount: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct OscPatch {
    on: bool,
    table: usize,
    pos: f32,
    /// Octave, semitone and fine together.
    st: f32,
    level: f32,
    pan: f32,
    copies: usize,
    detune: f32,
    blend: f32,
    spread: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FilterKind {
    #[default]
    Low12,
    Low24,
    Band12,
    High12,
    Notch,
    Ladder24,
    Diode,
}

#[derive(Debug, Clone, Copy)]
struct Patch {
    rate: f32,
    osc: [OscPatch; 2],
    sub: f32,
    noise: f32,
    fm: f32,
    filter_on: bool,
    filter: FilterKind,
    cutoff_note: f32,
    res: f32,
    drive: f32,
    key_track: f32,
    env: [EnvShape; 3],
    slots: [Slot; MOD_SLOTS as usize],
    glide_secs: f32,
    noise_scale: f32,
}

const fn sync_rate(i: usize) -> f64 {
    // Cycles per beat.
    match i {
        1 => 1.0 / 16.0,
        2 => 1.0 / 8.0,
        3 => 1.0 / 4.0,
        4 => 0.5,
        5 => 1.0,
        6 => 2.0,
        7 => 4.0,
        8 => 3.0,
        9 => 6.0,
        _ => 0.0,
    }
}

/// An instance.
pub struct Onda {
    os: usize,
    sr: f32,
    seed: u64,
    values: Box<[f32; params::param::COUNT]>,
    dirty: bool,
    patch: Patch,
    voices: Box<[Voice; VOICES]>,
    alloc: VoiceAllocator<VOICES>,
    bank: &'static Bank,
    wheel: Smoothed,
    touch: Smoothed,
    bend_s: Smoothed,
    expr: Smoothed,
    volume: Smoothed,
    /// LFOs that run for the whole instance.
    lfo: [LfoState; 2],
    lfo_hz: [f32; 2],
    lfo_sync: [usize; 2],
    shared: Shared,
    out: [OutStage; 2],
    dc_r: f32,
    rest: Rest,
    hold_len: u32,
}

impl Onda {
    pub fn new(sample_rate: f32, seed: u64) -> Onda {
        let os = if sample_rate < 64000.0 { 2 } else { 1 };
        let rate = sample_rate * os as f32;
        let patch = Patch {
            rate,
            osc: [OscPatch::default(); 2],
            sub: 0.0,
            noise: 0.0,
            fm: 0.0,
            filter_on: true,
            filter: FilterKind::Low24,
            cutoff_note: 100.0,
            res: 0.0,
            drive: 0.0,
            key_track: 0.0,
            env: [EnvShape::default(); 3],
            slots: [Slot::default(); MOD_SLOTS as usize],
            glide_secs: 0.0,
            noise_scale: 1.0 / (os as f32).sqrt(),
        };
        let voices: Box<[Voice; VOICES]> = Box::new(std::array::from_fn(|i| Voice::new(seed, i, &patch)));
        let mut t = Onda {
            os,
            sr: sample_rate,
            seed,
            values: Box::new(params::defaults()),
            dirty: true,
            patch,
            voices,
            alloc: VoiceAllocator::new(),
            bank: bank(),
            wheel: Smoothed::new(0.0, SMOOTH_MS, sample_rate),
            touch: Smoothed::new(0.0, SMOOTH_MS, sample_rate),
            bend_s: Smoothed::new(0.0, 2.0, sample_rate),
            expr: Smoothed::new(1.0, SMOOTH_MS, sample_rate),
            volume: Smoothed::new(0.0, SMOOTH_MS, sample_rate),
            lfo: [LfoState::new(sub_seed(seed, 71)), LfoState::new(sub_seed(seed, 72))],
            lfo_hz: [1.0; 2],
            lfo_sync: [0; 2],
            shared: Shared::default(),
            out: [OutStage::new(); 2],
            dc_r: dc_pole(DC_HZ, sample_rate),
            rest: Rest::new(),
            hold_len: (HOLD_SECS * sample_rate) as u32,
        };
        t.apply_patch();
        t.volume.snap();
        t
    }

    fn apply_patch(&mut self) {
        self.dirty = false;
        let g = |id: u32| self.values[id as usize];
        let ms = 0.001;
        let rate = self.patch.rate;
        let osc = |first: u32| -> OscPatch {
            let o = |id: u32| g(first + id - A_ON);
            OscPatch {
                on: o(A_ON) >= 0.5,
                table: o(A_TABLE) as usize,
                pos: o(A_POS),
                st: o(A_OCT).round() * 12.0 + o(A_SEMI).round() + o(A_FINE),
                level: o(A_LEVEL),
                pan: o(A_PAN),
                copies: o(A_VOICES) as usize + 1,
                detune: o(A_DETUNE),
                blend: o(A_BLEND),
                spread: o(A_SPREAD),
            }
        };
        self.patch.osc = [osc(A_ON), osc(B_ON)];
        self.patch.sub = g(SUB_LEVEL) * SUB_GAIN;
        self.patch.noise = g(NOISE_LEVEL) * NOISE_GAIN;
        self.patch.fm = g(FM_AMOUNT);
        self.patch.filter_on = g(FILTER_ON) >= 0.5;
        self.patch.filter = match g(FILTER_TYPE) as usize {
            0 => FilterKind::Low12,
            1 => FilterKind::Low24,
            2 => FilterKind::Band12,
            3 => FilterKind::High12,
            4 => FilterKind::Notch,
            5 => FilterKind::Ladder24,
            _ => FilterKind::Diode,
        };
        self.patch.cutoff_note = hz_to_note(g(CUTOFF));
        self.patch.res = g(RESONANCE);
        self.patch.drive = g(DRIVE);
        self.patch.key_track = g(KEY_TRACK);
        let env = |a: u32, d: u32, s: u32, r: u32| EnvShape::new(g(a) * ms, g(d) * ms, g(s), g(r) * ms, rate);
        self.patch.env = [
            env(E1_ATTACK, E1_DECAY, E1_SUSTAIN, E1_RELEASE),
            env(E2_ATTACK, E2_DECAY, E2_SUSTAIN, E2_RELEASE),
            env(E3_ATTACK, E3_DECAY, E3_SUSTAIN, E3_RELEASE),
        ];
        for s in 0..MOD_SLOTS {
            let b = MOD_BASE + 3 * s;
            self.patch.slots[s as usize] = Slot { source: g(b) as usize, dest: g(b + 1) as usize, amount: g(b + 2) };
        }
        let glide = g(GLIDE);
        self.patch.glide_secs = if glide <= 1.0 { 0.0 } else { glide * ms };
        self.lfo_hz = [g(L1_RATE), g(L2_RATE)];
        self.lfo_sync = [g(L1_SYNC) as usize, g(L2_SYNC) as usize];
        self.shared.lfo_wave = [g(L1_WAVE) as usize, g(L2_WAVE) as usize];
        self.shared.lfo_retrig = [g(L1_RETRIG) >= 0.5, g(L2_RETRIG) >= 0.5];
        let l = g(VOLUME) / 10.0;
        self.volume.target = l * l * OUTPUT_GAIN;
        let (mode, poly) = match g(VOICE_MODE) as usize {
            0 => (VoiceMode::Poly, (g(POLYPHONY).round() as usize).clamp(1, VOICES)),
            1 => (VoiceMode::Mono { priority: Priority::Last, multi_trigger: true }, 1),
            _ => (VoiceMode::Mono { priority: Priority::Last, multi_trigger: false }, 1),
        };
        let voices = &mut self.voices;
        self.alloc.set_mode(mode, 1, poly, |a| {
            if let VoiceAction::Release { voice } = a {
                voices[voice].release();
            }
        });
        for v in self.voices.iter_mut() {
            v.glide.set_time(self.patch.glide_secs, rate);
        }
        // A held voice takes a new unison layout at once.
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for o in 0..2 {
                v.osc[o].lay_out(&self.patch.osc[o]);
            }
        }
    }

    fn act(voices: &mut [Voice; VOICES], patch: &Patch, shared: &Shared, action: VoiceAction) {
        match action {
            VoiceAction::Start { voice, note, velocity, stolen, .. } => voices[voice].start(note, velocity, patch, shared, stolen),
            VoiceAction::Glide { voice, note } => voices[voice].glide_to(note),
            VoiceAction::Release { voice } => voices[voice].release(),
        }
    }

    fn handle(&mut self, m: Midi) {
        let (voices, patch, shared) = (&mut self.voices, &self.patch, &self.shared);
        match m {
            Midi::NoteOn { note, velocity } => {
                if self.rest.resting {
                    let gone = self.rest.wake() as f64;
                    // Free LFOs ran on while nothing played.
                    for k in 0..2 {
                        if self.lfo_sync[k] == 0 {
                            let inc = self.lfo_hz[k] as f64 / self.sr as f64;
                            let turned = self.lfo[k].phase + inc * gone;
                            self.lfo[k].phase = turned.fract();
                            if turned >= 1.0 {
                                self.lfo[k].held = self.lfo[k].rng.bipolar();
                            }
                        }
                    }
                }
                self.alloc.note_on(note, velocity, |a| Self::act(voices, patch, shared, a));
            }
            Midi::NoteOff { note } => self.alloc.note_off(note, |a| Self::act(voices, patch, shared, a)),
            Midi::Sustain(down) => self.alloc.set_pedal(down, |a| Self::act(voices, patch, shared, a)),
            Midi::AllNotesOff => self.alloc.release_all(|a| Self::act(voices, patch, shared, a)),
            Midi::PitchBend(b) => self.bend_s.target = b * self.values[BEND_RANGE as usize],
            Midi::ModWheel(w) => self.wheel.target = w,
            Midi::Aftertouch(a) => self.touch.target = a,
            Midi::Expression(e) => self.expr.target = e,
            Midi::ResetControllers => self.reset_controllers(),
            Midi::Other => {}
        }
    }

    fn reset_controllers(&mut self) {
        self.bend_s.target = 0.0;
        self.wheel.target = 0.0;
        self.touch.target = 0.0;
        self.expr.target = 1.0;
    }

    /// Per block: the sync'd LFOs read the song position.
    fn begin_block(&mut self, ctx: &FxContext) {
        for k in 0..2 {
            let per_beat = sync_rate(self.lfo_sync[k]);
            let inc = if per_beat > 0.0 {
                let inc = per_beat * ctx.tempo / 60.0 / self.sr as f64;
                if ctx.playing {
                    let want = (ctx.position * per_beat).rem_euclid(1.0);
                    if want < self.lfo[k].phase && self.lfo[k].phase - want > 0.5 {
                        self.lfo[k].held = self.lfo[k].rng.bipolar();
                    }
                    self.lfo[k].phase = want;
                }
                inc
            } else {
                self.lfo_hz[k] as f64 / self.sr as f64
            };
            // Per voice control tick.
            self.shared.lfo_inc[k] = inc * CONTROL_TICKS as f64 / self.os as f64;
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        if self.rest.resting {
            l.fill(0.0);
            r.fill(0.0);
            self.rest.samples += n as u64;
            self.wheel.snap();
            self.touch.snap();
            self.bend_s.snap();
            self.expr.snap();
            self.volume.snap();
            return;
        }
        let os = self.os;
        // An LFO moves at the rate of the voice's control tick, one per CONTROL_TICKS / os samples.
        let lfo_samples_per = CONTROL_TICKS as f64 / os as f64;
        for i in 0..n {
            self.shared.bend = self.bend_s.tick();
            self.shared.wheel = self.wheel.tick();
            self.shared.touch = self.touch.tick();
            // The instance-wide LFOs, every sample.
            for k in 0..2 {
                self.shared.lfo_global[k] = self.lfo[k].value(self.shared.lfo_wave[k]);
                self.lfo[k].advance(self.shared.lfo_inc[k] / lfo_samples_per);
            }
            let any = self.voices.iter().any(|v| v.active);
            let (mut al, mut ar) = (0.0f32, 0.0f32);
            if os == 2 {
                let (mut l0, mut r0, mut l1, mut r1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                if any {
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.tick(&self.patch, &self.shared, self.bank, &mut l0, &mut r0);
                    }
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.tick(&self.patch, &self.shared, self.bank, &mut l1, &mut r1);
                    }
                }
                al = self.out[0].dec.process(l0, l1);
                ar = self.out[1].dec.process(r0, r1);
            } else if any {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.tick(&self.patch, &self.shared, self.bank, &mut al, &mut ar);
                }
            }
            let g = self.volume.tick() * self.expr.tick();
            l[i] = self.out[0].block_dc(al, self.dc_r) * g;
            r[i] = self.out[1].block_dc(ar, self.dc_r) * g;
            for (k, v) in self.voices.iter().enumerate() {
                if !v.active && self.alloc.is_busy(k) {
                    self.alloc.voice_finished(k);
                }
            }
            if any {
                self.rest.hold = self.hold_len;
            } else {
                self.rest.hold = self.rest.hold.saturating_sub(1);
                if self.rest.hold == 0 {
                    self.rest.resting = true;
                    self.out = [OutStage::new(); 2];
                    for v in self.voices.iter_mut() {
                        v.filter = {
                            let mut f = FilterPair::default();
                            let hp = cutoff_g(120.0, self.patch.rate);
                            f.diode[0].set_feedback_highpass(hp);
                            f.diode[1].set_feedback_highpass(hp);
                            f
                        };
                    }
                    l[i + 1..].fill(0.0);
                    r[i + 1..].fill(0.0);
                    self.rest.samples += (n - i - 1) as u64;
                    return;
                }
            }
        }
    }
}

impl Instrument for Onda {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        self.dirty = true;
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], ctx: &FxContext) {
        if self.dirty {
            self.apply_patch();
        }
        self.begin_block(ctx);
        let n = l.len().min(r.len());
        let mut at = 0;
        for e in midi {
            let to = (e.offset as usize).clamp(at, n);
            self.render(&mut l[at..to], &mut r[at..to]);
            at = to;
            self.handle(Midi::parse(e.data));
        }
        self.render(&mut l[at..n], &mut r[at..n]);
    }

    fn all_notes_off(&mut self) {
        let (voices, patch, shared) = (&mut self.voices, &self.patch, &self.shared);
        self.alloc.release_all(|a| Self::act(voices, patch, shared, a));
    }

    fn reset(&mut self) {
        if self.dirty {
            self.apply_patch();
        }
        let (seed, patch) = (self.seed, self.patch);
        for (i, v) in self.voices.iter_mut().enumerate() {
            *v = Voice::new(seed, i, &patch);
            v.glide.set_time(patch.glide_secs, patch.rate);
        }
        self.alloc.reset();
        self.lfo = [LfoState::new(sub_seed(seed, 71)), LfoState::new(sub_seed(seed, 72))];
        self.out = [OutStage::new(); 2];
        self.rest = Rest::new();
        self.reset_controllers();
        self.wheel.snap();
        self.touch.snap();
        self.bend_s.snap();
        self.expr.snap();
        self.volume.snap();
    }

    fn tail_secs(&self) -> f32 {
        (self.values[E1_RELEASE as usize] * 0.001 * 3.84).min(60.0) + HOLD_SECS + 0.05
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: "builtin:onda",
        name: "Marbble Onda",
        group: "Modern Synths",
        about: "Two wavetable oscillators with position scan and unison, seven filters, three envelopes, two LFOs and a modulation matrix",
        params: PARAMS,
        sections: SECTIONS,
        meters: &[],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Onda::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
