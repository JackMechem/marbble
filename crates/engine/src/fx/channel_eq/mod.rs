//! Channel EQ, after Logic's: eight bands (a low cut, a low shelf, four bells, a high shelf and a
//! high cut) and an output level, as an Audio FX insert.
//!
//! - The filters are `filters.rs`'s biquads, run in f64. A parameter change marks the
//!   coefficients stale; the next block computes them again (no allocation) and goes on with
//!   the filters' state, so a drag doesn't click. A flat setting runs nothing.
//! - Strips used to have one fixed Channel EQ ahead of their inserts (`ChannelEq` in the model).
//!   Projects, channel strip settings and presets from then hold it in that form; [`values_of`]
//!   turns it into this effect's parameters (see `model/channel_eq_upgrade.rs`), and the result
//!   plays sample for sample as the fixed EQ did.
//! - The parameter layout is shared with the Linear Phase EQ (`linear_phase_eq/`), which adds
//!   its Quality, so presets and the editor (`src/plugins/effects/eq/EqEditor.tsx`) work for both.

pub mod filters;

use super::*;
use crate::model::{BandKind, ChannelEq, EqBand};
use filters::{EqCoeffs, EqState};

/// The plug-in id (also the preset folder of the Channel EQ's user presets, as before it was an insert).
pub const ID: &str = "builtin:channel-eq";

/// The bands, in order.
pub const BANDS: usize = 8;
/// The output level (dB).
pub const OUTPUT: u32 = 64;
/// One past the highest parameter id.
const ID_SPACE: usize = OUTPUT as usize + 1;

/// A band's on switch. Each band's parameters are `8 * band + field`.
pub const fn on(band: usize) -> u32 {
    (band * 8) as u32
}
/// A band's frequency (Hz).
pub const fn freq(band: usize) -> u32 {
    (band * 8 + 1) as u32
}
/// A band's gain (dB); the cut filters have none.
pub const fn gain(band: usize) -> u32 {
    (band * 8 + 2) as u32
}
/// A band's Q.
pub const fn q(band: usize) -> u32 {
    (band * 8 + 3) as u32
}
/// A cut filter's slope: 0 = 12, 1 = 24, 2 = 48 dB/octave.
pub const fn slope(band: usize) -> u32 {
    (band * 8 + 4) as u32
}

/// Whether a band is a cut filter (the first and last).
pub const fn is_cut(band: usize) -> bool {
    band == 0 || band == BANDS - 1
}

/// Each band's shape.
const KINDS: [BandKind; BANDS] =
    [BandKind::LowCut, BandKind::LowShelf, BandKind::Bell, BandKind::Bell, BandKind::Bell, BandKind::Bell, BandKind::HighShelf, BandKind::HighCut];
/// The cut filters' slopes (dB/octave) by choice index.
const SLOPE_DB: [u8; 3] = [12, 24, 48];
const SLOPES: &[&str] = &["12 dB/Oct", "24 dB/Oct", "48 dB/Oct"];

const fn toggle(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: 1.0, default, unit: Unit::None, kind: ParamKind::Toggle, log: false }
}
const fn hz(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 20.0, max: 20000.0, default, unit: Unit::Hz, kind: ParamKind::Continuous, log: true }
}
const fn db(id: u32, name: &'static str) -> ParamSpec {
    ParamSpec { id, name, min: -24.0, max: 24.0, default: 0.0, unit: Unit::Db, kind: ParamKind::Continuous, log: false }
}
const fn qf(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.1, max: 18.0, default, unit: Unit::None, kind: ParamKind::Continuous, log: true }
}
const fn slope_spec(id: u32, name: &'static str) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: 2.0, default: 1.0, unit: Unit::None, kind: ParamKind::Choice(SLOPES), log: false }
}

