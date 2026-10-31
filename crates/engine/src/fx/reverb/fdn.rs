//! The feedback delay network behind the Room, Hall, Chamber, Cathedral and Ambience types: the table
//! of rooms, sixteen delay lines with in-loop all-passes and a Hadamard mixing matrix.

use super::*;

/// A room type's geometry at 1× size.
pub(super) struct RoomDef {
    /// The shortest and longest network line, in ms.
    pub(super) lines: (f32, f32),
    /// The in-loop allpasses' nominal length, in ms.
    pub(super) allpass_ms: f32,
    /// The input diffusers' length scale.
    pub(super) diffuser: f32,
    /// Early reflections: tap count, first and last tap in ms, and the pattern's seed.
    /// `src/components/fx/Reverb.tsx` draws the same pattern; keep the two in step.
    pub(super) er: (usize, f32, f32, u32),
    /// When the late reverb starts after the pre-delay, in ms.
    pub(super) late_ms: f32,
}

pub(super) const ROOMS: [RoomDef; 6] = [
    // Plate: the network fields are unused. A real plate has no early reflections; the short
    // dense spray stands in for the onset of the plate's own diffusion.
    RoomDef { lines: (1.0, 1.0), allpass_ms: 1.0, diffuser: 1.0, er: (18, 0.7, 13.0, 0x68e3_1da4), late_ms: 0.0 },
    // Room
    RoomDef { lines: (9.0, 38.0), allpass_ms: 1.7, diffuser: 0.8, er: (14, 2.5, 42.0, 0x02f6_e2b1), late_ms: 8.0 },
    // Hall
    RoomDef { lines: (21.0, 97.0), allpass_ms: 3.1, diffuser: 1.3, er: (12, 8.0, 85.0, 0x9e37_79b9), late_ms: 22.0 },
    // Chamber
    RoomDef { lines: (13.0, 54.0), allpass_ms: 2.3, diffuser: 1.0, er: (16, 3.5, 58.0, 0x7f4a_7c15), late_ms: 12.0 },
    // Cathedral
    RoomDef { lines: (37.0, 158.0), allpass_ms: 4.4, diffuser: 1.6, er: (10, 16.0, 150.0, 0x5bd1_e995), late_ms: 40.0 },
    // Ambience
    RoomDef { lines: (4.3, 17.0), allpass_ms: 0.9, diffuser: 0.55, er: (20, 1.1, 22.0, 0x1b87_3593), late_ms: 3.0 },
];

const N: usize = 16;
pub(super) const MAX_TAPS: usize = 20;

/// Small irregular offsets so the line lengths aren't a clean geometric series.
const LINE_JITTER: [f32; N] = [0.0, 0.31, -0.22, 0.17, -0.35, 0.26, -0.08, 0.39, -0.29, 0.12, -0.18, 0.33, -0.37, 0.05, 0.24, -0.13];
/// The order lines take their lengths in, so neighbours in the matrix differ a lot.
const LINE_ORDER: [usize; N] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
/// Each in-loop allpass's length relative to the room's nominal one.
const ALLPASS_SPREAD: [f32; N] = [0.62, 1.31, 0.84, 1.12, 0.71, 1.38, 0.93, 1.05, 0.66, 1.24, 0.79, 1.17, 0.88, 1.43, 0.74, 1.0];
/// The input diffusers per channel, in ms at a diffuser scale of 1.
const DIFFUSER_MS: [[f32; 4]; 2] = [[1.37, 2.13, 3.73, 5.51], [1.53, 2.37, 3.31, 5.87]];

/// The Hadamard matrix entry: rows and columns are mutually orthogonal ±1 vectors.
const fn hadamard(row: usize, col: usize) -> f32 {
    if (row & col).count_ones().is_multiple_of(2) { 1.0 } else { -1.0 }
}

/// The next value of the shared pattern generator (an LCG, the same one the panel uses).
fn lcg(s: &mut u32) -> f32 {
    *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*s >> 8) as f32 / 16_777_216.0
}

