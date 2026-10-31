//! The Compressor, modelled on Logic Pro's: one set of controls in front of five circuit types,
//! each with the gain-reduction behaviour of the hardware it stands for.
//!
//! - **Platinum Digital**: a clean feedforward compressor working in the log domain, with the
//!   smooth-branching detector and quadratic soft knee from Giannoulis, Massberg & Reiss,
//!   "Digital Dynamic Range Compressor Design — A Tutorial and Analysis" (JAES 2012). The
//!   threshold, ratio, knee and time constants mean exactly what they say.
//! - **Studio VCA** (SSL G-bus, dbx 160): the same feedforward VCA topology with the console's
//!   stepped attack (0.1–30 ms) and release (0.1–1.2 s) positions, plus Auto release: a
//!   dual-time-constant release that recovers fast after transients and slowly after sustained
//!   compression.
//! - **Studio FET** (UREI 1176): a feedback compressor. The detector listens to the compressed
//!   output, and the loop is solved without delay each sample (Newton's method on the
//!   implicit equation), so it stays stable with its 20 µs attack. The ratio buttons are
//!   4/8/12/20:1 plus "All buttons in", whose bias shift lowers the threshold, slows the attack
//!   and over-compresses: above a point, louder input gives quieter output. The release is
//!   program-dependent (longer after heavy compression), and a FET-style odd-harmonic colour
//!   grows with gain reduction.
//! - **Classic Opto** (Teletronix LA-2A): an optical cell with a ~10 ms attack and the T4's
//!   two-stage release: about half the reduction recovers in ~60 ms, the rest over 1–15 s,
//!   depending on how long and how hard the cell was lit. Its soft knee has a ratio that keeps
//!   rising with level (Compress tops out near 3:1, Limit near 12:1). The panel shows the
//!   threshold as Peak Reduction.
//! - **Vintage Vari-Mu** (Fairchild 670, Manley): a variable-mu tube with a very wide knee
//!   whose ratio rises with level towards the Ratio setting, the 670's six time-constant
//!   positions (5 and 6 are program-dependent), and even-harmonic tube colour that grows with
//!   gain reduction.
//!
//! All circuits share the signal path around them: side-chain key or input → side-chain filter
//! → detector (peak or RMS) → stereo link → gain computer and ballistics → lookahead-delayed
//! audio × gain → circuit colour → makeup and auto gain → output distortion → dry/wet mix →
//! output gain. Gain reduction is computed in dB, so the envelopes behave the same at every
//! level.
//!
//! Provenance: written for Fermata from the papers and published behaviour of the units above;
//! no third-party code is included. The idea of letting the FET release slow down as reduction
//! builds up was suggested by the program-dependent "speed" variable in Airwindows' Pop2
//! (MIT licence, Copyright (c) 2016 airwindows), but none of its code is used.

use super::dyn_common::{
    db_to_lin, flush, knee_reduction, knee_reduction_slope, lin_to_db, program_reduction, DcBlock, Delay, Distortion, ScMode, Smoother, Svf,
};
use super::util::coeff;
use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit, MAX_METERS};

mod curve;
pub use curve::*;
mod params;
pub use params::*;
#[cfg(test)]
mod tests;

/// One channel's detector and envelope state. Reductions are in dB (positive).
#[derive(Debug, Clone, Copy, Default)]
struct Chan {
    /// Mean square, for RMS detection.
    ms: f32,
    /// The main envelope: Platinum/VCA reduction, the FET loop state, the opto's fast
    /// element, the vari-mu's first stage.
    gr: f32,
    /// Auto release's slow envelope, the opto cell's persistence, the vari-mu's second stage.
    slow: f32,
    /// The vari-mu's third stage (position 6).
    slower: f32,
    /// How hard and long the FET or the opto cell has been working (program memory).
    memory: f32,
    dc: DcBlock,
}

/// Per-block constants for the envelopes (time coefficients change slowly, so working them
/// out once per block is enough).
struct Block {
    k_att: f32,
    k_rel: f32,
    k_slow_att: f32,
    k_slow_rel: f32,
    k_slower_att: f32,
    k_slower_rel: f32,
    k_memory: f32,
    k_memory_rel: f32,
    auto_release: bool,
    multi: bool,
    sustained: bool,
    sr: f32,
}

