//! The Loudness Meter against BS.1770 and the EBU Tech 3341/3342 cases, and its pass-through.

use super::*;
use std::f64::consts::TAU;

const M: usize = 0;
const S: usize = 1;
const I: usize = 2;
const LRA: usize = 3;
const TP: usize = 4;
const PEAK_L: usize = 5;

/// Plays stereo sine segments of (dBFS peak level, seconds) through a fresh meter, with
/// continuous phase, and returns the final meters.
fn play(sr: f32, freq: f64, segments: &[(f64, f64)]) -> [f32; MAX_METERS] {
    let mut fx = Loudness::new(sr);
    play_on(&mut fx, sr, freq, segments)
}

fn play_on(fx: &mut Loudness, sr: f32, freq: f64, segments: &[(f64, f64)]) -> [f32; MAX_METERS] {
    let ctx = FxContext { sample_rate: sr, tempo: 120.0, playing: true, position: 0.0 };
    let mut n = 0usize;
    let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
    for &(db, secs) in segments {
        let amp = 10f64.powf(db / 20.0);
        let total = (secs * sr as f64).round() as usize;
        let mut done = 0;
        while done < total {
            let len = 1024.min(total - done);
            for i in 0..len {
                let v = (amp * (TAU * freq * (n + i) as f64 / sr as f64).sin()) as f32;
                l[i] = v;
                r[i] = v;
            }
            fx.process(&mut l[..len], &mut r[..len], None, &ctx);
            n += len;
            done += len;
        }
    }
    let mut m = [0.0; MAX_METERS];
    fx.meters(&mut m);
    m
}

#[test]
fn presets_use_real_params_in_range() {
    let info = info();
    for p in PRESETS {
        for &(id, v) in p.values {
            let spec = info.param(id).unwrap_or_else(|| panic!("{}: no parameter {id}", p.name));
            assert!(v >= spec.min && v <= spec.max, "{}: {} = {v} is out of range", p.name, spec.name);
        }
    }
    // Each preset's target is the one its name promises.
    for (name, want) in [("EBU", -23.0), ("Spotify", -14.0), ("Apple", -16.0), ("ATSC", -24.0), ("Podcast", -16.0), ("CD", -9.0)] {
        let p = PRESETS.iter().find(|p| p.name.starts_with(name)).unwrap();
        let mut fx = Loudness::new(48000.0);
        for &(id, v) in p.values {
            fx.set_param(id, v);
        }
        assert_eq!(fx.target_lufs(), want, "{}", p.name);
    }
}

fn near(v: f32, want: f64, tol: f64, what: &str) {
    assert!((v as f64 - want).abs() <= tol, "{what}: {v} (want {want} ±{tol})");
}

#[test]
fn k_weighting_matches_the_bs1770_table_at_48k() {
    let [shelf, hp] = k_weighting(48000.0);
    let want = [
        (shelf.b0, 1.535_124_859_586_97),
        (shelf.b1, -2.691_696_189_406_38),
        (shelf.b2, 1.198_392_810_852_85),
        (shelf.a1, -1.690_659_293_182_41),
        (shelf.a2, 0.732_480_774_215_85),
        (hp.b0, 1.0),
        (hp.b1, -2.0),
        (hp.b2, 1.0),
        (hp.a1, -1.990_047_454_833_98),
        (hp.a2, 0.990_072_250_366_21),
    ];
    for (i, (got, want)) in want.iter().enumerate() {
        assert!((got - want).abs() < 1e-8, "coefficient {i}: {got} vs {want}");
    }
}

#[test]
fn sine_at_minus_18_dbfs_reads_minus_18_lufs() {
    let m = play(48000.0, 1000.0, &[(-18.0, 5.0)]);
    near(m[M], -18.0, 0.1, "momentary");
    near(m[S], -18.0, 0.1, "short-term");
    near(m[I], -18.0, 0.1, "integrated");
    near(m[LRA], 0.0, 0.1, "range");
    near(m[PEAK_L], -18.0, 0.01, "sample peak");
}

#[test]
fn tech3341_cases_1_and_2() {
    for level in [-23.0, -33.0] {
        let m = play(48000.0, 1000.0, &[(level, 20.0)]);
        near(m[M], level, 0.1, "momentary");
        near(m[S], level, 0.1, "short-term");
        near(m[I], level, 0.1, "integrated");
    }
}

#[test]
fn tech3341_case_3_relative_gate() {
    let m = play(48000.0, 1000.0, &[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)]);
    near(m[I], -23.0, 0.1, "integrated");
}

#[test]
fn tech3341_case_4_absolute_and_relative_gates() {
    let m = play(48000.0, 1000.0, &[(-72.0, 10.0), (-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0), (-72.0, 10.0)]);
    near(m[I], -23.0, 0.1, "integrated");
}

#[test]
fn tech3341_case_5_power_average() {
    let m = play(48000.0, 1000.0, &[(-26.0, 20.0), (-20.0, 20.1), (-26.0, 20.0)]);
    near(m[I], -23.0, 0.1, "integrated");
}

#[test]
fn tech3342_loudness_range() {
    let cases: [(&[(f64, f64)], f64); 4] = [
        (&[(-20.0, 20.0), (-30.0, 20.0)], 10.0),
        (&[(-20.0, 20.0), (-15.0, 20.0)], 5.0),
        (&[(-40.0, 20.0), (-20.0, 20.0)], 20.0),
        (&[(-50.0, 20.0), (-35.0, 20.0), (-20.0, 20.0), (-35.0, 20.0), (-50.0, 20.0)], 15.0),
    ];
    for (i, (segments, want)) in cases.iter().enumerate() {
        let m = play(48000.0, 1000.0, segments);
        near(m[LRA], *want, 1.0, &format!("Tech 3342 case {}", i + 1));
    }
}

