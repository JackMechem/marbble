//! Transient detection on drum patterns, clicks, tones and ringing kicks.

use super::*;

const RATE: u32 = 48000;

/// Deterministic low-level noise, so tests don't depend on a random source.
fn noise(n: usize, level: f32) -> Vec<f32> {
    let mut x: u32 = 0x1234_5678;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * level
        })
        .collect()
}

/// Adds a decaying sine burst (a drum-like hit) at `at`.
fn hit(buf: &mut [f32], at: usize, amp: f32, freq: f32) {
    for i in 0..(RATE as usize / 4).min(buf.len() - at) {
        let t = i as f32 / RATE as f32;
        buf[at + i] += amp * (-t / 0.05).exp() * (std::f32::consts::TAU * freq * t).sin();
    }
}

/// Adds a short click: a few samples of a fast-decaying impulse.
fn click(buf: &mut [f32], at: usize, amp: f32) {
    for (i, g) in [1.0, -0.7, 0.45, -0.25, 0.1].iter().enumerate() {
        buf[at + i] += amp * g;
    }
}

/// Adds a kick drum at `at`: a beater's click, then a body sweeping down from `low + 110` Hz
/// to `low` that rises over `attack` seconds and rings with a time constant of `ring`.
fn kick(buf: &mut [f32], at: usize, amp: f32, low: f32, ring: f32, attack: f32) {
    let click = noise(72, 0.5);
    let mut phase = 0.0f32;
    for i in 0..(buf.len() - at) {
        let t = i as f32 / RATE as f32;
        phase += std::f32::consts::TAU * (low + 110.0 * (-t / 0.02).exp()) / RATE as f32;
        let body = (1.0 - (-t / attack.max(1e-6)).exp()) * (-t / ring).exp() * phase.sin();
        let tick = click.get(i).map_or(0.0, |v| v * (1.0 - i as f32 / 72.0));
        buf[at + i] += amp * (body + tick);
    }
}

/// Kicks on 1, the and of 2, 3, the sixteenth after it, the and of 4, and the next bar's 1
/// and 2 at 120 bpm, at uneven levels: where they start and how loud.
fn kick_pattern() -> (Vec<usize>, Vec<f32>) {
    let beat = RATE as usize / 2;
    let at = vec![2400, 2400 + beat * 3 / 2, 2400 + beat * 2, 2400 + beat * 9 / 4, 2400 + beat * 7 / 2, 2400 + beat * 4, 2400 + beat * 5];
    (at, vec![0.9, 0.5, 0.8, 0.35, 0.7, 0.2, 0.95])
}

fn near(found: &[i64], expected: &[usize], before_ms: f64, after_ms: f64) -> bool {
    let ms = |v: f64| (v * RATE as f64 / 1000.0) as i64;
    found.len() == expected.len() && found.iter().zip(expected).all(|(&f, &e)| f >= e as i64 - ms(before_ms) && f <= e as i64 + ms(after_ms))
}

#[test]
fn finds_each_hit_of_a_drum_pattern() {
    let mut buf = noise(RATE as usize * 4, 0.001);
    // Eighth notes at 120 bpm with uneven levels and two pitches (kick and snare-ish).
    let at: Vec<usize> = (0..14).map(|i| 6000 + i * 12000).collect();
    for (i, &a) in at.iter().enumerate() {
        hit(&mut buf, a, [0.9, 0.4, 0.7, 0.3][i % 4], if i % 2 == 0 { 70.0 } else { 220.0 });
    }
    let found = detect(&[buf], RATE, 0.5);
    assert!(near(&found, &at, 1.0, 1.0), "{found:?} vs {at:?}");
}

#[test]
fn finds_clicks_to_the_sample_in_stereo() {
    let n = RATE as usize * 2;
    let (mut l, mut r) = (noise(n, 0.0005), noise(n, 0.0005));
    let at: Vec<usize> = (0..8).map(|i| 1000 + i * 11_025).collect();
    for &a in &at {
        click(&mut l, a, 0.8);
        click(&mut r, a, 0.6);
    }
    let found = detect(&[l, r], RATE, 0.5);
    assert!(near(&found, &at, 0.1, 0.05), "{found:?} vs {at:?}");
}

