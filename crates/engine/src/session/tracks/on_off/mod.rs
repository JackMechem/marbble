//! Logic's Track On/Off, the session's side. `Track::off` is switched with `update_track`, like
//! Mute (a performance switch: no undo step of its own, and an undo or redo puts back the state
//! its snapshot had). `Project::is_off` applies the stacks above a track, `track_params` tells the
//! engine, and `update_pdc` leaves the track out of delay compensation. See `dsp/track_off/`.
//!
//! An Off track can't record. Record skips it and keeps its R button for later, and refuses when
//! no other track is enabled, so nothing is armed that won't be taken. Switching any track while
//! recording is refused: the track might be in the take, and the delay compensation the take is
//! placed with must stay as it is. Renders of a single track that's off (Freeze, Bounce in Place,
//! Export Region) are refused rather than writing silence.

use crate::session::*;

impl Session {
    /// Errors (naming the track) if any of `tracks` is off.
    pub(crate) fn check_on(&self, tracks: &[Id]) -> Result<()> {
        match tracks.iter().find(|id| self.project.is_off(**id)).and_then(|id| self.project.track(*id)) {
            Some(t) => Err(anyhow!("{} is off. Turn it on first.", t.name)),
            None => Ok(()),
        }
    }

    /// Why a track can't be switched on or off right now, if so.
    pub fn off_refused(&self) -> Option<String> {
        self.rec_punch.is_some().then(|| "Stop recording before turning a track on or off.".to_string())
    }
}

#[cfg(test)]
mod tests;