/// The Compressor instance.
pub struct Compressor {
    sr: f32,
    values: [f32; NPARAMS],
    circuit: Circuit,
    threshold: Smoother,
    ratio: Smoother,
    knee: Smoother,
    makeup: Smoother,
    auto_gain: Smoother,
    mix: Smoother,
    output: Smoother,
    link: Smoother,
    svf: Svf,
    sc_set: (ScMode, f32),
    chans: [Chan; 2],
    /// Lookahead delays: audio L/R, then the listened key L/R (so Listen stays aligned).
    delays: [Delay; 4],
    lookahead: usize,
    k_rms: f32,
    m_in: f32,
    m_out: f32,
    m_gr: f32,
    m_level: f32,
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        let mut values = [0.0; NPARAMS];
        for p in &PARAMS {
            values[p.id as usize] = p.default;
        }
        let v = |id: u32| values[id as usize];
        let max_delay = (0.020 * sr).ceil() as usize + 1;
        let mut c = Compressor {
            sr,
            values,
            circuit: Circuit::from_index(v(param::CIRCUIT)),
            threshold: Smoother::new(v(param::THRESHOLD), sr),
            ratio: Smoother::new(v(param::RATIO), sr),
            knee: Smoother::new(v(param::KNEE), sr),
            makeup: Smoother::new(v(param::MAKEUP), sr),
            auto_gain: Smoother::new(0.0, sr),
            mix: Smoother::new(v(param::MIX), sr),
            output: Smoother::new(v(param::OUTPUT), sr),
            link: Smoother::new(v(param::LINK), sr),
            svf: Svf::default(),
            sc_set: (ScMode::Off, 0.0),
            chans: [Chan { dc: DcBlock::new(sr), ..Chan::default() }; 2],
            delays: std::array::from_fn(|_| Delay::new(max_delay)),
            lookahead: 0,
            k_rms: coeff(8.0, sr),
            m_in: -120.0,
            m_out: -120.0,
            m_gr: 0.0,
            m_level: -120.0,
        };
        c.update_auto_gain();
        c.auto_gain.snap();
        c
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn curve_params(&self) -> CurveParams {
        CurveParams {
            circuit: self.circuit,
            threshold: self.threshold.target(),
            ratio: self.ratio.target(),
            knee: self.knee.target(),
            fet_mode: self.v(param::FET_RATIO).round().clamp(0.0, 4.0) as usize,
            opto_limit: self.v(param::OPTO_MODE) >= 0.5,
        }
    }

    /// Auto Gain makes up the reduction a signal at the chosen reference level would get.
    fn update_auto_gain(&mut self) {
        let reference = match self.v(param::AUTO_GAIN).round() as i32 {
            1 => Some(0.0),
            2 => Some(-12.0),
            3 => Some(-24.0),
            _ => None,
        };
        let db = reference.map_or(0.0, |r| static_reduction(&self.curve_params(), r));
        self.auto_gain.set(db);
    }

    /// The attack and release the current circuit actually uses, in ms, after its own range
    /// or switch positions. The panel mirrors this.
    pub fn effective_times(&self) -> (f32, f32) {
        let a = self.v(param::ATTACK);
        let r = self.v(param::RELEASE);
        match self.circuit {
            Circuit::Platinum => (a, r),
            Circuit::Vca => (nearest(a, &VCA_ATTACKS), nearest(r, &VCA_RELEASES)),
            Circuit::Fet => (a.clamp(FET_ATTACK.0, FET_ATTACK.1), r.clamp(FET_RELEASE.0, FET_RELEASE.1)),
            Circuit::Opto => (10.0, 60.0),
            Circuit::VariMu => {
                let tc = &VARI_MU_TC[self.v(param::TIME_CONSTANT).round().clamp(0.0, 5.0) as usize];
                (tc.attack, tc.release)
            }
        }
    }

