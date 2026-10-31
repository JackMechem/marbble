//! The Reverb's unit tests: decay times, stability, loudness, and that no denormals survive.

use super::*;

const SR: f32 = 48_000.0;

fn ctx() -> FxContext {
    FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 }
}

fn wet_only(room: usize) -> Reverb {
    let mut r = Reverb::new(SR);
    r.set_param(ROOM, room as f32);
    r.set_param(DRY, 0.0);
    r.set_param(WET, 1.0);
    r
}

fn run(fx: &mut Reverb, l: &mut [f32], r: &mut [f32]) {
    for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        fx.process(bl, br, None, &ctx());
    }
}

fn impulse_response(fx: &mut Reverb, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * SR) as usize;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    l[0] = 1.0;
    r[0] = 1.0;
    run(fx, &mut l, &mut r);
    (l, r)
}

/// RT60 from the Schroeder energy decay curve: the −5 to −35 dB slope (T30), doubled.
fn rt60(l: &[f32], r: &[f32]) -> f32 {
    let mut edc: Vec<f64> = l.iter().zip(r).map(|(a, b)| (a * a + b * b) as f64).collect();
    for i in (0..edc.len() - 1).rev() {
        edc[i] += edc[i + 1];
    }
    let total = edc[0];
    let db = |i: usize| 10.0 * (edc[i] / total).log10();
    let t5 = (0..edc.len()).find(|&i| db(i) <= -5.0).unwrap();
    let t35 = (0..edc.len()).find(|&i| db(i) <= -35.0).unwrap();
    2.0 * (t35 - t5) as f32 / SR
}

fn noise(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0
}

#[test]
#[allow(clippy::needless_range_loop)] // `room` is the value under test; ROOM_TYPES only names it.
fn decay_matches_rt60_for_every_room() {
    for room in 0..ROOM_TYPES.len() {
        for rt in [1.0f32, 3.0] {
            let mut fx = wet_only(room);
            fx.set_param(DECAY, rt * 1000.0);
            fx.set_param(DAMPING, 0.0);
            fx.set_param(BALANCE, 1.0);
            let (l, r) = impulse_response(&mut fx, rt * 1.6 + 0.5);
            let measured = rt60(&l, &r);
            let err = (measured - rt).abs() / rt;
            assert!(err < 0.2, "{}: RT60 {rt} s measured {measured:.2} s", ROOM_TYPES[room]);
        }
    }
}

#[test]
fn low_decay_and_damping_shape_the_decay() {
    // Longer lows, faster highs: the decay at 4 kHz is shorter than at 100 Hz.
    let band = |fx: &mut Reverb, hz: f32| {
        let n = (4.0 * SR) as usize;
        let mut l: Vec<f32> = (0..n).map(|i| if i < 4800 { (std::f32::consts::TAU * hz * i as f32 / SR).sin() } else { 0.0 }).collect();
        let mut r = l.clone();
        run(fx, &mut l, &mut r);
        // Level 1.5 s after the burst relative to just after it.
        let rms = |a: usize| (l[a..a + 4800].iter().map(|x| x * x).sum::<f32>() / 4800.0).sqrt();
        rms(4800 + 72_000) / rms(4800 + 4800)
    };
    let mut fx = wet_only(2);
    fx.set_param(DAMPING, 0.8);
    fx.set_param(LOW_DECAY, 2.0);
    fx.set_param(LOW_CUT, 20.0);
    let low = band(&mut fx, 100.0);
    fx.reset();
    let high = band(&mut fx, 6000.0);
    assert!(low > high * 10.0, "low {low} high {high}");
}

