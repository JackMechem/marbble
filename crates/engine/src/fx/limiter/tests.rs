//! The Limiter: the ceiling in every mode, latency, transparency and presets.

use super::*;

const SR: f32 = 48000.0;
const CTX: FxContext = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };

fn limiter(settings: &[(u32, f32)]) -> Limiter {
    let mut lim = Limiter::new(SR);
    for &(id, v) in settings {
        lim.set_param(id, v);
    }
    lim.reset();
    lim
}

fn run(lim: &mut Limiter, l: &mut [f32], r: &mut [f32]) {
    for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        lim.process(bl, br, None, &CTX);
    }
}

/// A hard test signal at +12 dBFS: tones that put peaks between samples (fs/4 at 45°,
/// and others near the top of the band), a kick-like thump and a burst of dense,
/// noise-like program (random tones up to 18 kHz, like real material above which there is
/// little energy).
fn hot_signal(len: usize, seed: u32) -> (Vec<f32>, Vec<f32>) {
    let mut state = seed;
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32
    };
    let tau = std::f32::consts::TAU;
    let mut cluster = || -> Vec<(f32, f32)> { (0..24).map(|_| (200.0 + 17800.0 * rand(), tau * rand())).collect() };
    let (burst, side) = (cluster(), cluster());
    let dense = |set: &[(f32, f32)], t: f32| set.iter().map(|&(f, ph)| (tau * f * t + ph).sin()).sum::<f32>() / 5.0;
    let mut l = Vec::with_capacity(len);
    let mut r = Vec::with_capacity(len);
    for n in 0..len {
        let t = n as f32 / SR;
        let mut x = 0.45 * (tau * 12000.0 * t + std::f32::consts::FRAC_PI_4).sin() + 0.3 * (tau * 997.0 * t).sin() + 0.25 * (tau * 15013.0 * t).sin();
        let sec = n % 24000;
        if sec < 2400 {
            // A decaying 60 Hz thump every half second.
            x += 1.2 * (tau * 60.0 * t).sin() * (-(sec as f32) / 600.0).exp();
        }
        if (12000..14000).contains(&sec) {
            let w = 0.5 - 0.5 * (tau * (sec - 12000) as f32 / 2000.0).cos();
            x += w * dense(&burst, t);
        }
        l.push(4.0 * x / 1.6);
        r.push(4.0 * (0.8 * x + 0.2 * dense(&side, t)) / 1.6);
    }
    (l, r)
}

/// A reference true-peak measurement: 16× windowed-sinc reconstruction over ±64 samples,
/// far more accurate than the limiter's own 4× estimator.
fn reference_true_peak(x: &[f32]) -> f32 {
    let half = 64i64;
    let mut peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
    for n in half as usize..x.len() - half as usize - 1 {
        if x[n].abs().max(x[n + 1].abs()) < 0.3 {
            continue;
        }
        for k in 1..16 {
            let t = n as f64 + k as f64 / 16.0;
            let mut acc = 0.0f64;
            for m in (n as i64 - half + 1)..=(n as i64 + half) {
                let d = t - m as f64;
                let sinc = (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d);
                let w = 0.5 + 0.5 * (std::f64::consts::PI * d / (half as f64 + 1.0)).cos();
                acc += x[m as usize] as f64 * sinc * w;
            }
            peak = peak.max(acc.abs());
        }
    }
    peak as f32
}

#[test]
fn precise_never_exceeds_the_ceiling_even_between_samples() {
    for (seed, ceil_db, look) in [(1, -0.1, 2.0), (5, -1.0, 5.0), (9, -0.3, 0.0)] {
        let mut lim = limiter(&[(param::CEILING, ceil_db), (param::LOOKAHEAD, look)]);
        let (mut l, mut r) = hot_signal(48000, seed);
        run(&mut lim, &mut l, &mut r);
        let ceil = db_to_lin(ceil_db);
        let sample_peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(sample_peak <= ceil, "sample peak {sample_peak}");
        // It is actually limiting (about 12 dB), not just turning down.
        assert!(sample_peak > 0.95 * ceil);
        for ch in [&l, &r] {
            let tp = lin_to_db(reference_true_peak(ch));
            assert!(tp <= ceil_db, "seed {seed}, lookahead {look}: true peak {tp} dBTP over {ceil_db}");
        }
    }
}

#[test]
fn every_mode_holds_the_ceiling() {
    for (mode, tp, look) in [(0.0, 0.0, 0.0), (0.0, 0.0, 5.0), (1.0, 1.0, 2.0), (2.0, 1.0, 2.0), (1.0, 0.0, 0.0), (2.0, 0.0, 10.0)] {
        let mut lim = limiter(&[(param::MODE, mode), (param::TRUE_PEAK, tp), (param::LOOKAHEAD, look), (param::CEILING, -1.0), (param::GAIN, 6.0)]);
        let (mut l, mut r) = hot_signal(30000, 7);
        run(&mut lim, &mut l, &mut r);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= db_to_lin(-1.0), "mode {mode} tp {tp} lookahead {look}: {peak}");
        let mut m = [0.0; MAX_METERS];
        lim.meters(&mut m);
        // (Adaptive's saturator does much of the work before the limiter sees it.)
        if mode < 2.0 {
            assert!(m[2] > 6.0, "gain reduction {}", m[2]);
        }
    }
}

#[test]
fn latency_equals_lookahead() {
    for (look, tp, want) in [(2.0, 1.0, 96), (5.0, 0.0, 240), (0.0, 0.0, 0), (0.0, 1.0, TP_MIN_LATENCY), (20.0, 1.0, 960)] {
        let lim = limiter(&[(param::LOOKAHEAD, look), (param::TRUE_PEAK, tp)]);
        assert_eq!(lim.latency(), want, "lookahead {look} ms, true peak {tp}");
    }
}

#[test]
fn transparent_below_the_ceiling() {
    for (tp, look) in [(1.0, 2.0), (0.0, 5.0), (0.0, 0.0)] {
        let mut lim = limiter(&[(param::TRUE_PEAK, tp), (param::LOOKAHEAD, look)]);
        let lat = lim.latency();
        let src: Vec<f32> = (0..20000).map(|n| 0.5 * (n as f32 * 0.031).sin() + 0.3 * (n as f32 * 0.0021).sin()).collect();
        let mut l = src.clone();
        let mut r: Vec<f32> = src.iter().map(|v| -v).collect();
        run(&mut lim, &mut l, &mut r);
        for i in 0..src.len() - lat {
            assert_eq!(l[i + lat], src[i], "sample {i} (tp {tp}, lookahead {look})");
            assert_eq!(r[i + lat], -src[i]);
        }
    }
}

#[test]
fn presets_are_valid() {
    assert!((5..=8).contains(&PRESETS.len()));
    super::super::dyn_common::check_presets(&PARAMS, PRESETS);
}

#[test]
fn silence_stays_clean() {
    for mode in [0.0, 1.0, 2.0] {
        let mut lim = limiter(&[(param::MODE, mode)]);
        let (mut l, mut r) = hot_signal(4800, 3);
        run(&mut lim, &mut l, &mut r);
        let mut l = vec![0.0f32; 96000];
        let mut r = vec![0.0f32; 96000];
        run(&mut lim, &mut l, &mut r);
        // Past the lookahead delay still holding the burst, silence in is silence out.
        assert!(l[1000..].iter().chain(&r[1000..]).all(|v| *v == 0.0), "mode {mode}");
        assert!(lim.env.is_finite() && lim.env_slow.is_finite());
    }
}
