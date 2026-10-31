//! The Compressor's unit tests.

use super::*;

const SR: f32 = 48000.0;
const CTX: FxContext = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };

fn comp(settings: &[(u32, f32)]) -> Compressor {
    let mut c = Compressor::new(SR);
    for &(id, v) in settings {
        c.set_param(id, v);
    }
    c.reset();
    c
}

/// Runs a constant (DC) level through and returns (output dB, gain reduction dB) at the
/// end. DC makes the peak and RMS detectors read the level exactly.
fn settle_dc(c: &mut Compressor, level_db: f32, secs: f32) -> (f32, f32) {
    let x = db_to_lin(level_db);
    let mut l = [x; 480];
    let mut r = [x; 480];
    let blocks = (secs * SR / 480.0) as usize;
    for _ in 0..blocks {
        l.fill(x);
        r.fill(x);
        c.process(&mut l, &mut r, None, &CTX);
    }
    let mut m = [0.0; MAX_METERS];
    c.meters(&mut m);
    (lin_to_db(l[479].abs()), m[2])
}

/// Runs `secs` of a signal and records the gain reduction meter once per 1 ms block.
fn gr_trace(c: &mut Compressor, level_db: f32, secs: f32) -> Vec<f32> {
    let x = db_to_lin(level_db);
    let mut out = Vec::new();
    let mut l = [x; 48];
    let mut r = [x; 48];
    let mut m = [0.0; MAX_METERS];
    for _ in 0..(secs * 1000.0) as usize {
        l.fill(x);
        r.fill(x);
        c.process(&mut l, &mut r, None, &CTX);
        c.meters(&mut m);
        out.push(m[2]);
    }
    out
}

/// Milliseconds until the trace first crosses `level`, going in its direction.
fn time_to(trace: &[f32], level: f32) -> f32 {
    let rising = trace.last().unwrap() >= &trace[0];
    trace.iter().position(|&g| if rising { g >= level } else { g <= level }).unwrap_or(trace.len()) as f32
}

#[test]
fn platinum_static_curve() {
    let mut c = comp(&[(param::THRESHOLD, -20.0), (param::RATIO, 4.0), (param::KNEE, 0.0), (param::ATTACK, 1.0), (param::RELEASE, 50.0)]);
    for (inp, want) in [(-40.0, -40.0), (-20.0, -20.0), (-10.0, -17.5), (0.0, -15.0)] {
        let (out, _) = settle_dc(&mut c, inp, 0.5);
        assert!((out - want).abs() < 0.05, "in {inp}: out {out}, want {want}");
    }
    // With a 12 dB knee, the threshold itself gets (1 - 1/R)·(K/2)²/(2K) = 1.125 dB.
    let mut c = comp(&[(param::THRESHOLD, -20.0), (param::RATIO, 4.0), (param::KNEE, 12.0), (param::ATTACK, 1.0), (param::RELEASE, 20.0)]);
    let (out, _) = settle_dc(&mut c, -20.0, 0.5);
    assert!((out - -21.125).abs() < 0.05, "knee: {out}");
    let (out, _) = settle_dc(&mut c, -30.0, 0.5);
    assert!((out - -30.0).abs() < 0.01, "below the knee: {out}");
}

#[test]
fn vca_and_fet_static_curves() {
    for detector in [0.0, 1.0] {
        let mut c = comp(&[(param::CIRCUIT, 1.0), (param::THRESHOLD, -30.0), (param::RATIO, 3.0), (param::KNEE, 0.0), (param::DETECTOR, detector)]);
        let (out, _) = settle_dc(&mut c, -6.0, 3.0);
        assert!((out - -22.0).abs() < 0.1, "vca: {out}");
    }
    // The FET's feedback loop lands on the button's ratio well above the knee.
    for (mode, ratio) in [(0.0, 4.0), (1.0, 8.0), (2.0, 12.0), (3.0, 20.0)] {
        let mut c = comp(&[(param::CIRCUIT, 2.0), (param::THRESHOLD, -40.0), (param::FET_RATIO, mode)]);
        let (o1, _) = settle_dc(&mut c, -10.0, 3.0);
        let (o2, _) = settle_dc(&mut c, 0.0, 3.0);
        let measured = 10.0 / (o2 - o1);
        assert!((measured - ratio).abs() / ratio < 0.08, "fet {ratio}:1 measured {measured}");
        let gr = static_reduction(&c.curve_params(), 0.0);
        assert!((o2 - (0.0 - gr)).abs() < 0.3, "fet static {o2} vs {}", -gr);
    }
}

#[test]
fn fet_all_buttons_overcompresses() {
    let mut c = comp(&[(param::CIRCUIT, 2.0), (param::THRESHOLD, -40.0), (param::FET_RATIO, 4.0)]);
    let (o1, _) = settle_dc(&mut c, -20.0, 3.0);
    let (o2, _) = settle_dc(&mut c, -6.0, 3.0);
    assert!(o2 < o1, "all buttons: louder in should be quieter out ({o1} → {o2})");
}

