//! The filters' responses, stability and self-oscillation.

use super::super::test_util::*;
use super::*;

/// Lets the ladder ring with no input, from a small kick, and returns the settled signal.
fn self_oscillation(hz: f32, rate: f32, k: f32, seconds: f32) -> Vec<f32> {
    let mut f = Ladder::default();
    f.set(cutoff_g(hz, rate), k);
    let n = (seconds * rate) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let input = if i < 8 { 0.2 } else { 0.0 };
        out.push(f.tick(input));
    }
    out.split_off(n / 2)
}

#[test]
fn self_oscillates_in_tune_across_the_range() {
    // A ladder at 2x, as a voice runs it.
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        let rate = 2.0 * sr;
        let mut worst = 0.0f32;
        for note in (24..=96).step_by(6) {
            let hz = super::super::note_hz(note as f32);
            let x = self_oscillation(hz, rate, 5.0, 2.5);
            let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.2, "{hz} Hz at {sr}: no self-oscillation (peak {peak})");
            let got = pitch_hz(&x, rate);
            let c = cents(got, hz);
            worst = worst.max(c.abs());
            assert!(c.abs() < 5.0, "{hz} Hz at {sr}: {got} Hz, {c} cents");
        }
        println!("ladder self-oscillation at {sr} x2, 24..96: worst {worst:.2} cents");
    }
}

#[test]
fn doesnt_self_oscillate_below_k_4() {
    let x = self_oscillation(1000.0, 96000.0, 3.7, 1.5);
    assert!(x.iter().all(|v| v.abs() < 1e-3), "{}", x.iter().fold(0.0f32, |m, v| m.max(v.abs())));
}

#[test]
fn rolls_off_24_db_per_octave_and_passes_dc() {
    let rate = 96000.0;
    let level = |hz: f32| {
        let mut f = Ladder::default();
        f.set(cutoff_g(1000.0, rate), 0.0);
        let n = 1 << 16;
        let x: Vec<f32> = (0..n).map(|i| f.tick(0.01 * (std::f64::consts::TAU * hz as f64 * i as f64 / rate as f64).sin() as f32)).collect();
        x[n / 2..].iter().fold(0.0f32, |m, v| m.max(v.abs())) / 0.01
    };
    let (a, b) = (level(4000.0), level(8000.0));
    let slope = db((b / a) as f64);
    assert!((-25.5..-22.0).contains(&slope), "{slope} dB per octave");
    assert!(db(level(50.0) as f64).abs() < 0.2, "passband {}", level(50.0));
    // Four poles at cutoff: each -3 dB, so about -12 dB.
    let at = db(level(1000.0) as f64);
    assert!((-13.0..-11.0).contains(&at), "{at} dB at the cutoff");
}

#[test]
fn no_nan_or_runaway_on_extreme_settings() {
    let rate = 96000.0;
    let cases = [(5.0f32, 5.0f32, 1.0f32), (40000.0, 5.0, 1.0), (40000.0, 20.0, 100.0), (20.0, 20.0, 100.0), (1000.0, 0.0, 1000.0), (1000.0, 5.0, 1e-30)];
    for (hz, k, level) in cases {
        let mut f = Ladder::default();
        f.set(cutoff_g(hz, rate), k);
        let mut peak = 0.0f32;
        for i in 0..200_000 {
            // An edgy input, and the cutoff jumping around at the audio rate too.
            let x = if (i / 37) % 2 == 0 { level } else { -level };
            let g = hz * (1.0 + 0.9 * ((i as f32) * 0.01).sin());
            f.set(cutoff_g(g, rate), k);
            let y = f.tick(x);
            assert!(y.is_finite(), "{hz} {k} {level}: {y}");
            peak = peak.max(y.abs());
        }
        assert!(peak < 1000.0 * level.max(1.0), "{hz} {k} {level}: {peak}");
    }
    // A NaN that gets in anyway doesn't stay.
    let mut f = Ladder::default();
    f.set(0.3, 3.0);
    f.tick(f32::NAN);
    f.tick(f32::NAN);
    for _ in 0..16 {
        f.tick(0.0);
    }
    assert!(f.tick(0.0).is_finite());
}

#[test]
fn state_doesnt_go_denormal() {
    let mut f = Ladder::default();
    f.set(cutoff_g(1000.0, 96000.0), 2.0);
    f.tick(1.0);
    for _ in 0..4_000_000 {
        f.tick(0.0);
    }
    assert!(f.z.iter().all(|z| *z == 0.0 || z.abs() >= f32::MIN_POSITIVE), "{:?}", f.z);
}

/// A filter's level at `hz` over `level` at the input, once settled.
fn gain_of(f: &mut impl ZdfFilter, hz: f32, rate: f32, level: f32) -> f32 {
    let n = 1 << 16;
    let x: Vec<f32> = (0..n).map(|i| f.tick(level * (std::f64::consts::TAU * hz as f64 * i as f64 / rate as f64).sin() as f32)).collect();
    x[n / 2..].iter().fold(0.0f32, |m, v| m.max(v.abs())) / level
}

