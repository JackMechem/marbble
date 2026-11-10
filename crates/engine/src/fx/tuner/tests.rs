//! The Tuner's pitch detection, note names and pass-through.

use super::*;
use std::f64::consts::PI;

#[test]
fn presets_use_real_params_in_range() {
    let info = info();
    for p in PRESETS {
        for &(id, v) in p.values {
            let spec = info.param(id).unwrap_or_else(|| panic!("{}: no parameter {id}", p.name));
            assert!(v >= spec.min && v <= spec.max, "{}: {} = {v} is out of range", p.name, spec.name);
        }
    }
}

const SR: f32 = 48000.0;

fn ctx(sr: f32) -> FxContext {
    FxContext { sample_rate: sr, tempo: 120.0, playing: true, position: 0.0 }
}

fn midi_hz(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

/// Feeds `secs` of a generated mono signal through the tuner and returns the meters after
/// every block.
fn run(t: &mut Tuner, sr: f32, secs: f64, mut gen: impl FnMut(usize) -> f32) -> Vec<[f32; MAX_METERS]> {
    let total = (secs * sr as f64) as usize;
    let mut out = Vec::new();
    let mut n = 0;
    let block = 512;
    while n < total {
        let len = block.min(total - n);
        let mut l: Vec<f32> = (n..n + len).map(&mut gen).collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r, None, &ctx(sr));
        let mut m = [0.0; MAX_METERS];
        t.meters(&mut m);
        out.push(m);
        n += len;
    }
    out
}

fn sine(f: f64, sr: f32, amp: f32) -> impl FnMut(usize) -> f32 {
    move |n| amp * (TAU * f * n as f64 / sr as f64).sin() as f32
}

/// A band-limited sawtooth, summed from its harmonics up to Nyquist.
fn saw(f: f64, sr: f32, amp: f32) -> impl FnMut(usize) -> f32 {
    let harmonics = ((sr as f64 * 0.5) / f) as usize;
    move |n| {
        let ph = TAU * f * n as f64 / sr as f64;
        let s: f64 = (1..=harmonics).map(|k| (k as f64 * ph).sin() / k as f64).sum();
        amp * (s * 2.0 / PI) as f32
    }
}

fn cents_between(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

/// Checks frequency, note and cents over the sustained part (after 0.5 s).
fn assert_steady(meters: &[[f32; MAX_METERS]], sr: f32, f: f64, note: i32, label: &str) {
    let start = (0.5 * sr as f64 / 512.0) as usize;
    for m in &meters[start..] {
        let err = cents_between(m[0] as f64, f);
        assert!(err.abs() <= 1.0, "{label}: {} Hz is {err:.2} cents off {f:.3} Hz", m[0]);
        assert_eq!(m[1] as i32, note, "{label}: wrong note at {} Hz", m[0]);
        assert!(m[2].abs() <= 1.0, "{label}: cents meter {}", m[2]);
        assert!(m[3] > 0.9, "{label}: confidence {}", m[3]);
    }
}

// E1, A1, B0 (5-string low B), E2, A4, C6, C7.
const NOTES: [i32; 7] = [28, 33, 23, 40, 69, 84, 96];

#[test]
fn sines_are_within_a_cent() {
    for sr in [44100.0, SR, 96000.0] {
        for &note in &NOTES {
            let f = midi_hz(note as f64);
            let mut t = Tuner::new(sr);
            let m = run(&mut t, sr, 1.5, sine(f, sr, 0.5));
            assert_steady(&m, sr, f, note, &format!("sine {f:.2} Hz at {sr}"));
        }
    }
}

/// An input with a DC offset and nothing else once kept the detector busy on the DC
/// blocker's decaying, denormal output, slowly enough to take the audio thread down.
#[test]
fn a_dc_offset_is_silence() {
    let mut t = Tuner::new(SR);
    t.set_param(THRESHOLD, -90.0);
    let m = run(&mut t, SR, 2.0, |_| 0.3);
    assert!(m.iter().all(|m| m[1] == -1.0), "no note from DC");
    // The level smooths over 0.1 s, so the blocker's settling takes a while to leave it.
    assert!(m.last().unwrap()[4] < -90.0, "no level from DC: {:?}", m.last());
}

/// A plug-in blowing up before the tuner once overflowed the NSDF sums into NaN, and the
/// peak search stopped advancing: the audio thread hung until the kernel killed it.
#[test]
fn a_runaway_input_neither_hangs_nor_sticks() {
    let f = midi_hz(45.0);
    let mut t = Tuner::new(SR);
    let mut tone = sine(f, SR, 0.5);
    let half = (0.5 * SR) as usize;
    run(&mut t, SR, 1.0, |n| if n < half { tone(n) } else { 1e30 * tone(n) });
    t.reset();
    let m = run(&mut t, SR, 1.5, sine(f, SR, 0.5));
    assert_steady(&m, SR, f, 45, "after a runaway input");
}

#[test]
fn sawtooths_are_within_a_cent_and_octave_correct() {
    for &note in &NOTES {
        let f = midi_hz(note as f64);
        let mut t = Tuner::new(SR);
        let m = run(&mut t, SR, 1.5, saw(f, SR, 0.5));
        assert_steady(&m, SR, f, note, &format!("saw {f:.2} Hz"));
    }
}

#[test]
fn strong_second_harmonic_keeps_the_octave() {
    // A plucked bass string: the fundamental weaker than the octave above it.
    for &note in &[23, 28, 33, 40] {
        let f = midi_hz(note as f64);
        let amps = [0.4, 1.0, 0.5, 0.3, 0.2, 0.1];
        let gen = move |n: usize| {
            let ph = TAU * f * n as f64 / SR as f64;
            let s: f64 = amps.iter().enumerate().map(|(k, a)| a * ((k + 1) as f64 * ph + k as f64).sin()).sum();
            (0.25 * s) as f32
        };
        let mut t = Tuner::new(SR);
        let m = run(&mut t, SR, 1.5, gen);
        assert_steady(&m, SR, f, note, &format!("bass {f:.2} Hz"));
    }
}

#[test]
fn detuned_tone_reads_its_cents() {
    let f = 440.0 * 2f64.powf(13.0 / 1200.0);
    let mut t = Tuner::new(SR);
    let m = run(&mut t, SR, 1.0, sine(f, SR, 0.3));
    let last = m.last().unwrap();
    assert_eq!(last[1] as i32, 69);
    assert!((last[2] - 13.0).abs() < 1.0, "cents {}", last[2]);
}

#[test]
fn silence_and_noise_read_nothing() {
    let mut t = Tuner::new(SR);
    for m in run(&mut t, SR, 1.0, |_| 0.0) {
        assert_eq!(m[0], 0.0);
        assert_eq!(m[1], -1.0);
    }
    // White noise from a fixed LCG, loud enough to pass the level gate.
    let mut seed = 0x1234_5678u32;
    let noise = move |_| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
    };
    let mut t = Tuner::new(SR);
    for m in run(&mut t, SR, 2.0, noise) {
        assert_eq!(m[0], 0.0, "noise detected as {} Hz", m[0]);
    }
    // A quiet tone below the threshold is gated too.
    let mut t = Tuner::new(SR);
    let m = run(&mut t, SR, 1.0, sine(440.0, SR, 0.0005));
    assert_eq!(m.last().unwrap()[0], 0.0);
}

