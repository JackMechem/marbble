//! Dragging regions past the last track (Logic's behaviour): each source lane that falls past
//! the end gets a new track of its own below the last one, and the regions move or copy there
//! with whatever else the same drag did, as one edit. The Tracks area works out which regions
//! go where (`src/tracks/regions/newTrackDrag.ts`).

use crate::session::regions::moves::RegionMove;
use crate::session::*;

impl Session {
    /// Moves regions as [`Session::drag_move_regions`] does, with `lanes` going onto new tracks:
    /// (source track, its regions' moves), in the order the new tracks are to have. A region on
    /// a frozen track, or heading onto one, stays where it is. Returns the regions moved to new
    /// tracks and selects the last new track, as Move to New Track does.
    pub fn drag_regions_to_new_tracks(&mut self, moves: &[RegionMove], lanes: &[(Id, Vec<RegionMove>)], automation: Option<bool>) -> Vec<Id> {
        let mut all: Vec<RegionMove> = moves.iter().filter(|m| self.movable(m.id, m.track)).cloned().collect();
        let mut made = Vec::new();
        let mut last = None;
        for (source, regions) in lanes {
            let on_lane: Vec<&RegionMove> =
                regions.iter().filter(|m| self.find_region(m.id).is_some_and(|(t, _)| t == *source) && self.movable(m.id, None)).collect();
            if on_lane.is_empty() {
                continue;
            }
            let Some(track) = self.new_last_track(*source) else { continue };
            all.extend(on_lane.iter().map(|m| RegionMove { id: m.id, start: m.start, track: Some(track) }));
            made.extend(on_lane.iter().map(|m| m.id));
            last = Some(track);
        }
        self.sync_tracks();
        // The new lanes are empty, so the drag mode only rearranges the lanes regions left.
        self.drag_move_regions(&all, automation);
        if last.is_some() {
            self.select_track(last);
        }
        made
    }

    /// Copies regions as [`Session::drag_paste_regions`] does (an Alt-drag), with `lanes` going
    /// onto new tracks: (source track, the copies in place). The originals stay. Returns every
    /// copy made and selects the last new track.
    pub fn drag_copy_regions_to_new_tracks(&mut self, items: &[(Id, Region)], lanes: &[(Id, Vec<Region>)]) -> Vec<Id> {
        self.load_clips(lanes.iter().flat_map(|(_, regions)| regions.iter().flat_map(|r| r.clips())));
        let mut all: Vec<(Id, Region)> = items.iter().filter(|(t, _)| self.project.track(*t).is_some_and(|t| t.frozen.is_none())).cloned().collect();
        let mut last = None;
        for (source, regions) in lanes {
            let on_lane: Vec<&Region> = regions.iter().filter(|r| self.region_fits(*source, r)).collect();
            if on_lane.is_empty() {
                continue;
            }
            let Some(track) = self.new_last_track(*source) else { continue };
            all.extend(on_lane.into_iter().map(|r| (track, r.clone())));
            last = Some(track);
        }
        self.sync_tracks();
        let made = self.drag_paste_regions(&all, 0);
        if last.is_some() {
            self.select_track(last);
        }
        made
    }

    /// Whether a drag can take region `id` off its track and onto `to`: neither may be frozen.
    fn movable(&self, id: Id, to: Option<Id>) -> bool {
        let live = |t: Id| self.project.track(t).is_some_and(|t| t.frozen.is_none());
        self.find_region(id).is_some_and(|(from, _)| live(from)) && to.is_none_or(live)
    }

    /// An empty copy of `source` with its settings, as Move to New Track makes, but below every
    /// track and outside any stack: the drag showed it below the last track, whatever `source`
    /// sits in. The caller runs `sync_tracks`. (Leaving a summing stack, a track on the default
    /// output plays to Stereo Out.)
    fn new_last_track(&mut self, source: Id) -> Option<Id> {
        if !matches!(self.project.track(source)?.kind, TrackKind::Audio | TrackKind::Instrument) {
            return None;
        }
        let id = *self.duplicate_tracks(&[source], false).first()?;
        let at = self.project.tracks.iter().position(|t| t.id == id)?;
        let mut t = self.project.tracks.remove(at);
        t.parent = None;
        self.project.tracks.push(t);
        Some(id)
    }
}

#[cfg(test)]
mod tests;
