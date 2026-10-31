//! What differs between the compressor's circuits: their time constants and static gain curves
//! ([`static_reduction`]). The panel's graph mirrors the latter.

use super::*;

/// The circuit types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Circuit {
    Platinum,
    Vca,
    Fet,
    Opto,
    VariMu,
}

impl Circuit {
    pub fn from_index(i: f32) -> Circuit {
        match i.round() as i32 {
            1 => Circuit::Vca,
            2 => Circuit::Fet,
            3 => Circuit::Opto,
            4 => Circuit::VariMu,
            _ => Circuit::Platinum,
        }
    }
}

/// The SSL bus compressor's attack positions, in ms. The VCA circuit snaps to the nearest.
pub const VCA_ATTACKS: [f32; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
/// The SSL bus compressor's release positions, in ms (Auto is the Auto Release switch).
pub const VCA_RELEASES: [f32; 4] = [100.0, 300.0, 600.0, 1200.0];
/// The 1176's attack range, in ms.
pub const FET_ATTACK: (f32, f32) = (0.02, 0.8);
/// The 1176's release range, in ms.
pub const FET_RELEASE: (f32, f32) = (50.0, 1100.0);

/// One Fairchild 670 time-constant position: attack, then release stages in ms. Positions 5
/// and 6 add slower stages that charge only under repeated or sustained reduction.
pub(super) struct TimeConstant {
    pub(super) attack: f32,
    pub(super) release: f32,
    pub(super) multi: Option<f32>,
    pub(super) sustained: Option<f32>,
}

pub(super) const VARI_MU_TC: [TimeConstant; 6] = [
    TimeConstant { attack: 0.2, release: 300.0, multi: None, sustained: None },
    TimeConstant { attack: 0.2, release: 800.0, multi: None, sustained: None },
    TimeConstant { attack: 0.4, release: 2000.0, multi: None, sustained: None },
    TimeConstant { attack: 0.4, release: 5000.0, multi: None, sustained: None },
    TimeConstant { attack: 0.4, release: 2000.0, multi: Some(10000.0), sustained: None },
    TimeConstant { attack: 0.2, release: 300.0, multi: Some(10000.0), sustained: Some(25000.0) },
];

/// Snaps a time to the nearest of a set of positions, in log terms (like a stepped switch).
pub(super) fn nearest(ms: f32, steps: &[f32]) -> f32 {
    let mut best = steps[0];
    for &s in steps {
        if (ms.ln() - s.ln()).abs() < (ms.ln() - best.ln()).abs() {
            best = s;
        }
    }
    best
}

/// The FET ratio buttons: (feedback slope S = R - 1, knee width on the input side in dB).
fn fet_curve(mode: usize) -> (f32, f32) {
    match mode {
        0 => (3.0, 8.0),
        1 => (7.0, 6.0),
        2 => (11.0, 5.0),
        _ => (19.0, 4.0),
    }
}

/// All-buttons-in: how far it lowers the threshold (dB), where its over-compression starts
/// above the threshold (dB), and how many dB of extra reduction it adds per dB beyond that.
/// More than 1 dB/dB is what makes the output fall as the input rises.
const ABI_SHIFT: f32 = 3.0;
const ABI_KNEE: f32 = 4.0;
const ABI_FOLD: f32 = 1.15;

/// The opto curve for Compress / Limit: (slope towards the top ratio, softness in dB).
pub(super) fn opto_curve(limit: bool) -> (f32, f32) {
    if limit {
        (1.0 - 1.0 / 12.0, 3.0)
    } else {
        (1.0 - 1.0 / 3.0, 5.0)
    }
}

/// How soft the vari-mu curve is: its ratio rises over about ±2 widths around the threshold.
pub(super) const VARI_MU_WIDTH: f32 = 8.0;

/// The settings that shape the static curve, gathered so the curve can be evaluated alone.
#[derive(Debug, Clone, Copy)]
pub struct CurveParams {
    pub circuit: Circuit,
    pub threshold: f32,
    pub ratio: f32,
    pub knee: f32,
    pub fet_mode: usize,
    pub opto_limit: bool,
}

impl CurveParams {
    /// The FET's feedback slope and knee in the output domain. A knee `k` dB wide on the
    /// input side is `2k / (S + 2)` wide on the output side the detector sees.
    pub(super) fn fet(&self) -> (f32, f32, bool) {
        let abi = self.fet_mode >= 4;
        let (s, k_in) = fet_curve(self.fet_mode);
        (s, 2.0 * k_in / (s + 2.0), abi)
    }

    pub(super) fn fet_threshold(&self) -> f32 {
        if self.fet_mode >= 4 {
            self.threshold - ABI_SHIFT
        } else {
            self.threshold
        }
    }
}

/// All-buttons-in over-compression added to the FET target for an input `xo` dB over the
/// (shifted) threshold.
#[inline]
pub(super) fn abi_extra(xo: f32) -> f32 {
    ABI_FOLD * (xo - ABI_KNEE).max(0.0)
}

/// The steady-state gain reduction in dB for a constant detector level `x` dB: what the
/// compressor settles to, and what Auto Gain compensates.
pub fn static_reduction(p: &CurveParams, x: f32) -> f32 {
    match p.circuit {
        Circuit::Platinum | Circuit::Vca => knee_reduction(x - p.threshold, 1.0 - 1.0 / p.ratio.max(1.0), p.knee),
        Circuit::Fet => {
            // Solve c = S·knee(x - T - c) (+ all-buttons fold) for the reduction c.
            let (s, kn, abi) = p.fet();
            let xo = x - p.fet_threshold();
            let extra = if abi { abi_extra(xo) } else { 0.0 };
            let mut c = 0.0f32;
            for _ in 0..30 {
                let f = c - knee_reduction(xo - c, s, kn) - extra;
                let fp = 1.0 + knee_reduction_slope(xo - c, s, kn);
                c -= f / fp;
            }
            c.max(0.0)
        }
        Circuit::Opto => {
            let (slope, w) = opto_curve(p.opto_limit);
            program_reduction(x - p.threshold, slope, w)
        }
        Circuit::VariMu => program_reduction(x - p.threshold, 1.0 - 1.0 / p.ratio.max(1.0), VARI_MU_WIDTH),
    }
}