#[test]
#[allow(clippy::needless_range_loop)] // `room` is the value under test; ROOM_TYPES only names it.
fn stable_at_max_decay_and_freeze() {
    for room in 0..ROOM_TYPES.len() {
        let mut fx = wet_only(room);
        fx.set_param(DECAY, 20_000.0);
        fx.set_param(LOW_DECAY, 2.0);
        fx.set_param(DAMPING, 0.0);
        fx.set_param(SIZE, 1.0);
        fx.set_param(DENSITY, 1.0);
        fx.set_param(DIFFUSION, 1.0);
        fx.set_param(MOD_DEPTH, 1.0);
        fx.set_param(MOD_RATE, 5.0);
        let mut seed = 1u32;
        let mut peak = 0.0f32;
        let mut held = [0.0f32; 2];
        let blocks = (30.0 * SR / 512.0) as usize;
        for b in 0..blocks {
            if b == blocks / 3 {
                fx.set_param(FREEZE, 1.0);
            }
            let mut l: Vec<f32> = (0..512).map(|_| noise(&mut seed) * 0.5).collect();
            let mut r: Vec<f32> = (0..512).map(|_| noise(&mut seed) * 0.5).collect();
            fx.process(&mut l, &mut r, None, &ctx());
            let e: f32 = l.iter().chain(&r).map(|x| x * x).sum();
            assert!(e.is_finite(), "{}", ROOM_TYPES[room]);
            peak = l.iter().chain(&r).fold(peak, |p, x| p.max(x.abs()));
            // Frozen energy over half a second, a second in and at the end.
            if (blocks / 3 + 100..blocks / 3 + 150).contains(&b) {
                held[0] += e;
            }
            if b >= blocks - 50 {
                held[1] += e;
            }
        }
        assert!(peak < 30.0, "{}: peak {peak}", ROOM_TYPES[room]);
        let change = 10.0 * (held[1] / held[0]).log10();
        assert!(change.abs() < 4.0, "{}: freeze drifted {change:.1} dB", ROOM_TYPES[room]);
    }
}

#[test]
#[allow(clippy::needless_range_loop)] // `room` is the value under test; ROOM_TYPES only names it.
fn wet_is_decorrelated() {
    for room in 0..ROOM_TYPES.len() {
        let mut fx = wet_only(room);
        let mut seed = 7u32;
        let n = (3.0 * SR) as usize;
        let mut l: Vec<f32> = (0..n).map(|_| noise(&mut seed)).collect();
        let mut r = l.clone();
        run(&mut fx, &mut l, &mut r);
        let (a, b) = (&l[24_000..], &r[24_000..]);
        let dot: f64 = a.iter().zip(b).map(|(x, y)| (*x as f64) * (*y as f64)).sum();
        let ea: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum();
        let eb: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum();
        let rho = dot / (ea * eb).sqrt();
        assert!(rho.abs() < 0.3, "{}: correlation {rho:.2}", ROOM_TYPES[room]);
        // Mono-compatible: the sum keeps most of the energy.
        let mono: f64 = a.iter().zip(b).map(|(x, y)| ((*x + *y) as f64).powi(2)).sum();
        assert!(mono / (ea + eb) > 0.6, "{}: mono loss", ROOM_TYPES[room]);
    }
}

#[test]
#[allow(clippy::needless_range_loop)] // `room` is the value under test; ROOM_TYPES only names it.
fn silence_after_the_tail_without_denormals() {
    for room in 0..ROOM_TYPES.len() {
        let mut fx = wet_only(room);
        fx.set_param(DECAY, 1500.0);
        let tail = fx.tail_secs();
        let (l, r) = impulse_response(&mut fx, tail + 1.0);
        let after = (tail * SR) as usize;
        let peak = l[after..].iter().chain(&r[after..]).fold(0.0f32, |p, x| p.max(x.abs()));
        assert!(peak < 1e-3, "{}: {peak} after the tail", ROOM_TYPES[room]);
        // Keep going: it reaches exact zero and never leaves subnormals behind.
        let mut l = vec![0.0; (25.0 * SR) as usize];
        let mut r = l.clone();
        run(&mut fx, &mut l, &mut r);
        assert!(!fx.has_subnormal(), "{}", ROOM_TYPES[room]);
        let end = l.len() - 4800;
        assert!(l[end..].iter().chain(&r[end..]).all(|&x| x == 0.0), "{}: not silent", ROOM_TYPES[room]);
    }
}

