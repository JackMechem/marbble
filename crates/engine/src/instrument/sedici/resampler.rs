//! The output resampler: brings the instrument's own rate (`DX_RATE`) to the session's, through
//! a Kaiser-windowed sinc, so the folding stays the instrument's and adds none of its own.

use super::*;

/// Brings the instrument's rate to the session's: a Kaiser-windowed sinc, its corner under both
/// rates' Nyquist, per phase and interpolated between phases.
pub(super) struct Resampler {
    pub(super) table: Vec<f32>,
    pub(super) ring: [f32; TAPS],
    pub(super) w: usize,
    /// How far into the next input tick the output sample lies, 0..1.
    pub(super) pos: f64,
    pub(super) step: f64,
}

pub(super) fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0f64, 1.0f64);
    for k in 1..40 {
        term *= (x / (2.0 * k as f64)) * (x / (2.0 * k as f64));
        sum += term;
    }
    sum
}

impl Resampler {
    pub(super) fn new(sample_rate: f32) -> Resampler {
        let host = sample_rate as f64;
        // In cycles per tick of the instrument's rate.
        let corner = (0.45 * host.min(DX_RATE)) / DX_RATE;
        let beta = 7.0;
        let half = TAPS as f64 / 2.0;
        let mut table = vec![0.0f32; (PHASES + 1) * TAPS];
        for p in 0..=PHASES {
            let frac = p as f64 / PHASES as f64;
            for t in 0..TAPS {
                // Tap t is `t + 1 - half` ticks before... the ring's newest is at the end, and the
                // output lies `frac` of a tick ahead of the middle.
                let x = t as f64 - half + 1.0 - frac;
                let s = if x.abs() < 1e-9 { 2.0 * corner } else { (std::f64::consts::TAU * corner * x).sin() / (std::f64::consts::PI * x) };
                let r = (x / half).clamp(-1.0, 1.0);
                let w = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta);
                table[p * TAPS + t] = (s * w) as f32;
            }
            // Unity at DC at every phase.
            let row = &mut table[p * TAPS..(p + 1) * TAPS];
            let sum: f32 = row.iter().sum();
            for v in row.iter_mut() {
                *v /= sum;
            }
        }
        Resampler { table, ring: [0.0; TAPS], w: 0, pos: 0.0, step: DX_RATE / host }
    }

    #[inline]
    pub(super) fn push(&mut self, x: f32) {
        self.ring[self.w] = x;
        self.w = (self.w + 1) % TAPS;
    }

    #[inline]
    pub(super) fn read(&self) -> f32 {
        let at = self.pos * PHASES as f64;
        let p = at as usize;
        let f = (at - p as f64) as f32;
        let (a, b) = (&self.table[p * TAPS..(p + 1) * TAPS], &self.table[(p + 1) * TAPS..(p + 2) * TAPS]);
        let mut sum = 0.0f32;
        for t in 0..TAPS {
            let v = self.ring[(self.w + t) % TAPS];
            sum += v * (a[t] + (b[t] - a[t]) * f);
        }
        sum
    }

    pub(super) fn clear(&mut self) {
        self.ring = [0.0; TAPS];
        self.w = 0;
        self.pos = 0.0;
    }
}
