//! Loudness Meter: EBU R128 / ITU-R BS.1770-4 loudness, loudness range and true peak, after
//! Logic's Loudness Meter. The audio passes through untouched.
//!
//! - **K-weighting:** the two BS.1770 stages (a high shelf and a high-pass), derived from their
//!   analog parameters for the actual sample rate. At 48 kHz they reproduce the standard's
//!   coefficient table.
//! - **Windows:** the K-weighted power is summed in 100 ms sub-blocks. Momentary loudness is
//!   the last 4 (400 ms), short-term the last 30 (3 s); both update every 100 ms.
//! - **Integrated:** gating blocks of 400 ms every 100 ms, with the -70 LUFS absolute gate and the
//!   -10 LU relative gate. The blocks go into a histogram of 0.1 LU bins over -70..+5 LUFS that
//!   keeps each bin's count and summed power, so the gated mean is exact (only blocks in the bin
//!   holding the relative gate are rounded) and nothing grows with the programme length.
//! - **Loudness range:** EBU Tech 3342, from a second histogram of short-term values: the -70
//!   absolute and -20 LU relative gates, then the 10th to 95th percentile spread.
//! - **True peak:** 4x oversampling with a windowed-sinc interpolator, maximum since reset.
//!
//! The integrated and range measurements only run while the transport plays, so the time since
//! reset or play start is what they cover; momentary, short-term and the peaks always run.

use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit, MAX_METERS};
use std::f64::consts::PI;

pub const TARGET: u32 = 0;
pub const CUSTOM_TARGET: u32 = 1;
pub const SCALE: u32 = 2;
pub const RESET: u32 = 3;
pub const AUTO_RESET: u32 = 4;

/// The loudness targets offered by the Target choice, in LUFS (the last entry is Custom).
pub const TARGETS: [f32; 4] = [-23.0, -16.0, -14.0, -24.0];

static PARAMS: [ParamSpec; 5] = [
    ParamSpec {
        id: TARGET,
        name: "Target",
        min: 0.0,
        max: 4.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Choice(&["−23 LUFS (EBU R128)", "−16 LUFS (Apple Music)", "−14 LUFS (Spotify, YouTube)", "−24 LUFS (ATSC A/85)", "Custom"]),
        log: false,
    },
    ParamSpec { id: CUSTOM_TARGET, name: "Custom Target", min: -36.0, max: -6.0, default: -23.0, unit: Unit::Db, kind: ParamKind::Continuous, log: false },
    ParamSpec { id: SCALE, name: "Scale", min: 0.0, max: 1.0, default: 0.0, unit: Unit::None, kind: ParamKind::Choice(&["EBU +9", "EBU +18"]), log: false },
    ParamSpec { id: RESET, name: "Reset", min: 0.0, max: 1.0, default: 0.0, unit: Unit::None, kind: ParamKind::Toggle, log: false },
    ParamSpec { id: AUTO_RESET, name: "Auto-Reset on Play", min: 0.0, max: 1.0, default: 1.0, unit: Unit::None, kind: ParamKind::Toggle, log: false },
];

/// Meter order: momentary, short-term and integrated loudness (LUFS), loudness range (LU), true
/// peak since reset (dBTP), and the current sample peaks of the left and right channels (dBFS).
/// Loudness and peaks read [`FLOOR`] when there is nothing to measure.
static METERS: [&str; 7] = ["momentary", "short_term", "integrated", "range", "true_peak", "peak_l", "peak_r"];

/// The Target choice index for Custom.
const CUSTOM: f32 = TARGETS.len() as f32;

