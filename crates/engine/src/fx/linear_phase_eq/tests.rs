//! The Linear Phase EQ's response, latency, click-free changes, renders and presets.

use super::super::chorus::tests::{CTX, SR};
use super::*;

fn noise(len: usize) -> Vec<f32> {
    let mut s = 0x9e37_79b9u32;
    (0..len)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.5
        })
        .collect()
}

fn offline(values: &[(u32, f32)]) -> LinearPhaseEq {
    let mut e = LinearPhaseEq::new(SR);
    e.set_offline(true);
    for &(id, v) in values {
        e.set_param(id, v);
    }
    e.reset();
    e
}

fn run(e: &mut LinearPhaseEq, l: &mut [f32], r: &mut [f32], block: usize) {
    for (bl, br) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        e.process(bl, br, None, &CTX);
    }
}

/// The impulse response, `len` samples.
fn impulse(e: &mut LinearPhaseEq, len: usize) -> Vec<f32> {
    let mut l = vec![0.0f32; len];
    l[0] = 1.0;
    let mut r = l.clone();
    run(e, &mut l, &mut r, 512);
    l
}

/// The response at `f` Hz of an impulse response, delayed back by `d` frames: magnitude in
/// dB and the phase left over (zero for a pure delay of `d`).
fn response(h: &[f32], f: f64, d: usize) -> (f64, f64) {
    let w = std::f64::consts::TAU * f / SR as f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &x) in h.iter().enumerate() {
        let ph = -w * (n as f64 - d as f64);
        re += x as f64 * ph.cos();
        im += x as f64 * ph.sin();
    }
    (10.0 * (re * re + im * im).log10(), im.atan2(re))
}

#[test]
fn quality_sets_length_and_latency() {
    assert_eq!(taps(0, 48_000.0), 2047);
    assert_eq!(taps(1, 44_100.0), 4095);
    assert_eq!(taps(2, 96_000.0), 16_383);
    let mut e = LinearPhaseEq::new(SR);
    assert_eq!(e.latency(), 2047);
    e.set_param(QUALITY, 0.0);
    assert_eq!(e.latency(), 1023);
    e.set_param(QUALITY, 2.0);
    assert_eq!(e.latency(), 4095);
}

#[test]
fn flat_is_a_pure_delay() {
    for quality in 0..3 {
        let mut e = offline(&[(QUALITY, quality as f32)]);
        let d = e.latency();
        let x = noise(30_000);
        let (mut l, mut r) = (x.clone(), x.clone());
        run(&mut e, &mut l, &mut r, 333);
        let err = (d..x.len()).map(|n| (l[n] - x[n - d]).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-6, "quality {quality}: error {err}");
        assert!(l[..d].iter().all(|v| v.abs() < 1e-6));
    }
}

#[test]
fn a_bell_hits_its_gain_with_linear_phase() {
    let mut e = offline(&[(freq(4), 1000.0), (gain(4), 6.0), (q(4), 1.1)]);
    let d = e.latency();
    let h = impulse(&mut e, 2 * d + 1);
    let (g, _) = response(&h, 1000.0, d);
    assert!((g - 6.0).abs() < 0.1, "gain at 1 kHz {g}");
    // Symmetric about the centre: constant group delay.
    for k in 1..=d {
        assert!((h[d - k] - h[d + k]).abs() < 1e-6, "tap ±{k}: {} {}", h[d - k], h[d + k]);
    }
    // So the phase, once the delay is taken out, is zero (or π where the gain would be
    // negative, which a bell never is) at every frequency.
    for f in [50.0, 300.0, 1000.0, 2500.0, 8000.0, 15000.0] {
        let (_, ph) = response(&h, f, d);
        assert!(ph.abs() < 1e-3, "phase at {f} Hz: {ph}");
    }
    // And it follows the Channel EQ away from the bell.
    let c = crate::fx::channel_eq::filters::coefficients(&channel_eq(|id| e.param(id)), SR as f64);
    for f in [100.0, 500.0, 2000.0, 5000.0] {
        let want = 20.0 * c.magnitude(std::f64::consts::TAU * f / SR as f64).log10();
        let (got, _) = response(&h, f, d);
        assert!((got - want).abs() < 0.1, "{f} Hz: {got} vs {want}");
    }
}

#[test]
fn output_gain_alone_stays_exact() {
    let mut e = offline(&[(OUTPUT, -6.0)]);
    let d = e.latency();
    let h = impulse(&mut e, d + 10);
    assert!((h[d] - util::db_to_gain(-6.0)).abs() < 1e-6);
}

