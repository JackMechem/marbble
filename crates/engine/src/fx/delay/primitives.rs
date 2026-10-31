//! The small building blocks the Delay shares with the Reverb and others: the fractional delay
//! [`Line`], a [`Biquad`], a quadrature oscillator ([`Osc`]), [`flush`], and tempo-sync note values.
//!
//! Called once per sample per channel, so they are `#[inline]` where it counts.

/// The highest sample rate buffers are sized for.
pub(crate) const MAX_RATE: f32 = 192_000.0;

/// Flushes values below −300 dB to exactly zero, so decaying feedback loops never reach
/// subnormal floats (which are very slow on x86) and can't get stuck in a rounding limit cycle
/// just above zero.
#[inline]
pub(crate) fn flush(x: f32) -> f32 {
    if x.abs() < 1e-15 {
        0.0
    } else {
        x
    }
}

/// A ring-buffer delay line.
///
/// The buffer is allocated once for the largest size at [`MAX_RATE`]; `set_active` picks how much
/// of it is in use for the actual sample rate, so memory pages the line never wraps into are
/// never touched.
pub(crate) struct Line {
    buf: Vec<f32>,
    len: usize,
    w: usize,
}

impl Line {
    pub(crate) fn new(capacity: usize) -> Line {
        let capacity = capacity.max(8);
        Line { buf: vec![0.0; capacity], len: capacity, w: 0 }
    }

    /// Uses the first `len` samples of the buffer, and clears them.
    pub(crate) fn set_active(&mut self, len: usize) {
        self.len = len.clamp(8, self.buf.len());
        self.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.buf[..self.len].fill(0.0);
        self.w = 0;
    }

    /// The longest delay `read_frac` can serve.
    pub(crate) fn max_delay(&self) -> f32 {
        (self.len - 3) as f32
    }

    #[inline]
    pub(crate) fn write(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w += 1;
        if self.w == self.len {
            self.w = 0;
        }
    }

    /// The sample written `d` writes ago: 1 is the most recent. Read before writing to get a
    /// delay of exactly `d` samples.
    #[inline]
    pub(crate) fn read(&self, d: usize) -> f32 {
        let d = d.min(self.len);
        let i = if d > self.w { self.w + self.len - d } else { self.w - d };
        self.buf[i]
    }

    /// A fractional read with linear interpolation (for taps that only drift slowly).
    #[inline]
    pub(crate) fn read_lin(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, self.max_delay());
        let di = d as usize;
        let f = d - di as f32;
        let a = self.read(di);
        a + (self.read(di + 1) - a) * f
    }

    /// A fractional read with 4-point (Catmull-Rom) Hermite interpolation, for modulated and
    /// gliding delays. Its gain never exceeds 1, so it is safe inside feedback loops, and an
    /// integer delay reads back exactly.
    #[inline]
    pub(crate) fn read_frac(&self, d: f32) -> f32 {
        let d = d.clamp(2.0, self.max_delay());
        let di = d as usize;
        let f = d - di as f32;
        let xm1 = self.read(di - 1);
        let x0 = self.read(di);
        if f == 0.0 {
            return x0;
        }
        let x1 = self.read(di + 1);
        let x2 = self.read(di + 2);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }

    #[cfg(test)]
    pub(crate) fn has_subnormal(&self) -> bool {
        self.buf.iter().any(|x| x.is_subnormal())
    }
}

/// An RBJ-cookbook biquad (transposed direct form II).
#[derive(Clone, Copy)]
pub(crate) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl Biquad {
    pub(crate) fn new() -> Biquad {
        Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, s1: 0.0, s2: 0.0 }
    }

    pub(super) fn set(&mut self, b: [f64; 3], a: [f64; 3]) {
        self.b0 = (b[0] / a[0]) as f32;
        self.b1 = (b[1] / a[0]) as f32;
        self.b2 = (b[2] / a[0]) as f32;
        self.a1 = (a[1] / a[0]) as f32;
        self.a2 = (a[2] / a[0]) as f32;
    }

    /// A 12 dB/oct Butterworth low-pass. The state is kept so sweeps don't click.
    pub(crate) fn set_lowpass(&mut self, freq: f32, sample_rate: f32) {
        let w = std::f64::consts::TAU * (freq.min(sample_rate * 0.45) as f64) / sample_rate as f64;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        self.set([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]);
    }

    /// A 12 dB/oct Butterworth high-pass.
    pub(crate) fn set_highpass(&mut self, freq: f32, sample_rate: f32) {
        let w = std::f64::consts::TAU * (freq.min(sample_rate * 0.45) as f64) / sample_rate as f64;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        self.set([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]);
    }

    #[inline]
    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s1;
        self.s1 = flush(self.b1 * x - self.a1 * y + self.s2);
        self.s2 = flush(self.b2 * x - self.a2 * y);
        y
    }

    pub(crate) fn clear(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    #[cfg(test)]
    pub(crate) fn has_subnormal(&self) -> bool {
        self.s1.is_subnormal() || self.s2.is_subnormal()
    }
}

/// A sine oscillator by rotation: two multiplies per sample, no `sin()` calls.
#[derive(Clone, Copy)]
pub(crate) struct Osc {
    s: f32,
    c: f32,
    ks: f32,
    kc: f32,
}

impl Osc {
    pub(crate) fn new(phase: f32) -> Osc {
        Osc { s: phase.sin(), c: phase.cos(), ks: 0.0, kc: 1.0 }
    }

    /// Restarts at `phase`, keeping the frequency, so a reset effect renders the same every time.
    pub(crate) fn set_phase(&mut self, phase: f32) {
        self.s = phase.sin();
        self.c = phase.cos();
    }

    pub(crate) fn set_freq(&mut self, hz: f32, sample_rate: f32) {
        let w = std::f32::consts::TAU * hz / sample_rate;
        self.ks = w.sin();
        self.kc = w.cos();
    }

    /// Advances one sample and returns the sine.
    #[inline]
    pub(crate) fn tick(&mut self) -> f32 {
        let s = self.s * self.kc + self.c * self.ks;
        self.c = self.c * self.kc - self.s * self.ks;
        self.s = s;
        s
    }

    /// The cosine of the current phase (the quadrature output).
    #[inline]
    pub(crate) fn cos(&self) -> f32 {
        self.c
    }

    /// Pulls the amplitude back to 1; rounding makes a rotation drift slowly. Call once a block.
    pub(crate) fn normalize(&mut self) {
        let g = 1.5 - 0.5 * (self.s * self.s + self.c * self.c);
        self.s *= g;
        self.c *= g;
    }
}

/// Tempo-sync note values, as shown in the choice pop-ups ("." dotted, "T" triplet).
pub(crate) const NOTE_LABELS: &[&str] =
    &["1/1", "1/1.", "1/1T", "1/2", "1/2.", "1/2T", "1/4", "1/4.", "1/4T", "1/8", "1/8.", "1/8T", "1/16", "1/16.", "1/16T", "1/32", "1/32.", "1/32T"];

/// The length of a note value in quarter-note beats.
pub(crate) fn note_beats(index: usize) -> f64 {
    let base = [4.0, 2.0, 1.0, 0.5, 0.25, 0.125][(index / 3).min(5)];
    match index % 3 {
        0 => base,
        1 => base * 1.5,
        _ => base * 2.0 / 3.0,
    }
}

/// The length of a note value in ms at a tempo.
pub(crate) fn note_ms(index: usize, tempo: f64) -> f32 {
    (note_beats(index) * 60_000.0 / tempo.max(1.0)) as f32
}