/// The local ratio (input dB per output dB) of the steady-state curve at `x`.
fn local_ratio(p: &CurveParams, x: f32) -> f32 {
    let y = |x: f32| x - static_reduction(p, x);
    1.0 / (y(x + 0.5) - y(x - 0.5))
}

#[test]
fn opto_and_vari_mu_ratio_rises_with_level() {
    for circuit in [Circuit::Opto, Circuit::VariMu] {
        let p = CurveParams { circuit, threshold: -30.0, ratio: 6.0, knee: 0.0, fet_mode: 0, opto_limit: false };
        let ratios: Vec<f32> = [-40.0, -32.0, -26.0, -18.0, -6.0].iter().map(|&x| local_ratio(&p, x)).collect();
        assert!(ratios.windows(2).all(|w| w[1] > w[0]), "{circuit:?}: {ratios:?}");
        assert!(ratios[0] < 1.3 && *ratios.last().unwrap() > 2.0, "{circuit:?}: {ratios:?}");
    }
    // Limit is steeper than Compress.
    let comp_p = CurveParams { circuit: Circuit::Opto, threshold: -30.0, ratio: 1.0, knee: 0.0, fet_mode: 0, opto_limit: false };
    let lim_p = CurveParams { opto_limit: true, ..comp_p };
    assert!(local_ratio(&lim_p, -10.0) > 2.0 * local_ratio(&comp_p, -10.0));
    // And the running compressor settles onto its static curve.
    for circuit in [3.0, 4.0] {
        let mut c = comp(&[(param::CIRCUIT, circuit), (param::THRESHOLD, -30.0), (param::RATIO, 6.0)]);
        let (_, gr) = settle_dc(&mut c, -10.0, 20.0);
        let want = static_reduction(&c.curve_params(), -10.0);
        assert!((gr - want).abs() < 0.1, "circuit {circuit}: gr {gr} want {want}");
    }
}

#[test]
fn platinum_attack_and_release_match_the_knobs() {
    // Threshold -20, 4:1, hard knee: a step to -10 dB settles at 7.5 dB of reduction. A
    // one-pole envelope reaches 63 % of a step in one time constant.
    let mut c = comp(&[(param::THRESHOLD, -20.0), (param::RATIO, 4.0), (param::KNEE, 0.0), (param::ATTACK, 20.0), (param::RELEASE, 200.0)]);
    let up = gr_trace(&mut c, -10.0, 0.4);
    assert!((up.last().unwrap() - 7.5).abs() < 0.01);
    let t_att = time_to(&up, 7.5 * (1.0 - (-1.0f32).exp()));
    assert!((t_att - 20.0).abs() <= 1.5, "attack {t_att} ms");
    let down = gr_trace(&mut c, -40.0, 1.5);
    let t_rel = time_to(&down, 7.5 * (-1.0f32).exp());
    assert!((t_rel - 200.0).abs() <= 3.0, "release {t_rel} ms");
}

#[test]
fn fet_attacks_faster_than_opto() {
    let attack_ms = |circuit: f32| {
        let mut c = comp(&[(param::CIRCUIT, circuit), (param::THRESHOLD, -30.0), (param::ATTACK, 0.02), (param::FET_RATIO, 1.0)]);
        let trace = gr_trace(&mut c, -6.0, 0.3);
        let fin = *trace.last().unwrap();
        time_to(&trace, fin * 0.63)
    };
    let fet = attack_ms(2.0);

    let opto = attack_ms(3.0);
    assert!(fet <= 1.0 && opto >= 5.0, "fet {fet} ms, opto {opto} ms");
}

#[test]
fn opto_release_lengthens_after_long_heavy_compression() {
    let release_ms = |hold_secs: f32| {
        let mut c = comp(&[(param::CIRCUIT, 3.0), (param::THRESHOLD, -40.0)]);
        gr_trace(&mut c, -6.0, hold_secs);
        let trace = gr_trace(&mut c, -80.0, 20.0);
        time_to(&trace, 1.0)
    };
    let short = release_ms(0.1);
    let long = release_ms(8.0);
    assert!(short < 400.0, "short burst released in {short} ms");
    assert!(long > 4.0 * short && long > 2000.0, "long compression released in {long} ms (short {short})");
}

#[test]
fn opto_releases_in_two_stages() {
    let mut c = comp(&[(param::CIRCUIT, 3.0), (param::THRESHOLD, -40.0)]);
    let held = *gr_trace(&mut c, -6.0, 8.0).last().unwrap();
    let trace = gr_trace(&mut c, -80.0, 10.0);
    // Most of the first half goes quickly, the rest hangs on for seconds.
    assert!(trace[200] < 0.6 * held && trace[200] > 0.3 * held, "after 200 ms: {} of {held}", trace[200]);
    assert!(trace[2000] > 0.15 * held, "after 2 s: {} of {held}", trace[2000]);
}

