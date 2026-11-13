//! Plug-in tails around a render (#490): a render starts from silence and leaves none of its sound
//! behind for live playback.
//!
//! Built-in instruments and effects are simply reset. A VST3 can't be: `setProcessing` and
//! `setActive` clear nothing in many (LSP's Multi-Sampler plays its sample to the end whatever
//! the note-offs, a Dragonfly reverb keeps most of its tail), cycling `setActive` costs
//! milliseconds a plug-in, and some reload on it (LSP's sampler then loses samples now and then,
//! #37). So each one is fed silence, with its notes ended, until it has been quiet for a while.
//! A plug-in that's quiet already costs a few blocks. Renders hold the engine lock, so this runs
//! off the audio thread.

use super::*;

/// Below about −120 dB a plug-in's output is silence, as for `idle/`.
const QUIET: f32 = 1e-6;
/// How long its output must stay quiet: its reported tail and latency, within these bounds. Most
/// plug-ins report no tail at all, whatever they ring for.
const HOLD_SECS: f64 = 0.1;
const HOLD_MAX_SECS: f64 = 2.0;
/// The most it is fed, for one that never goes quiet (it hisses, say, or plays a long sample).
const LIMIT_SECS: f64 = 10.0;

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
        let (hold, hold_max, limit) = (secs(HOLD_SECS), secs(HOLD_MAX_SECS), secs(LIMIT_SECS));
        let run = |p: &mut PluginProcessor, midi: &[MidiEvent], l: &mut [f32], r: &mut [f32]| {
            let n = p.max_block().min(l.len());
            let hold = (p.ring_out() as u64).clamp(hold, hold_max);
            ring_out(n, hold, limit, |first| {
                l[..n].fill(0.0);
                r[..n].fill(0.0);
                p.process(&mut l[..n], &mut r[..n], if first { midi } else { &[] }, &t);
                (0..).map_while(|bus| if bus == 0 { Some((&l[..n], &r[..n])) } else { p.output_bus(bus, n) }).any(|(a, b)| loud(a) || loud(b))
            });
        };
        let (mut l, mut r) = (vec![0.0f32; MAX_BLOCK], vec![0.0f32; MAX_BLOCK]);
        let iso = self.isolate.as_deref();
        for track in &mut self.tracks {
            let skipped = iso.is_some_and(|i| i.role(track.id) == Role::Skip);
            if skipped || track.params.off || track.dormant || track.left_out {
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

/// Some sample of `a` isn't silence (a NaN counts as sound).
#[allow(clippy::neg_cmp_op_on_partial_ord)] // The test must also catch NaN, so it can't be `>=`.
fn loud(a: &[f32]) -> bool {
    a.iter().any(|s| !(s.abs() < QUIET))
}

/// Runs `block`, which plays `n` frames of silence (with its notes ended when told it's the
/// first) and says whether anything sounded, until nothing has for `hold` frames, or for `limit`
/// frames in all. Returns the frames it ran.
fn ring_out(n: usize, hold: u64, limit: u64, mut block: impl FnMut(bool) -> bool) -> u64 {
    let (mut ran, mut quiet) = (0u64, 0u64);
    while quiet < hold && ran < limit {
        quiet = if block(ran == 0) { 0 } else { quiet + n as u64 };
        ran += n as u64;
    }
    ran
}

#[cfg(test)]
mod tests;
