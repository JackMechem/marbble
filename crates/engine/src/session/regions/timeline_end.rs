//! The end of the timeline ([`MAX_PROJECT_LENGTH`], Logic Pro's): no edit may put a region past
//! it. Without it, Repeat on regions spread along the timeline multiplies its length, and a few
//! hundred edits made a project days long (#435).

use crate::session::*;

impl Session {
    /// Refuses an edit that put a region past the end of the timeline, or further past it than
    /// the project reached before (one opened that way still edits): puts the project back as it
    /// was and drops the edit's undo step. Call straight after an edit recorded with `checkpoint`
    /// (the command wrappers do, through [`Session::edit_guard`]).
    pub fn timeline_guard(&mut self) -> Result<()> {
        let Some(before) = self.undo_stack.last().map(|e| e.project.regions_end()) else { return Ok(()) };
        if self.project.regions_end() <= MAX_PROJECT_LENGTH.max(before) {
            return Ok(());
        }
        self.abandon_checkpoint();
        let minutes = MAX_PROJECT_LENGTH / (self.project.sample_rate.max(1) as i64 * 60);
        Err(anyhow!("That would put a region past the end of the timeline, {} h {:02} min in.", minutes / 60, minutes % 60))
    }
}

#[cfg(test)]
mod tests {
    use crate::session::tests::scratch_session;
    use crate::session::*;

    /// An instrument track with a one-bar MIDI region whose end is `bars` bars before the end of
    /// the timeline.
    fn near_the_end(bars: i64) -> (Session, Id, i64) {
        let (mut s, _) = scratch_session("timeline-end");
        let bar = s.project.samples_per_bar().round() as i64;
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let r = s.create_midi_region(t, MAX_PROJECT_LENGTH - (bars + 1) * bar, bar).unwrap();
        (s, r, bar)
    }

    fn regions(s: &Session) -> usize {
        s.project.tracks.iter().map(|t| t.regions.len()).sum()
    }

    #[test]
    fn repeat_up_to_the_end_is_fine() {
        let (mut s, r, _) = near_the_end(2);
        s.checkpoint("Repeat Regions", None, false);
        s.repeat_regions(&[r], 2);
        assert!(s.edit_guard().is_ok());
        assert_eq!(s.project.regions_end(), MAX_PROJECT_LENGTH);
        assert_eq!(regions(&s), 3);
    }

    #[test]
    fn repeat_past_the_end_is_refused_and_leaves_no_undo_step() {
        let (mut s, r, _) = near_the_end(2);
        let before = s.undo_label();
        s.checkpoint("Repeat Regions", None, false);
        s.repeat_regions(&[r], 3);
        let refused = s.edit_guard().unwrap_err().to_string();
        assert!(refused.contains("past the end of the timeline, 12 h 25 min in"), "{refused}");
        assert_eq!(regions(&s), 1);
        assert_eq!(s.undo_label(), before);
    }

    #[test]
    fn the_loop_switch_loops_the_last_region_up_to_the_end() {
        let (mut s, r, _) = near_the_end(1);
        s.checkpoint("Loop Region", None, false);
        s.set_regions_looped(&[r], true);
        assert!(s.edit_guard().is_ok());
        assert_eq!(s.project.regions_end(), MAX_PROJECT_LENGTH);
    }

    #[test]
    fn a_project_already_past_the_end_still_edits_but_reaches_no_further() {
        let (mut s, r, bar) = near_the_end(0);
        s.project.tracks[0].regions[0].length = 3 * bar;
        let end = s.project.regions_end();
        // Shorter, though still past the end: fine.
        s.checkpoint("Resize Region", None, false);
        s.resize_region(r, end - 3 * bar, 2 * bar);
        assert!(s.edit_guard().is_ok());
        assert_eq!(s.project.regions_end(), end - bar);
        // Longer again: refused.
        s.checkpoint("Resize Region", None, false);
        s.resize_region(r, end - 3 * bar, 4 * bar);
        assert!(s.edit_guard().is_err());
        assert_eq!(s.project.regions_end(), end - bar);
    }
}