/// The parameters, defaulting to `ChannelEq::default`. The Linear Phase EQ has these and its
/// Quality. The ranges are the editor's (and controller assignments'), so a setting from the
/// fixed Channel EQ fits them unclamped.
pub const BAND_PARAMS: [ParamSpec; 33] = [
    toggle(on(0), "Low Cut On", 0.0),
    hz(freq(0), "Low Cut Frequency", 30.0),
    qf(q(0), "Low Cut Q", 0.71),
    slope_spec(slope(0), "Low Cut Slope"),
    toggle(on(1), "Low Shelf On", 1.0),
    hz(freq(1), "Low Shelf Frequency", 80.0),
    db(gain(1), "Low Shelf Gain"),
    qf(q(1), "Low Shelf Q", 0.71),
    toggle(on(2), "Bell 1 On", 1.0),
    hz(freq(2), "Bell 1 Frequency", 160.0),
    db(gain(2), "Bell 1 Gain"),
    qf(q(2), "Bell 1 Q", 1.1),
    toggle(on(3), "Bell 2 On", 1.0),
    hz(freq(3), "Bell 2 Frequency", 500.0),
    db(gain(3), "Bell 2 Gain"),
    qf(q(3), "Bell 2 Q", 1.1),
    toggle(on(4), "Bell 3 On", 1.0),
    hz(freq(4), "Bell 3 Frequency", 1500.0),
    db(gain(4), "Bell 3 Gain"),
    qf(q(4), "Bell 3 Q", 1.1),
    toggle(on(5), "Bell 4 On", 1.0),
    hz(freq(5), "Bell 4 Frequency", 5000.0),
    db(gain(5), "Bell 4 Gain"),
    qf(q(5), "Bell 4 Q", 1.1),
    toggle(on(6), "High Shelf On", 1.0),
    hz(freq(6), "High Shelf Frequency", 10000.0),
    db(gain(6), "High Shelf Gain"),
    qf(q(6), "High Shelf Q", 0.71),
    toggle(on(7), "High Cut On", 0.0),
    hz(freq(7), "High Cut Frequency", 18000.0),
    qf(q(7), "High Cut Q", 0.71),
    slope_spec(slope(7), "High Cut Slope"),
    db(OUTPUT, "Output Gain"),
];

static PARAMS: [ParamSpec; 33] = BAND_PARAMS;

/// The fixed Channel EQ's factory presets, as they were ("Flat" stands for the defaults).
#[rustfmt::skip] // One row per preset's values: split per value, these tables run to thousands of lines.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Flat", values: &[] },
    FactoryPreset { name: "Low Cut 80 Hz", values: &[(on(0), 1.0), (freq(0), 80.0), (q(0), 0.71), (slope(0), 1.0)] },
    FactoryPreset { name: "Vocal Presence", values: &[(on(0), 1.0), (freq(0), 90.0), (q(0), 0.71), (freq(3), 350.0), (gain(3), -2.0), (q(3), 1.2), (freq(5), 3500.0), (gain(5), 3.0), (q(5), 0.9)] },
    FactoryPreset { name: "Vocal Air", values: &[(on(0), 1.0), (freq(0), 100.0), (q(0), 0.71), (freq(5), 5000.0), (gain(5), 1.5), (q(5), 1.0), (freq(6), 12000.0), (gain(6), 4.0), (q(6), 0.71)] },
    FactoryPreset { name: "Kick Punch", values: &[(on(0), 1.0), (freq(0), 30.0), (q(0), 0.71), (freq(2), 60.0), (gain(2), 4.0), (q(2), 1.2), (freq(3), 380.0), (gain(3), -5.0), (q(3), 1.4), (freq(5), 4000.0), (gain(5), 4.0), (q(5), 1.1)] },
    FactoryPreset { name: "Snare Crack", values: &[(on(0), 1.0), (freq(0), 80.0), (q(0), 0.71), (freq(2), 200.0), (gain(2), 3.0), (q(2), 1.2), (freq(3), 800.0), (gain(3), -3.0), (q(3), 1.5), (freq(5), 5000.0), (gain(5), 4.0), (q(5), 1.0)] },
    FactoryPreset { name: "Bass Clarity", values: &[(on(0), 1.0), (freq(0), 35.0), (q(0), 0.71), (freq(3), 250.0), (gain(3), -3.0), (q(3), 1.2), (freq(4), 800.0), (gain(4), 2.5), (q(4), 1.2), (on(7), 1.0), (freq(7), 8000.0), (q(7), 0.71)] },
    FactoryPreset { name: "Acoustic Guitar", values: &[(on(0), 1.0), (freq(0), 80.0), (q(0), 0.71), (freq(3), 220.0), (gain(3), -3.0), (q(3), 1.0), (freq(5), 3000.0), (gain(5), 2.0), (q(5), 1.0), (freq(6), 10000.0), (gain(6), 2.5), (q(6), 0.71)] },
    FactoryPreset { name: "Telephone", values: &[(on(0), 1.0), (freq(0), 400.0), (q(0), 0.71), (slope(0), 2.0), (freq(4), 1500.0), (gain(4), 4.0), (q(4), 1.0), (on(7), 1.0), (freq(7), 3200.0), (q(7), 0.71), (slope(7), 2.0)] },
    FactoryPreset { name: "Master Polish", values: &[(on(0), 1.0), (freq(0), 25.0), (q(0), 0.71), (freq(1), 80.0), (gain(1), 1.0), (q(1), 0.71), (freq(3), 300.0), (gain(3), -1.0), (q(3), 0.8), (freq(6), 12000.0), (gain(6), 1.5), (q(6), 0.71)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: ID, name: "Channel EQ", category: "EQ", params: &PARAMS, meters: &[], sidechain: false, new: |sr| Box::new(ChannelEqFx::new(sr)) }
}

