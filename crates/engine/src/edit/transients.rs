//! Transient (onset) detection, for the Audio Track editor's transient markers, Split at
//! Transients, the sampler's slices and Drum Replacement.
//!
//! The signal is measured in short hops (about 1.3 ms) two ways: the peak level, and the peak of
//! its first difference, which is the top of the spectrum, where a beater's click is and a low
//! drum's ringing isn't. An onset is a hop that jumps, in either, above the loudest of the hops
//! just before it (the last 25 ms, a period of 40 Hz, so a ringing kick never stands out against
//! itself) by more than a threshold. The detector then waits for the level to settle, and for a
//! minimum gap, before it can fire again. Each onset is moved back to where its attack starts, at
//! a zero crossing, so a split there doesn't cut into the hit.

/// Length of an analysis hop in seconds (64 frames at 48 kHz).
const HOP_SECONDS: f64 = 1.0 / 750.0;
/// What an onset is measured against: the loudest hop in this long before it.
const RECENT_SECONDS: f64 = 0.025;
/// How long after its attack a hit's peak is looked for.
const PEAK_SECONDS: f64 = 0.020;
/// Level treated as silence.
const FLOOR: f32 = 1e-6;

fn db(v: f32) -> f32 {
    20.0 * v.max(FLOOR).log10()
}

/// An onset, with how loud it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Where its attack starts, in frames from the start of the audio.
    pub at: i64,
    /// Its loudest sample in the first [`PEAK_SECONDS`] (before the next hit).
    pub peak_at: i64,
    /// How far that peak rises above what was already sounding (linear, the loudest channel):
    /// its whole level after silence, less on top of a ringing drum.
    pub level: f32,
}

/// Detection settings derived from a 0..1 sensitivity.
struct Params {
    /// How far (dB) a hop must rise above the ones before it.
    rise_db: f32,
    /// Hops quieter than this (dB below the loudest) never count, so noise and reverb tails
    /// are ignored.
    gate_db: f32,
    /// Shortest distance between two onsets, in hops: the hold that keeps one hit from
    /// triggering twice.
    min_gap: usize,
}

impl Params {
    fn new(sensitivity: f32, rate: f64, hop: usize) -> Params {
        let s = sensitivity.clamp(0.0, 1.0);
        Params {
            // 20 dB at the lowest sensitivity down to 6 dB at the highest.
            rise_db: 20.0 - 14.0 * s,
            // 24 dB below the loudest hop down to 60 dB below.
            gate_db: 24.0 + 36.0 * s,
            // 80 ms down to 30 ms.
            min_gap: (((0.080 - 0.050 * s as f64) * rate) / hop as f64).round().max(1.0) as usize,
        }
    }
}

/// The audio being analysed: its channels, the same length.
struct Audio<'a> {
    ch: Vec<&'a [f32]>,
    frames: usize,
}

impl Audio<'_> {
    fn mono(&self, i: usize) -> f32 {
        self.ch.iter().map(|c| c[i]).sum()
    }

    fn level(&self, i: usize) -> f32 {
        self.ch.iter().fold(0.0f32, |m, c| m.max(c[i].abs()))
    }

    /// The first difference at `i`, the loudest channel's.
    fn edge(&self, i: usize) -> f32 {
        self.ch.iter().fold(0.0f32, |m, c| m.max((c[i] - c[i.saturating_sub(1)]).abs()))
    }
}

/// Onset positions (frames from the start of the audio, ascending) in `channels`. Higher
/// `sensitivity` (0..1) finds softer and closer hits.
pub fn detect<C: AsRef<[f32]>>(channels: &[C], sample_rate: u32, sensitivity: f32) -> Vec<i64> {
    hits(channels, sample_rate, sensitivity).into_iter().map(|h| h.at).collect()
}

