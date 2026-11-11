//! A track's MIDI settings on live input: MIDI keyboards, Musical Typing, Step Input and the
//! editors' note previews all reach the instrument through here (`strip.rs`), on the audio thread.
//!
//! Each key remembers the note its note-on became, so its note-off ends that note even if the
//! Transpose changed while it was held, and a key a limit left out stays silent at its note-off.
//! The settings themselves are pure functions in `crate::edit::track_midi`; Delay doesn't apply
//! to what is played live, as nothing can play early.

use crate::edit::track_midi::TrackMidiSettings;

/// The note each held key plays (None: not held, or left out by a limit).
#[derive(Debug, Clone, Copy)]
pub struct LiveKeys {
    playing: [Option<u8>; 128],
}

impl Default for LiveKeys {
    fn default() -> Self {
        LiveKeys { playing: [None; 128] }
    }
}

impl LiveKeys {
    /// The message the instrument gets for live message `m`, or None when it gets nothing.
    /// Controllers, pitch bend and the rest pass unchanged.
    pub fn play(&mut self, s: &TrackMidiSettings, m: [u8; 3]) -> Option<[u8; 3]> {
        let key = m[1] as usize & 127;
        match m[0] & 0xF0 {
            0x90 if m[2] > 0 => {
                self.playing[key] = s.plays(m[1], m[2]).then(|| s.pitch(m[1]));
                self.playing[key].map(|pitch| [m[0], pitch, s.velocity(m[2])])
            }
            0x80 | 0x90 => self.playing[key].take().map(|pitch| [m[0], pitch, m[2]]),
            // Polyphonic aftertouch follows its key's note.
            0xA0 => self.playing[key].map(|pitch| [m[0], pitch, m[2]]),
            _ => Some(m),
        }
    }

    /// Forgets every held key, once the notes they played have been ended another way.
    pub fn clear(&mut self) {
        self.playing = [None; 128];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(f: impl FnOnce(&mut TrackMidiSettings)) -> TrackMidiSettings {
        let mut s = TrackMidiSettings::default();
        f(&mut s);
        s
    }

    #[test]
    fn defaults_pass_everything_unchanged() {
        let s = TrackMidiSettings::default();
        let mut k = LiveKeys::default();
        for m in [[0x90, 60, 100], [0xA0, 60, 30], [0x80, 60, 0], [0xB0, 64, 127], [0xE0, 0, 64], [0x91, 0, 1], [0x81, 0, 0]] {
            assert_eq!(k.play(&s, m), Some(m));
        }
    }

    #[test]
    fn notes_are_transposed_and_their_velocity_offset() {
        let s = settings(|s| {
            s.transpose = -12;
            s.velocity = 40;
        });
        let mut k = LiveKeys::default();
        assert_eq!(k.play(&s, [0x90, 60, 100]), Some([0x90, 48, 127]));
        assert_eq!(k.play(&s, [0xA0, 60, 20]), Some([0xA0, 48, 20]));
        assert_eq!(k.play(&s, [0x80, 60, 64]), Some([0x80, 48, 64]));
        // Once released, a stray note-off for the key sends nothing.
        assert_eq!(k.play(&s, [0x80, 60, 0]), None);
    }

    #[test]
    fn a_note_off_ends_the_note_its_key_started_after_the_transpose_changes() {
        let mut k = LiveKeys::default();
        assert_eq!(k.play(&settings(|s| s.transpose = 5), [0x90, 60, 90]), Some([0x90, 65, 90]));
        // A note-on with velocity 0 is a note-off too.
        assert_eq!(k.play(&settings(|s| s.transpose = 7), [0x90, 60, 0]), Some([0x90, 65, 0]));
    }

    #[test]
    fn keys_a_limit_leaves_out_stay_silent() {
        let s = settings(|s| {
            s.key_limit = (48, 72);
            s.velocity_limit = (20, 127);
        });
        let mut k = LiveKeys::default();
        assert_eq!(k.play(&s, [0x90, 47, 100]), None);
        assert_eq!(k.play(&s, [0x90, 60, 10]), None);
        assert_eq!(k.play(&s, [0xA0, 47, 50]), None);
        assert_eq!(k.play(&s, [0x80, 47, 0]), None);
        assert_eq!(k.play(&s, [0x80, 60, 0]), None);
        assert_eq!(k.play(&s, [0x90, 72, 20]), Some([0x90, 72, 20]));
        // Narrowing the limit while a key is held still lets its note end.
        let narrower = settings(|s| s.key_limit = (0, 10));
        assert_eq!(k.play(&narrower, [0x80, 72, 0]), Some([0x80, 72, 0]));
    }

    #[test]
    fn clearing_forgets_held_keys() {
        let s = TrackMidiSettings::default();
        let mut k = LiveKeys::default();
        k.play(&s, [0x90, 60, 100]);
        k.clear();
        assert_eq!(k.play(&s, [0x80, 60, 0]), None);
    }
}