/// Factory presets, one per delivery target. Each lists only what it changes from the defaults
/// (EBU R128 target, EBU +9 scale, Auto-Reset on).
#[rustfmt::skip] // One row per preset's values: split per value, these tables run to thousands of lines.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "EBU R128 (−23 LUFS)", values: &[(TARGET, 0.0)] },
    FactoryPreset { name: "Spotify / YouTube (−14)", values: &[(TARGET, 2.0), (SCALE, 1.0)] },
    FactoryPreset { name: "Apple Music (−16)", values: &[(TARGET, 1.0), (SCALE, 1.0)] },
    FactoryPreset { name: "ATSC A/85 (−24)", values: &[(TARGET, 3.0)] },
    FactoryPreset { name: "Podcast (−16)", values: &[(TARGET, CUSTOM), (CUSTOM_TARGET, -16.0), (SCALE, 1.0)] },
    FactoryPreset { name: "CD Master (−9)", values: &[(TARGET, CUSTOM), (CUSTOM_TARGET, -9.0), (SCALE, 1.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:loudness",
        name: "Loudness Meter",
        category: "Metering",
        params: &PARAMS,
        meters: &METERS,
        sidechain: false,
        new: |sr| Box::new(Loudness::new(sr)),
    }
}

/// What the meters read for silence or no measurement yet.
pub const FLOOR: f32 = -120.0;

const ABS_GATE: f64 = -70.0;
const HIST_MIN: f64 = ABS_GATE;
const HIST_STEP: f64 = 0.1;
/// -70..+5 LUFS in 0.1 LU bins; anything louder lands in the top bin.
const HIST_BINS: usize = 750;
const MOMENTARY_SUBS: usize = 4;
const SHORT_TERM_SUBS: usize = 30;
/// Taps per phase of the true-peak interpolator.
const TP_TAPS: usize = 16;
const TP_PHASES: usize = 4;
/// The sample peak meters fall at this rate (dB per second).
const PEAK_FALL_DB: f32 = 15.0;

/// BS.1770 loudness of a summed channel power.
fn lufs(power: f64) -> f64 {
    if power > 0.0 {
        -0.691 + 10.0 * power.log10()
    } else {
        f64::NEG_INFINITY
    }
}

fn meter(v: f64) -> f32 {
    (v as f32).max(FLOOR)
}

/// A biquad in transposed direct form II, run in f64.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// The K-weighting stages for a sample rate: the high shelf that models the head, then the
/// RLB high-pass. The analog parameters are those that reproduce the BS.1770-4 48 kHz table
/// through the bilinear transform (as libebur128 derives them).
fn k_weighting(sr: f64) -> [Biquad; 2] {
    let (f0, gain_db, q) = (1_681.974_450_955_533, 3.999_843_853_973_347, 0.707_175_236_955_419_6);
    let k = (PI * f0 / sr).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
        ..Biquad::default()
    };
    let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = (PI * f0 / sr).tan();
    let a0 = 1.0 + k / q + k * k;
    let highpass = Biquad { b0: 1.0, b1: -2.0, b2: 1.0, a1: 2.0 * (k * k - 1.0) / a0, a2: (1.0 - k / q + k * k) / a0, ..Biquad::default() };
    [shelf, highpass]
}

/// Loudness values binned by 0.1 LU, with each bin's summed power for exact power averages.
struct Histogram {
    count: [u32; HIST_BINS],
    power: [f64; HIST_BINS],
}

impl Histogram {
    fn new() -> Histogram {
        Histogram { count: [0; HIST_BINS], power: [0.0; HIST_BINS] }
    }

    fn clear(&mut self) {
        self.count.fill(0);
        self.power.fill(0.0);
    }

    fn bin(loudness: f64) -> usize {
        (((loudness - HIST_MIN) / HIST_STEP).floor().max(0.0) as usize).min(HIST_BINS - 1)
    }

    /// Adds a block's power, applying the absolute gate.
    fn add(&mut self, power: f64) {
        let l = lufs(power);
        if l > ABS_GATE {
            let i = Histogram::bin(l);
            self.count[i] += 1;
            self.power[i] += power;
        }
    }

    /// The first bin at or above the relative gate `offset` LU below the power average of
    /// everything stored, with the number of values at and above it.
    fn relative_gate(&self, offset: f64) -> Option<(usize, u64)> {
        let n: u64 = self.count.iter().map(|&c| c as u64).sum();
        if n == 0 {
            return None;
        }
        let total: f64 = self.power.iter().sum();
        let start = Histogram::bin(lufs(total / n as f64) - offset);
        let kept: u64 = self.count[start..].iter().map(|&c| c as u64).sum();
        (kept > 0).then_some((start, kept))
    }

    /// BS.1770 integrated loudness of the stored gating blocks.
    fn integrated(&self) -> Option<f64> {
        let (start, n) = self.relative_gate(10.0)?;
        let power: f64 = self.power[start..].iter().sum();
        Some(lufs(power / n as f64))
    }

    /// EBU Tech 3342 loudness range of the stored short-term values.
    fn range(&self) -> f64 {
        let Some((start, n)) = self.relative_gate(20.0) else { return 0.0 };
        let percentile = |p: f64| {
            let rank = ((n - 1) as f64 * p).round() as u64;
            let mut seen = 0u64;
            for i in start..HIST_BINS {
                seen += self.count[i] as u64;
                if seen > rank {
                    return HIST_MIN + (i as f64 + 0.5) * HIST_STEP;
                }
            }
            HIST_MIN + HIST_BINS as f64 * HIST_STEP
        };
        percentile(0.95) - percentile(0.10)
    }
}

/// One channel's true-peak detector: the latest samples, doubled so the newest `TP_TAPS` are
/// always one contiguous slice.
struct TruePeak {
    hist: [f32; 2 * TP_TAPS],
    pos: usize,
}

impl TruePeak {
    fn new() -> TruePeak {
        TruePeak { hist: [0.0; 2 * TP_TAPS], pos: 0 }
    }

