//! Which thread played a strip, how much of its block it took and how many of its plug-ins slept
//! through it, for the Diagnostics window
//! (`session/engine/diagnostics.rs`). One per strip, shared with the session as its `Meter` is, and
//! made off the audio thread when the strip is built (`Session::build_track`).
//!
//! The thread that plays the strip, the audio callback or a processing thread, writes it with
//! relaxed stores and nothing else: no allocation, lock or system call (the time comes from the
//! strip's `spent`, which the block loop measures anyway). A strip is played by one thread at a
//! time, so a plain load and store keeps the peak; the session's read may lose one peak to a block
//! that lands between its read and its reset, which a few reads a second make up for.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// No thread has played the strip yet.
const NO_THREAD: u32 = u32::MAX;

/// The slot a strip's thread writes. See the module docs.
#[derive(Debug)]
pub struct StripLoad {
    /// 0 for the audio callback, n for `marbble-dsp-n`.
    thread: AtomicU32,
    /// The latest block's time over its audio's, as f32 bits.
    share: AtomicU32,
    /// The worst since the session last read it, as f32 bits.
    peak: AtomicU32,
    /// Inserts asleep in the latest block (see `idle/`).
    asleep: AtomicU32,
    /// Its source is rendered ahead (see `ahead/`).
    ahead: AtomicBool,
}

impl Default for StripLoad {
    fn default() -> Self {
        StripLoad {
            thread: AtomicU32::new(NO_THREAD),
            share: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            asleep: AtomicU32::new(0),
            ahead: AtomicBool::new(false),
        }
    }
}

/// What [`StripLoad::take`] read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StripLoadReading {
    /// The thread that played the latest block (0: the audio callback), or None before any.
    pub thread: Option<u32>,
    /// The latest block's time as a share of the block's audio (0.25: a quarter of the buffer).
    pub share: f32,
    /// The worst share since the last read.
    pub peak: f32,
    /// Its inserts asleep in the latest block.
    pub asleep: u32,
    /// Its source is rendered ahead.
    pub ahead: bool,
}

impl StripLoad {
    /// On the thread that played the strip: thread `thread` took `secs` over a block of `budget`
    /// seconds of audio.
    pub(crate) fn played(&self, thread: usize, secs: f32, budget: f32) {
        let share = if budget > 0.0 { secs / budget } else { 0.0 };
        self.thread.store(thread as u32, Ordering::Relaxed);
        self.share.store(share.to_bits(), Ordering::Relaxed);
        if share > f32::from_bits(self.peak.load(Ordering::Relaxed)) {
            self.peak.store(share.to_bits(), Ordering::Relaxed);
        }
    }

    /// On the thread that played the strip: `n` of its inserts slept through the block.
    pub(crate) fn asleep(&self, n: u32) {
        self.asleep.store(n, Ordering::Relaxed);
    }

    /// On the audio thread: the strip's source went ahead, or came back.
    pub(crate) fn ahead(&self, on: bool) {
        self.ahead.store(on, Ordering::Relaxed);
    }

    /// For the session: the latest block, and the worst since the last call, which starts afresh.
    pub fn take(&self) -> StripLoadReading {
        let thread = self.thread.load(Ordering::Relaxed);
        StripLoadReading {
            thread: (thread != NO_THREAD).then_some(thread),
            share: f32::from_bits(self.share.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.peak.swap(0, Ordering::Relaxed)),
            asleep: self.asleep.load(Ordering::Relaxed),
            ahead: self.ahead.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_thread_the_share_and_the_peak_since_the_last_read() {
        let s = StripLoad::default();
        assert_eq!(s.take(), StripLoadReading::default());
        s.played(2, 0.001, 0.004);
        s.played(0, 0.0005, 0.004);
        let r = s.take();
        assert_eq!(r.thread, Some(0));
        assert_eq!((r.share, r.peak), (0.125, 0.25));
        // The peak starts afresh; the latest block stays.
        assert_eq!(s.take().peak, 0.0);
        assert_eq!(s.take().share, 0.125);
        s.played(1, 0.001, 0.0);
        assert_eq!(s.take().share, 0.0, "no audio: no share");
    }
}
