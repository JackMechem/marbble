//! The keys held on the live MIDI input, for the control bar's chord display. The input callbacks
//! (midir's threads) and the on-screen keyboards mark notes here as they pass; each tick reads them.
//! Nothing here runs on the audio thread.

use std::sync::atomic::{AtomicU64, Ordering};

/// One bit per note number. Notes are kept across channels: a key is down while any channel holds
/// it, and its note-off on any channel lets it up.
#[derive(Default)]
pub struct HeldNotes {
    down: [AtomicU64; 2],
    /// Struck since the last [`HeldNotes::take`], so a key tapped between two ticks still shows.
    struck: [AtomicU64; 2],
}

impl HeldNotes {
    /// Follows one message: note-on, note-off, and All Sound Off or All Notes Off.
    pub fn note(&self, d: [u8; 3]) {
        let (word, bit) = ((d[1] as usize >> 6) & 1, 1u64 << (d[1] & 63));
        match d[0] & 0xF0 {
            0x90 if d[2] > 0 => {
                self.down[word].fetch_or(bit, Ordering::Relaxed);
                self.struck[word].fetch_or(bit, Ordering::Relaxed);
            }
            0x80 | 0x90 => {
                self.down[word].fetch_and(!bit, Ordering::Relaxed);
            }
            0xB0 if d[1] == 120 || d[1] == 123 => {
                for w in &self.down {
                    w.store(0, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    /// The notes down now or struck since the last call, lowest first.
    pub fn take(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (w, (down, struck)) in self.down.iter().zip(&self.struck).enumerate() {
            let bits = down.load(Ordering::Relaxed) | struck.swap(0, Ordering::Relaxed);
            out.extend((0..64).filter(|b| bits >> b & 1 == 1).map(|b| (w * 64 + b) as u8));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_shows_while_held_and_goes_when_released() {
        let h = HeldNotes::default();
        for n in [64, 60, 67] {
            h.note([0x90, n, 100]);
        }
        assert_eq!(h.take(), vec![60, 64, 67]);
        h.note([0x80, 64, 0]);
        // Note-on with velocity 0 is a note-off.
        h.note([0x90, 60, 0]);
        assert_eq!(h.take(), vec![67]);
        h.note([0x80, 67, 0]);
        assert_eq!(h.take(), Vec::<u8>::new());
    }

    #[test]
    fn a_key_tapped_between_two_reads_shows_once() {
        let h = HeldNotes::default();
        h.note([0x91, 127, 100]);
        h.note([0x81, 127, 0]);
        h.note([0x90, 0, 100]);
        h.note([0x80, 0, 0]);
        assert_eq!(h.take(), vec![0, 127]);
        assert_eq!(h.take(), Vec::<u8>::new());
    }

    #[test]
    fn all_notes_off_lets_every_key_up() {
        let h = HeldNotes::default();
        h.note([0x90, 60, 100]);
        h.note([0x92, 100, 100]);
        h.take();
        h.note([0xB0, 123, 0]);
        assert_eq!(h.take(), Vec::<u8>::new());
        // Other controllers and pitch bend leave the keys alone.
        h.note([0x90, 60, 100]);
        h.take();
        h.note([0xB0, 64, 127]);
        h.note([0xE0, 0, 64]);
        assert_eq!(h.take(), vec![60]);
    }
}