    /// Takes one sample and returns the largest magnitude among it and the interpolated points
    /// between the samples around the interpolator's centre.
    #[inline]
    fn push(&mut self, x: f32, taps: &[[f32; TP_TAPS]; TP_PHASES]) -> f32 {
        self.hist[self.pos] = x;
        self.hist[self.pos + TP_TAPS] = x;
        self.pos = (self.pos + 1) % TP_TAPS;
        let window = &self.hist[self.pos..self.pos + TP_TAPS];
        let mut peak = x.abs();
        for phase in &taps[1..] {
            let y: f32 = window.iter().zip(phase).map(|(a, b)| a * b).sum();
            peak = peak.max(y.abs());
        }
        peak
    }
}

/// Kaiser-windowed sinc phases for interpolating at quarter-sample offsets. Phase p estimates
/// the signal p/4 of a sample after the window's centre sample; phase 0 is that sample itself.
fn true_peak_taps() -> [[f32; TP_TAPS]; TP_PHASES] {
    fn bessel_i0(x: f64) -> f64 {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..30 {
            term *= (x / (2.0 * k as f64)).powi(2);
            sum += term;
        }
        sum
    }
    let beta = 7.0;
    let half = TP_TAPS as f64 / 2.0;
    let centre = half - 1.0;
    let mut taps = [[0.0f32; TP_TAPS]; TP_PHASES];
    for (p, phase) in taps.iter_mut().enumerate() {
        let t = centre + p as f64 / TP_PHASES as f64;
        let mut h = [0.0f64; TP_TAPS];
        for (j, v) in h.iter_mut().enumerate() {
            let u = t - j as f64;
            let sinc = if u.abs() < 1e-12 { 1.0 } else { (PI * u).sin() / (PI * u) };
            let w = (1.0 - (u / half).powi(2)).max(0.0).sqrt();
            *v = sinc * bessel_i0(beta * w) / bessel_i0(beta);
        }
        let sum: f64 = h.iter().sum();
        for (o, v) in phase.iter_mut().zip(h) {
            *o = (v / sum) as f32;
        }
    }
    taps
}

pub struct Loudness {
    target: f32,
    custom_target: f32,
    scale: f32,
    reset_held: bool,
    auto_reset: bool,

    k: [[Biquad; 2]; 2],
    tp_taps: [[f32; TP_TAPS]; TP_PHASES],
    tp: [TruePeak; 2],
    sub_len: usize,
    sub_n: usize,
    sub_power: f64,
    /// The summed K-weighted power of the latest sub-blocks (a ring, newest at `sub_pos - 1`).
    subs: [f64; SHORT_TERM_SUBS],
    sub_pos: usize,
    sub_count: usize,
    /// Consecutive sub-blocks measured while playing since the last measurement reset.
    run: usize,
    was_playing: bool,
    blocks: Box<Histogram>,
    short_terms: Box<Histogram>,

    momentary: f32,
    short_term: f32,
    integrated: f32,
    range: f32,
    true_peak: f32,
    peak: [f32; 2],
    sr: f32,
}

impl Loudness {
    pub fn new(sample_rate: f32) -> Loudness {
        let k = k_weighting(sample_rate as f64);
        Loudness {
            target: 0.0,
            custom_target: -23.0,
            scale: 0.0,
            reset_held: false,
            auto_reset: true,
            k: [k, k],
            tp_taps: true_peak_taps(),
            tp: [TruePeak::new(), TruePeak::new()],
            sub_len: (sample_rate as f64 * 0.1).round().max(1.0) as usize,
            sub_n: 0,
            sub_power: 0.0,
            subs: [0.0; SHORT_TERM_SUBS],
            sub_pos: 0,
            sub_count: 0,
            run: 0,
            was_playing: false,
            blocks: Box::new(Histogram::new()),
            short_terms: Box::new(Histogram::new()),
            momentary: FLOOR,
            short_term: FLOOR,
            integrated: FLOOR,
            range: 0.0,
            true_peak: 0.0,
            peak: [0.0; 2],
            sr: sample_rate,
        }
    }

    /// Starts a new integrated / range / true-peak measurement.
    fn clear_measurement(&mut self) {
        self.blocks.clear();
        self.short_terms.clear();
        self.integrated = FLOOR;
        self.range = 0.0;
        self.true_peak = 0.0;
        self.run = 0;
        // The first gating block starts here, not part-way through a sub-block.
        self.sub_n = 0;
        self.sub_power = 0.0;
    }

    /// The mean power of the latest `n` sub-blocks (fewer right after a reset).
    fn window(&self, n: usize) -> f64 {
        let n = n.min(self.sub_count);
        if n == 0 {
            return 0.0;
        }
        let sum: f64 = (1..=n).map(|i| self.subs[(self.sub_pos + SHORT_TERM_SUBS - i) % SHORT_TERM_SUBS]).sum();
        sum / (n * self.sub_len) as f64
    }

