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
mod tests;
