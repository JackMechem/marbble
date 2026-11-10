//! One voice of Marbble Onda: its two wavetable oscillators with their unison copies, sub and noise,
//! the two filters, the envelopes and its own LFOs, ticking at the oversampled rate. What changes in
//! steps is in `Patch` (`mod.rs`); what every voice's control tick shares is in [`Shared`].

use super::*;
use crate::synth_dsp::alloc::unison_spread;
use crate::synth_dsp::analog::{sub_seed, Rng};
use crate::synth_dsp::env::Envelope;
use crate::synth_dsp::filter::{cutoff_g, DiodeLadder, Ladder, Svf, ZdfFilter};
use crate::synth_dsp::osc::{Noise, Osc, Wave};
use crate::synth_dsp::smooth::Glide;
use crate::synth_dsp::{note_hz, tanh_fast};
use tables::{level_for, Bank, FRAMES};

/// A voice-wise LFO: the value is worked out from a phase each voice can hold itself.
#[derive(Debug, Clone, Copy)]
pub(super) struct LfoState {
    pub(super) phase: f64,
    pub(super) held: f32,
    pub(super) rng: Rng,
}

impl LfoState {
    pub(super) fn new(seed: u64) -> LfoState {
        let mut rng = Rng::new(seed);
        let held = rng.bipolar();
        LfoState { phase: 0.0, held, rng }
    }

    #[inline]
    pub(super) fn value(&self, wave: usize) -> f32 {
        let p = self.phase as f32;
        match wave {
            0 => (p * std::f32::consts::TAU).sin(),
            1 => 1.0 - 4.0 * (p - 0.5).abs(),
            2 => 2.0 * p - 1.0,
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => self.held,
        }
    }

    #[inline]
    pub(super) fn advance(&mut self, inc: f64) {
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.held = self.rng.bipolar();
        }
    }
}

/// One copy in an oscillator's unison stack.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Copy1 {
    pub(super) phase: f64,
    /// Offset in semitones from the unison, ±1.
    pub(super) x: f32,
    pub(super) gain_l: f32,
    pub(super) gain_r: f32,
}

/// One oscillator in a voice.
#[derive(Debug, Clone, Copy)]
pub(super) struct OscV {
    pub(super) copies: [Copy1; UNISON_MAX],
    pub(super) count: usize,
    /// Its position and level, ramped per control tick.
    pub(super) pos: f32,
    pub(super) pos_step: f32,
    pub(super) level: f32,
    pub(super) level_step: f32,
    /// This control tick's frequency per copy, as a tick's phase increment.
    pub(super) inc_center: f32,
    pub(super) inc_spread: f32,
    pub(super) mip: usize,
}

impl OscV {
    pub(super) fn new(rng: &mut Rng) -> OscV {
        let mut copies = [Copy1::default(); UNISON_MAX];
        for c in &mut copies {
            c.phase = rng.unit() as f64;
        }
        OscV { copies, count: 1, pos: 0.0, pos_step: 0.0, level: 0.0, level_step: 0.0, inc_center: 0.0, inc_spread: 0.0, mip: 0 }
    }

    /// Lays out the copies for the patch: detune by place, level by blend, pan by width and pan.
    pub(super) fn lay_out(&mut self, p: &OscPatch) {
        self.count = p.copies.clamp(1, UNISON_MAX);
        let n = self.count;
        let power = 1.0 / (1.0 + p.blend * (n as f32 - 1.0)).sqrt();
        for (k, c) in self.copies.iter_mut().take(n).enumerate() {
            let (x, side) = unison_spread(k as u8, n as u8);
            c.x = x;
            // The centre copies are "the note" and the others come in with the blend.
            let centre = x.abs() < 0.34 || n == 1;
            let weight = if centre { 1.0 } else { p.blend };
            let pan = (p.pan + side * p.spread).clamp(-1.0, 1.0);
            let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            c.gain_l = angle.cos() * std::f32::consts::SQRT_2 * weight * power;
            c.gain_r = angle.sin() * std::f32::consts::SQRT_2 * weight * power;
        }
    }

