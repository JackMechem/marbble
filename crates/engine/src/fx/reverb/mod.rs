//! Reverb: an algorithmic studio reverb in the spirit of Logic's ChromaVerb and SilverVerb.
//!
//! Two engines sit behind the Room Type choice:
//! - **Plate** is Jon Dattorro's plate (J. Dattorro, "Effect Design, Part 1: Reverberator and
//!   Other Filters", JAES 45(9), 1997). The topology, delay lengths and output taps follow the
//!   paper's figure and table; the code is written from the paper, not ported from any
//!   implementation. Its single damping filter per tank half is replaced by the two-band decay
//!   filter below, and every element scales with Size.
//! - **Room, Hall, Chamber, Cathedral, Ambience** share a 16-line feedback delay network with a
//!   Hadamard mixing matrix. Each line has a short in-loop allpass (Density), a gently modulated
//!   read (against metallic ringing), and a two-band decay filter whose gains come from the RT60
//!   so every line decays at the same rate per second (Jot's method). Input diffusers smear the
//!   attack before the network, and a tapped delay per room type gives the early reflections.
//!
//! The decay filter is a low shelf and a high shelf around a mid gain. Each first-order shelf's
//! magnitude stays between its two plateau gains, so the loop gain never exceeds the larger of
//! the low and mid gains, which are below 1 for any finite RT60: the network can't run away.
//! Freeze sets an hour-long RT60 and mutes the input.

use super::delay::{flush, note_ms, Biquad, Line, Osc, MAX_RATE, NOTE_LABELS};
use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit};

mod fdn;
use fdn::*;
mod params;
pub use params::*;
mod parts;
use parts::*;
mod plate;
use plate::*;
#[cfg(test)]
mod tests;

/// The early buffer's length: the latest tap or late feed at the largest size.
fn early_ms() -> f32 {
    ROOMS.iter().map(|r| r.er.2.max(r.late_ms)).fold(0.0, f32::max) * SIZE_MAX + 2.0
}

/// The longest early-reflection smearing allpass.
const SMEAR_MS: f32 = 12.0;

#[inline]
fn approach(v: &mut f32, target: f32, k: f32) {
    *v += (target - *v) * k;
}