fn ring(f: &mut impl ZdfFilter, seconds: f32, rate: f32) -> Vec<f32> {
    let n = (seconds * rate) as usize;
    let mut out: Vec<f32> = (0..n).map(|i| f.tick(if i < 8 { 0.2 } else { 0.0 })).collect();
    out.split_off(n / 2)
}

#[test]
fn ota_cascade_rolls_off_24_db_and_self_oscillates_in_tune() {
    let rate = 96000.0;
    let g = cutoff_g(1000.0, rate);
    let mut f = OtaCascade::new(1.0, 0.0);
    f.set(g, 0.0);
    let (a, b) = (gain_of(&mut f, 4000.0, rate, 0.01), gain_of(&mut f, 8000.0, rate, 0.01));
    let slope = db((b / a) as f64);
    assert!((-25.5..-22.0).contains(&slope), "{slope} dB per octave");
    assert!(db(gain_of(&mut f, 50.0, rate, 0.01) as f64).abs() < 0.2);
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        let rate = 2.0 * sr;
        let mut worst = 0.0f32;
        for note in (36..=96).step_by(6) {
            let hz = crate::synth_dsp::note_hz(note as f32);
            let mut f = OtaCascade::new(1.0, 0.0);
            f.set(cutoff_g(hz, rate), 4.3);
            let x = ring(&mut f, 2.5, rate);
            assert!(x.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.1, "{hz}: no self-oscillation");
            let c = cents(pitch_hz(&x, rate), hz);
            worst = worst.max(c.abs());
        }
        println!("OTA cascade self-oscillation at {sr} x2, notes 36..96: worst {worst:.1} cents");
        assert!(worst < 30.0, "{worst}");
    }
    // Below 4 it dies away.
    let mut f = OtaCascade::new(1.0, 0.0);
    f.set(cutoff_g(1000.0, rate), 3.6);
    assert!(ring(&mut f, 1.5, rate).iter().all(|v| v.abs() < 1e-3));
}

#[test]
fn ota_cascade_bass_keeps_the_pass_band_as_resonance_rises() {
    let rate = 96000.0;
    let g = cutoff_g(2000.0, rate);
    let level = |bass: f32, k: f32| {
        let mut f = OtaCascade::new(1.0, bass);
        f.set(g, k);
        gain_of(&mut f, 80.0, rate, 0.02)
    };
    assert!(level(0.0, 3.5) < 0.3, "{}", level(0.0, 3.5));
    assert!((level(1.0, 3.5) - 1.0).abs() < 0.1, "{}", level(1.0, 3.5));
}

#[test]
fn ota_slew_bends_big_edges_earlier() {
    let rate = 96000.0;
    let g = cutoff_g(1000.0, rate);
    let at = |slew: f32, level: f32| {
        let mut f = OtaCascade::new(slew, 0.0);
        f.set(g, 0.0);
        gain_of(&mut f, 2000.0, rate, level)
    };
    assert!(at(2.0, 2.0) < at(0.5, 2.0), "{} {}", at(2.0, 2.0), at(0.5, 2.0));
    // Quietly, they agree.
    assert!((at(2.0, 0.005) - at(0.5, 0.005)).abs() < 0.01);
}

#[test]
fn diode_ladder_slope_is_shallower_than_24_db_near_the_cutoff() {
    let rate = 192000.0;
    let g = cutoff_g(1000.0, rate);
    let level = |hz: f32| {
        let mut f = DiodeLadder::default();
        f.set(g, 0.0);
        gain_of(&mut f, hz, rate, 0.01)
    };
    let near = db((level(2000.0) / level(1000.0)) as f64);
    let above = db((level(4000.0) / level(2000.0)) as f64);
    let far = db((level(16000.0) / level(8000.0)) as f64);
    println!("diode ladder: an octave up from the cutoff {near:.1} dB, the next {above:.1} dB, far above {far:.1} dB");
    assert!((-17.0..-10.0).contains(&near), "{near}");
    assert!((-21.0..-15.0).contains(&above), "{above}");
    assert!((-26.0..-20.0).contains(&far), "{far}");
    // Its lowest pole sits about a decade below the cutoff: the pass band sags long before it.
    assert!(db(level(10.0) as f64).abs() < 0.3);
    assert!((-7.0..-1.0).contains(&db(level(150.0) as f64)), "{}", db(level(150.0) as f64));
}

#[test]
fn diode_ladder_peaks_at_its_cutoff_and_starts_ringing_at_1() {
    for &(hz, rate) in &[(200.0f32, 96000.0f32), (800.0, 96000.0), (3000.0, 96000.0)] {
        let mut f = DiodeLadder::default();
        f.set(cutoff_g(hz, rate), 1.05);
        let x = ring(&mut f, 2.5, rate);
        assert!(x.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.05, "{hz}: nothing");
        let c = cents(pitch_hz(&x, rate), hz);
        println!("diode ladder self-oscillation at {hz} Hz: {c:.1} cents");
        // No trim as the transistor ladder has: the instrument it models doesn't track a keyboard.
        assert!(c.abs() < 120.0, "{hz}: {c}");
        let mut f = DiodeLadder::default();
        f.set(cutoff_g(hz, rate), 0.92);
        assert!(ring(&mut f, 2.0, rate).iter().all(|v| v.abs() < 1e-2), "{hz} rings below 1");
    }
}

