//! Region edits as the Tracks area makes them: in the project's drag mode (Logic's Drag
//! pop-up), and with Move Automation with Regions. No Overlap and the Shuffle modes are
//! applied on top of the plain edits (`move_regions`, `resize_region` and so on), whose X-Fade
//! handling stays as it is. The rules themselves are in `drag_modes.rs` and `auto_move.rs`.
//!
//! The Tauri commands for user edits call these; edits made on the way to something else
//! (range edits, Bounce in Place, Move to New Track, Repeat) use the plain ones, so they
//! don't shuffle a lane behind the user's back.

use crate::edit::auto_move::{move_with_regions, Shift};
use crate::edit::drag_modes::{no_overlap, shuffle, shuffle_resize, Side, Span};
use crate::session::regions::moves::RegionMove;
use crate::session::*;

/// Every track's lane as the drag modes see it, before an edit.
type Lanes = Vec<(Id, Vec<Span>)>;

impl Session {
    /// Moves regions (a drag in the Tracks area) in the drag mode. `automation` says whether
    /// the automation under each region that stays on its track goes with it; None follows
    /// Settings › Editing, where Ask counts as Never (the frontend asks, and passes the
    /// answer).
    pub fn drag_move_regions(&mut self, moves: &[RegionMove], automation: Option<bool>) {
        let ids: Vec<Id> = moves.iter().map(|m| m.id).collect();
        let (lanes, was) = (self.lanes(), self.places(&ids));
        self.move_regions(moves);
        self.apply_drag_mode(&lanes, &ids);
        self.move_automation(&was, automation);
    }

    /// Nudges regions by `delta` samples in the drag mode, as a move.
    pub fn drag_nudge_regions(&mut self, ids: &[Id], delta: i64, automation: Option<bool>) {
        let (lanes, was) = (self.lanes(), self.places(ids));
        self.nudge_regions(ids, delta);
        self.apply_drag_mode(&lanes, ids);
        self.move_automation(&was, automation);
    }

    /// Trims a region's edges in the drag mode: No Overlap stops it at its neighbours, the
    /// Shuffle modes move them along.
    pub fn drag_resize_region(&mut self, id: Id, start: i64, length: i64) {
        let (start, length) = self.zero_trim(id, start, length);
        let before = self.find_region(id).map(|(t, _)| (t, self.lane(t)));
        self.resize_region(id, start, length);
        match self.side() {
            Some(side) => {
                let (Some((track, lane)), Some(r)) = (before, self.region(id)) else { return };
                let starts = shuffle_resize(&lane, id, r.end() - r.start, side);
                self.place_regions(track, &starts);
            }
            None => self.trim_overlaps(&[id]),
        }
    }

    /// Trims several regions in the drag mode, as one drag trims every selected region:
    /// `resizes` holds each region's new `(id, start, length)`, measured from where the
    /// regions were before any of them changed. In the Shuffle modes one trim moves the
    /// regions after (or before) it along its lane, so each is applied as the same change to
    /// its region's edges from wherever the trims before it left the region.
    pub fn drag_resize_regions(&mut self, resizes: &[(Id, i64, i64)]) {
        let was: Vec<(Id, i64, i64)> = resizes.iter().filter_map(|&(id, ..)| self.region(id).map(|r| (id, r.start, r.start + r.length))).collect();
        for &(id, start, length) in resizes {
            let Some(&(_, start0, end0)) = was.iter().find(|w| w.0 == id) else { continue };
            // No Overlap may have removed it on the way.
            let Some(r) = self.region(id) else { continue };
            let new_start = r.start + start - start0;
            let new_end = r.start + r.length + start + length - end0;
            self.drag_resize_region(id, new_start, new_end - new_start);
        }
    }

    /// Deletes regions; in the Shuffle modes the regions after (Shuffle L) or before
    /// (Shuffle R) close the gap.
    pub fn drag_delete_regions(&mut self, ids: &[Id]) {
        let lanes = self.lanes();
        self.delete_regions(ids);
        self.apply_drag_mode(&lanes, ids);
    }

    /// Pastes (or Alt-drag copies) regions in the drag mode. Returns the new regions.
    pub fn drag_paste_regions(&mut self, items: &[(Id, Region)], offset: i64) -> Vec<Id> {
        let lanes = self.lanes();
        let made = self.paste_regions(items, offset);
        self.apply_drag_mode(&lanes, &made);
        made.into_iter().filter(|&id| self.find_region(id).is_some()).collect()
    }

    fn side(&self) -> Option<Side> {
        match self.project.drag_mode {
            DragMode::ShuffleL => Some(Side::Left),
            DragMode::ShuffleR => Some(Side::Right),
            _ => None,
        }
    }

    fn lane(&self, track: Id) -> Vec<Span> {
        self.project.track(track).map(|t| t.regions.iter().map(|r| Span { id: r.id, start: r.start, len: r.end() - r.start }).collect()).unwrap_or_default()
    }