/// One channel's early reflection pattern at 1× size: tap times (ms) and gains.
pub(super) fn er_pattern(def: &RoomDef, channel: u32) -> ([f32; MAX_TAPS], [f32; MAX_TAPS]) {
    let (n, t0, t1, seed) = def.er;
    let mut times = [0.0; MAX_TAPS];
    let mut gains = [0.0; MAX_TAPS];
    let mut s = seed.wrapping_add(channel * 7919);
    let mut energy = 0.0;
    for k in 0..n.min(MAX_TAPS) {
        let pos = (k as f32 + 0.2 + 0.6 * lcg(&mut s)) / n as f32;
        times[k] = t0 + (t1 - t0) * pos;
        let amp = 0.2f32.powf(pos) * (0.55 + 0.45 * lcg(&mut s));
        gains[k] = if lcg(&mut s) < 0.5 { -amp } else { amp };
        energy += amp * amp;
    }
    if energy > 0.0 {
        let norm = 0.8 / energy.sqrt();
        gains.iter_mut().for_each(|g| *g *= norm);
    }
    (times, gains)
}

fn is_prime(n: usize) -> bool {
    if n < 4 {
        return n > 1;
    }
    if n.is_multiple_of(2) {
        return false;
    }
    let mut d = 3;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 2;
    }
    true
}

/// Line lengths are rounded up to a prime so no two share a factor (fewer coinciding echoes).
pub(super) fn next_prime(n: usize) -> usize {
    let mut n = n.max(2);
    while !is_prime(n) {
        n += 1;
    }
    n
}

pub(super) struct Fdn {
    lines: [Line; N],
    allpasses: [Line; N],
    filters: [DecayFilter; N],
    /// The current (gliding) and target line lengths in samples.
    cur: [f32; N],
    target: [f32; N],
    allpass_len: [usize; N],
    osc: [Osc; N],
    diffusers: [[Line; 4]; 2],
    diffuser_len: [[usize; 4]; 2],
}

impl Fdn {
    /// Buffer sizes in samples at a sample rate: lines, in-loop allpasses, input diffusers.
    fn sizes(sample_rate: f32) -> [usize; 3] {
        let max = |f: &dyn Fn(&RoomDef) -> f32| ROOMS.iter().map(f).fold(0.0, f32::max);
        let line_ms = max(&|r| r.lines.1) * 1.06 * SIZE_MAX + MOD_MS * 2.0 + 1.0;
        let ap_ms = max(&|r| r.allpass_ms) * 1.5;
        let diff_ms = max(&|r| r.diffuser) * 6.0;
        [line_ms, ap_ms, diff_ms].map(|ms| buf_len(ms, sample_rate))
    }

    pub(super) fn new(alloc_rate: f32) -> Fdn {
        let [line, ap, diff] = Fdn::sizes(alloc_rate);
        Fdn {
            lines: std::array::from_fn(|_| Line::new(line)),
            allpasses: std::array::from_fn(|_| Line::new(ap)),
            filters: [DecayFilter::new(); N],
            cur: [100.0; N],
            target: [100.0; N],
            allpass_len: [8; N],
            osc: std::array::from_fn(|i| Osc::new(i as f32 * 2.399)),
            diffusers: std::array::from_fn(|_| std::array::from_fn(|_| Line::new(diff))),
            diffuser_len: [[8; 4]; 2],
        }
    }

    /// Sizes the buffers in use for a sample rate (the room with the longest lines at full size).
    pub(super) fn set_rate(&mut self, sample_rate: f32) {
        let [line, ap, diff] = Fdn::sizes(sample_rate);
        self.lines.iter_mut().for_each(|l| l.set_active(line));
        self.allpasses.iter_mut().for_each(|l| l.set_active(ap));
        self.diffusers.iter_mut().flatten().for_each(|l| l.set_active(diff));
    }

    /// Sets the line lengths for a room and size. With `snap` they jump there; otherwise they
    /// glide (a size change while playing).
    // Indexes parallel per-channel or per-line arrays together: an index reads best here.
    #[allow(clippy::needless_range_loop)]
    pub(super) fn configure(&mut self, def: &RoomDef, scale: f32, sample_rate: f32, snap: bool) {
        let (lo, hi) = def.lines;
        for i in 0..N {
            let pos = (LINE_ORDER[i] as f32 + LINE_JITTER[i]).clamp(0.0, 15.0) / 15.0;
            let ms = lo * (hi / lo).powf(pos) * scale;
            let samples = ms_to_samples(ms, sample_rate) as usize;
            let max = (self.lines[i].max_delay() - ms_to_samples(MOD_MS * 2.0, sample_rate)) as usize;
            self.target[i] = next_prime(samples.max(8)).min(max) as f32;
            if snap {
                self.cur[i] = self.target[i];
            }
            let ap = ms_to_samples(def.allpass_ms * ALLPASS_SPREAD[i], sample_rate) as usize;
            self.allpass_len[i] = next_prime(ap.max(2)).min(self.allpasses[i].max_delay() as usize);
        }
        for ch in 0..2 {
            for k in 0..4 {
                let d = ms_to_samples(DIFFUSER_MS[ch][k] * def.diffuser, sample_rate) as usize;
                self.diffuser_len[ch][k] = next_prime(d.max(2)).min(self.diffusers[ch][k].max_delay() as usize);
            }
        }
    }