/// A cut filter's slope choice for a slope in dB/octave, rounded as `filters.rs` rounds it.
fn slope_choice(db_per_oct: u8) -> f32 {
    match db_per_oct {
        0..=12 => 0.0,
        13..=24 => 1.0,
        _ => 2.0,
    }
}

/// The bands the parameter values describe. Allocates nothing.
fn bands_of(value: impl Fn(u32) -> f32) -> [EqBand; BANDS] {
    std::array::from_fn(|b| EqBand {
        kind: KINDS[b],
        on: value(on(b)) >= 0.5,
        freq: value(freq(b)),
        gain: if is_cut(b) { 0.0 } else { value(gain(b)) },
        q: value(q(b)),
        slope: if is_cut(b) { SLOPE_DB[(value(slope(b)).round().max(0.0) as usize).min(2)] } else { 24 },
    })
}

/// The Channel EQ settings the parameter values describe.
pub fn channel_eq(value: impl Fn(u32) -> f32) -> ChannelEq {
    ChannelEq { on: true, bands: bands_of(&value).to_vec(), output_db: value(OUTPUT) }
}

/// The parameter values (every one) of a fixed Channel EQ's settings. Its on switch has no
/// parameter: an EQ switched off is flat, and an insert is bypassed instead. Bands beyond the
/// eight, or of another shape than the band in that place, don't occur.
pub fn values_of(eq: &ChannelEq) -> Vec<(u32, f32)> {
    let default = ChannelEq::default();
    let mut out = Vec::with_capacity(BAND_PARAMS.len());
    for b in 0..BANDS {
        let band = eq.bands.get(b).unwrap_or(&default.bands[b]);
        out.push((on(b), band.on as u8 as f32));
        out.push((freq(b), band.freq));
        if is_cut(b) {
            out.push((q(b), band.q));
            out.push((slope(b), slope_choice(band.slope)));
        } else {
            out.push((gain(b), band.gain));
            out.push((q(b), band.q));
        }
    }
    out.push((OUTPUT, eq.output_db));
    out
}

pub struct ChannelEqFx {
    sample_rate: f64,
    values: [f32; ID_SPACE],
    coeffs: EqCoeffs,
    state: EqState,
    /// A parameter changed since the coefficients were computed.
    stale: bool,
}

impl ChannelEqFx {
    pub fn new(sample_rate: f32) -> ChannelEqFx {
        let mut values = [0.0f32; ID_SPACE];
        for p in PARAMS.iter() {
            values[p.id as usize] = p.default;
        }
        ChannelEqFx { sample_rate: sample_rate as f64, values, coeffs: EqCoeffs::default(), state: EqState::default(), stale: true }
    }

    /// Computes the filters for the current values. A flat setting clears the filters' memory,
    /// as the fixed Channel EQ did.
    fn update(&mut self) {
        let bands = bands_of(|id| self.values[id as usize]);
        self.coeffs = filters::coefficients_of(true, &bands, self.values[OUTPUT as usize], self.sample_rate);
        if !self.coeffs.active {
            self.state.reset();
        }
        self.stale = false;
    }
}

impl Effect for ChannelEqFx {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(v) = self.values.get_mut(id as usize) else { return };
        if *v != value {
            *v = value;
            self.stale = true;
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, _ctx: &FxContext) {
        if self.stale {
            self.update();
        }
        self.state.process(&self.coeffs, l, r);
    }