fn onset(fx: &mut Reverb) -> usize {
    let (l, r) = impulse_response(fx, 0.8);
    let peak = l.iter().chain(&r).fold(0.0f32, |p, x| p.max(x.abs()));
    (0..l.len()).find(|&i| l[i].abs().max(r[i].abs()) > peak * 1e-3).unwrap()
}

#[test]
fn predelay_is_accurate() {
    for room in [0, 2, 5] {
        let mut base = wet_only(room);
        base.set_param(PREDELAY, 0.0);
        let zero = onset(&mut base);
        let mut fx = wet_only(room);
        fx.set_param(PREDELAY, 37.0);
        assert_eq!(onset(&mut fx) - zero, 1776, "{}", ROOM_TYPES[room]);
        // Synced: a 1/16 at 120 BPM is 125 ms.
        let mut fx = wet_only(room);
        fx.set_param(SYNC, 1.0);
        fx.set_param(PREDELAY_NOTE, 12.0);
        assert_eq!(onset(&mut fx) - zero, 6000, "{}", ROOM_TYPES[room]);
    }
}

#[test]
fn room_change_and_size_sweep_stay_clean() {
    let mut fx = wet_only(2);
    let mut seed = 3u32;
    let mut last = 0.0f32;
    let mut worst = 0.0f32;
    for b in 0..400 {
        if b % 80 == 40 {
            fx.set_param(ROOM, ((b / 80) % ROOM_TYPES.len()) as f32);
        }
        fx.set_param(SIZE, 0.5 + 0.5 * (b as f32 * 0.05).sin());
        let mut l: Vec<f32> = (0..512).map(|_| noise(&mut seed) * 0.1).collect();
        let mut r = l.clone();
        fx.process(&mut l, &mut r, None, &ctx());
        for &y in &l {
            assert!(y.is_finite());
            worst = worst.max((y - last).abs());
            last = y;
        }
    }
    assert!(worst < 0.5, "step {worst}");
}

#[test]
fn rooms_have_similar_loudness() {
    let mut levels = Vec::new();
    for room in 0..ROOM_TYPES.len() {
        let mut fx = wet_only(room);
        let (l, r) = impulse_response(&mut fx, 3.0);
        let e: f32 = l.iter().chain(&r).map(|x| x * x).sum();
        levels.push(10.0 * e.log10());
    }
    let max = levels.iter().cloned().fold(f32::MIN, f32::max);
    let min = levels.iter().cloned().fold(f32::MAX, f32::min);
    assert!(max - min < 8.0, "levels {levels:?}");
}

#[test]
fn presets_are_in_range() {
    assert!((10..=14).contains(&PRESETS.len()));
    for p in PRESETS {
        for &(id, v) in p.values {
            let spec = PARAMS.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{}: no param {id}", p.name));
            assert!(v >= spec.min && v <= spec.max, "{}: {} = {v}", p.name, spec.name);
        }
    }
    // Each one runs cleanly.
    for p in PRESETS {
        let mut fx = Reverb::new(SR);
        for &(id, v) in p.values {
            fx.set_param(id, v);
        }
        let (l, r) = impulse_response(&mut fx, 0.5);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()), "{}", p.name);
    }
}

#[test]
fn tail_follows_decay() {
    let mut fx = wet_only(2);
    fx.set_param(DECAY, 2000.0);
    fx.set_param(PREDELAY, 0.0);
    let t = fx.tail_secs();
    assert!(t > 3.0 && t < 3.5, "{t}");
    fx.set_param(FREEZE, 1.0);
    assert_eq!(fx.tail_secs(), MAX_TAIL_SECS);
}