    /// One tick of the stack through a bank's table. `fm` is in cycles.
    #[inline]
    pub(super) fn tick(&mut self, bank: &Bank, table: usize, fm: f32, l: &mut f32, r: &mut f32) {
        self.pos += self.pos_step;
        self.level += self.level_step;
        let at = (self.pos.clamp(0.0, 1.0)) * (FRAMES - 1) as f32;
        let f0 = at as usize;
        let frac = at - f0 as f32;
        let f1 = (f0 + 1).min(FRAMES - 1);
        let (a, b) = (bank.cycle(table, f0, self.mip).samples, bank.cycle(table, f1, self.mip).samples);
        let len = a.len();
        let mask = len - 1;
        let lf = len as f32;
        for c in self.copies.iter_mut().take(self.count) {
            let inc = (self.inc_center + self.inc_spread * c.x) as f64;
            // Phase in f64 for exact pitch; the read itself needs less.
            let p = c.phase;
            c.phase += inc;
            if c.phase >= 1.0 {
                c.phase -= 1.0;
            }
            // Into 0..1 by hand: the library's remainder is a call, this is a few cycles.
            let x = p as f32 + fm;
            let x = x - x as i32 as f32;
            let read = (if x < 0.0 { x + 1.0 } else { x }) * lf;
            let i = read as usize;
            let t = read - i as f32;
            // The two frames crossfaded, four points of it, then a Catmull-Rom cubic between them.
            let at = |k: usize| {
                let j = k & mask;
                a[j] + (b[j] - a[j]) * frac
            };
            let (p0, p1, p2, p3) = (at(i.wrapping_add(mask)), at(i), at(i + 1), at(i + 2));
            let c1 = 0.5 * (p2 - p0);
            let c2 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
            let c3 = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
            let v = (p1 + t * (c1 + t * (c2 + t * c3))) * self.level;
            *l += v * c.gain_l;
            *r += v * c.gain_r;
        }
    }
}

/// The voice's filter pair: stereo.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct FilterPair {
    pub(super) a: [Svf; 2],
    pub(super) b: [Svf; 2],
    pub(super) ladder: [Ladder; 2],
    pub(super) diode: [DiodeLadder; 2],
}

impl FilterPair {
    #[inline]
    pub(super) fn tick(&mut self, ch: usize, kind: FilterKind, g: f32, res: f32, x: f32) -> f32 {
        match kind {
            FilterKind::Low12 => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x)
            }
            FilterKind::Low24 => {
                self.a[ch].set(g, res * 0.5);
                self.b[ch].set(g, res);
                let y = self.a[ch].tick(x);
                self.b[ch].tick(y)
            }
            FilterKind::Band12 => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x);
                self.a[ch].bp
            }
            FilterKind::High12 => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x);
                self.a[ch].hp
            }
            FilterKind::Notch => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x);
                self.a[ch].lp + self.a[ch].hp
            }
            FilterKind::Ladder24 => {
                self.ladder[ch].set(g, res * 4.3);
                self.ladder[ch].tick(x)
            }
            FilterKind::Diode => {
                self.diode[ch].set(g, res * 0.98);
                // It sits lower than the others at the same input: brought up to meet them.
                self.diode[ch].tick(x) * 3.0
            }
        }
    }
}

pub(super) struct Voice {
    pub(super) active: bool,
    pub(super) gated: bool,
    pub(super) osc: [OscV; 2],
    pub(super) sub: Osc,
    pub(super) noise: Noise,
    pub(super) filter: FilterPair,
    pub(super) env: [Envelope; 3],
    pub(super) lfo: [LfoState; 2],
    pub(super) glide: Glide,
    pub(super) played: bool,
    pub(super) velocity: f32,
    pub(super) note: u8,
    pub(super) ctl: u32,
    /// Ramped per control tick, from the slots: the filter's note and resonance, FM and the pan.
    pub(super) cut: f32,
    pub(super) cut_step: f32,
    pub(super) res: f32,
    pub(super) fm: f32,
    pub(super) fm_step: f32,
    pub(super) pan: f32,
}

/// What every voice's control tick shares.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Shared {
    pub(super) bend: f32,
    pub(super) wheel: f32,
    pub(super) touch: f32,
    /// The LFOs that aren't per voice: their value (retrigger off), or their increment.
    pub(super) lfo_inc: [f64; 2],
    pub(super) lfo_wave: [usize; 2],
    pub(super) lfo_retrig: [bool; 2],
    pub(super) lfo_global: [f32; 2],
}