#[test]
fn note_drops_after_the_tone_stops() {
    let mut t = Tuner::new(SR);
    let f = 110.0;
    let m = run(&mut t, SR, 2.0, |n| if n < 48000 { (0.5 * (TAU * f * n as f64 / 48000.0).sin()) as f32 } else { 0.0 });
    assert!((m[80][0] as f64 - f).abs() < 0.1);
    assert_eq!(m.last().unwrap()[0], 0.0);
}

#[test]
fn reference_changes_the_cents() {
    let mut t = Tuner::new(SR);
    t.set_param(REFERENCE, 442.0);
    let m = run(&mut t, SR, 1.0, sine(440.0, SR, 0.5));
    let last = m.last().unwrap();
    let expect = cents_between(440.0, 442.0) as f32;
    assert!((last[0] - 440.0).abs() < 0.05, "frequency doesn't depend on the reference");
    assert_eq!(last[1] as i32, 69);
    assert!((last[2] - expect).abs() < 0.5, "cents {} vs {expect}", last[2]);
    // Moving the reference updates the reading straight away.
    t.set_param(REFERENCE, 430.0);
    let mut out = [0.0; MAX_METERS];
    t.meters(&mut out);
    assert!((out[2] - cents_between(440.0, 430.0) as f32).abs() < 0.5, "cents {}", out[2]);
}

#[test]
fn transpose_shifts_the_note_name() {
    let mut t = Tuner::new(SR);
    t.set_param(TRANSPOSE, 2.0);
    let m = run(&mut t, SR, 1.0, sine(midi_hz(58.0), SR, 0.5));
    assert_eq!(m.last().unwrap()[1] as i32, 60, "a B-flat instrument's written C");
}

#[test]
fn note_has_hysteresis() {
    let mut t = Tuner::new(SR);
    // A4 + 45 cents, then + 55 cents: still A4 (with the needle pinned), then + 65: B-flat.
    run(&mut t, SR, 0.6, sine(midi_hz(69.45), SR, 0.5));
    let m = run(&mut t, SR, 0.6, sine(midi_hz(69.55), SR, 0.5));
    assert_eq!(m.last().unwrap()[1] as i32, 69);
    let m = run(&mut t, SR, 0.6, sine(midi_hz(69.65), SR, 0.5));
    let last = m.last().unwrap();
    assert_eq!(last[1] as i32, 70);
    assert!((last[2] + 35.0).abs() < 1.0, "cents {}", last[2]);
}

#[test]
fn passes_audio_bit_exact_and_mutes() {
    let mut t = Tuner::new(SR);
    let mut seed = 7u32;
    let input: Vec<f32> = (0..4096)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f32::from_bits((seed >> 9) | 0x3f00_0000) - 0.75
        })
        .collect();
    let (mut l, mut r) = (input.clone(), input.iter().map(|x| -x).collect::<Vec<_>>());
    for (bl, br) in l.chunks_mut(1024).zip(r.chunks_mut(1024)) {
        t.process(bl, br, None, &ctx(SR));
    }
    assert!(l.iter().zip(&input).all(|(a, b)| a.to_bits() == b.to_bits()));
    assert!(r.iter().zip(&input).all(|(a, b)| a.to_bits() == (-b).to_bits()));

    t.set_param(MUTE, 1.0);
    let (mut l, mut r) = (input[..1024].to_vec(), input[..1024].to_vec());
    t.process(&mut l, &mut r, None, &ctx(SR));
    assert!(l.iter().chain(&r).all(|&x| x == 0.0));
    let mut m = [0.0; MAX_METERS];
    t.meters(&mut m);
    assert!(m[4] > -40.0, "the level meter still reads the input while muted");
}
