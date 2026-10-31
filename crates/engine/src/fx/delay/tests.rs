//! The Delay's unit tests.

use super::*;

const SR: f32 = 48_000.0;

fn ctx(tempo: f64) -> FxContext {
    FxContext { sample_rate: SR, tempo, playing: true, position: 0.0 }
}

/// A delay with clean, wet-only settings and sync off.
fn clean(mode: usize) -> Delay {
    let mut d = Delay::new(SR);
    d.set_param(MODE, mode as f32);
    d.set_param(SYNC, 0.0);
    d.set_param(FEEDBACK_L, 0.0);
    d.set_param(FEEDBACK_R, 0.0);
    d.set_param(DRY, 0.0);
    d.set_param(WET, 1.0);
    d.set_param(MOD_DEPTH, 0.0);
    d
}

fn run(d: &mut Delay, l: &mut [f32], r: &mut [f32], tempo: f64) {
    for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        d.process(bl, br, None, &ctx(tempo));
    }
}

fn impulse(len: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0; len];
    let mut r = vec![0.0; len];
    l[0] = 1.0;
    r[0] = 1.0;
    (l, r)
}

fn peak_index(x: &[f32]) -> usize {
    (0..x.len()).max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs())).unwrap()
}

#[test]
fn note_values() {
    assert_eq!(note_ms(6, 120.0), 500.0); // 1/4
    assert_eq!(note_ms(10, 120.0), 375.0); // 1/8 dotted
    assert!((note_ms(11, 120.0) - 166.666_67).abs() < 1e-3); // 1/8 triplet
    assert_eq!(note_ms(0, 120.0), 2000.0); // 1/1
    assert_eq!(NOTE_LABELS.len(), 18);
}

#[test]
fn first_echo_at_set_time_ms() {
    for mode in [STEREO, TAPE, DIGITAL] {
        let mut d = clean(mode);
        d.set_param(TIME_L, 250.0);
        d.set_param(TIME_R, 250.0);
        d.set_param(WOW, 0.0);
        d.set_param(FLUTTER, 0.0);
        let (mut l, mut r) = impulse(20_000);
        run(&mut d, &mut l, &mut r, 120.0);
        assert_eq!(peak_index(&l), 12_000, "mode {mode}");
        assert!(l[..12_000].iter().all(|x| x.abs() < 1e-6), "nothing before the echo, mode {mode}");
        if mode != TAPE {
            assert!((l[12_000] - 1.0).abs() < 1e-6);
        }
    }
}

#[test]
fn first_echo_synced_at_120_bpm() {
    let mut d = clean(STEREO);
    d.set_param(SYNC, 1.0);
    d.set_param(NOTE_L, 6.0); // 1/4 = 500 ms
    d.set_param(NOTE_R, 10.0); // 1/8 dotted = 375 ms
    let (mut l, mut r) = impulse(30_000);
    run(&mut d, &mut l, &mut r, 120.0);
    assert_eq!(peak_index(&l), 24_000);
    assert_eq!(peak_index(&r), 18_000);
    assert!((l[24_000] - 1.0).abs() < 1e-6 && (r[18_000] - 1.0).abs() < 1e-6);
}

#[test]
fn feedback_decays_by_the_set_amount_per_repeat() {
    let mut d = clean(DIGITAL);
    d.set_param(TIME_L, 100.0);
    d.set_param(FEEDBACK_L, 0.5);
    let (mut l, mut r) = impulse(48_000);
    run(&mut d, &mut l, &mut r, 120.0);
    for k in 1..8 {
        let expect = 0.5f32.powi(k as i32 - 1);
        assert!((l[4800 * k] - expect).abs() < 1e-5, "repeat {k}: {} vs {expect}", l[4800 * k]);
        assert!((r[4800 * k] - expect).abs() < 1e-5);
    }
    // Stereo mode keeps independent feedback per side.
    let mut d = clean(STEREO);
    d.set_param(TIME_L, 100.0);
    d.set_param(TIME_R, 100.0);
    d.set_param(FEEDBACK_L, 0.5);
    d.set_param(FEEDBACK_R, 0.25);
    let (mut l, mut r) = impulse(48_000);
    run(&mut d, &mut l, &mut r, 120.0);
    assert!((l[14_400] - 0.25).abs() < 1e-5);
    assert!((r[14_400] - 0.0625).abs() < 1e-5);
}