    fn lanes(&self) -> Lanes {
        self.project.tracks.iter().map(|t| (t.id, self.lane(t.id))).collect()
    }

    /// Where each region is: (region, track, start, end).
    fn places(&self, ids: &[Id]) -> Vec<(Id, Id, i64, i64)> {
        ids.iter().filter_map(|&id| self.find_region(id).map(|(t, _)| (id, t, self.region(id).unwrap()))).map(|(id, t, r)| (id, t, r.start, r.end())).collect()
    }

    /// After `ids` were moved, deleted or pasted (`lanes` is how the tracks were before):
    /// No Overlap shortens what now overlaps, the Shuffle modes rearrange the lanes the
    /// regions left and arrived on.
    fn apply_drag_mode(&mut self, lanes: &Lanes, ids: &[Id]) {
        let Some(side) = self.side() else {
            if self.project.drag_mode == DragMode::NoOverlap {
                self.trim_overlaps(ids);
            }
            return;
        };
        for (track, before) in lanes {
            let leaving: Vec<Id> = before.iter().map(|s| s.id).filter(|id| ids.contains(id)).collect();
            let arriving: Vec<Span> = self.lane(*track).into_iter().filter(|s| ids.contains(&s.id)).collect();
            if leaving.is_empty() && arriving.is_empty() {
                continue;
            }
            let starts = shuffle(before, &leaving, &arriving, side);
            self.place_regions(*track, &starts);
        }
    }

    /// Moves regions of `track` to the given starts.
    fn place_regions(&mut self, track: Id, starts: &[(Id, i64)]) {
        let Some(t) = self.project.track_mut(track) else { return };
        let mut changed = false;
        for r in &mut t.regions {
            if let Some(&(_, s)) = starts.iter().find(|(id, _)| *id == r.id) {
                if s != r.start {
                    r.move_to(s.max(0));
                    changed = true;
                }
            }
        }
        if changed {
            self.push_regions(track);
            self.dirty = true;
        }
    }

    /// No Overlap: shortens the regions that overlap one of `ids`, removing those covered
    /// from their start.
    fn trim_overlaps(&mut self, ids: &[Id]) {
        let tracks: Vec<Id> = self.project.tracks.iter().filter(|t| t.regions.iter().any(|r| ids.contains(&r.id))).map(|t| t.id).collect();
        for track in tracks {
            let cuts = no_overlap(&self.lane(track), ids);
            if cuts.is_empty() {
                continue;
            }
            let t = self.project.track_mut(track).unwrap();
            t.regions.retain(|r| !cuts.iter().any(|&(id, len)| id == r.id && len <= 0));
            for r in &mut t.regions {
                if let Some(&(_, len)) = cuts.iter().find(|(id, _)| *id == r.id) {
                    shorten(r, len);
                }
            }
            self.push_regions(track);
            self.dirty = true;
        }
    }

    /// Move Automation with Regions: the automation under each region in `was` (where it
    /// was before the edit) moves to where the region is now, if it stayed on its track.
    fn move_automation(&mut self, was: &[(Id, Id, i64, i64)], automation: Option<bool>) {
        if !automation.unwrap_or(self.prefs.editing.move_automation == MoveAutomation::Always) {
            return;
        }
        let mut by_track: Vec<(Id, Vec<Shift>)> = Vec::new();
        for &(id, track, lo, hi) in was {
            let Some((now, idx)) = self.find_region(id) else { continue };
            let start = self.project.track(now).unwrap().regions[idx].start;
            if now != track || start == lo {
                continue;
            }
            let shift = Shift { lo, hi, delta: start - lo };
            match by_track.iter_mut().find(|(t, _)| *t == track) {
                Some((_, v)) => v.push(shift),
                None => by_track.push((track, vec![shift])),
            }
        }
        for (track, shifts) in by_track {
            let Some(t) = self.project.track_mut(track) else { continue };
            let mut changed = false;
            for lane in &mut t.automation {
                let moved = move_with_regions(&lane.points, &shifts);
                if moved != lane.points {
                    lane.points = moved;
                    changed = true;
                }
            }
            if changed {
                self.push_automation(track);
                self.dirty = true;
            }
        }
    }
}

/// Shortens a region to sound for `len` samples: its loop first, then the region itself.
fn shorten(r: &mut Region, len: i64) {
    let end = r.start + len;
    if end < r.start + r.length {
        r.length = len;
        r.loop_end = None;
        if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
            *fade_out = (*fade_out).min(len);
            *fade_in = (*fade_in).min(len - *fade_out);
        }
    } else {
        r.loop_end = Some(end);
        if !r.is_looped() {
            r.loop_end = None;
        }
    }
}

#[cfg(test)]
mod tests;
