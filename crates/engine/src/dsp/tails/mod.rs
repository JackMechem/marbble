//! Plug-in tails around a render (#490): a render starts from silence and leaves none of its sound
//! behind for live playback.
//!
//! Built-in instruments and effects are simply reset. A VST3 can't be: `setProcessing` and
//! `setActive` clear nothing in many (LSP's Multi-Sampler plays its sample to the end whatever
//! the note-offs, a Dragonfly reverb keeps most of its tail), cycling `setActive` costs
//! milliseconds a plug-in, and some reload on it (LSP's sampler then loses samples now and then,
//! #37). So each one is fed silence, with its notes ended, until it has been quiet for a while,
//! or its output has stopped falling below −30 dB (a noise floor: tape hiss, an amp model's hum,
//! a limiter's dither). A plug-in that's quiet already costs a few blocks, and the whole
//! ring-out has a time limit. Renders hold the engine lock, so this runs off the audio thread.

use std::time::Duration;

use super::*;

/// Below about −120 dB a plug-in's output is silence, as for `idle/`.
const QUIET: f32 = 1e-6;
/// How long its output must stay quiet: its reported tail and latency, within these bounds. Most
/// plug-ins report no tail at all, whatever they ring for.
const HOLD_SECS: f64 = 0.1;
const HOLD_MAX_SECS: f64 = 2.0;
/// The output's peak is compared window by window, once the hold has passed.
const WINDOW_SECS: f64 = 0.25;
/// Below −30 dB, an output that falls less than this from window to window, twice running, is a
/// noise floor (CHOWTapeModel's Degrade hisses at −40 dB), which never goes quiet; a reverb with a
/// 25 s decay time falls by this much. Louder, it may be a sample playing at a steady level, which
/// plays on.
const FLOOR: f32 = 0.0316;
const FALLING: f32 = 0.933; // −0.6 dB
/// The most a plug-in is fed, for one that never goes quiet (a long sample, say).
const LIMIT_SECS: f64 = 10.0;
/// The longest a ring-out may take, all plug-ins together. A render has two: one before, one after.
const BUDGET: Duration = Duration::from_secs(1);

/// How a plug-in's ring-out ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Quiet,
    /// A steady noise floor.
    Floor,
    /// `LIMIT_SECS` of it.
    Limit,
    /// The ring-out's time ran out.
    OutOfTime,
}

impl Engine {
    /// Ends the notes of every VST3 instrument the render plays and feeds it and every VST3
    /// effect silence until it's quiet. Off and bypassed plug-ins are left alone, as are effects
    /// asleep (quiet already, see `idle/`).
    pub(super) fn ring_out_plugins(&mut self) {
        self.reclaim_all();
        // As in `process`, so a tail decays as it would while playing.
        let _ftz = super::denormals::NoDenormals::new();
        let t = TransportInfo { playing: false, recording: false, ..self.transport() };
        let secs = |s: f64| (s * self.sample_rate) as u64;
        let (hold, hold_max) = (secs(HOLD_SECS), secs(HOLD_MAX_SECS));
        let rule = Rule { window: secs(WINDOW_SECS), limit: secs(LIMIT_SECS) };
        let started = Instant::now();
        let out_of_time = || started.elapsed() >= BUDGET;
        // The plug-ins cut short, and how many more weren't played out at all.
        let (mut cut, mut skipped) = (Vec::new(), 0);
        let mut run = |p: &mut PluginProcessor, midi: &[MidiEvent], l: &mut [f32], r: &mut [f32]| {
            let n = p.max_block().min(l.len());
            if !cut.is_empty() {
                // Out of time: only the notes' end, so none hangs.
                if !midi.is_empty() {
                    l[..n].fill(0.0);
                    r[..n].fill(0.0);
                    p.process(&mut l[..n], &mut r[..n], midi, &t);
                }
                skipped += 1;
                return;
            }
            let hold = (p.ring_out() as u64).clamp(hold, hold_max);
            let end = ring_out(&rule, n, hold, &out_of_time, |first| {
                l[..n].fill(0.0);
                r[..n].fill(0.0);
                p.process(&mut l[..n], &mut r[..n], if first { midi } else { &[] }, &t);
                (0..).map_while(|bus| if bus == 0 { Some((&l[..n], &r[..n])) } else { p.output_bus(bus, n) }).fold(0.0, |m, (a, b)| peak(a).max(peak(b)).max(m))
            });
            if end.1 == End::OutOfTime {
                cut.push(p.name().to_string());
            }
        };
        let (mut l, mut r) = (vec![0.0f32; MAX_BLOCK], vec![0.0f32; MAX_BLOCK]);
        let iso = self.isolate.as_deref();
        for track in &mut self.tracks {
            let outside = iso.is_some_and(|i| i.role(track.id) == Role::Skip);
            if outside || track.params.off || track.dormant || track.left_out {
                continue;
            }
            if awake_vst(&mut track.instrument, true).is_some() {
                // The note-offs (and sustain pedal up, pitch bend centred) go in the first block.
                track.midi.clear();
                end_sounding_notes(track);
                let notes = std::mem::take(&mut track.midi);
                if let Some(p) = awake_vst(&mut track.instrument, true) {
                    run(p, &notes, &mut l, &mut r);
                }
                track.midi = notes;
                track.midi.clear();
            }
            for slot in &mut track.inserts {
                if let Some(p) = awake_vst(slot, false) {
                    run(p, &[], &mut l, &mut r);
                }
            }
        }
        // Stereo Out's inserts play only in a render of the whole mix.
        if iso.is_none() {
            for slot in &mut self.master_inserts {
                if let Some(p) = awake_vst(slot, false) {
                    run(p, &[], &mut l, &mut r);
                }
            }
        }
        if !cut.is_empty() {
            log::warn!("a render's plug-in tails took over {BUDGET:?}: {} still sounding, {skipped} more not played out", cut.join(", "));
        }
    }
}

