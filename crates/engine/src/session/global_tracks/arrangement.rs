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
mod tests {
    use super::*;
    use crate::audio_file::clip::Clip;
    use crate::audio_io::AudioConfig;

    /// A session on the silent ALSA null device with two audio tracks, at 120 BPM in 4/4 (a
    /// bar is 96 000 samples at 48 kHz), and three two-bar sections from bar 1.
    fn session() -> (Session, Vec<Id>, Vec<Id>, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("marbble-arrangement-{}", std::process::id()));
        let path = s.dir.join("Audio Files").join("tone.wav");
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![vec![0.25; 2_000_000]]));
        let tracks: Vec<Id> = (0..2).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
        let sections: Vec<Id> = (0..3)
            .map(|_| {
                let id = s.add_arrangement_marker(None);
                s.resize_arrangement_marker(id, 1000);
                id
            })
            .collect();
        (s, tracks, sections, clip)
    }

    fn region(s: &mut Session, t: Id, clip: Id, start: i64, length: i64) -> Id {
        let id = s.project.alloc_id();
        let content = RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
        s.project.track_mut(t).unwrap().regions.push(Region {
            id,
            name: "R".into(),
            start,
            length,
            muted: false,
            color: None,
            content,
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        });
        s.push_regions(t);
        id
    }

    /// (start, end, clip offset) of a track's regions, by start.
    fn spans(s: &Session, t: Id) -> Vec<(i64, i64, i64)> {
        let mut v: Vec<(i64, i64, i64)> = s
            .project
            .track(t)
            .unwrap()
            .regions
            .iter()
            .map(|r| match r.content {
                RegionContent::Audio { offset, .. } => (r.start, r.end(), offset),
                _ => (r.start, r.end(), 0),
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn adding_names_and_lays_out_eight_bars() {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let a = s.add_arrangement_marker(None);
        let b = s.add_arrangement_marker(Some("Chorus".into()));
        let bar = (4.0 * 60.0 / s.project.tempo * s.project.sample_rate as f64) as i64;
        let v = &s.project.arrangement;
        assert_eq!((v[0].id, v[0].name.as_str(), v[0].start, v[0].length), (a, "Intro", 0, 8 * bar));
        assert_eq!((v[1].id, v[1].name.as_str(), v[1].start, v[1].length), (b, "Chorus", 8 * bar, 8 * bar));
    }

    #[test]
    fn moving_a_section_takes_its_regions_and_cuts_the_ones_across_its_edge() {
        let (mut s, t, sec, clip) = session();
        // Sections: 0..1000, 1000..2000, 2000..3000. A region across the 2|3 boundary on track
        // 0, one inside section 3 on track 1, and one after the arrangement.
        let across = region(&mut s, t[0], clip, 1500, 1000);
        let inside = region(&mut s, t[1], clip, 2200, 300);
        region(&mut s, t[1], clip, 3500, 100);
        let picked = s.move_arrangement_marker(sec[2], Some(sec[1]), false).unwrap();
        // Section 3 is now at 1000..2000 and section 2 at 2000..3000.
        assert_eq!(s.project.arrangement.iter().map(|x| (x.id, x.start)).collect::<Vec<_>>(), vec![(sec[0], 0), (sec[2], 1000), (sec[1], 2000)]);
        // The region's first half (section 2) moved to 2500..3000; its second half (section
        // 3, playing from offset 500) to 1000..1500.
        assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 500), (2500, 3000, 0)]);
        assert_eq!(spans(&s, t[1]), vec![(1200, 1500, 0), (3500, 3600, 0)]);
        let first = s.project.track(t[0]).unwrap().regions.iter().find(|r| r.start == 2500).unwrap().id;
        assert_eq!(first, across, "the first piece keeps the id");
        let mut picked = picked;
        picked.sort();
        let mut want: Vec<Id> = s.project.tracks.iter().flat_map(|x| x.regions.iter()).filter(|r| r.start >= 1000 && r.end() <= 2000).map(|r| r.id).collect();
        want.sort();
        assert_eq!(picked, want);
        assert!(picked.contains(&inside));
    }

    #[test]
    fn copy_and_delete_with_content_ripple_the_rest() {
        let (mut s, t, sec, clip) = session();
        region(&mut s, t[0], clip, 100, 800);
        region(&mut s, t[1], clip, 2500, 1000);
        s.add_marker(500, Some("Hit".into()));
        // A copy of section 1 at the end of the arrangement.
        let made = s.move_arrangement_marker(sec[0], None, true).unwrap();
        assert_eq!(s.project.arrangement.len(), 4);
        assert_eq!(spans(&s, t[0]), vec![(100, 900, 0), (3100, 3900, 0)]);
        // The region crossing the arrangement's end: its tail is after the arrangement, so it
        // moves along with the time after it.
        assert_eq!(spans(&s, t[1]), vec![(2500, 3000, 0), (4000, 4500, 500)]);
        assert_eq!(made.len(), 1);
        assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![500, 3500]);
        // Delete section 1 and its content: everything after it moves 1000 earlier.
        s.delete_arrangement_marker(sec[0], true).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(2100, 2900, 0)]);
        assert_eq!(spans(&s, t[1]), vec![(1500, 2000, 0), (3000, 3500, 500)]);
        assert_eq!(s.project.arrangement[0].start, 0);
        assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![2500]);
    }

    #[test]
    fn stereo_out_automation_moves_with_its_section() {
        let (mut s, _, sec, _) = session();
        let pt = |pos: i64, value: f32| crate::model::AutoPoint { pos, value, curve: 0.0 };
        // A dip in the middle section's Stereo Out volume.
        s.set_automation_lane(MASTER_ID, AutoParam::Volume, vec![pt(1200, 0.0), pt(1500, -20.0), pt(1800, 0.0)]);
        s.move_arrangement_marker(sec[1], None, false).unwrap();
        let pos: Vec<i64> = s.project.master.automation[0].points.iter().map(|p| p.pos).collect();
        assert!(pos.contains(&2500), "the dip moved with section 2 to 2000..3000: {pos:?}");
        assert!(!pos.contains(&1500));
    }

    #[test]
    fn a_frozen_track_in_the_way_refuses_the_move() {
        let (mut s, t, sec, clip) = session();
        region(&mut s, t[0], clip, 1200, 100);
        s.project.track_mut(t[0]).unwrap().frozen = Some(FreezeInfo { clip, start: 0 });
        assert!(s.move_arrangement_marker(sec[1], None, false).is_err());
        assert_eq!(spans(&s, t[0]), vec![(1200, 1300, 0)]);
        assert_eq!(s.project.arrangement[1].start, 1000, "nothing moved");
    }

    /// Every note the project plays: (timeline start, pitch), sorted.
    fn heard(s: &Session) -> Vec<(i64, u8)> {
        let mut v: Vec<(i64, u8)> = s
            .played_notes()
            .iter()
            .flat_map(|p| {
                let start = s.region(p.region).unwrap().start;
                p.notes.iter().map(move |n| (start + n.start, n.pitch))
            })
            .collect();
        v.sort();
        v
    }

    /// A pattern region over `bars` bars from 0 whose kick plays every third sixteenth, so
    /// each bar starts on another step of the row. Returns it, the track and the bar length.
    fn pattern_region(s: &mut Session, bars: i64) -> (Id, Id, i64) {
        let bar = s.tempo_map().samples_per_bar_at(0.0).round() as i64;
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let id = s.create_pattern_region(t, 0, bars * bar).unwrap();
        let mut p = s.region(id).unwrap().pattern.clone().unwrap();
        p.rows[0].length = Some(3);
        p.rows[0].steps = vec![crate::edit::pattern::Step { on: true, ..Default::default() }];
        s.set_pattern(id, p).unwrap();
        (id, t, bar)
    }

    #[test]
    fn pattern_regions_cut_by_a_move_play_on_from_the_cut() {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let (id, t, bar) = pattern_region(&mut s, 3);
        let sec: Vec<Id> = (0..3)
            .map(|_| {
                let id = s.add_arrangement_marker(None);
                s.resize_arrangement_marker(id, bar);
                id
            })
            .collect();
        let before = heard(&s);
        assert!(before.contains(&(0, 36)) && !before.contains(&(bar, 36)), "bar 2 starts on the row's second step");
        // Bars 2 and 3 trade places, each playing what it played before.
        s.move_arrangement_marker(sec[2], Some(sec[1]), false).unwrap();
        let mut want: Vec<(i64, u8)> = before
            .iter()
            .map(|&(x, p)| {
                (
                    if x < bar {
                        x
                    } else if x < 2 * bar {
                        x + bar
                    } else {
                        x - bar
                    },
                    p,
                )
            })
            .collect();
        want.sort();
        assert_eq!(heard(&s), want);
        let regions = &s.project.track(t).unwrap().regions;
        assert_eq!(regions.len(), 3);
        assert!(regions.iter().any(|r| r.id == id && r.pattern.as_ref().unwrap().offset == 0.0));
        // Splitting by hand still starts the pattern again.
        let piece = regions.iter().find(|r| r.start == 0).unwrap().id;
        s.split_regions_at(&[piece], &[bar / 2]);
        let second = s.project.track(t).unwrap().regions.iter().find(|r| r.start == bar / 2).unwrap();
        assert_eq!(second.pattern.as_ref().unwrap().offset, 0.0);
        assert!(heard(&s).contains(&(bar / 2, 36)));
    }

    #[test]
    fn inactive_track_alternatives_move_with_their_sections() {
        let (mut s, t, sec, clip) = session();
        region(&mut s, t[0], clip, 1500, 1000);
        // Alternative B is a copy, then loses its region for one in section 3 alone; A waits.
        s.new_track_alternative(t[0], true).unwrap();
        s.project.track_mut(t[0]).unwrap().regions.clear();
        region(&mut s, t[0], clip, 2200, 300);
        s.move_arrangement_marker(sec[2], Some(sec[1]), false).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1200, 1500, 0)]);
        // Switching to A finds its region cut and moved as the active one would have been.
        s.switch_track_alternative(t[0], "A").unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 500), (2500, 3000, 0)]);
        let ids: Vec<Id> =
            s.project.tracks.iter().flat_map(|x| x.regions.iter().chain(x.alternatives.iter().flat_map(|a| a.regions.iter()))).map(|r| r.id).collect();
        assert!(ids.iter().all(|id| ids.iter().filter(|x| *x == id).count() == 1), "every piece has an id of its own: {ids:?}");
        // A frozen track's alternatives can't change: a move that would cut one is refused.
        s.delete_track_alternative(t[0], "B").unwrap();
        s.new_track_alternative(t[0], false).unwrap();
        s.project.track_mut(t[0]).unwrap().frozen = Some(FreezeInfo { clip, start: 0 });
        assert!(s.move_arrangement_marker(sec[2], None, false).is_err());
        s.switch_track_alternative(t[0], "A").unwrap_err();
        assert_eq!(s.project.track(t[0]).unwrap().alternatives[0].regions.len(), 2, "nothing moved");
    }

    #[test]
    fn looped_regions_across_an_edge_become_real_copies() {
        let (mut s, t, sec, clip) = session();
        let id = region(&mut s, t[0], clip, 600, 300);
        s.project.track_mut(t[0]).unwrap().regions[0].loop_end = Some(1500);
        s.move_arrangement_marker(sec[1], Some(sec[0]), false).unwrap();
        // Passes 600..900, 900..1200, 1200..1500; section 2 (1000..2000) goes to 0..1000 and
        // section 1 to 1000..2000.
        assert_eq!(spans(&s, t[0]), vec![(0, 200, 100), (200, 500, 0), (1600, 1900, 0), (1900, 2000, 0)]);
        assert!(s.project.track(t[0]).unwrap().regions.iter().all(|r| r.loop_end.is_none()));
        assert!(s.project.track(t[0]).unwrap().regions.iter().any(|r| r.id == id));
    }
}