#[test]
fn cross_feed_moves_repeats_across() {
    let mut d = clean(STEREO);
    d.set_param(TIME_L, 100.0);
    d.set_param(TIME_R, 100.0);
    d.set_param(FEEDBACK_L, 0.8);
    d.set_param(FEEDBACK_R, 0.8);
    d.set_param(CROSSFEED, 1.0);
    let mut l = vec![0.0; 20_000];
    let mut r = vec![0.0; 20_000];
    l[0] = 1.0;
    run(&mut d, &mut l, &mut r, 120.0);
    assert!((l[4800] - 1.0).abs() < 1e-6 && r[4800].abs() < 1e-6);
    assert!((r[9600] - 0.8).abs() < 1e-5 && l[9600].abs() < 1e-6);
}

#[test]
fn ping_pong_alternates_channels() {
    let mut d = clean(PING_PONG);
    d.set_param(TIME_L, 100.0);
    d.set_param(TIME_R, 100.0);
    d.set_param(FEEDBACK_L, 0.6);
    let (mut l, mut r) = impulse(30_000);
    run(&mut d, &mut l, &mut r, 120.0);
    let expect = [(1.0, 0.0), (0.0, 0.6), (0.36, 0.0), (0.0, 0.216)];
    for (k, (el, er)) in expect.iter().enumerate() {
        let i = 4800 * (k + 1);
        assert!((l[i] - el).abs() < 1e-5 && (r[i] - er).abs() < 1e-5, "hop {k}: {} {}", l[i], r[i]);
    }
}

/// The largest sample-to-sample step of a 220 Hz sine through the delay while its time
/// changes mid-stream.
fn max_step(mode: usize, change: impl Fn(&mut Delay, usize) -> f64) -> f32 {
    let mut d = clean(mode);
    d.set_param(TIME_L, 300.0);
    d.set_param(TIME_R, 300.0);
    d.set_param(WOW, 0.0);
    d.set_param(FLUTTER, 0.0);
    d.set_param(SATURATION, 0.0);
    let mut worst = 0.0f32;
    let mut last = 0.0f32;
    let mut t = 0usize;
    for block in 0..300 {
        let tempo = change(&mut d, block);
        let mut l: Vec<f32> = (0..512).map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * (t + i) as f32 / SR).sin()).collect();
        let mut r = l.clone();
        d.process(&mut l, &mut r, None, &ctx(tempo));
        for &y in &l {
            if t > 20_000 {
                worst = worst.max((y - last).abs());
            }
            last = y;
            t += 1;
        }
    }
    worst
}

#[test]
fn time_changes_do_not_click() {
    let slope = std::f32::consts::TAU * 220.0 * 0.5 / SR;
    let jump = |d: &mut Delay, block: usize| {
        if block == 100 {
            d.set_param(TIME_L, 120.0);
            d.set_param(TIME_R, 120.0);
        }
        if block == 200 {
            d.set_param(TIME_L, 700.0);
            d.set_param(TIME_R, 700.0);
        }
        120.0
    };
    // An abrupt jump between two phases of the sine could step by up to 1.0.
    let digital = max_step(DIGITAL, jump);
    assert!(digital < 1.3 * slope, "digital step {digital} vs slope {slope}");
    let tape = max_step(TAPE, jump);
    assert!(tape < 2.5 * slope, "tape step {tape} vs slope {slope}");
    // A tempo change while synced.
    let tempo = |d: &mut Delay, block: usize| {
        d.set_param(SYNC, 1.0);
        d.set_param(NOTE_L, 9.0);
        if block < 150 { 120.0 } else { 97.0 }
    };
    let synced = max_step(STEREO, tempo);
    assert!(synced < 1.3 * slope, "synced step {synced}");
}

#[test]
fn tape_is_bounded_at_full_feedback() {
    let mut d = clean(TAPE);
    d.set_param(TIME_L, 80.0);
    d.set_param(TIME_R, 110.0);
    d.set_param(FEEDBACK_L, 1.0);
    d.set_param(FEEDBACK_R, 1.0);
    d.set_param(CROSSFEED, 0.3);
    d.set_param(SATURATION, 0.0); // the gentlest drive is the loosest bound
    d.set_param(WOW, 1.0);
    d.set_param(FLUTTER, 1.0);
    let mut peak = 0.0f32;
    let mut late = 0.0f32;
    for block in 0..(20 * 48_000 / 512) {
        let mut l: Vec<f32> = (0..512).map(|_| if block < 200 { d.noise() } else { 0.0 }).collect();
        let mut r = l.clone();
        d.process(&mut l, &mut r, None, &ctx(120.0));
        for (&a, &b) in l.iter().zip(&r) {
            assert!(a.is_finite() && b.is_finite());
            peak = peak.max(a.abs()).max(b.abs());
            if block > 1800 {
                late = late.max(a.abs());
            }
        }
    }
    assert!(peak < 4.0, "peak {peak}");
    // Over 100 % it keeps sounding on its own: tape self-oscillation.
    assert!(late > 0.05, "late {late}");
}