#[test]
fn changes_while_playing_fade_without_clicks() {
    // Playing live: the worker designs the new kernel and the convolver fades to it.
    let mut e = LinearPhaseEq::new(SR);
    let f = 1000.0;
    let amp = 0.5f32;
    let mut t = 0usize;
    let mut sine = |n: usize| -> Vec<f32> {
        let v = (t..t + n).map(|i| amp * (std::f32::consts::TAU * f * i as f32 / SR).sin()).collect();
        t += n;
        v
    };
    let mut out = Vec::new();
    let mut block = |e: &mut LinearPhaseEq, out: &mut Vec<f32>| {
        let mut l = sine(256);
        let mut r = l.clone();
        e.process(&mut l, &mut r, None, &CTX);
        out.extend_from_slice(&l);
    };
    for _ in 0..40 {
        block(&mut e, &mut out);
    }
    e.set_param(freq(4), f);
    e.set_param(gain(4), 12.0);
    // Give the worker time between blocks, as real time would.
    for _ in 0..60 {
        block(&mut e, &mut out);
        std::thread::sleep(Duration::from_millis(2));
    }
    let end_peak = out[out.len() - 2000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((end_peak / amp - 4.0).abs() < 0.1, "the boost arrived: peak {end_peak}");
    // The largest step of the boosted sine is 2π·f/sr of its peak; a hard switch from the
    // old level to the new would step by up to 1.5.
    let biggest = 4.0 * amp * std::f32::consts::TAU * f / SR;
    let step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(step < biggest * 1.1, "largest step {step} (the sine's own: {biggest})");
}

#[test]
fn an_instance_starts_at_its_saved_values() {
    // As the session makes one: no waiting for the worker once it plays.
    let info = info();
    let mut b = crate::dsp::BuiltinDsp::new(&info, SR, &[(gain(4), 12.0), (freq(4), 1000.0)]);
    let sine: Vec<f32> = (0..8192).map(|i| 0.25 * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
    let (mut l, mut r) = (sine.clone(), sine);
    for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        b.process(bl, br, None, &CTX);
    }
    let peak = l[6000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak / 0.25 - 4.0).abs() < 0.1, "peak {peak}");
}

#[test]
fn presets_and_extremes_stay_finite() {
    for p in PRESETS {
        let mut e = offline(p.values);
        let (mut l, mut r) = (noise(20_000), noise(20_000));
        run(&mut e, &mut l, &mut r, 512);
        assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 4.0), "{}", p.name);
    }
    // Every band at full boost, narrow, with both cuts at 48 dB/oct.
    let mut vals = vec![(OUTPUT, 24.0), (QUALITY, 0.0)];
    for b in 0..BANDS {
        vals.push((on(b), 1.0));
        vals.push((q(b), 18.0));
        if is_cut(b) {
            vals.push((slope(b), 2.0));
        } else {
            vals.push((gain(b), 24.0));
        }
    }
    let mut e = offline(&vals);
    let (mut l, mut r) = (noise(20_000), noise(20_000));
    run(&mut e, &mut l, &mut r, 1024);
    assert!(l.iter().all(|x| x.is_finite()));
}

#[test]
fn a_render_follows_automation_the_same_whatever_played_before() {
    // A render waits for a new kernel at most every `DESIGN_INTERVAL`. When the first of
    // those waits comes mustn't depend on how long the instance played before the render:
    // in a large project the device's callbacks are slow, so a render right after an edit
    // that made the instance can follow only a few frames of it, a later one many.
    let render = |live: usize| {
        let mut e = LinearPhaseEq::new(SR);
        let (mut l, mut r) = (noise(live), noise(live));
        run(&mut e, &mut l, &mut r, 512);
        e.set_offline(true);
        e.reset();
        let (mut l, mut r) = (noise(20_000), noise(20_000));
        for (k, (bl, br)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
            // A lane on Bell 3's gain that starts moving with the second block.
            if k > 0 {
                e.set_param(gain(4), (k % 12) as f32);
            }
            e.process(bl, br, None, &CTX);
        }
        l.iter().map(|x| x.to_bits()).collect::<Vec<u32>>()
    };
    assert!(render(0) == render(48_000), "the same render after a second of playing as after none");
}

/// A render plays the new settings at the same frame whether the worker keeps up or not: here
/// it has stopped altogether, as a loaded machine may not run it for a long while.
#[test]
fn a_render_takes_the_new_filter_without_the_worker() {
    let render = |worker: bool, change: bool| {
        let mut e = offline(&[]);
        if !worker {
            let l = e.link.as_ref().unwrap();
            l.shared.stop.store(true, Ordering::Release);
            l.worker.unpark();
            std::thread::sleep(Duration::from_millis(50));
        }
        let (mut l, mut r) = (noise(8192), noise(8192));
        for (k, (bl, br)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
            if k == 4 && change {
                e.set_param(gain(4), 12.0);
            }
            e.process(bl, br, None, &CTX);
        }
        l.iter().map(|x| x.to_bits()).collect::<Vec<u32>>()
    };
    let t = Instant::now();
    let without = render(false, true);
    assert!(t.elapsed() < Duration::from_secs(2), "it didn't wait for the worker: {:?}", t.elapsed());
    assert!(without != render(false, false), "the new filter plays");
    assert!(without == render(true, true), "the same render with the worker stalled as without");
}

#[test]
fn params_match_the_channel_eq_defaults() {
    let eq = channel_eq(|id| PARAMS.iter().find(|p| p.id == id).map(|p| p.default).unwrap_or(0.0));
    let def = ChannelEq::default();
    assert_eq!(serde_json::to_string(&eq).unwrap(), serde_json::to_string(&def).unwrap());
    assert_eq!(PARAMS.len(), 8 * 4 + 2);
}