    fn reset(&mut self) {
        self.state.reset();
        if self.stale {
            self.update();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::chorus::tests::{CTX, SR};
    use super::*;

    fn noise(len: usize, seed: u32) -> Vec<f32> {
        let mut x = seed.wrapping_mul(2_654_435_761).max(1);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32) * 1.6 - 0.8
            })
            .collect()
    }

    fn shaped() -> ChannelEq {
        let mut eq = ChannelEq::default();
        eq.bands[0].on = true;
        eq.bands[0].freq = 60.0;
        eq.bands[0].slope = 48;
        eq.bands[2].gain = 4.5;
        eq.bands[2].q = 2.3;
        eq.bands[4].gain = -7.2;
        eq.bands[4].freq = 1830.0;
        eq.bands[6].gain = 3.0;
        eq.bands[7].on = true;
        eq.bands[7].freq = 9000.0;
        eq.bands[7].slope = 12;
        eq.bands[7].q = 2.0;
        eq.output_db = -1.5;
        eq
    }

    #[test]
    fn plays_a_fixed_channel_eq_sample_for_sample() {
        // What the strip's fixed EQ did: the coefficients straight from the settings.
        let eq = shaped();
        let (mut l, mut r) = (noise(9000, 1), noise(9000, 2));
        let (mut wl, mut wr) = (l.clone(), r.clone());
        let mut st = EqState::default();
        let c = filters::coefficients(&eq, SR as f64);
        for (a, b) in wl.chunks_mut(256).zip(wr.chunks_mut(256)) {
            st.process(&c, a, b);
        }
        // The insert, through the host's clamping, in blocks of another size.
        let info = info();
        let mut fx = crate::dsp::BuiltinDsp::new(&info, SR, &values_of(&eq));
        for (a, b) in l.chunks_mut(300).zip(r.chunks_mut(300)) {
            fx.process(a, b, None, &CTX);
        }
        assert!(l.iter().zip(&wl).all(|(a, b)| a.to_bits() == b.to_bits()), "left differs");
        assert!(r.iter().zip(&wr).all(|(a, b)| a.to_bits() == b.to_bits()), "right differs");
    }

    #[test]
    fn values_round_trip_and_defaults_are_flat() {
        let eq = shaped();
        let back = channel_eq(|id| values_of(&eq).iter().find(|v| v.0 == id).map(|v| v.1).unwrap_or(0.0));
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&eq).unwrap());
        let def = channel_eq(|id| PARAMS.iter().find(|p| p.id == id).map(|p| p.default).unwrap_or(0.0));
        assert_eq!(serde_json::to_string(&def).unwrap(), serde_json::to_string(&ChannelEq::default()).unwrap());
        // At its defaults it passes the signal untouched.
        let mut fx = ChannelEqFx::new(SR);
        let (mut l, mut r) = (noise(512, 3), noise(512, 4));
        let (dl, dr) = (l.clone(), r.clone());
        fx.process(&mut l, &mut r, None, &CTX);
        assert!(l == dl && r == dr);
    }

    #[test]
    fn going_flat_clears_the_filters() {
        let mut fx = ChannelEqFx::new(SR);
        fx.set_param(gain(3), 12.0);
        let (mut l, mut r) = (noise(512, 5), noise(512, 6));
        fx.process(&mut l, &mut r, None, &CTX);
        fx.set_param(gain(3), 0.0);
        fx.set_param(gain(3), 12.0);
        // Changed back before a block ran: the filters go on.
        let (mut a, mut b) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        fx.process(&mut a, &mut b, None, &CTX);
        assert!(a.iter().any(|v| *v != 0.0), "rings on from the last block");
        fx.set_param(gain(3), 0.0);
        let (mut a, mut b) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        fx.process(&mut a, &mut b, None, &CTX);
        fx.set_param(gain(3), 12.0);
        let (mut a, mut b) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        fx.process(&mut a, &mut b, None, &CTX);
        assert!(a.iter().chain(&b).all(|v| *v == 0.0), "a flat block emptied them");
    }

    #[test]
    fn factory_presets_match_the_old_ones() {
        // Spot checks against the fixed EQ's presets as they were written.
        let preset = |name: &str| {
            let p = PRESETS.iter().find(|p| p.name == name).unwrap();
            channel_eq(|id| p.values.iter().find(|v| v.0 == id).map(|v| v.1).unwrap_or_else(|| PARAMS.iter().find(|s| s.id == id).unwrap().default))
        };
        let t = preset("Telephone");
        assert!(t.bands[0].on && t.bands[0].freq == 400.0 && t.bands[0].slope == 48 && t.bands[7].slope == 48 && t.bands[4].gain == 4.0);
        let k = preset("Kick Punch");
        assert!(k.bands[2].freq == 60.0 && k.bands[3].gain == -5.0 && k.bands[5].q == 1.1 && !k.bands[7].on);
        for p in PRESETS {
            for (id, v) in p.values {
                let spec = PARAMS.iter().find(|s| s.id == *id).unwrap_or_else(|| panic!("{}: no parameter {id}", p.name));
                assert!(*v >= spec.min && *v <= spec.max, "{}: {} out of range", p.name, spec.name);
            }
        }
    }
}