#[test]
fn clean_modes_are_bounded_at_full_feedback() {
    for mode in [STEREO, PING_PONG, DIGITAL] {
        let mut d = clean(mode);
        d.set_param(TIME_L, 50.0);
        d.set_param(TIME_R, 50.0);
        d.set_param(FEEDBACK_L, 1.0);
        d.set_param(FEEDBACK_R, 1.0);
        let mut peak = 0.0f32;
        for _ in 0..(10 * 48_000 / 512) {
            let mut l: Vec<f32> = (0..512).map(|_| d.noise()).collect();
            let mut r = l.clone();
            d.process(&mut l, &mut r, None, &ctx(120.0));
            peak = l.iter().chain(&r).fold(peak, |p, x| p.max(x.abs()));
        }
        assert!(peak.is_finite() && peak < 4.5, "mode {mode}: peak {peak}");
    }
}

#[test]
fn freeze_holds_the_loop_and_ignores_input() {
    let mut d = clean(DIGITAL);
    d.set_param(TIME_L, 100.0);
    d.set_param(FEEDBACK_L, 0.3);
    let (mut l, mut r) = impulse(2400);
    run(&mut d, &mut l, &mut r, 120.0);
    d.set_param(FREEZE, 1.0);
    // New input is ignored and the first echo comes round at full level for good.
    let mut l = vec![0.0; 48_000 * 5];
    let mut r = l.clone();
    l[24_000] = 1.0;
    run(&mut d, &mut l, &mut r, 120.0);
    let last = &l[l.len() - 4800..];
    let p = last.iter().fold(0.0f32, |a, x| a.max(x.abs()));
    assert!((p - 1.0).abs() < 1e-3, "held level {p}");
    assert!(last.iter().filter(|x| x.abs() > 1e-3).count() == 1, "only the frozen echo");
    assert_eq!(d.tail_secs(), MAX_TAIL_SECS);
}

#[test]
fn silence_stays_clean_without_denormals() {
    let mut d = clean(TAPE);
    d.set_param(TIME_L, 30.0);
    d.set_param(TIME_R, 40.0);
    d.set_param(FEEDBACK_L, 0.5);
    d.set_param(FEEDBACK_R, 0.5);
    d.set_param(LOW_CUT, 200.0);
    d.set_param(HIGH_CUT, 3000.0);
    let (mut l, mut r) = impulse(48_000 * 20);
    run(&mut d, &mut l, &mut r, 120.0);
    assert!(!d.has_subnormal());
    assert!(l[l.len() - 48_000..].iter().all(|&x| x == 0.0), "decays to exact silence");
}

#[test]
fn presets_are_in_range() {
    assert!((10..=12).contains(&PRESETS.len()));
    for p in PRESETS {
        for &(id, v) in p.values {
            let spec = PARAMS.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{}: no param {id}", p.name));
            assert!(v >= spec.min && v <= spec.max, "{}: {} = {v}", p.name, spec.name);
        }
        let mut d = Delay::new(SR);
        for &(id, v) in p.values {
            d.set_param(id, v);
        }
        let (mut l, mut r) = impulse(48_000);
        run(&mut d, &mut l, &mut r, 120.0);
        assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 4.0), "{}", p.name);
    }
}

#[test]
fn tail_follows_feedback_and_time() {
    let mut d = clean(DIGITAL);
    d.set_param(TIME_L, 500.0);
    assert!((d.tail_secs() - 0.5).abs() < 1e-6);
    d.set_param(FEEDBACK_L, 0.5);
    // 0.5^n = −60 dB after about 10 repeats.
    let t = d.tail_secs();
    assert!(t > 5.0 && t < 6.0, "{t}");
    d.set_param(FEEDBACK_L, 1.0);
    let t = d.tail_secs();
    assert!(t > 30.0 * 0.9, "{t}");
}