    fn block_consts(&self) -> Block {
        let sr = self.sr;
        let (a, r) = self.effective_times();
        let auto = self.v(param::AUTO_RELEASE) >= 0.5;
        let none = Block {
            k_att: coeff(a, sr),
            k_rel: coeff(r, sr),
            k_slow_att: 0.0,
            k_slow_rel: 0.0,
            k_slower_att: 0.0,
            k_slower_rel: 0.0,
            k_memory: 0.0,
            k_memory_rel: 0.0,
            auto_release: false,
            multi: false,
            sustained: false,
            sr,
        };
        match self.circuit {
            // Auto release: a fast stage for transients and a slow one that only charges
            // under sustained reduction. Platinum scales both from the Release knob; the VCA's
            // Auto position has fixed console-style times.
            Circuit::Platinum if auto => {
                Block { k_rel: coeff((r * 0.3).max(10.0), sr), k_slow_att: coeff(300.0, sr), k_slow_rel: coeff(r * 3.0, sr), auto_release: true, ..none }
            }
            Circuit::Vca if auto => Block { k_rel: coeff(100.0, sr), k_slow_att: coeff(300.0, sr), k_slow_rel: coeff(1200.0, sr), auto_release: true, ..none },
            Circuit::Platinum | Circuit::Vca => none,
            Circuit::Fet => {
                // All buttons in slows the attack, which lets the transients through.
                let a = if self.curve_params().fet_mode >= 4 { a * 3.0 } else { a };
                Block { k_att: coeff(a, sr), k_memory: coeff(400.0, sr), ..none }
            }
            Circuit::Opto => Block {
                // The cell's persistence charges over about a second; its memory of hard work
                // builds over a few seconds and fades over half a minute (the slow release
                // itself depends on that memory, so it is worked out per sample).
                k_slow_att: coeff(1000.0, sr),
                k_memory: coeff(3000.0, sr),
                k_memory_rel: coeff(30000.0, sr),
                ..none
            },
            Circuit::VariMu => {
                let tc = &VARI_MU_TC[self.v(param::TIME_CONSTANT).round().clamp(0.0, 5.0) as usize];
                Block {
                    // Repeated peaks within a fraction of a second charge the multi-peak stage;
                    // only seconds of sustained level charge the slowest one.
                    k_slow_att: coeff(150.0, sr),
                    k_slow_rel: tc.multi.map_or(0.0, |ms| coeff(ms, sr)),
                    k_slower_att: coeff(2000.0, sr),
                    k_slower_rel: tc.sustained.map_or(0.0, |ms| coeff(ms, sr)),
                    multi: tc.multi.is_some(),
                    sustained: tc.sustained.is_some(),
                    ..none
                }
            }
        }
    }

    /// Advances one channel's envelope for a detector level `x` (dB) and returns its gain
    /// reduction in dB.
    #[inline]
    fn reduce(ch: &mut Chan, x: f32, cp: &CurveParams, b: &Block) -> f32 {
        match cp.circuit {
            Circuit::Platinum | Circuit::Vca => {
                // Smooth branching peak detector in the log domain (Giannoulis et al., §3.2).
                let t = knee_reduction(x - cp.threshold, 1.0 - 1.0 / cp.ratio, cp.knee);
                let k = if t > ch.gr { b.k_att } else { b.k_rel };
                ch.gr = flush(ch.gr + k * (t - ch.gr));
                if b.auto_release {
                    let ks = if t > ch.slow { b.k_slow_att } else { b.k_slow_rel };
                    ch.slow = flush(ch.slow + ks * (t - ch.slow));
                    ch.gr.max(0.75 * ch.slow)
                } else {
                    ch.gr
                }
            }
            Circuit::Fet => {
                // Feedback: the target depends on the output level x - c, which depends on the
                // reduction c being solved for. c = c₀ + k·(t(x - c) - c₀) is monotonic in c,
                // so a few Newton steps from c₀ find it; the loop can't ring however fast the
                // attack.
                let (s, kn, abi) = cp.fet();
                let xo = x - cp.fet_threshold();
                let extra = if abi { abi_extra(xo) } else { 0.0 };
                let c0 = ch.gr;
                let attacking = knee_reduction(xo - c0, s, kn) + extra > c0;
                let k = if attacking {
                    b.k_att
                } else {
                    // Program-dependent release: after heavy, sustained reduction the FET
                    // recovers up to 2.5× slower. k ≈ 1/(τ·fs) at these lengths, so dividing
                    // k stretches τ.
                    b.k_rel / (1.0 + 1.5 * (ch.memory / 12.0).min(1.0))
                };
                let mut c = c0;
                for _ in 0..4 {
                    let f = c - c0 - k * (knee_reduction(xo - c, s, kn) + extra - c0);
                    let fp = 1.0 + k * knee_reduction_slope(xo - c, s, kn);
                    c -= f / fp;
                }
                ch.gr = flush(c.max(0.0));
                ch.memory = flush(ch.memory + b.k_memory * (ch.gr - ch.memory));
                ch.gr
            }
            Circuit::Opto => {
                let (slope, w) = opto_curve(cp.opto_limit);
                let t = program_reduction(x - cp.threshold, slope, w);
                // The fast part of the cell: ~10 ms attack, ~60 ms release.
                let k = if t > ch.gr { b.k_att } else { b.k_rel };
                ch.gr = flush(ch.gr + k * (t - ch.gr));
                // The slow part: charges over about a second of reduction and lets go over
                // 1 s (briefly lit) to 15 s (worked hard for a long time).
                let ks = if t > ch.slow { b.k_slow_att } else { 1.0 / ((1.0 + 14.0 * ch.memory) * b.sr) };
                ch.slow = flush(ch.slow + ks * (t - ch.slow));
                let lit = (t / 12.0).min(1.0);
                let km = if lit > ch.memory { b.k_memory } else { b.k_memory_rel };
                ch.memory = flush(ch.memory + km * (lit - ch.memory));
                ch.gr.max(0.5 * ch.slow)
            }
            Circuit::VariMu => {
                let t = program_reduction(x - cp.threshold, 1.0 - 1.0 / cp.ratio, VARI_MU_WIDTH);
                let k = if t > ch.gr { b.k_att } else { b.k_rel };
                ch.gr = flush(ch.gr + k * (t - ch.gr));
                let mut gr = ch.gr;
                if b.multi {
                    let ks = if t > ch.slow { b.k_slow_att } else { b.k_slow_rel };
                    ch.slow = flush(ch.slow + ks * (t - ch.slow));
                    gr = gr.max(ch.slow);
                }
                if b.sustained {
                    let ks = if t > ch.slower { b.k_slower_att } else { b.k_slower_rel };
                    ch.slower = flush(ch.slower + ks * (t - ch.slower));
                    gr = gr.max(ch.slower);
                }
                gr
            }
        }
    }

