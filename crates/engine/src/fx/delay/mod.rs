//! Delay: Logic's Stereo Delay and Tape Delay in one effect.
//!
//! Four modes share one pair of delay lines:
//! - **Stereo**: independent left and right times and feedback, with cross-feed.
//! - **Ping-Pong**: the input, summed to mono, bounces left → right → left.
//! - **Tape**: wow and flutter, saturation and a darkening filter in the loop. Time changes glide
//!   like a varispeed transport, and feedback can pass 100 % and self-oscillate, held in check by
//!   the saturation.
//! - **Digital**: one linked time, no colouring, feedback clamped below 100 %.
//!
//! Time changes (including tempo changes while synced) crossfade between two read heads in the
//! clean modes and glide in Tape mode, so neither clicks.
//!
//! This module also holds the small building blocks the reverb shares: [`Line`], [`Biquad`],
//! [`flush`] and the tempo-sync note values.

use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit};

mod params;
pub use params::*;
mod primitives;
pub(crate) use primitives::*;
#[cfg(test)]
mod tests;

/// One channel's read position. In the clean modes a time change crossfades from `cur` to
/// `next`; in Tape mode `cur` glides.
#[derive(Clone, Copy)]
struct Head {
    cur: f32,
    next: f32,
    /// Crossfade progress, 0 when not fading.
    x: f32,
}

impl Head {
    #[inline]
    fn tick_fade(&mut self, target: f32, step: f32) {
        if self.x == 0.0 {
            if (target - self.cur).abs() >= 0.5 {
                self.next = target;
                self.x = step;
            }
        } else {
            self.x += step;
            if self.x >= 1.0 {
                self.cur = self.next;
                self.x = 0.0;
            }
        }
    }

    #[inline]
    fn tick_glide(&mut self, target: f32, k: f32, fade_step: f32) {
        if self.x > 0.0 {
            // A crossfade left over from a clean mode finishes first.
            self.tick_fade(self.next, fade_step);
            return;
        }
        // The slew limit keeps the varispeed between 0.1× and 1.9×.
        self.cur += ((target - self.cur) * k).clamp(-0.9, 0.9);
    }

    #[inline]
    fn read(&self, line: &Line, m: f32) -> f32 {
        let a = line.read_frac(self.cur + m);
        if self.x > 0.0 {
            let b = line.read_frac(self.next + m);
            a + (b - a) * self.x
        } else {
            a
        }
    }
}