#[test]
fn sustained_tone_is_one_onset() {
    // Silence, then a tone with gentle tremolo: only its start is a transient.
    let mut buf = vec![0.0f32; RATE as usize * 3];
    let start = 4800;
    for (i, v) in buf.iter_mut().enumerate().skip(start) {
        let t = (i - start) as f32 / RATE as f32;
        let trem = 1.0 + 0.15 * (std::f32::consts::TAU * 5.0 * t).sin();
        *v = 0.5 * trem * (std::f32::consts::TAU * 440.0 * t).sin();
    }
    let found = detect(&[buf], RATE, 0.8);
    assert!(near(&found, &[start], 1.0, 1.0), "{found:?}");
}

#[test]
fn sensitivity_decides_about_soft_hits() {
    let mut buf = noise(RATE as usize * 2, 0.00002);
    hit(&mut buf, 4800, 0.9, 120.0);
    // 36 dB down: below the gate at low sensitivity, found at high.
    hit(&mut buf, 48000, 0.9 * 0.016, 120.0);
    hit(&mut buf, 72000, 0.9, 120.0);
    assert_eq!(detect(std::slice::from_ref(&buf), RATE, 0.0).len(), 2);
    let all = detect(&[buf], RATE, 1.0);
    assert!(near(&all, &[4800, 48000, 72000], 1.0, 1.0), "{all:?}");
}

#[test]
fn silence_and_empty_audio_have_none() {
    assert!(detect(&[vec![0.0; 48000]], RATE, 1.0).is_empty());
    assert!(detect::<Vec<f32>>(&[], RATE, 0.5).is_empty());
    assert!(detect(&[noise(48000, 1e-6)], RATE, 1.0).is_empty());
}

#[test]
fn ringing_kicks_trigger_once_each_at_their_attack() {
    // (lowest pitch, ring, attack): tight, boomy with a soft beater, short, and very long.
    for (low, ring, attack) in [(50.0, 0.35, 0.0), (45.0, 0.6, 0.002), (60.0, 0.2, 0.0005), (40.0, 0.8, 0.004)] {
        let mut buf = noise(RATE as usize * 6, 0.0005);
        let (at, amps) = kick_pattern();
        for (&a, &m) in at.iter().zip(&amps) {
            kick(&mut buf, a, m, low, ring, attack);
        }
        for sensitivity in [0.5, 1.0] {
            let found = detect(std::slice::from_ref(&buf), RATE, sensitivity);
            assert!(near(&found, &at, 0.2, 0.5), "{low} Hz, {ring} s, {sensitivity}: {found:?} vs {at:?}");
        }
    }
}

#[test]
fn a_hits_level_is_what_it_adds() {
    let mut buf = noise(RATE as usize * 6, 0.0005);
    let (at, amps) = kick_pattern();
    for (&a, &m) in at.iter().zip(&amps) {
        kick(&mut buf, a, m, 60.0, 0.2, 0.0005);
    }
    let found = hits(&[buf], RATE, 1.0);
    assert_eq!(found.len(), at.len());
    // After silence, its own peak; and louder hits read louder, on a tail or not.
    assert!((found[0].level - 0.9).abs() < 0.05 && (found[6].level - 0.95).abs() < 0.05, "{found:?}");
    let mut by_amp: Vec<usize> = (0..at.len()).collect();
    by_amp.sort_by(|&a, &b| amps[a].total_cmp(&amps[b]));
    assert!(by_amp.windows(2).all(|w| found[w[0]].level < found[w[1]].level), "{found:?}");
    // The peak is in the body, a few milliseconds in.
    assert!(found.iter().zip(&at).all(|(h, &a)| h.peak_at > a as i64 && h.peak_at < a as i64 + RATE as i64 / 50), "{found:?}");
}

#[test]
fn bleed_on_a_ringing_kick_reads_quiet() {
    // A hi-hat's bleed (bright noise at -26 dB) 100 ms into a ringing kick: the click may
    // count as an onset, but it adds little to the level.
    let mut buf = noise(RATE as usize * 2, 0.0005);
    kick(&mut buf, 4800, 0.9, 50.0, 0.35, 0.0);
    let hat = noise(1440, 0.05);
    for (i, w) in hat.windows(2).enumerate() {
        buf[9600 + i] += (w[1] - w[0]) * (1.0 - i as f32 / 1440.0);
    }
    let found = hits(&[buf], RATE, 1.0);
    assert!(found[0].at.abs_diff(4800) < 10 && found[0].level > 0.85, "{found:?}");
    assert!(found[1..].iter().all(|h| h.level < 0.1), "{found:?}");
}
