//! The Arrangement track (Logic's Global Tracks › Arrangement): named sections that tile the
//! timeline. Adding, renaming, recolouring and resizing a section only changes the markers.
//! Moving, copying or deleting one with its content rearranges the project: every region on
//! every track (its inactive track alternatives' too) is cut at the section edges and its
//! pieces move with their section, and so do
//! automation (Stereo Out's too) and the Marker track's markers. The tempo and signature tracks stay as they are.
//! The order arithmetic is in `crate::edit::arrangement`.

use anyhow::{anyhow, Result};

use crate::edit::arrangement::{self, Piece, Plan};
use crate::model::tempo::TempoMap;
use crate::session::regions::range_edit::sub_region;
use crate::session::*;

/// Whether the time map leaves `r` exactly where it is.
pub(crate) fn untouched(r: &Region, pieces: &[Piece]) -> bool {
    let (lo, hi) = (r.start, r.end());
    pieces.iter().any(|p| p.is_identity() && p.contains(lo, hi)) && !pieces.iter().any(|p| p.copy && p.overlaps(lo, hi))
}

/// Whether the time map changes any of the track's regions, its inactive alternatives' included.
pub(crate) fn track_touched(t: &Track, pieces: &[Piece]) -> bool {
    t.regions.iter().chain(t.alternatives.iter().flat_map(|a| a.regions.iter())).any(|r| !untouched(r, pieces))
}

/// Maps the track's regions through the time map, and its inactive alternatives' with them, so
/// switching to one afterwards finds it in place.
pub(crate) fn remap_track(t: &mut Track, pieces: &[Piece], map: &TempoMap, alloc: &mut dyn FnMut() -> Id) {
    t.regions = remap_regions(&t.regions, pieces, map, alloc);
    for a in &mut t.alternatives {
        a.regions = remap_regions(&a.regions, pieces, map, alloc);
    }
}

/// The part of `r` between `a` and `b`, as the edits that move time cut it (Arrangement moves
/// and section edits): unlike splitting by hand, which starts a pattern region's pattern again,
/// a pattern region's piece plays on from where it was cut. `map` is the tempo map it plays
/// through.
pub(crate) fn cut_piece(r: &Region, a: i64, b: i64, id: Id, map: &TempoMap) -> Region {
    let mut out = sub_region(r, a, b, id);
    if let Some(p) = &mut out.pattern {
        p.offset += map.beats_at(out.start as f64) - map.beats_at(r.start as f64);
    }
    out
}

/// The regions of a track after the time map: a region inside one piece moves whole (loop and
/// all); one across a piece's edge is cut there (a loop first becomes real copies). The first
/// piece of each region keeps its id, so a region that only moves stays selected.
pub(crate) fn remap_regions(regions: &[Region], pieces: &[Piece], map: &TempoMap, alloc: &mut dyn FnMut() -> Id) -> Vec<Region> {
    // Originals first, so the copies are the ones with new ids.
    let order: Vec<&Piece> = pieces.iter().filter(|p| !p.copy).chain(pieces.iter().filter(|p| p.copy)).collect();
    let mut out = Vec::new();
    for r in regions {
        let mut keep = Some(r.id);
        let mut next_id = |alloc: &mut dyn FnMut() -> Id| keep.take().unwrap_or_else(alloc);
        for p in order.iter().filter(|p| p.overlaps(r.start, r.end())) {
            if p.contains(r.start, r.end()) {
                let mut m = r.clone();
                m.id = next_id(alloc);
                m.move_to(r.start + p.delta());
                out.push(m);
                continue;
            }
            let passes = if r.is_looped() { r.real_copies() } else { vec![r.clone()] };
            for pass in passes.iter().filter(|q| p.overlaps(q.start, q.start + q.length)) {
                let mut m = cut_piece(pass, p.start, p.end, next_id(alloc), map);
                m.start += p.delta();
                out.push(m);
            }
        }
    }
    out
}

impl Session {
    fn arrangement_section(&self, id: Id) -> Option<&ArrangementMarker> {
        self.project.arrangement.iter().find(|s| s.id == id)
    }