    /// The circuit's own colour on the gain-reduced signal (before makeup).
    #[inline]
    fn colour(ch: &mut Chan, circuit: Circuit, abi: bool, gr: f32, y: f32) -> f32 {
        match circuit {
            Circuit::Platinum | Circuit::Vca => y,
            Circuit::Fet => {
                // Odd harmonics from the FET, stronger the harder it works (and much stronger
                // with all buttons in). tanh(y) ≈ y - y³/3, so the colour also grows with level.
                let a = if abi { (0.3 + 0.05 * gr).min(1.0) } else { (0.12 + 0.03 * gr).min(0.8) };
                y + a * (y.tanh() - y)
            }
            Circuit::Opto => {
                // The LA-2A's tube make-up stage: a faint, constant second harmonic.
                let t = y.tanh();
                ch.dc.process(y + 0.01 * t * t)
            }
            Circuit::VariMu => {
                // Even harmonics from the remote-cutoff tube, rising with reduction.
                let b = (0.02 + 0.006 * gr).min(0.15);
                let t = y.tanh();
                ch.dc.process(y + b * t * t)
            }
        }
    }
}

impl Effect for Compressor {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        match id {
            param::CIRCUIT => {
                let c = Circuit::from_index(value);
                if c != self.circuit {
                    // Each circuit's envelopes mean different things; start the new one clean.
                    self.circuit = c;
                    for ch in &mut self.chans {
                        *ch = Chan { dc: ch.dc, ..Chan::default() };
                    }
                }
            }
            param::THRESHOLD => self.threshold.set(value),
            param::RATIO => self.ratio.set(value.max(1.0)),
            param::KNEE => self.knee.set(value.max(0.0)),
            param::MAKEUP => self.makeup.set(value),
            param::MIX => self.mix.set(value.clamp(0.0, 1.0)),
            param::OUTPUT => self.output.set(value),
            param::LINK => self.link.set(value.clamp(0.0, 1.0)),
            param::LOOKAHEAD => {
                self.lookahead = (value.max(0.0) * 0.001 * self.sr).round() as usize;
                for d in &mut self.delays {
                    d.set_len(self.lookahead);
                }
            }
            _ => {}
        }
        self.update_auto_gain();
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _ctx: &FxContext) {
        let n = l.len().min(r.len());
        let b = self.block_consts();
        let circuit = self.circuit;
        let fet_mode = self.curve_params().fet_mode;
        let abi = circuit == Circuit::Fet && fet_mode >= 4;
        let opto_limit = self.v(param::OPTO_MODE) >= 0.5;
        // Platinum and VCA follow the Detection switch; the others are peak-sensing by design
        // (their own ballistics do the averaging).
        let rms = matches!(circuit, Circuit::Platinum | Circuit::Vca) && self.v(param::DETECTOR) >= 0.5;
        let distortion = Distortion::from_index(self.v(param::DISTORTION));
        let listen = self.v(param::LISTEN) >= 0.5;

        let sc_mode = ScMode::from_index(self.v(param::SC_MODE));
        let sc_freq = self.v(param::SC_FREQ);
        if (sc_mode, sc_freq) != self.sc_set {
            let q = if sc_mode == ScMode::BandPass { 1.4 } else { std::f32::consts::FRAC_1_SQRT_2 };
            self.svf.set(sc_freq, q, self.sr);
            self.sc_set = (sc_mode, sc_freq);
        }

        let mut m_in = 0.0f32;
        let mut m_out = 0.0f32;
        let mut m_gr = 0.0f32;
        let mut m_level = -120.0f32;
        for i in 0..n {
            let cp = CurveParams { circuit, threshold: self.threshold.next(), ratio: self.ratio.next(), knee: self.knee.next(), fet_mode, opto_limit };
            let link = self.link.next();
            let gain_db = self.makeup.next() + self.auto_gain.next();
            let mix = self.mix.next();
            let out_gain = db_to_lin(self.output.next());

            let (xl, xr) = (l[i], r[i]);
            m_in = m_in.max(xl.abs()).max(xr.abs());
            let (kl, kr) = match key {
                Some((a, b)) if i < a.len() && i < b.len() => (a[i], b[i]),
                _ => (xl, xr),
            };
            let dl = self.svf.process(0, kl, sc_mode);
            let dr = self.svf.process(1, kr, sc_mode);

            // Detector level per channel, then linked towards the louder one.
            let (al, ar) = if rms {
                let [c0, c1] = &mut self.chans;
                c0.ms = flush(c0.ms + self.k_rms * (dl * dl - c0.ms));
                c1.ms = flush(c1.ms + self.k_rms * (dr * dr - c1.ms));
                (c0.ms.sqrt(), c1.ms.sqrt())
            } else {
                (dl.abs(), dr.abs())
            };
            let mut el = lin_to_db(al);
            let mut er = lin_to_db(ar);
            let emax = el.max(er);
            el += link * (emax - el);
            er += link * (emax - er);
            m_level = m_level.max(emax);

            let grl = Self::reduce(&mut self.chans[0], el, &cp, &b);
            let grr = Self::reduce(&mut self.chans[1], er, &cp, &b);
            m_gr = m_gr.max(grl).max(grr);

            let yl = self.delays[0].process(xl);
            let yr = self.delays[1].process(xr);
            let ll = self.delays[2].process(dl);
            let lr = self.delays[3].process(dr);
            if listen {
                l[i] = ll;
                r[i] = lr;
                m_out = m_out.max(ll.abs()).max(lr.abs());
                continue;
            }

            let wet_l = Self::colour(&mut self.chans[0], circuit, abi, grl, yl * db_to_lin(-grl));
            let wet_r = Self::colour(&mut self.chans[1], circuit, abi, grr, yr * db_to_lin(-grr));
            let g = db_to_lin(gain_db);
            let wet_l = distortion.apply(wet_l * g);
            let wet_r = distortion.apply(wet_r * g);
            let ol = (yl * (1.0 - mix) + wet_l * mix) * out_gain;
            let or = (yr * (1.0 - mix) + wet_r * mix) * out_gain;
            l[i] = ol;
            r[i] = or;
            m_out = m_out.max(ol.abs()).max(or.abs());
        }
        self.m_in = lin_to_db(m_in);
        self.m_out = lin_to_db(m_out);
        self.m_gr = m_gr;
        self.m_level = m_level;
    }

    fn reset(&mut self) {
        for s in [&mut self.threshold, &mut self.ratio, &mut self.knee, &mut self.makeup, &mut self.auto_gain, &mut self.mix, &mut self.output, &mut self.link]
        {
            s.snap();
        }
        for ch in &mut self.chans {
            let mut dc = ch.dc;
            dc.reset();
            *ch = Chan { dc, ..Chan::default() };
        }
        self.svf.reset();
        for d in &mut self.delays {
            d.reset();
        }
        self.m_in = -120.0;
        self.m_out = -120.0;
        self.m_gr = 0.0;
        self.m_level = -120.0;
    }

    fn latency(&self) -> usize {
        self.lookahead
    }

    fn tail_secs(&self) -> f32 {
        self.lookahead as f32 / self.sr
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.m_in;
        out[1] = self.m_out;
        out[2] = self.m_gr;
        out[3] = self.m_level;
    }
}