impl Voice {
    pub(super) fn new(seed: u64, index: usize, patch: &Patch) -> Voice {
        let vs = sub_seed(seed, index as u64);
        let mut rng = Rng::new(sub_seed(vs, 1));
        let mut sub = Osc::new(Wave::Sine);
        sub.reset(rng.unit() as f64);
        let mut filter = FilterPair::default();
        let hp = cutoff_g(120.0, patch.rate);
        filter.diode[0].set_feedback_highpass(hp);
        filter.diode[1].set_feedback_highpass(hp);
        Voice {
            active: false,
            gated: false,
            osc: [OscV::new(&mut rng), OscV::new(&mut rng)],
            sub,
            noise: Noise::new(sub_seed(vs, 2)),
            filter,
            env: [Envelope::default(); 3],
            lfo: [LfoState::new(sub_seed(vs, 3)), LfoState::new(sub_seed(vs, 4))],
            glide: Glide::default(),
            played: false,
            velocity: 1.0,
            note: 60,
            ctl: 0,
            cut: 0.0,
            cut_step: 0.0,
            res: 0.0,
            fm: 0.0,
            fm_step: 0.0,
            pan: 0.0,
        }
    }

    pub(super) fn start(&mut self, note: u8, velocity: f32, patch: &Patch, shared: &Shared, stolen: bool) {
        let n = note as f32;
        if patch.glide_secs > 0.0 && self.played {
            self.glide.target = n;
        } else {
            self.glide.jump(n);
        }
        self.played = true;
        self.active = true;
        self.gated = true;
        self.note = note;
        self.velocity = velocity;
        for e in &mut self.env {
            e.gate_on();
        }
        for k in 0..2 {
            if shared.lfo_retrig[k] {
                self.lfo[k].phase = 0.0;
            }
        }
        for o in 0..2 {
            self.osc[o].lay_out(&patch.osc[o]);
        }
        // The control tick lands now, and a new voice lands at its values, with no ramp from nothing.
        self.ctl = 0;
        if !stolen {
            self.cut = f32::NAN;
        }
    }

    pub(super) fn glide_to(&mut self, note: u8) {
        self.glide.target = note as f32;
        self.note = note;
    }

    pub(super) fn release(&mut self) {
        self.gated = false;
        for e in &mut self.env {
            e.gate_off();
        }
    }

    /// Works out the slots.
    #[inline]
    pub(super) fn control(&mut self, patch: &Patch, sh: &Shared) {
        let n = self.note as f32;
        let mut src = Sources::default();
        src.0[1] = self.env[1].level;
        src.0[2] = self.env[2].level;
        for k in 0..2 {
            src.0[3 + k] = if sh.lfo_retrig[k] { self.lfo[k].value(sh.lfo_wave[k]) } else { sh.lfo_global[k] };
            if sh.lfo_retrig[k] {
                self.lfo[k].advance(sh.lfo_inc[k]);
            }
        }
        src.0[5] = self.velocity;
        src.0[6] = (n - FILTER_REF_NOTE) / 60.0;
        src.0[7] = sh.wheel;
        src.0[8] = sh.touch;
        // Each destination's sum.
        let mut d = [0.0f32; params::MOD_DESTS.len()];
        for s in &patch.slots {
            if s.source != 0 && s.dest != 0 {
                d[s.dest] += src.0[s.source] * s.amount;
            }
        }
        let note = self.glide.value;
        let steps = CONTROL_TICKS as f32;
        for o in 0..2 {
            let p = &patch.osc[o];
            let (pos_d, pitch_d, level_d) = if o == 0 { (d[1], d[3], d[5]) } else { (d[2], d[4], d[6]) };
            let v = &mut self.osc[o];
            let pos = (p.pos + pos_d * POS_REACH).clamp(0.0, 1.0);
            let level = if p.on { (p.level * (1.0 + level_d)).clamp(0.0, 2.0) } else { 0.0 };
            if self.cut.is_nan() {
                v.pos = pos;
                v.level = level;
            }
            v.pos_step = (pos - v.pos) / steps;
            v.level_step = (level - v.level) / steps;
            let centre = note + sh.bend + p.st + pitch_d * PITCH_REACH_ST;
            let hz = note_hz(centre);
            let inc = hz / patch.rate;
            v.inc_center = inc;
            let detune = (p.detune + d[9]).clamp(0.0, 1.0) * DETUNE_ST;
            // A small offset: the first-order term of 2^(st/12) is good enough, and costs nothing.
            v.inc_spread = inc * detune * (std::f32::consts::LN_2 / 12.0);
            let top = inc + v.inc_spread.abs();
            v.mip = level_for(top);
        }
        let cut = patch.cutoff_note + (note - FILTER_REF_NOTE) * patch.key_track + d[7] * CUTOFF_REACH_ST;
        if self.cut.is_nan() {
            self.cut = cut;
        }
        self.cut_step = (cut - self.cut) / steps;
        self.res = (patch.res + d[8]).clamp(0.0, 1.0);
        let fm = (patch.fm + d[10]).clamp(0.0, 1.0) * FM_CYCLES;
        self.fm_step = (fm - self.fm) / steps;
        self.pan = d[11].clamp(-1.0, 1.0);
    }

