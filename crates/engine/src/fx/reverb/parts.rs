//! What both reverb engines are built from: all-pass stages on a [`Line`], the two-band decay filter
//! and the time conversions.

use super::*;

pub(super) fn ms_to_samples(ms: f32, sample_rate: f32) -> f32 {
    ms * 0.001 * sample_rate
}

/// A buffer long enough for `ms` at a sample rate, with room for interpolation.
pub(super) fn buf_len(ms: f32, sample_rate: f32) -> usize {
    ms_to_samples(ms, sample_rate) as usize + 8
}

/// A Schroeder allpass with a whole-sample delay.
#[inline]
pub(super) fn allpass(line: &mut Line, x: f32, d: usize, g: f32) -> f32 {
    let v = line.read(d);
    let w = flush(x + g * v);
    line.write(w);
    v - g * w
}

/// A Schroeder allpass with a fractional (modulated or gliding) delay.
#[inline]
pub(super) fn allpass_frac(line: &mut Line, x: f32, d: f32, g: f32) -> f32 {
    let v = line.read_frac(d);
    let w = flush(x + g * v);
    line.write(w);
    v - g * w
}

/// The per-line decay: a mid gain with first-order low and high shelves around it.
#[derive(Clone, Copy)]
pub(super) struct DecayFilter {
    pub(super) g: f32,
    /// Low and high gains relative to the mid gain.
    pub(super) low: f32,
    pub(super) high: f32,
    pub(super) a_low: f32,
    pub(super) a_high: f32,
    pub(super) s_low: f32,
    pub(super) s_high: f32,
}

impl DecayFilter {
    pub(super) fn new() -> DecayFilter {
        DecayFilter { g: 0.0, low: 1.0, high: 1.0, a_low: 0.0, a_high: 0.0, s_low: 0.0, s_high: 0.0 }
    }

    /// Sets the gains for a loop `delay` samples long so each band decays by 60 dB in its RT60.
    pub(super) fn set(&mut self, delay: f32, rt_mid: f32, rt_low: f32, rt_high: f32, sample_rate: f32) {
        let gain = |rt: f32| 10f32.powf(-3.0 * delay / (rt * sample_rate));
        self.g = gain(rt_mid);
        self.low = gain(rt_low) / self.g;
        self.high = gain(rt_high) / self.g;
        self.a_low = 1.0 - (-std::f32::consts::TAU * LOW_XOVER_HZ / sample_rate).exp();
        self.a_high = 1.0 - (-std::f32::consts::TAU * HIGH_XOVER_HZ / sample_rate).exp();
    }

    #[inline]
    pub(super) fn process(&mut self, x: f32) -> f32 {
        self.s_low = flush(self.s_low + self.a_low * (x - self.s_low));
        let x = x + (self.low - 1.0) * self.s_low;
        self.s_high = flush(self.s_high + self.a_high * (x - self.s_high));
        let hp = x - self.s_high;
        self.g * (self.s_high + self.high * hp)
    }

    pub(super) fn clear(&mut self) {
        self.s_low = 0.0;
        self.s_high = 0.0;
    }
}

/// Decay settings shared by both engines.
#[derive(Clone, Copy)]
pub(super) struct Rt {
    pub(super) mid: f32,
    pub(super) low: f32,
    pub(super) high: f32,
}
