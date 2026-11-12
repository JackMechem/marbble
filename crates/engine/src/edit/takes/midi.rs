//! MIDI take folders: their content is a copy of the take they play, written back to the take and
//! read from it as edits go (see `TakeFolder`).

use std::sync::Arc;

use crate::model::{Region, RegionContent};

/// Writes a MIDI take folder's content back into the take it plays (see [`TakeFolder`]), before
/// anything reads the takes or changes which one plays.
pub fn save_midi_take(r: &mut Region) {
    let (Some(f), RegionContent::Midi { notes, cc }) = (r.takes.as_mut(), &r.content) else { return };
    if let Some(t) = f.midi_take() {
        let offset = f.offset;
        f.store_midi(t, offset, r.length, notes, cc);
    }
}

/// Makes a MIDI take folder's content the notes of the take it plays, inside the region.
pub fn load_midi_take(r: &mut Region) {
    let (Some(f), RegionContent::Midi { notes, cc }) = (r.takes.as_mut(), &mut r.content) else { return };
    // The comp names the take, even after the one it named was deleted, and so does its place
    // in the list of comps.
    if let (None, Some(t)) = (f.comp.first(), f.midi_take()) {
        f.select_whole(t);
        f.store_comp();
    }
    if let Some(t) = f.midi_take() {
        let (n, c) = f.midi_window(t, f.offset, r.length);
        (*notes, *cc) = (Arc::new(n), Arc::new(c));
    }
}

/// Trims a MIDI take folder `r` to `start..start + length` while its notes stay put: its take
/// time follows, and its content becomes the take's notes in the new span, so trimming never
/// loses any (unlike a plain MIDI region, whose content is all it has).
pub fn retrim_midi_folder(r: &mut Region, start: i64, length: i64) {
    if r.takes.is_none() || !matches!(r.content, RegionContent::Midi { .. }) {
        return;
    }
    save_midi_take(r);
    r.takes.as_mut().unwrap().offset += start - r.start;
    r.start = start;
    r.length = length;
    load_midi_take(r);
}