/// The onsets [`detect`] finds, with their peaks.
pub fn hits<C: AsRef<[f32]>>(channels: &[C], sample_rate: u32, sensitivity: f32) -> Vec<Hit> {
    let frames = channels.iter().map(|c| c.as_ref().len()).min().unwrap_or(0);
    if frames == 0 {
        return Vec::new();
    }
    let a = Audio { ch: channels.iter().map(|c| &c.as_ref()[..frames]).collect(), frames };
    let rate = sample_rate.max(1) as f64;
    let hop = ((rate * HOP_SECONDS).round() as usize).max(8);
    let hops = frames.div_ceil(hop);

    // Peak level and peak edge per hop, across channels.
    let mut level = vec![0.0f32; hops];
    let mut edge = vec![0.0f32; hops];
    for k in 0..hops {
        for i in k * hop..((k + 1) * hop).min(frames) {
            level[k] = level[k].max(a.level(i));
            edge[k] = edge[k].max(a.edge(i));
        }
    }
    let loudest = level.iter().fold(0.0f32, |m, &v| m.max(v));
    let sharpest = edge.iter().fold(0.0f32, |m, &v| m.max(v));
    if loudest < 1e-5 {
        return Vec::new();
    }
    let p = Params::new(sensitivity, rate, hop);
    let (gate, edge_gate) = (db(loudest) - p.gate_db, db(sharpest) - p.gate_db);
    let recent = ((RECENT_SECONDS * rate) / hop as f64).round().max(1.0) as usize;
    let before = |band: &[f32], k: usize| band[k.saturating_sub(recent)..k].iter().fold(FLOOR, |m, &v| m.max(v));

    let mut onsets: Vec<(i64, f32)> = Vec::new();
    let mut armed = true;
    let mut last: Option<usize> = None;
    #[allow(clippy::needless_range_loop)] // `k` is the hop number the whole loop works in.
    for k in 0..hops {
        let (bg, bg_edge) = (before(&level, k), before(&edge, k));
        let rise = db(level[k]) - db(bg);
        let edge_rise = db(edge[k]) - db(bg_edge);
        let loud = db(level[k]) >= gate;
        let jumps = rise >= p.rise_db || (edge_rise >= p.rise_db && db(edge[k]) >= edge_gate);
        if armed && loud && jumps && last.is_none_or(|l| k - l >= p.min_gap) {
            let at = attack_start(&a, k, hop, (bg, level[k]), (bg_edge, edge[k]));
            if onsets.last().is_none_or(|&(prev, _)| at > prev) {
                onsets.push((at, bg));
            }
            last = Some(k);
            armed = false;
        } else if rise < p.rise_db * 0.5 && edge_rise < p.rise_db * 0.5 {
            // Settled (or decaying): the next jump counts as a new hit.
            armed = true;
        }
    }

    // Each hit's peak, before the next one starts.
    let window = (PEAK_SECONDS * rate).round() as usize;
    (0..onsets.len())
        .map(|n| {
            let (at, bg) = onsets[n];
            let end = onsets.get(n + 1).map_or(frames, |&(next, _)| next as usize).min(at as usize + window).max(at as usize + 1).min(frames);
            let (peak_at, peak) = (at as usize..end).map(|i| (i, a.level(i))).fold((at as usize, 0.0f32), |m, x| if x.1 > m.1 { x } else { m });
            Hit { at, peak_at: peak_at as i64, level: (peak - bg).max(0.0) }
        })
        .collect()
}

/// Where the attack detected in hop `k` begins: the first sample, from the hop before, that
/// clearly stands out from what came before it, in level or in edge, moved back along its slope
/// to the zero crossing before it. `level` and `edge` are each (the loudest before, this hop's).
fn attack_start(a: &Audio, k: usize, hop: usize, level: (f32, f32), edge: (f32, f32)) -> i64 {
    let from = k.saturating_sub(1) * hop;
    let to = ((k + 1) * hop).min(a.frames);
    let by_level = (from..to).find(|&i| a.level(i) >= (level.0 * 1.25).max(level.1 * 0.1));
    let by_edge = (from..to).find(|&i| a.edge(i) >= (edge.0 * 2.0).max(edge.1 * 0.1));
    let Some(first) = by_level.into_iter().chain(by_edge).min() else { return (k * hop) as i64 };
    // Follow the attack's slope back towards zero (same sign, shrinking, still moving faster
    // than what came before), at most a hop.
    let sign = a.mono(first) >= 0.0;
    let mut i = first;
    let limit = first.saturating_sub(hop);
    while i > limit && (a.mono(i - 1) >= 0.0) == sign && a.mono(i - 1).abs() < a.mono(i).abs() && a.edge(i - 1) > edge.0 {
        i -= 1;
    }
    i as i64
}

#[cfg(test)]
mod tests {
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
}
