//! One voice of Marbble Sedici: six sine operators in one of the 32 algorithms with one feedback
//! loop, their envelopes and the pitch envelope, ticking at the instrument's own rate (`DX_RATE`).

use super::*;
use crate::synth_dsp::analog::{sub_seed, Rng};
use crate::synth_dsp::pow2;
use env::{DxEnv, TOP};

/// One operator in one voice.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Op {
    pub(super) phase: u32,
    pub(super) inc: u32,
    pub(super) gain: f32,
    pub(super) gain_step: f32,
    pub(super) env: DxEnv,
    /// Its frequency before pitch modulation.
    pub(super) hz: f32,
}

/// The voice's pitch envelope.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PitchEnv {
    pub(super) st: f32,
    pub(super) ix: usize,
    pub(super) down: bool,
}

pub(super) struct Voice {
    pub(super) active: bool,
    pub(super) gated: bool,
    pub(super) note: f32,
    pub(super) target_note: f32,
    pub(super) velocity: f32,
    pub(super) key: u8,
    pub(super) ops: [Op; 6],
    pub(super) out: [f32; 6],
    pub(super) fb: [f32; 2],
    pub(super) pitch_env: PitchEnv,
    pub(super) block_left: u32,
    pub(super) fade: u32,
    pub(super) pending: Option<(u8, f32)>,
    pub(super) pending_off: bool,
    /// Stops at its fourth level rather than droning on: the transport stopped.
    pub(super) force_end: bool,
    pub(super) force_gain: f32,
}

impl Voice {
    pub(super) fn new(seed: u64, index: usize) -> Voice {
        let mut rng = Rng::new(sub_seed(sub_seed(seed, index as u64), 11));
        let mut ops = [Op::default(); 6];
        for o in &mut ops {
            o.phase = rng.next_u64() as u32;
        }
        Voice {
            active: false,
            gated: false,
            note: 60.0,
            target_note: 60.0,
            velocity: 1.0,
            key: 60,
            ops,
            out: [0.0; 6],
            fb: [0.0; 2],
            pitch_env: PitchEnv::default(),
            block_left: 0,
            fade: 0,
            pending: None,
            pending_off: false,
            force_end: false,
            force_gain: 1.0,
        }
    }

    /// Starts the note: the operators take their levels and their envelopes start from wherever
    /// they are.
    pub(super) fn begin(&mut self, note: u8, velocity: f32, patch: &Patch, glide_from: Option<f32>) {
        self.gated = true;
        self.active = true;
        self.velocity = velocity;
        self.force_end = false;
        self.force_gain = 1.0;
        let n = note as f32 + patch.transpose;
        self.target_note = n;
        self.note = glide_from.unwrap_or(n);
        self.key = note;
        for (o, p) in self.ops.iter_mut().zip(&patch.ops) {
            let nn = note as i32;
            o.env.rates = p.rates;
            o.env.levels = p.levels;
            o.env.out_level = p.out_level(nn, velocity);
            o.env.rate_scale = env::keyboard_rate_scale(nn, p.rate_scale);
            o.env.key_on();
            if patch.key_sync {
                o.phase = 0;
            }
        }
        self.pitch_env.ix = 0;
        self.pitch_env.down = true;
        if patch.key_sync {
            self.fb = [0.0; 2];
        }
        // The envelopes' next block starts at once.
        self.block_left = 0;
    }

    pub(super) fn start(&mut self, note: u8, velocity: f32, stolen: bool, patch: &Patch) {
        let was_on = self.active;
        if stolen && was_on && patch.key_sync {
            // Fade the old note out for 2 ms, then jump the phases where nobody can hear them.
            self.pending = Some((note, velocity));
            self.pending_off = false;
            if self.fade == 0 {
                self.fade = FADE_TICKS;
            }
        } else {
            let from = if patch.glide_secs > 0.0 && self.note > 0.0 && was_on { Some(self.note) } else { None };
            self.begin(note, velocity, patch, from);
        }
    }

    /// The panel changed under this voice.
    pub(super) fn refresh(&mut self, patch: &Patch) {
        let nn = self.key as i32;
        for (o, p) in self.ops.iter_mut().zip(&patch.ops) {
            o.env.retarget(p.rates, p.levels, p.out_level(nn, self.velocity), env::keyboard_rate_scale(nn, p.rate_scale));
        }
    }

    pub(super) fn glide_to(&mut self, note: u8, patch: &Patch) {
        self.target_note = note as f32 + patch.transpose;
    }

    pub(super) fn release(&mut self) {
        if self.pending.is_some() {
            self.pending_off = true;
            return;
        }
        self.gated = false;
        for o in &mut self.ops {
            o.env.key_off();
        }
        self.pitch_env.down = false;
        if self.pitch_env.ix < 3 {
            self.pitch_env.ix = 3;
        }
    }