    fn end_sub_block(&mut self, playing: bool) {
        self.subs[self.sub_pos] = self.sub_power;
        self.sub_pos = (self.sub_pos + 1) % SHORT_TERM_SUBS;
        self.sub_count = (self.sub_count + 1).min(SHORT_TERM_SUBS);
        self.sub_power = 0.0;
        self.sub_n = 0;
        let (m, s) = (self.window(MOMENTARY_SUBS), self.window(SHORT_TERM_SUBS));
        self.momentary = meter(lufs(m));
        self.short_term = meter(lufs(s));
        if !playing {
            self.run = 0;
            return;
        }
        self.run += 1;
        if self.run >= MOMENTARY_SUBS {
            self.blocks.add(m);
            self.integrated = self.blocks.integrated().map_or(FLOOR, meter);
        }
        if self.run >= SHORT_TERM_SUBS {
            self.short_terms.add(s);
            self.range = self.short_terms.range() as f32;
        }
    }

    /// The target in LUFS.
    pub fn target_lufs(&self) -> f32 {
        TARGETS.get(self.target as usize).copied().unwrap_or(self.custom_target)
    }
}

impl Effect for Loudness {
    fn set_param(&mut self, id: u32, value: f32) {
        match id {
            TARGET => self.target = value.round(),
            CUSTOM_TARGET => self.custom_target = value,
            SCALE => self.scale = value.round(),
            RESET => {
                // The panel pulses this; only the rising edge resets.
                let on = value >= 0.5;
                if on && !self.reset_held {
                    self.clear_measurement();
                }
                self.reset_held = on;
            }
            AUTO_RESET => self.auto_reset = value >= 0.5,
            _ => {}
        }
    }

    fn param(&self, id: u32) -> f32 {
        match id {
            TARGET => self.target,
            CUSTOM_TARGET => self.custom_target,
            SCALE => self.scale,
            RESET => self.reset_held as u8 as f32,
            AUTO_RESET => self.auto_reset as u8 as f32,
            _ => 0.0,
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        if ctx.playing && !self.was_playing && self.auto_reset {
            self.clear_measurement();
        }
        self.was_playing = ctx.playing;

        let mut block_peak = [0.0f32; 2];
        let mut block_tp = 0.0f32;
        for (&a, &b) in l.iter().zip(r.iter()) {
            let [[shelf_l, hp_l], [shelf_r, hp_r]] = &mut self.k;
            let yl = hp_l.run(shelf_l.run(a as f64));
            let yr = hp_r.run(shelf_r.run(b as f64));
            self.sub_power += yl * yl + yr * yr;
            block_peak[0] = block_peak[0].max(a.abs());
            block_peak[1] = block_peak[1].max(b.abs());
            block_tp = block_tp.max(self.tp[0].push(a, &self.tp_taps)).max(self.tp[1].push(b, &self.tp_taps));
            self.sub_n += 1;
            if self.sub_n == self.sub_len {
                self.end_sub_block(ctx.playing);
            }
        }
        self.true_peak = self.true_peak.max(block_tp);
        let fall = 10f32.powf(-PEAK_FALL_DB * l.len() as f32 / self.sr / 20.0);
        for (p, b) in self.peak.iter_mut().zip(block_peak) {
            *p = (*p * fall).max(b);
        }
        // Keep denormals out of the filters after the input goes silent.
        for ch in &mut self.k {
            for f in ch {
                if f.z1.abs() < 1e-30 && f.z2.abs() < 1e-30 {
                    f.clear();
                }
            }
        }
    }

    /// Clears the filters and the sliding windows. The integrated, range and true-peak
    /// measurement survives (the Reset command or play start clears it), so a host reset on
    /// stop or locate doesn't throw away a whole pass's reading.
    fn reset(&mut self) {
        for ch in &mut self.k {
            for f in ch {
                f.clear();
            }
        }
        self.tp = [TruePeak::new(), TruePeak::new()];
        self.sub_n = 0;
        self.sub_power = 0.0;
        self.subs = [0.0; SHORT_TERM_SUBS];
        self.sub_count = 0;
        self.run = 0;
        self.momentary = FLOOR;
        self.short_term = FLOOR;
        self.peak = [0.0; 2];
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        let db = |g: f32| if g > 0.0 { (20.0 * g.log10()).max(FLOOR) } else { FLOOR };
        out[0] = self.momentary;
        out[1] = self.short_term;
        out[2] = self.integrated;
        out[3] = self.range;
        out[4] = db(self.true_peak);
        out[5] = db(self.peak[0]);
        out[6] = db(self.peak[1]);
    }
}

#[cfg(test)]
mod tests;