    /// One tick. Adds to the two buses.
    #[inline]
    pub(super) fn tick(&mut self, patch: &Patch, sh: &Shared, bank: &Bank, bus_l: &mut f32, bus_r: &mut f32) {
        if self.ctl == 0 {
            self.ctl = CONTROL_TICKS;
            // Env 2 and 3 move here; they lead the audio by a tick at most, and the ramps smooth it.
            self.env[1].tick(&patch.env[1], CONTROL_TICKS as f32);
            self.env[2].tick(&patch.env[2], CONTROL_TICKS as f32);
            self.control(patch, sh);
        }
        self.ctl -= 1;
        self.glide.tick();

        let (mut xl, mut xr) = (0.0f32, 0.0f32);
        // B first: it's the modulator.
        let table_b = patch.osc[1].table;
        let (mut bl, mut br) = (0.0f32, 0.0f32);
        if patch.osc[1].on || self.fm > 0.0 || self.fm_step != 0.0 {
            self.osc[1].tick(bank, table_b, 0.0, &mut bl, &mut br);
        }
        self.fm += self.fm_step;
        let fm = self.fm * 0.5 * (bl + br);
        if patch.osc[0].on {
            self.osc[0].tick(bank, patch.osc[0].table, fm, &mut xl, &mut xr);
        }
        if patch.osc[1].on {
            xl += bl;
            xr += br;
        }
        if patch.sub > 0.0 {
            let inc = (note_hz(self.glide.value + sh.bend - 12.0) / patch.rate) as f64;
            let s = self.sub.tick(inc, None).0 * patch.sub;
            xl += s;
            xr += s;
        }
        if patch.noise > 0.0 {
            let n = self.noise.white() * patch.noise * patch.noise_scale;
            xl += n;
            xr += n;
        }
        xl *= OSC_GAIN;
        xr *= OSC_GAIN;
        self.cut += self.cut_step;
        let (mut yl, mut yr) = (xl, xr);
        if patch.filter_on {
            if patch.drive > 0.0 {
                let d = 1.0 + patch.drive * 6.0;
                yl = tanh_fast(yl * d);
                yr = tanh_fast(yr * d);
            }
            let g = cutoff_g(note_hz(self.cut.clamp(-24.0, 150.0)), patch.rate);
            yl = self.filter.tick(0, patch.filter, g, self.res, yl);
            yr = self.filter.tick(1, patch.filter, g, self.res, yr);
        }
        let a = self.env[0].tick(&patch.env[0], 1.0);
        let angle = (self.pan + 1.0) * std::f32::consts::FRAC_PI_4;
        // cos/sin only when a slot moves it.
        let (pl, pr) = if self.pan == 0.0 { (1.0, 1.0) } else { (angle.cos() * std::f32::consts::SQRT_2, angle.sin() * std::f32::consts::SQRT_2) };
        *bus_l += yl * a * pl;
        *bus_r += yr * a * pr;
        if !self.env[0].active() {
            self.active = false;
            self.gated = false;
            for e in &mut self.env {
                e.reset();
            }
        }
    }
}