#[test]
fn diode_feedback_highpass_keeps_the_low_end() {
    let rate = 96000.0;
    let g = cutoff_g(1000.0, rate);
    let level = |hp: f32| {
        let mut f = DiodeLadder::default();
        f.set(g, 0.85);
        f.set_feedback_highpass(hp);
        gain_of(&mut f, 40.0, rate, 0.02)
    };
    let (off, on) = (level(0.0), level(cutoff_g(150.0, rate)));
    println!("diode ladder at 40 Hz, resonance 0.85: {off:.3} without the high-pass, {on:.3} with");
    assert!(on > 2.0 * off, "{on} {off}");
}

#[test]
fn svf_outputs_and_resonance() {
    let rate = 96000.0;
    let g = cutoff_g(1000.0, rate);
    let sig = |hz: f32, r: f32, pick: fn(&Svf) -> f32| {
        let mut f = Svf::default();
        f.set(g, r);
        let n = 1 << 16;
        let mut peak = 0.0f32;
        for i in 0..n {
            f.tick(0.01 * (std::f64::consts::TAU * hz as f64 * i as f64 / rate as f64).sin() as f32);
            if i > n / 2 {
                peak = peak.max(pick(&f).abs());
            }
        }
        peak / 0.01
    };
    let lp = |f: &Svf| f.lp;
    let hp = |f: &Svf| f.hp;
    let bp = |f: &Svf| f.bp;
    let notch = |f: &Svf| f.lp + f.hp;
    let slope = db((sig(8000.0, 0.0, lp) / sig(4000.0, 0.0, lp)) as f64);
    assert!((-13.5..-10.5).contains(&slope), "{slope}");
    assert!(db(sig(100.0, 0.0, lp) as f64).abs() < 0.2);
    assert!(db(sig(10000.0, 0.0, hp) as f64).abs() < 0.3);
    assert!(sig(100.0, 0.5, hp) < 0.05);
    assert!(sig(100.0, 0.5, bp) < 0.2 && sig(10000.0, 0.5, bp) < 0.2 && sig(1000.0, 0.5, bp) > 0.9);
    assert!(sig(1000.0, 0.2, notch) < 0.02, "{}", sig(1000.0, 0.2, notch));
    assert!(sig(1000.0, 0.95, lp) > 5.0, "{}", sig(1000.0, 0.95, lp));
}

#[test]
fn new_filters_stay_finite_and_bounded() {
    let rate = 96000.0;
    fn run(mut f: impl ZdfFilter, res: f32, rate: f32) {
        for &(hz, level) in &[(5.0f32, 1.0f32), (40000.0, 1.0), (40000.0, 100.0), (20.0, 100.0), (1000.0, 1000.0), (1000.0, 1e-30)] {
            let mut peak = 0.0f32;
            for i in 0..200_000 {
                let x = if (i / 37) % 2 == 0 { level } else { -level };
                let c = hz * (1.0 + 0.9 * ((i as f32) * 0.01).sin());
                f.set(cutoff_g(c, rate), res);
                let y = f.tick(x);
                assert!(y.is_finite(), "{hz} {level}");
                peak = peak.max(y.abs());
            }
            assert!(peak < 2000.0 * level.max(1.0), "{hz} {level}: {peak}");
        }
        f.tick(f32::NAN);
        f.tick(f32::NAN);
        for _ in 0..16 {
            f.tick(0.0);
        }
        f.reset();
        assert!(f.tick(0.0).is_finite());
    }
    run(OtaCascade::new(1.6, 0.6), 5.0, rate);
    run(OtaCascade::new(0.5, 0.0), 8.0, rate);
    run(DiodeLadder::default(), 1.5, rate);
    run(Svf::default(), 1.0, rate);
    let mut d = DiodeLadder::default();
    d.set_feedback_highpass(cutoff_g(150.0, rate));
    run(d, 1.2, rate);
}

#[test]
fn new_filters_dont_go_denormal() {
    let g = cutoff_g(1000.0, 96000.0);
    let mut f = OtaCascade::new(1.0, 0.0);
    f.set(g, 2.0);
    f.tick(1.0);
    let mut d = DiodeLadder::default();
    d.set(g, 0.5);
    d.set_feedback_highpass(cutoff_g(100.0, 96000.0));
    d.tick(1.0);
    let mut s = Svf::default();
    s.set(g, 0.5);
    s.tick(1.0);
    for _ in 0..4_000_000 {
        f.tick(0.0);
        d.tick(0.0);
        s.tick(0.0);
    }
    let ok = |z: f32| z == 0.0 || z.abs() >= f32::MIN_POSITIVE;
    assert!(f.z.iter().all(|z| ok(*z)), "{:?}", f.z);
    assert!(d.z.iter().all(|z| ok(*z)) && ok(d.hp_z), "{:?}", d.z);
    assert!(ok(s.ic1) && ok(s.ic2));
}
