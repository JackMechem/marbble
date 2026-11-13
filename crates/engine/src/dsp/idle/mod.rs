//! Plug-ins fed silence sleep (#442), so a stopped or sparse project costs next to nothing: a
//! plug-in fed silence costs as much as one fed audio, and an amp model or a reverb costs a lot.
//!
//! An insert falls asleep once its input and side-chain key have been silent, and its own output
//! quiet, for as long as it may ring on (its tail and latency) and at least `HOLD_SECS`. Asleep, it
//! isn't called and puts out silence. It wakes for a block with signal at its input or key, or with
//! edits, automation or a fade (its buses being switched) waiting, and runs that block. A plug-in that makes sound of its own, such as
//! hiss, never has quiet output, so it never sleeps; nor does one with a tail that never ends.
//!
//! Instrument slots stay awake: a plug-in instrument may play by itself as the transport runs.
//! Sleeping is decided by the signal alone, so a render sleeps and wakes where playback does.

use super::*;

/// Below about −160 dB the input carries nothing.
const SILENT: f32 = 1e-8;
/// Below about −120 dB an output may be replaced by silence.
const QUIET: f32 = 1e-6;
/// The least time a plug-in must have been quiet before it sleeps, for one whose tail or latency
/// is longer than it says.
const HOLD_SECS: f64 = 2.0;

/// How long an insert has been quiet, and whether it sleeps.
#[derive(Debug, Clone, Copy, Default)]
pub struct Idle {
    /// Frames since its input last had signal or its output was last above `QUIET`.
    quiet: u64,
    asleep: bool,
}

/// Every sample of the block below `limit` (a NaN isn't).
fn below(limit: f32, l: &[f32], r: &[f32]) -> bool {
    l.iter().chain(r).all(|s| s.abs() < limit)
}

/// A plug-in's input `l`, `r` and its key carry nothing.
pub(super) fn silent(l: &[f32], r: &[f32], key: crate::fx::Key) -> bool {
    below(SILENT, l, r) && key.is_none_or(|(a, b)| below(SILENT, a, b))
}

impl InsertDsp {
    /// Whether it sleeps through a block whose input (and key) is `silent`.
    pub(super) fn sleeps(&self, silent: bool) -> bool {
        self.idle.asleep && silent && !self.plugin.wants_to_run()
    }

    /// After it ran on a block, whose input was `silent` and whose output is `l`, `r`: counts
    /// toward sleeping.
    pub(super) fn count_quiet(&mut self, silent: bool, l: &[f32], r: &[f32], sr: f64) {
        if silent && below(QUIET, l, r) {
            self.idle.quiet += l.len() as u64;
            let hold = self.plugin.ring_out(sr).map(|f| f.max((HOLD_SECS * sr) as u64));
            self.idle.asleep = hold.is_some_and(|h| self.idle.quiet >= h);
        } else {
            self.idle = Idle::default();
        }
    }

    /// Whether it sleeps now: its input was silent and its output quiet for as long as it rings.
    pub(super) fn asleep(&self) -> bool {
        self.idle.asleep
    }

    /// Awake, with nothing counted: after a reset, or with the track switched off.
    pub(super) fn wake_up(&mut self) {
        self.idle = Idle::default();
    }
}

impl Plugin {
    /// Edits, automation or a fade wait for its next block.
    fn wants_to_run(&self) -> bool {
        matches!(self, Plugin::Vst(p) if p.wants_to_run())
    }

    /// How long its output may go on after its input stops, in frames, or None if it may never
    /// stop (it doesn't sleep).
    fn ring_out(&self, sr: f64) -> Option<u64> {
        match self {
            Plugin::Vst(p) => (p.ring_out() != u32::MAX).then(|| p.ring_out() as u64),
            Plugin::Builtin(b) => Some(b.ring_out(sr)),
            Plugin::Instrument(_) => None,
        }
    }
}

#[cfg(test)]
mod tests;