#[test]
fn mix_zero_is_exactly_dry() {
    for circuit in 0..5 {
        let mut c = comp(&[(param::CIRCUIT, circuit as f32), (param::MIX, 0.0), (param::THRESHOLD, -40.0), (param::DISTORTION, 1.0)]);
        let src: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
        let mut l = src.clone();
        let mut r = src.clone();
        for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            c.process(bl, br, None, &CTX);
        }
        assert_eq!(l, src, "circuit {circuit}");
        assert_eq!(r, src);
    }
}

#[test]
fn auto_gain_is_roughly_level_neutral() {
    for (setting, level) in [(2.0, -12.0), (3.0, -24.0), (1.0, 0.0)] {
        let mut c = comp(&[(param::THRESHOLD, -30.0), (param::RATIO, 4.0), (param::AUTO_GAIN, setting), (param::ATTACK, 1.0)]);
        let (out, gr) = settle_dc(&mut c, level, 0.5);
        assert!(gr > 4.0, "auto gain {setting}: gr {gr} out {out}");
        assert!((out - level).abs() < 0.1, "auto gain {setting}: in {level} out {out}");
    }
}

#[test]
fn lookahead_reports_latency_and_delays_audio() {
    let mut c = comp(&[(param::LOOKAHEAD, 5.0), (param::THRESHOLD, 0.0)]);
    assert_eq!(c.latency(), 240);
    let mut l = vec![0.0f32; 1024];
    let mut r = vec![0.0f32; 1024];
    l[0] = 0.5;
    r[0] = 0.5;
    c.process(&mut l, &mut r, None, &CTX);
    assert_eq!(l[240], 0.5);
    assert!(l.iter().enumerate().all(|(i, &v)| i == 240 || v == 0.0));
}

#[test]
fn side_chain_key_drives_the_detector() {
    let mut c = comp(&[(param::THRESHOLD, -30.0), (param::RATIO, 10.0), (param::KNEE, 0.0), (param::ATTACK, 1.0)]);
    let mut l = [0.1f32; 480];
    let mut r = [0.1f32; 480];
    let key = [0.9f32; 480];
    for _ in 0..50 {
        l.fill(0.1);
        r.fill(0.1);
        c.process(&mut l, &mut r, Some((&key, &key)), &CTX);
    }
    // The quiet input (-20 dB, under the threshold by itself) is ducked by the loud key.
    assert!(l[479] < 0.02, "{}", l[479]);
}

#[test]
fn listen_hears_the_filtered_key() {
    let mut c = comp(&[(param::LISTEN, 1.0), (param::SC_MODE, 1.0), (param::SC_FREQ, 1000.0)]);
    // DC goes through a high-pass side chain as nothing.
    let mut l = [0.5f32; 1024];
    let mut r = [0.5f32; 1024];
    for _ in 0..20 {
        l.fill(0.5);
        r.fill(0.5);
        c.process(&mut l, &mut r, None, &CTX);
    }
    assert!(l[1023].abs() < 1e-3);
}

#[test]
fn silence_stays_clean() {
    for circuit in 0..5 {
        let mut c =
            comp(&[(param::CIRCUIT, circuit as f32), (param::SC_MODE, 3.0), (param::DETECTOR, 1.0), (param::AUTO_RELEASE, 1.0), (param::TIME_CONSTANT, 5.0)]);
        // A loud burst, then a long silence: every state decays to exact zeros.
        gr_trace(&mut c, -3.0, 0.2);
        let mut l = [0.0f32; 1024];
        let mut r = [0.0f32; 1024];
        for _ in 0..2000 {
            l.fill(0.0);
            r.fill(0.0);
            c.process(&mut l, &mut r, None, &CTX);
        }
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && (*v == 0.0 || v.abs() > f32::MIN_POSITIVE)), "circuit {circuit}");
        for ch in &c.chans {
            for v in [ch.ms, ch.gr, ch.slow, ch.slower, ch.memory] {
                assert!(v == 0.0 || v.abs() > f32::MIN_POSITIVE, "circuit {circuit}: denormal state {v}");
            }
        }
    }
}

#[test]
fn presets_are_valid() {
    assert!((10..=14).contains(&PRESETS.len()));
    super::super::dyn_common::check_presets(&PARAMS, PRESETS);
}

#[test]
fn vca_snaps_to_console_positions() {
    let c = comp(&[(param::CIRCUIT, 1.0), (param::ATTACK, 2.0), (param::RELEASE, 900.0)]);
    assert_eq!(c.effective_times(), (3.0, 1200.0));
}