    pub(super) fn set_decay(&mut self, rt: Rt, sample_rate: f32) {
        for i in 0..N {
            // The in-loop allpass adds its length to the loop on average.
            let delay = self.target[i] + self.allpass_len[i] as f32;
            self.filters[i].set(delay, rt.mid, rt.low, rt.high, sample_rate);
        }
    }

    pub(super) fn set_mod_rate(&mut self, hz: f32, sample_rate: f32) {
        for (i, osc) in self.osc.iter_mut().enumerate() {
            // Spread the rates so the lines never beat together.
            osc.set_freq(hz * (0.62 + 0.76 * LINE_ORDER[i] as f32 / 15.0), sample_rate);
        }
    }

    /// The mean loop length, for level normalisation.
    pub(super) fn mean_len(&self) -> f32 {
        self.target.iter().sum::<f32>() / N as f32
    }

    pub(super) fn clear(&mut self) {
        for line in self.lines.iter_mut().chain(&mut self.allpasses) {
            line.clear();
        }
        for ch in &mut self.diffusers {
            for line in ch {
                line.clear();
            }
        }
        for f in &mut self.filters {
            f.clear();
        }
        for (i, o) in self.osc.iter_mut().enumerate() {
            o.set_phase(i as f32 * 2.399);
        }
    }

    /// One sample. `x` is the stereo input already scaled; returns the late reverb.
    #[inline]
    // Indexes parallel per-channel or per-line arrays together: an index reads best here.
    #[allow(clippy::needless_range_loop)]
    pub(super) fn tick(&mut self, x: [f32; 2], diffusion: f32, density: f32, excursion: f32, glide: f32) -> [f32; 2] {
        // Input diffusion: four allpasses per side smear transients before the network.
        let mut d = x;
        for ch in 0..2 {
            for k in 0..4 {
                let g = if k % 2 == 0 { diffusion } else { -diffusion };
                d[ch] = allpass(&mut self.diffusers[ch][k], d[ch], self.diffuser_len[ch][k], g);
            }
        }

        let mut y = [0.0f32; N];
        for i in 0..N {
            self.cur[i] += ((self.target[i] - self.cur[i]) * glide).clamp(-LINE_SLEW, LINE_SLEW);
            // The excursion is centred one depth past the set length, so it never reads too close.
            let m = excursion * (1.0 + self.osc[i].tick());
            let v = self.lines[i].read_frac(self.cur[i] + m);
            y[i] = allpass(&mut self.allpasses[i], v, self.allpass_len[i], density);
        }

        // Outputs: two orthogonal Hadamard rows, so left and right are decorrelated.
        let (mut out_l, mut out_r) = (0.0, 0.0);
        for i in 0..N {
            out_l += hadamard(4, i) * y[i];
            out_r += hadamard(8, i) * y[i];
        }

        // Decay, then mix through the Hadamard matrix (a fast Walsh-Hadamard transform).
        let mut m = [0.0f32; N];
        for i in 0..N {
            m[i] = self.filters[i].process(y[i]);
        }
        let mut h = 1;
        while h < N {
            for i in (0..N).step_by(2 * h) {
                for j in i..i + h {
                    let (a, b) = (m[j], m[j + h]);
                    m[j] = a + b;
                    m[j + h] = a - b;
                }
            }
            h *= 2;
        }
        for i in 0..N {
            // The input reaches every line, with orthogonal sign patterns per side.
            let input = hadamard(1, i) * d[0] + hadamard(2, i) * d[1];
            self.lines[i].write(flush(0.25 * m[i] + input));
        }
        [out_l * 0.25, out_r * 0.25]
    }

    pub(super) fn renormalize(&mut self) {
        for osc in &mut self.osc {
            osc.normalize();
        }
    }

    #[cfg(test)]
    pub(super) fn has_subnormal(&self) -> bool {
        self.lines.iter().chain(&self.allpasses).any(|l| l.has_subnormal())
            || self.diffusers.iter().flatten().any(|l| l.has_subnormal())
            || self.filters.iter().any(|f| f.s_low.is_subnormal() || f.s_high.is_subnormal())
    }
}
