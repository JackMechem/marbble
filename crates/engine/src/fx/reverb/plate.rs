//! Jon Dattorro's plate, the Plate room type: the constants from the paper's figure and table, scaled
//! by Size and the sample rate.

use super::*;

/// The paper's sample rate; its lengths are in samples at this rate.
const DATTORRO_RATE: f32 = 29_761.0;
const PLATE_INPUT: [usize; 4] = [142, 107, 379, 277];
const PLATE_INPUT_GAIN: [f32; 4] = [0.75, 0.75, 0.625, 0.625];
/// Each tank half: modulated allpass, delay, allpass, delay.
const PLATE_TANK: [[f32; 4]; 2] = [[672.0, 4453.0, 1800.0, 3720.0], [908.0, 4217.0, 2656.0, 3163.0]];
/// The paper's modulation excursion, in samples at its rate.
pub(super) const PLATE_EXCURSION: f32 = 16.0;
/// Output taps: (half, element: 1 = first delay, 2 = second allpass, 3 = second delay, offset,
/// sign), from the paper's table.
type Tap = (usize, usize, f32, f32);
const PLATE_TAPS: [[Tap; 7]; 2] = [
    [(1, 1, 266.0, 1.0), (1, 1, 2974.0, 1.0), (1, 2, 1913.0, -1.0), (1, 3, 1996.0, 1.0), (0, 1, 1990.0, -1.0), (0, 2, 187.0, -1.0), (0, 3, 1066.0, -1.0)],
    [(0, 1, 353.0, 1.0), (0, 1, 3627.0, 1.0), (0, 2, 1228.0, -1.0), (0, 3, 2673.0, 1.0), (1, 1, 2111.0, -1.0), (1, 2, 335.0, -1.0), (1, 3, 121.0, -1.0)],
];

pub(super) struct Plate {
    pub(super) input: [Line; 4],
    pub(super) input_len: [usize; 4],
    /// Per half: the four tank elements' buffers.
    pub(super) tank: [[Line; 4]; 2],
    pub(super) filters: [DecayFilter; 2],
    pub(super) osc: Osc,
    /// Samples per paper sample (sample rate / 29761), times the gliding size scale.
    pub(super) k: f32,
    pub(super) scale: f32,
    pub(super) scale_target: f32,
    pub(super) feed: [f32; 2],
}

impl Plate {
    /// A buffer for `n` paper samples at the largest size, plus the modulation.
    pub(super) fn size(n: f32, sample_rate: f32) -> usize {
        let k = sample_rate / DATTORRO_RATE;
        (n * k * SIZE_MAX + PLATE_EXCURSION * k * 3.0) as usize + 8
    }

    pub(super) fn new(alloc_rate: f32) -> Plate {
        let k = alloc_rate / DATTORRO_RATE;
        Plate {
            input: std::array::from_fn(|i| Line::new(Plate::size(PLATE_INPUT[i] as f32, alloc_rate))),
            input_len: PLATE_INPUT,
            tank: std::array::from_fn(|h| std::array::from_fn(|e| Line::new(Plate::size(PLATE_TANK[h][e], alloc_rate)))),
            filters: [DecayFilter::new(); 2],
            osc: Osc::new(0.0),
            k,
            scale: 1.0,
            scale_target: 1.0,
            feed: [0.0; 2],
        }
    }

    pub(super) fn set_rate(&mut self, sample_rate: f32) {
        for (i, line) in self.input.iter_mut().enumerate() {
            line.set_active(Plate::size(PLATE_INPUT[i] as f32, sample_rate));
        }
        for (h, half) in self.tank.iter_mut().enumerate() {
            for (e, line) in half.iter_mut().enumerate() {
                line.set_active(Plate::size(PLATE_TANK[h][e], sample_rate));
            }
        }
        self.k = sample_rate / DATTORRO_RATE;
        // The input diffusers are fixed: scaled by the sample rate only.
        for (len, input) in self.input_len.iter_mut().zip(&PLATE_INPUT) {
            *len = ((*input as f32 * self.k).round() as usize).max(1);
        }
    }

    pub(super) fn set_decay(&mut self, rt: Rt, sample_rate: f32) {
        for (filter, tank) in self.filters.iter_mut().zip(&PLATE_TANK) {
            let half: f32 = tank.iter().sum::<f32>() * self.k * self.scale_target;
            filter.set(half, rt.mid, rt.low, rt.high, sample_rate);
        }
    }

    pub(super) fn clear(&mut self) {
        for line in self.input.iter_mut().chain(self.tank.iter_mut().flatten()) {
            line.clear();
        }
        for f in &mut self.filters {
            f.clear();
        }
        self.feed = [0.0; 2];
        self.osc.set_phase(0.0);
    }

    /// One sample of mono input; returns the stereo tank output.
    #[inline]
    // Indexes parallel per-channel or per-line arrays together: an index reads best here.
    #[allow(clippy::needless_range_loop)]
    pub(super) fn tick(&mut self, x: f32, diffusion: f32, density: f32, excursion: f32, glide: f32) -> [f32; 2] {
        self.scale += ((self.scale_target - self.scale) * glide).clamp(-SCALE_SLEW, SCALE_SLEW);
        let mut d = x;
        for i in 0..4 {
            d = allpass(&mut self.input[i], d, self.input_len[i], PLATE_INPUT_GAIN[i] * diffusion);
        }
        // Decay diffusion 1 (negative in the paper) and 2.
        let dd1 = -(0.35 + 0.4 * density);
        let dd2 = 0.25 + 0.25 * density;
        let k = self.k * self.scale;
        let lfo = [self.osc.tick(), self.osc.cos()];
        let mut feed = [0.0; 2];
        for h in 0..2 {
            let len = PLATE_TANK[h];
            let input = d + self.feed[1 - h];
            let [ap1, d1, ap2, d2] = &mut self.tank[h];
            let a = allpass_frac(ap1, input, len[0] * k + excursion * (1.0 + lfo[h]), dd1);
            let b = d1.read_frac(len[1] * k);
            d1.write(flush(a));
            let c = self.filters[h].process(b);
            let e = allpass_frac(ap2, c, len[2] * k, dd2);
            feed[h] = d2.read_frac(len[3] * k);
            d2.write(flush(e));
        }
        self.feed = feed;
        let mut out = [0.0; 2];
        for (o, taps) in out.iter_mut().zip(&PLATE_TAPS) {
            for &(h, e, at, sign) in taps {
                *o += sign * self.tank[h][e].read_lin(at * k);
            }
        }
        [out[0] * 0.6, out[1] * 0.6]
    }

    #[cfg(test)]
    pub(super) fn has_subnormal(&self) -> bool {
        self.input.iter().chain(self.tank.iter().flatten()).any(|l| l.has_subnormal())
            || self.filters.iter().any(|f| f.s_low.is_subnormal() || f.s_high.is_subnormal())
    }
}