#[test]
fn true_peak_finds_the_intersample_peak() {
    // A sine at fs/4 sampled 45° off its peaks: every sample is at 0.707 of the true peak.
    let sr = 48000.0;
    let mut fx = Loudness::new(sr);
    let ctx = FxContext { sample_rate: sr, tempo: 120.0, playing: true, position: 0.0 };
    let amp = 0.5f64;
    for b in 0..20 {
        // A sine switched on abruptly really does overshoot between samples, so measure
        // after the onset.
        if b == 2 {
            fx.set_param(RESET, 1.0);
            fx.set_param(RESET, 0.0);
        }
        let mut l: Vec<f32> = (0..1024).map(|i| (amp * (PI / 2.0 * (b * 1024 + i) as f64 + PI / 4.0).sin()) as f32).collect();
        let mut r = l.clone();
        fx.process(&mut l, &mut r, None, &ctx);
    }
    let mut m = [0.0; MAX_METERS];
    fx.meters(&mut m);
    near(m[PEAK_L], 20.0 * (amp * 0.5f64.sqrt()).log10(), 0.01, "sample peak");
    near(m[TP], 20.0 * amp.log10(), 0.1, "true peak");
}

#[test]
fn true_peak_of_full_scale_sines() {
    // Off the fs/4 special case, across the band: the true peak is the sine's amplitude.
    for f in [1000.0, 5000.0, 12000.0, 17000.0] {
        let mut fx = Loudness::new(48000.0);
        // Skip the onset's overshoot (see above). 0.1 s is a whole number of cycles, so the
        // second call carries on in phase.
        play_on(&mut fx, 48000.0, f, &[(-6.0, 0.1)]);
        fx.set_param(RESET, 1.0);
        let m = play_on(&mut fx, 48000.0, f, &[(-6.0, 0.5)]);
        near(m[TP], -6.0, 0.1, &format!("true peak at {f} Hz"));
        near(m[PEAK_L], -6.0, 0.4, &format!("sample peak at {f} Hz"));
    }
}

#[test]
fn loudness_is_the_same_at_any_sample_rate() {
    for f in [100.0, 1000.0, 3000.0] {
        let at48 = play(48000.0, f, &[(-20.0, 4.0)])[I] as f64;
        for sr in [44100.0, 96000.0] {
            let other = play(sr, f, &[(-20.0, 4.0)]);
            near(other[I], at48, 0.05, &format!("{f} Hz at {sr} vs 48k"));
            near(other[M], at48, 0.05, &format!("{f} Hz momentary at {sr} vs 48k"));
        }
    }
}

#[test]
fn reset_and_auto_reset_start_a_new_measurement() {
    let mut fx = Loudness::new(48000.0);
    play_on(&mut fx, 48000.0, 1000.0, &[(-20.0, 5.0)]);
    // The panel pulses Reset; the measurement restarts and settles on the new level.
    fx.set_param(RESET, 1.0);
    fx.set_param(RESET, 0.0);
    let mut m = [0.0; MAX_METERS];
    fx.meters(&mut m);
    assert_eq!(m[I], FLOOR);
    assert_eq!(m[TP], FLOOR);
    let mut m = play_on(&mut fx, 48000.0, 1000.0, &[(-30.0, 5.0)]);
    near(m[I], -30.0, 0.1, "integrated after reset");

    // Stopping keeps the reading; the next play start clears it.
    let stopped = FxContext { sample_rate: 48000.0, tempo: 120.0, playing: false, position: 0.0 };
    let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
    fx.process(&mut l, &mut r, None, &stopped);
    fx.reset();
    fx.meters(&mut m);
    near(m[I], -30.0, 0.1, "integrated after stop");
    let m = play_on(&mut fx, 48000.0, 1000.0, &[(-14.0, 5.0)]);
    near(m[I], -14.0, 0.1, "integrated after auto-reset");
    near(m[LRA], 0.0, 0.1, "range after auto-reset");
}

#[test]
fn silence_reads_the_floor() {
    let m = play(48000.0, 1000.0, &[(-200.0, 4.0)]);
    assert_eq!(m[M], FLOOR);
    assert_eq!(m[I], FLOOR);
    assert_eq!(m[LRA], 0.0);
}

#[test]
fn passes_audio_bit_exact() {
    let mut fx = Loudness::new(44100.0);
    let ctx = FxContext { sample_rate: 44100.0, tempo: 120.0, playing: true, position: 0.0 };
    let mut seed = 99u32;
    let input: Vec<f32> = (0..8192)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f32::from_bits((seed >> 9) | 0x3f80_0000) - 1.5
        })
        .collect();
    let (mut l, mut r) = (input.clone(), input.iter().rev().copied().collect::<Vec<_>>());
    for (bl, br) in l.chunks_mut(1000).zip(r.chunks_mut(1000)) {
        fx.process(bl, br, None, &ctx);
    }
    assert!(l.iter().zip(&input).all(|(a, b)| a.to_bits() == b.to_bits()));
    assert!(r.iter().zip(input.iter().rev()).all(|(a, b)| a.to_bits() == b.to_bits()));
}