/// The slot's VST3, if it runs: not bypassed, and (unless `instrument`, which never sleeps) not
/// asleep.
fn awake_vst(slot: &mut Option<InsertDsp>, instrument: bool) -> Option<&mut PluginProcessor> {
    match slot {
        Some(ins) if !ins.bypass && (instrument || !ins.asleep()) => match &mut ins.plugin {
            Plugin::Vst(p) => Some(p),
            _ => None,
        },
        _ => None,
    }
}

/// The largest sample of `a`, with a NaN as infinitely loud.
fn peak(a: &[f32]) -> f32 {
    a.iter().fold(0.0, |m: f32, s| if s.is_nan() { f32::INFINITY } else { m.max(s.abs()) })
}

/// When a ring-out stops, in frames: see `ring_out`.
struct Rule {
    window: u64,
    limit: u64,
}

/// Runs `block`, which plays `n` frames of silence (with its notes ended when told it's the
/// first) and returns its output's peak, until
/// - nothing has sounded for `hold` frames,
/// - or, once `hold` frames have run, the peak over two windows running has stayed below `FLOOR`
///   and not fallen from the window before (a noise floor),
/// - or `rule.limit` frames have run, or `out_of_time`.
///
/// Returns the frames it ran, and why it stopped.
fn ring_out(rule: &Rule, n: usize, hold: u64, out_of_time: &dyn Fn() -> bool, mut block: impl FnMut(bool) -> f32) -> (u64, End) {
    let (mut ran, mut quiet) = (0u64, 0u64);
    // The peak of the window being played and of the one before, where the next one starts, and
    // how many windows running have been flat.
    let (mut now, mut before, mut next, mut flat) = (0.0f32, None::<f32>, rule.window, 0);
    loop {
        let p = block(ran == 0);
        ran += n as u64;
        quiet = if p < QUIET { quiet + n as u64 } else { 0 };
        now = now.max(p);
        if quiet >= hold {
            return (ran, End::Quiet);
        }
        if ran >= next {
            flat = if now < FLOOR && before.is_some_and(|b| now > b * FALLING) { flat + 1 } else { 0 };
            if ran >= hold && flat >= 2 {
                return (ran, End::Floor);
            }
            (before, now, next) = (Some(now), 0.0, next + rule.window);
        }
        if ran >= rule.limit {
            return (ran, End::Limit);
        }
        if out_of_time() {
            return (ran, End::OutOfTime);
        }
    }
}

#[cfg(test)]
mod tests;
