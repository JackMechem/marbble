//! A track's MIDI settings on the session side: the Track inspector's edit, and the step of the
//! playback list (`regions_dsp`) that plays the track's regions through them. The transforms are
//! pure functions in [`crate::edit::track_midi`]; live input meets the settings in the engine
//! (`dsp/live_keys.rs`), which gets them with the track's parameters.

use std::collections::HashMap;

use crate::edit::region_params as rp;
use crate::edit::track_midi::{self as tm, TrackMidiSettings};
use crate::session::*;

/// A playback entry's notes, shared between the entries that play them.
type NoteList = Arc<Vec<Note>>;

/// A change from the Track inspector's MIDI rows. The fields given are set.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TrackMidiPatch {
    pub transpose: Option<i16>,
    pub velocity: Option<i16>,
    pub key_limit: Option<(u8, u8)>,
    pub velocity_limit: Option<(u8, u8)>,
    pub delay: Option<f64>,
    /// Switches the delay's unit; without `delay`, the value is converted so the track plays as
    /// before at the project's start.
    pub delay_ms: Option<bool>,
}

impl TrackMidiPatch {
    /// The undo label: the Inspector's name for what changes.
    pub fn label(&self) -> &'static str {
        match self {
            p if p.transpose.is_some() => "Track Transpose",
            p if p.velocity.is_some() => "Track Velocity",
            p if p.key_limit.is_some() => "Key Limit",
            p if p.velocity_limit.is_some() => "Velocity Limit",
            _ => "Track Delay",
        }
    }
}

impl Session {
    /// Sets an instrument track's MIDI settings (Track inspector). Values are kept in range.
    pub fn set_track_midi(&mut self, id: Id, patch: &TrackMidiPatch) -> Result<()> {
        let t = self.project.track(id).ok_or_else(|| anyhow!("no such track"))?;
        if t.kind != TrackKind::Instrument {
            return Err(anyhow!("Only software instrument tracks have MIDI settings"));
        }
        if t.frozen.is_some() {
            return Err(anyhow!("{} is frozen. Unfreeze it to change its MIDI settings.", t.name));
        }
        let map = self.tempo_map();
        let mut s = t.midi_settings;
        s.transpose = patch.transpose.unwrap_or(s.transpose);
        s.velocity = patch.velocity.unwrap_or(s.velocity);
        s.key_limit = patch.key_limit.unwrap_or(s.key_limit);
        s.velocity_limit = patch.velocity_limit.unwrap_or(s.velocity_limit);
        if let Some(ms) = patch.delay_ms {
            let d = rp::delay_in(s.delay, s.delay_ms, ms, 0.0, &map);
            // Whole ticks, tenths of a millisecond, as the Region inspector's Delay.
            s.delay = if ms { (d * 10.0).round() / 10.0 } else { d.round() };
            s.delay_ms = ms;
        }
        s.delay = patch.delay.unwrap_or(s.delay);
        self.project.track_mut(id).unwrap().midi_settings = s.clamped();
        self.push_regions(id);
        let params = Self::track_params(&self.project, self.project.track(id).unwrap());
        self.send(Cmd::Params(id, params));
        self.dirty = true;
        Ok(())
    }

    /// The last step of a track's playback list: its MIDI entries played through the track's
    /// MIDI settings. Entries that shared a note list still do.
    pub(crate) fn play_through_track_midi(&self, s: &TrackMidiSettings, list: &mut [RegionDsp]) {
        if s.is_default() {
            return;
        }
        let map = self.tempo_map();
        let shift = s.constant_shift(&map);
        // By the list each came from, which is kept alive so its address can't be reused.
        let mut played: HashMap<*const Vec<Note>, (NoteList, NoteList)> = HashMap::new();
        for entry in list.iter_mut() {
            let RegionDsp::Midi { start, length, notes, cc } = entry else { continue };
            let changed = played.entry(Arc::as_ptr(notes)).or_insert_with(|| (notes.clone(), Arc::new(tm::notes(s, notes)))).1.clone();
            match shift {
                Some(d) => {
                    *start += d;
                    *notes = changed;
                }
                None => {
                    let moved = tm::delay_entry(s, *start, *length, &changed, cc, &map);
                    (*start, *length) = (moved.start, moved.length);
                    (*notes, *cc) = (Arc::new(moved.notes), Arc::new(moved.cc));
                }
            }
        }
    }
}