    /// The + button: a section after the last one (at the project start for the first), named
    /// from Logic's list or `name`, eight bars long at the tempo where it starts.
    pub fn add_arrangement_marker(&mut self, name: Option<String>) -> Id {
        let start = arrangement::end(&self.project.arrangement).unwrap_or(0);
        let map = TempoMap::from_project(&self.project);
        let bar = map.bar_at(start as f64);
        let length = (map.bar_start(bar + arrangement::DEFAULT_BARS).round() as i64 - start).max(1);
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| arrangement::next_name(&self.project.arrangement));
        let id = self.project.alloc_id();
        self.project.arrangement = arrangement::add(&self.project.arrangement, id, name, length);
        self.dirty = true;
        id
    }

    pub fn rename_arrangement_marker(&mut self, id: Id, name: String) {
        if let Some(s) = self.project.arrangement.iter_mut().find(|s| s.id == id) {
            s.name = name;
            self.dirty = true;
        }
    }

    /// None colours the section by its name.
    pub fn set_arrangement_marker_color(&mut self, id: Id, color: Option<String>) {
        if let Some(s) = self.project.arrangement.iter_mut().find(|s| s.id == id) {
            s.color = color;
            self.dirty = true;
        }
    }

    /// Dragging a section's right edge: the later sections move along; the content stays.
    pub fn resize_arrangement_marker(&mut self, id: Id, length: i64) {
        self.project.arrangement = arrangement::resize(&self.project.arrangement, id, length);
        self.dirty = true;
    }

    /// Moves section `id` with its content to just before section `before` (None: after the
    /// last one), or with `copy` puts a copy of it there. Returns the regions now in the moved
    /// or copied section, to select.
    pub fn move_arrangement_marker(&mut self, id: Id, before: Option<Id>, copy: bool) -> Result<Vec<Id>> {
        self.arrangement_section(id).ok_or_else(|| anyhow!("no such arrangement marker"))?;
        let plan = if copy {
            let new_id = self.project.alloc_id();
            arrangement::copy_section(&self.project.arrangement, id, before, new_id)
        } else {
            arrangement::move_section(&self.project.arrangement, id, before)
        };
        match plan {
            Some(plan) => self.apply_arrangement(plan),
            None => Ok(Vec::new()),
        }
    }

    /// Delete Arrangement Marker (`content` false: the section before takes over its time) or
    /// Delete Arrangement Marker and Content (the section's time goes, and everything after
    /// it moves earlier).
    pub fn delete_arrangement_marker(&mut self, id: Id, content: bool) -> Result<()> {
        if !content {
            self.project.arrangement = arrangement::remove(&self.project.arrangement, id);
            self.dirty = true;
            return Ok(());
        }
        let plan = arrangement::delete_section(&self.project.arrangement, id).ok_or_else(|| anyhow!("no such arrangement marker"))?;
        self.apply_arrangement(plan).map(|_| ())
    }

    /// Rearranges the project's content by the plan's time map and sets its sections. Refused
    /// (with nothing changed) when it would cut or move a frozen track's regions.
    fn apply_arrangement(&mut self, plan: Plan) -> Result<Vec<Id>> {
        let pieces = &plan.pieces;
        let touched: Vec<Id> = self.project.tracks.iter().filter(|t| track_touched(t, pieces)).map(|t| t.id).collect();
        self.check_unfrozen(&touched)?;
        let map = self.tempo_map();
        let mut next = self.project.next_id;
        let mut alloc = || {
            next += 1;
            next - 1
        };
        let mut lanes_changed = Vec::new();
        for t in self.project.tracks.iter_mut() {
            if touched.contains(&t.id) {
                remap_track(t, pieces, &map, &mut alloc);
            }
            let mut changed = false;
            for lane in t.automation.iter_mut() {
                let points = arrangement::remap_points(&lane.points, pieces);
                if points != lane.points {
                    lane.points = points;
                    changed = true;
                }
            }
            if changed {
                lanes_changed.push(t.id);
            }
        }
        // Stereo Out's automation goes with its time too.
        let mut master = false;
        for lane in self.project.master.automation.iter_mut() {
            let points = arrangement::remap_points(&lane.points, pieces);
            master |= points != lane.points;
            lane.points = points;
        }
        if master {
            lanes_changed.push(MASTER_ID);
        }
        // Markers go with their time (a copied section gets copies of its markers).
        let mut markers = Vec::new();
        for m in &self.project.markers {
            for (k, pos) in arrangement::map_pos(pieces, m.pos).into_iter().enumerate() {
                markers.push(Marker { id: if k == 0 { m.id } else { alloc() }, pos, ..m.clone() });
            }
        }
        self.project.next_id = next;
        self.project.markers = markers;
        self.project.markers.sort_by_key(|m| (m.pos, m.id));
        self.project.arrangement = plan.sections;
        for t in touched {
            self.push_regions(t);
        }
        for t in lanes_changed {
            self.push_automation(t);
        }
        self.dirty = true;
        let Some((a, b)) = plan.focus else { return Ok(Vec::new()) };
        Ok(self.project.tracks.iter().flat_map(|t| t.regions.iter()).filter(|r| r.start >= a && r.end() <= b).map(|r| r.id).collect())
    }
}

#[cfg(test)]
mod tests;