pub struct Reverb {
    values: [f32; PARAMS.len()],
    sr: f32,
    /// The room type running now; a different requested one fades over.
    room: usize,
    fade: f32,
    fdn: Fdn,
    plate: Plate,
    pre: [Line; 2],
    predelay: f32,
    /// The pre-delayed input, which the early reflection taps and the late feed read.
    early_buf: [Line; 2],
    early_smear: [Line; 2],
    early_smear_len: [usize; 2],
    er_times: [[f32; MAX_TAPS]; 2],
    er_gains: [[f32; MAX_TAPS]; 2],
    er_count: usize,
    /// The gliding size scale for the early taps.
    scale: f32,
    scale_target: f32,
    low_cut: [Biquad; 2],
    high_cut: [Biquad; 2],
    // Smoothed per-sample gains.
    dry: f32,
    wet: f32,
    width: f32,
    early: f32,
    late: f32,
    input: f32,
    tempo: f64,
    /// Decay, filter or modulation settings changed; recompute at the next block.
    dirty: bool,
    /// The geometry (room or size) changed.
    reshape: bool,
    primed: bool,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Reverb {
        let alloc = sample_rate.max(MAX_RATE);
        let cap = |ms: f32| buf_len(ms, alloc);
        let mut r = Reverb {
            values: std::array::from_fn(|i| PARAMS[i].default),
            sr: sample_rate,
            room: 2,
            fade: 1.0,
            fdn: Fdn::new(alloc),
            plate: Plate::new(alloc),
            pre: [Line::new(cap(MAX_PREDELAY_MS)), Line::new(cap(MAX_PREDELAY_MS))],
            predelay: 0.0,
            early_buf: [Line::new(cap(early_ms())), Line::new(cap(early_ms()))],
            early_smear: [Line::new(cap(SMEAR_MS)), Line::new(cap(SMEAR_MS))],
            early_smear_len: [8; 2],
            er_times: [[0.0; MAX_TAPS]; 2],
            er_gains: [[0.0; MAX_TAPS]; 2],
            er_count: 0,
            scale: 1.0,
            scale_target: 1.0,
            low_cut: [Biquad::new(); 2],
            high_cut: [Biquad::new(); 2],
            dry: 1.0,
            wet: 0.0,
            width: 1.0,
            early: 0.0,
            late: 0.0,
            input: 0.0,
            tempo: 120.0,
            dirty: true,
            reshape: true,
            primed: false,
        };
        r.room = r.requested_room();
        r.set_rate(sample_rate);
        r
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn on(&self, id: u32) -> bool {
        self.v(id) >= 0.5
    }

    fn requested_room(&self) -> usize {
        (self.v(ROOM).round() as usize).min(ROOM_TYPES.len() - 1)
    }

    fn set_rate(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        for line in &mut self.pre {
            line.set_active(buf_len(MAX_PREDELAY_MS, sample_rate));
        }
        for line in &mut self.early_buf {
            line.set_active(buf_len(early_ms(), sample_rate));
        }
        for line in &mut self.early_smear {
            line.set_active(buf_len(SMEAR_MS, sample_rate));
        }
        self.fdn.set_rate(sample_rate);
        self.plate.set_rate(sample_rate);
        self.dirty = true;
        self.reshape = true;
        self.reset();
    }

    fn size_scale(&self) -> f32 {
        SIZE_MIN + SIZE_SPAN * self.v(SIZE)
    }

    /// Applies the room and size to the engines and the early pattern.
    fn configure(&mut self, snap: bool) {
        let def = &ROOMS[self.room];
        let scale = self.size_scale();
        self.scale_target = scale;
        self.plate.scale_target = scale;
        if snap {
            self.scale = scale;
            self.plate.scale = scale;
        }
        if self.room != PLATE {
            self.fdn.configure(def, scale, self.sr, snap);
        }
        self.er_count = def.er.0.min(MAX_TAPS);
        for ch in 0..2 {
            let (t, g) = er_pattern(def, ch as u32);
            self.er_times[ch] = t;
            self.er_gains[ch] = g;
            let smear = ms_to_samples(def.er.1.max(1.0) * 0.6 + 1.3 * ch as f32, self.sr) as usize;
            self.early_smear_len[ch] = next_prime(smear.max(2)).min(self.early_smear[ch].max_delay() as usize);
        }
        self.dirty = true;
    }

    fn rt(&self) -> Rt {
        if self.on(FREEZE) {
            return Rt { mid: FREEZE_RT60, low: FREEZE_RT60, high: FREEZE_RT60 };
        }
        let mid = self.v(DECAY) * 0.001;
        Rt { mid, low: mid * self.v(LOW_DECAY), high: mid * (1.0 - 0.85 * self.v(DAMPING)) }
    }

    fn predelay_ms(&self, tempo: f64) -> f32 {
        let ms = if self.on(SYNC) { note_ms(self.v(PREDELAY_NOTE).round() as usize, tempo) } else { self.v(PREDELAY) };
        ms.clamp(0.0, MAX_PREDELAY_MS)
    }

    /// Input gain so a room's loudness doesn't depend much on its size: a network's energy
    /// grows with the decay time per sample of loop, so longer lines need more input.
    fn input_gain(&self) -> f32 {
        if self.room == PLATE {
            return 0.55;
        }
        let reference = ms_to_samples(40.0, self.sr);
        0.32 * (self.fdn.mean_len() / reference).sqrt()
    }

    fn clear_engines(&mut self) {
        self.fdn.clear();
        self.plate.clear();
        for line in &mut self.early_smear {
            line.clear();
        }
    }

    #[cfg(test)]
    fn has_subnormal(&self) -> bool {
        self.fdn.has_subnormal()
            || self.plate.has_subnormal()
            || self.pre.iter().chain(&self.early_buf).chain(&self.early_smear).any(|l| l.has_subnormal())
            || self.low_cut.iter().chain(&self.high_cut).any(|b| b.has_subnormal())
    }
}

impl Effect for Reverb {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(v) = self.values.get_mut(id as usize) else { return };
        *v = value;
        match id {
            SIZE => self.reshape = true,
            DECAY | DAMPING | LOW_DECAY | FREEZE | MOD_RATE | LOW_CUT | HIGH_CUT => self.dirty = true,
            _ => {}
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    // Indexes parallel per-channel or per-line arrays together: an index reads best here.
    #[allow(clippy::needless_range_loop)]
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        if ctx.sample_rate > 0.0 && (ctx.sample_rate - self.sr).abs() > 0.5 && ctx.sample_rate <= MAX_RATE {
            self.set_rate(ctx.sample_rate);
        }
        self.tempo = ctx.tempo;
        let sr = self.sr;

        // A room change fades the wet signal out, swaps the engine with its state cleared, and
        // fades back in. Before the first block it just switches.
        let want = self.requested_room();
        let mut fade_target = 1.0;
        if want != self.room {
            if !self.primed || self.fade <= 0.0 {
                self.room = want;
                self.clear_engines();
                self.configure(true);
                self.reshape = false;
            } else {
                fade_target = 0.0;
            }
        }
        if self.reshape {
            self.reshape = false;
            self.configure(!self.primed);
        }
        if self.dirty {
            self.dirty = false;
            let rt = self.rt();
            self.fdn.set_decay(rt, sr);
            self.plate.set_decay(rt, sr);
            let rate = self.v(MOD_RATE);
            self.fdn.set_mod_rate(rate, sr);
            self.plate.osc.set_freq(rate, sr);
            for b in &mut self.low_cut {
                b.set_highpass(self.values[LOW_CUT as usize], sr);
            }
            for b in &mut self.high_cut {
                b.set_lowpass(self.values[HIGH_CUT as usize], sr);
            }
        }

        let plate = self.room == PLATE;
        let freeze = self.on(FREEZE);
        let balance = self.v(BALANCE);
        let early_t = (2.0 * (1.0 - balance)).min(1.0);
        let late_t = (2.0 * balance).min(1.0);
        let input_t = if freeze { 0.0 } else { self.input_gain() };
        let (dry_t, wet_t, width_t) = (self.v(DRY), self.v(WET), self.v(WIDTH));
        // Two samples is the least a Hermite read can delay; the extra one is constant.
        let predelay_t = ms_to_samples(self.predelay_ms(ctx.tempo), sr).round() + 2.0;
        if !self.primed {
            self.primed = true;
            self.dry = dry_t;
            self.wet = wet_t;
            self.width = width_t;
            self.early = early_t;
            self.late = late_t;
            self.input = input_t;
            self.predelay = predelay_t;
        }

        let ks = super::util::coeff(20.0, sr);
        let fade_k = super::util::coeff(TYPE_FADE_MS / 4.0, sr);
        let glide = super::util::coeff(150.0, sr);
        let pd_glide = super::util::coeff(60.0, sr);
        let diffusion = 0.72 * self.v(DIFFUSION);
        let density = 0.62 * self.v(DENSITY);
        let depth = self.v(MOD_DEPTH);
        let excursion = depth * ms_to_samples(MOD_MS, sr) * 0.5;
        let plate_excursion = depth / 0.35 * PLATE_EXCURSION * self.plate.k * 0.5;
        let smear_g = 0.5 * self.v(DIFFUSION);
        // The paper's input diffusion is the default 70 %.
        let plate_diffusion = (self.v(DIFFUSION) / 0.7).min(1.1);
        let late_ms = ROOMS[self.room].late_ms;

        for n in 0..l.len() {
            approach(&mut self.dry, dry_t, ks);
            approach(&mut self.wet, wet_t, ks);
            approach(&mut self.width, width_t, ks);
            approach(&mut self.early, early_t, ks);
            approach(&mut self.late, late_t, ks);
            approach(&mut self.input, input_t, ks);
            // Pre-delay glides, slew-limited, so automating it doesn't click.
            self.predelay += ((predelay_t - self.predelay) * pd_glide).clamp(-0.5, 0.5);
            self.scale += ((self.scale_target - self.scale) * glide).clamp(-SCALE_SLEW, SCALE_SLEW);

            let (il, ir) = (l[n], r[n]);
            let mut x = [0.0; 2];
            for (ch, input) in [il, ir].into_iter().enumerate() {
                self.pre[ch].write(flush(input));
                x[ch] = self.pre[ch].read_frac(self.predelay) * self.input;
                self.early_buf[ch].write(flush(x[ch]));
            }

            let mut early = [0.0f32; 2];
            // Early reflections: each side's taps read mostly its own side.
            let ms = 0.001 * sr * self.scale;
            for ch in 0..2 {
                let (own, other) = (&self.early_buf[ch], &self.early_buf[1 - ch]);
                let mut acc = 0.0;
                for k in 0..self.er_count {
                    let d = 1.0 + self.er_times[ch][k] * ms;
                    acc += self.er_gains[ch][k] * (0.8 * own.read_lin(d) + 0.2 * other.read_lin(d));
                }
                early[ch] = allpass(&mut self.early_smear[ch], acc, self.early_smear_len[ch], smear_g);
            }
            let d = 1.0 + late_ms * ms;
            let feed = [self.early_buf[0].read_lin(d), self.early_buf[1].read_lin(d)];
            let late = if plate {
                self.plate.tick(0.5 * (feed[0] + feed[1]), plate_diffusion, self.v(DENSITY), plate_excursion, glide)
            } else {
                self.fdn.tick(feed, diffusion, density, excursion, glide)
            };

            let mut wet = [0.0; 2];
            for ch in 0..2 {
                let w = early[ch] * self.early + late[ch] * self.late;
                wet[ch] = self.high_cut[ch].process(self.low_cut[ch].process(w));
            }
            approach(&mut self.fade, fade_target, fade_k);
            let mid = 0.5 * (wet[0] + wet[1]);
            let side = 0.5 * (wet[0] - wet[1]) * self.width;
            let g = self.wet * self.fade;
            l[n] = il * self.dry + (mid + side) * g;
            r[n] = ir * self.dry + (mid - side) * g;
        }
        if fade_target == 0.0 && self.fade < 1e-3 {
            self.fade = 0.0;
        }
        self.fdn.renormalize();
        self.plate.osc.normalize();
    }

    fn reset(&mut self) {
        for line in self.pre.iter_mut().chain(&mut self.early_buf) {
            line.clear();
        }
        self.clear_engines();
        for b in self.low_cut.iter_mut().chain(&mut self.high_cut) {
            b.clear();
        }
        self.fade = 1.0;
        self.primed = false;
        self.reshape = true;
    }

    fn tail_secs(&self) -> f32 {
        if self.on(FREEZE) {
            return MAX_TAIL_SECS;
        }
        let rt = self.rt();
        let longest = rt.mid.max(rt.low);
        let early = ROOMS[self.room].er.2.max(ROOMS[self.room].late_ms) * SIZE_MAX * 0.001;
        (1.5 * longest + self.predelay_ms(self.tempo) * 0.001 + early).min(MAX_TAIL_SECS)
    }
}