    /// Per 64 ticks: the envelopes, pitch, and each operator's next gain and rate.
    pub(super) fn control(&mut self, patch: &Patch, c: &Control) {
        // Portamento: a steady fraction of the way per block.
        if self.note != self.target_note {
            if patch.glide_secs <= 0.0 {
                self.note = self.target_note;
            } else {
                let k = 1.0 - (-3.0 * BLOCK as f32 / (patch.glide_secs * DX_RATE as f32)).exp();
                self.note += (self.target_note - self.note) * k;
                if (self.target_note - self.note).abs() < 1e-3 {
                    self.note = self.target_note;
                }
            }
        }
        // The pitch envelope, to the next level at its rate.
        let e = &mut self.pitch_env;
        if e.ix < 3 || (e.ix < 4 && !e.down) {
            let target = patch.pitch_levels[e.ix];
            // Octaves a second to semitones a block.
            let step = patch.pitch_rates[e.ix] * 12.0 * BLOCK as f32 / DX_RATE as f32;
            if (target - e.st).abs() <= step {
                e.st = target;
                e.ix += 1;
            } else {
                e.st += step * (target - e.st).signum();
            }
        }
        let pitch = c.pitch_st + self.pitch_env.st;
        let ratio = pow2(pitch / 12.0);
        let mut carriers_done = true;
        for (i, (o, p)) in self.ops.iter_mut().zip(&patch.ops).enumerate() {
            o.hz = p.hz(self.note);
            let hz = if p.fixed { o.hz } else { o.hz * ratio };
            o.inc = (hz as f64 / DX_RATE * 4_294_967_296.0) as u32;
            let level = o.env.block();
            // Amp mod takes its level down, as attenuation in the same log scale.
            let l = level - p.ams * c.am_units;
            let target = if p.on { pow2((l - TOP) / 256.0) } else { 0.0 };
            let target = if l <= 16.0 { 0.0 } else { target };
            o.gain_step = (target - o.gain) / BLOCK as f32;
            if patch.algo.carriers & (1 << i) != 0 && p.on && (!o.env.finished() || o.gain > END_GAIN || target > END_GAIN) {
                carriers_done = false;
            }
        }
        if !self.gated && carriers_done && self.pending.is_none() {
            self.active = false;
            for o in &mut self.ops {
                o.gain = 0.0;
                o.gain_step = 0.0;
            }
            self.out = [0.0; 6];
            self.fb = [0.0; 2];
        }
    }

    /// One tick at the instrument's rate. Returns what it adds to the output.
    #[inline]
    pub(super) fn tick(&mut self, patch: &Patch, c: &Control, table: &[f32; SIN_SIZE + 1]) -> f32 {
        if self.block_left == 0 {
            self.block_left = BLOCK;
            self.control(patch, c);
            if !self.active {
                return 0.0;
            }
        }
        self.block_left -= 1;
        let a = &patch.algo;
        let mut sum = 0.0f32;
        let grit = patch.grit;
        let fb = (self.fb[0] + self.fb[1]) * 0.5 * patch.fb_gain;
        for i in (0..6).rev() {
            let o = &mut self.ops[i];
            o.gain += o.gain_step;
            let mut m = 0.0f32;
            for k in 0..a.mod_count[i] as usize {
                m += self.out[a.mods[i][k] as usize];
            }
            m *= MOD_CYCLES;
            if a.fb_to as usize == i {
                m += fb;
            }
            let phase = o.phase.wrapping_add(((m * 16_777_216.0) as i32 as u32).wrapping_shl(8));
            o.phase = o.phase.wrapping_add(o.inc);
            let idx = (phase >> (32 - SIN_BITS)) as usize;
            let s = if grit {
                table[idx]
            } else {
                let frac = (phase & ((1 << (32 - SIN_BITS)) - 1)) as f32 / (1u32 << (32 - SIN_BITS)) as f32;
                table[idx] + (table[idx + 1] - table[idx]) * frac
            };
            let mut y = s * o.gain;
            if grit {
                // An operator's 14 bits.
                y = (y * 8192.0) as i32 as f32 / 8192.0;
            }
            self.out[i] = y;
            if a.carriers & (1 << i) != 0 {
                sum += y;
            }
        }
        // The loop reads the operator it's from, after this tick.
        self.fb[1] = self.fb[0];
        self.fb[0] = self.out[a.fb_from as usize];
        let mut y = sum;
        if self.fade > 0 {
            y *= self.fade as f32 / FADE_TICKS as f32;
            self.fade -= 1;
            if self.fade == 0 {
                if let Some((note, velocity)) = self.pending.take() {
                    self.begin(note, velocity, patch, None);
                    if self.pending_off {
                        self.pending_off = false;
                        self.release();
                    }
                }
            }
        }
        if self.force_end {
            self.force_gain *= 0.9997;
            y *= self.force_gain;
            if self.force_gain < 1e-3 {
                self.active = false;
            }
        }
        y
    }
}