/// `tanh` by a Padé approximant, exact at ±3 where it reaches ±1.
#[inline]
fn tanh_approx(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Tape saturation with unity small-signal gain, so quiet repeats keep the set feedback.
#[inline]
fn saturate(x: f32, drive: f32) -> f32 {
    tanh_approx(drive * x) / drive
}

/// A transparent safety limiter for the clean modes: it only acts above 1.5 (+3.5 dBFS).
#[inline]
fn guard(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.5 {
        x
    } else {
        x.signum() * (1.5 + tanh_approx(a - 1.5))
    }
}

/// A one-pole smoother step.
#[inline]
fn approach(v: &mut f32, target: f32, k: f32) {
    *v += (target - *v) * k;
}

pub struct Delay {
    values: [f32; PARAMS.len()],
    sr: f32,
    lines: [Line; 2],
    heads: [Head; 2],
    low_cut: [Biquad; 2],
    high_cut: [Biquad; 2],
    filters_dirty: bool,
    /// Tape mode's darkening low-pass and its DC-blocking high-pass, one pole each.
    dark: [f32; 2],
    dc: [f32; 2],
    chorus: Osc,
    wow: Osc,
    flutter: [Osc; 2],
    rng: u32,
    wow_drift: f32,
    flutter_jitter: f32,
    // Smoothed per-sample values.
    dry: f32,
    wet: f32,
    width: f32,
    gate: f32,
    fb: [f32; 2],
    cross: f32,
    /// The tempo of the last block, for `tail_secs`.
    tempo: f64,
    /// False until the first block after a reset, which snaps every smoother to its target.
    primed: bool,
}

impl Delay {
    pub fn new(sample_rate: f32) -> Delay {
        let alloc = sample_rate.max(MAX_RATE);
        let cap = ((MAX_TIME_MS + MARGIN_MS) * 0.001 * alloc) as usize + 8;
        let mut d = Delay {
            values: std::array::from_fn(|i| PARAMS[i].default),
            sr: sample_rate,
            lines: [Line::new(cap), Line::new(cap)],
            heads: [Head { cur: 2.0, next: 2.0, x: 0.0 }; 2],
            low_cut: [Biquad::new(); 2],
            high_cut: [Biquad::new(); 2],
            filters_dirty: true,
            dark: [0.0; 2],
            dc: [0.0; 2],
            chorus: Osc::new(0.0),
            wow: Osc::new(0.0),
            flutter: [Osc::new(0.0), Osc::new(1.3)],
            rng: 0x2545_f491,
            wow_drift: 0.0,
            flutter_jitter: 0.0,
            dry: 1.0,
            wet: 0.0,
            width: 1.0,
            gate: 1.0,
            fb: [0.0; 2],
            cross: 0.0,
            tempo: 120.0,
            primed: false,
        };
        d.set_rate(sample_rate);
        d
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn on(&self, id: u32) -> bool {
        self.v(id) >= 0.5
    }

    fn mode(&self) -> usize {
        (self.v(MODE).round() as usize).min(DIGITAL)
    }

    fn set_rate(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        let len = ((MAX_TIME_MS + MARGIN_MS) * 0.001 * sample_rate) as usize + 8;
        for line in &mut self.lines {
            line.set_active(len);
        }
        self.wow.set_freq(0.55, sample_rate);
        self.flutter[0].set_freq(6.3, sample_rate);
        self.flutter[1].set_freq(10.9, sample_rate);
        self.filters_dirty = true;
        self.reset();
    }

    /// The left and right delay times in ms for the current mode, sync and link settings.
    fn times_ms(&self, tempo: f64) -> [f32; 2] {
        let time = |ms: u32, note: u32| {
            let t = if self.on(SYNC) { note_ms(self.v(note).round() as usize, tempo) } else { self.v(ms) };
            t.clamp(1.0, MAX_TIME_MS)
        };
        let l = time(TIME_L, NOTE_L);
        let linked = self.mode() == DIGITAL || self.on(LINK);
        [l, if linked { l } else { time(TIME_R, NOTE_R) }]
    }

    /// The loop gains per channel (each repeat is scaled by this).
    fn feedback(&self) -> [f32; 2] {
        if self.on(FREEZE) {
            return [1.0; 2];
        }
        let mode = self.mode();
        let l = self.v(FEEDBACK_L);
        let r = if mode == DIGITAL || mode == PING_PONG || self.on(LINK) { l } else { self.v(FEEDBACK_R) };
        match mode {
            DIGITAL => [l.min(DIGITAL_MAX_FEEDBACK), r.min(DIGITAL_MAX_FEEDBACK)],
            TAPE => [l * TAPE_OVERDRIVE, r * TAPE_OVERDRIVE],
            _ => [l, r],
        }
    }

    fn noise(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    #[cfg(test)]
    fn has_subnormal(&self) -> bool {
        self.lines.iter().any(|l| l.has_subnormal())
            || self.low_cut.iter().chain(&self.high_cut).any(|b| b.has_subnormal())
            || self.dark.iter().chain(&self.dc).any(|x| x.is_subnormal())
    }
}

impl Effect for Delay {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
            if id == LOW_CUT || id == HIGH_CUT {
                self.filters_dirty = true;
            }
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
        let mode = self.mode();
        let freeze = self.on(FREEZE);
        let clean = mode != TAPE;

        if self.filters_dirty {
            self.filters_dirty = false;
            let (lc, hc) = (self.v(LOW_CUT), self.v(HIGH_CUT));
            for b in &mut self.low_cut {
                b.set_highpass(lc, sr);
            }
            for b in &mut self.high_cut {
                b.set_lowpass(hc, sr);
            }
        }
        let low_cut = !freeze && self.v(LOW_CUT) > LOW_CUT_OFF + 0.5;
        let high_cut = !freeze && self.v(HIGH_CUT) < HIGH_CUT_OFF - 1.0;
        let tape = mode == TAPE && !freeze;

        // Targets for this block.
        let times = self.times_ms(ctx.tempo);
        let max_d = self.lines[0].max_delay() - MARGIN_MS * 0.001 * sr;
        let target = times.map(|ms| {
            let d = (ms * 0.001 * sr).clamp(2.0, max_d);
            // Clean modes read whole samples, so unmodulated repeats lose no treble.
            if clean { d.round() } else { d }
        });
        let fb_t = self.feedback();
        let cross_t = if mode == PING_PONG { 0.0 } else { self.v(CROSSFEED) };
        let dry_t = self.v(DRY);
        let wet_t = self.v(WET);
        let width_t = self.v(WIDTH);
        let gate_t = if freeze { 0.0 } else { 1.0 };

        if freeze {
            // Straight to unity so the loop holds exactly what it had.
            self.fb = fb_t;
        }
        if !self.primed {
            self.primed = true;
            for (h, t) in self.heads.iter_mut().zip(target) {
                *h = Head { cur: t, next: t, x: 0.0 };
            }
            self.fb = fb_t;
            self.cross = cross_t;
            self.dry = dry_t;
            self.wet = wet_t;
            self.width = width_t;
            self.gate = gate_t;
        }

        let ks = super::util::coeff(20.0, sr);
        let smooth = self.v(SMOOTH);
        let glide_k = super::util::coeff(40.0 + 1460.0 * smooth, sr);
        let fade_step = 1.0 / (XFADE_MS * 0.001 * sr);
        let ms = 0.001 * sr;
        self.chorus.set_freq(self.v(MOD_RATE), sr);
        let chorus = self.v(MOD_DEPTH) * MOD_MS * ms;
        // Wow scales with the delay time (a slower transport stretches a longer stretch of
        // tape); flutter is a small fixed jitter.
        let wow = if tape { self.v(WOW) } else { 0.0 };
        let wow_amt = times.map(|t| wow * (0.5 + 0.003 * t) * ms);
        let flutter = if tape { self.v(FLUTTER) * 0.12 * ms } else { 0.0 };
        // Smooth tames the random parts of the modulation.
        let drift_k = super::util::coeff(400.0 + 2000.0 * smooth, sr);
        let jitter_k = super::util::coeff(4.0 + 40.0 * smooth, sr);
        let drive = 0.4 + 2.6 * self.v(SATURATION);
        let dark_k = super::util::coeff(1000.0 / (std::f32::consts::TAU * 5500.0), sr);
        let dc_k = super::util::coeff(1000.0 / (std::f32::consts::TAU * 50.0), sr);

        for n in 0..l.len() {
            approach(&mut self.dry, dry_t, ks);
            approach(&mut self.wet, wet_t, ks);
            approach(&mut self.width, width_t, ks);
            approach(&mut self.gate, gate_t, ks);
            approach(&mut self.fb[0], fb_t[0], ks);
            approach(&mut self.fb[1], fb_t[1], ks);
            approach(&mut self.cross, cross_t, ks);

            // Modulation: a quadrature chorus LFO, plus wow and flutter shared by both channels
            // (one tape transport).
            let ch = self.chorus.tick() * chorus;
            let cc = self.chorus.cos() * chorus;
            let mut tape_mod = [0.0; 2];
            if tape {
                let nz = self.noise();
                approach(&mut self.wow_drift, nz, drift_k);
                let nz = self.noise();
                approach(&mut self.flutter_jitter, nz, jitter_k);
                let w = 0.7 * self.wow.tick() + 6.0 * self.wow_drift;
                let f = 0.6 * self.flutter[0].tick() + 0.25 * self.flutter[1].tick() + 1.5 * self.flutter_jitter;
                // Offset so the modulation only ever lengthens the delay: the set time stays the
                // earliest an echo can arrive.
                let (w, f) = ((1.0 + w).max(0.0), (1.0 + f).max(0.0));
                tape_mod = [w * wow_amt[0] + f * flutter, w * wow_amt[1] + f * flutter];
            }

            for c in 0..2 {
                if clean {
                    self.heads[c].tick_fade(target[c], fade_step);
                } else {
                    self.heads[c].tick_glide(target[c], glide_k, fade_step);
                }
            }
            let yl = self.heads[0].read(&self.lines[0], chorus + ch + tape_mod[0]);
            let yr = self.heads[1].read(&self.lines[1], chorus + cc + tape_mod[1]);

            let (il, ir) = (l[n], r[n]);
            let (mut xl, mut xr) = if mode == PING_PONG {
                (0.5 * (il + ir) * self.gate + self.fb[0] * yr, self.fb[1] * yl)
            } else {
                let c = self.cross;
                (
                    il * self.gate + self.fb[0] * ((1.0 - c) * yl + c * yr),
                    ir * self.gate + self.fb[1] * ((1.0 - c) * yr + c * yl),
                )
            };
            if low_cut {
                xl = self.low_cut[0].process(xl);
                xr = self.low_cut[1].process(xr);
            }
            if high_cut {
                xl = self.high_cut[0].process(xl);
                xr = self.high_cut[1].process(xr);
            }
            let mut x = [xl, xr];
            for c in 0..2 {
                x[c] = if tape {
                    let s = saturate(x[c], drive);
                    self.dark[c] = flush(self.dark[c] + (s - self.dark[c]) * dark_k);
                    self.dc[c] = flush(self.dc[c] + (self.dark[c] - self.dc[c]) * dc_k);
                    self.dark[c] - self.dc[c]
                } else {
                    guard(x[c])
                };
                self.lines[c].write(flush(x[c]));
            }

            let mid = 0.5 * (yl + yr);
            let side = 0.5 * (yl - yr) * self.width;
            l[n] = il * self.dry + (mid + side) * self.wet;
            r[n] = ir * self.dry + (mid - side) * self.wet;
        }
        self.chorus.normalize();
        self.wow.normalize();
        self.flutter[0].normalize();
        self.flutter[1].normalize();
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for b in self.low_cut.iter_mut().chain(&mut self.high_cut) {
            b.clear();
        }
        self.dark = [0.0; 2];
        self.dc = [0.0; 2];
        self.wow_drift = 0.0;
        self.flutter_jitter = 0.0;
        self.chorus.set_phase(0.0);
        self.wow.set_phase(0.0);
        self.flutter[0].set_phase(0.0);
        self.flutter[1].set_phase(1.3);
        self.rng = 0x2545_f491;
        self.primed = false;
    }

    fn tail_secs(&self) -> f32 {
        if self.on(FREEZE) {
            return MAX_TAIL_SECS;
        }
        let times = self.times_ms(self.tempo);
        // A repeat takes the longer time; ping-pong hops alternate between the two.
        let hop = if self.mode() == PING_PONG { 0.5 * (times[0] + times[1]) } else { times[0].max(times[1]) } * 0.001;
        let g = self.feedback().iter().fold(0.0f32, |a, &b| a.max(b));
        if g < 1e-4 {
            return hop;
        }
        if g >= 0.999 {
            return MAX_TAIL_SECS;
        }
        // Repeats until −60 dB.
        let repeats = (1e-3f32).ln() / g.ln();
        ((repeats + 1.0) * hop).min(MAX_TAIL_SECS)
    }
}
