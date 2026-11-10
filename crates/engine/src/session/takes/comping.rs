//! Quick Swipe Comping and the edits of a take folder's takes: comp ranges and sections, choosing,
//! renaming, rating and deleting takes, and what a folder's comp plays when rendered.

use crate::edit::takes::pieces::pass_pieces;
use crate::session::*;

impl Session {
    /// What a take folder's first pass plays (frames x channels), as `render_region` does for
    /// plain audio: the comp with its crossfades, the folder's gain and fades.
    pub(crate) fn render_comp(&self, r: &Region, f: &TakeFolder, channels: usize) -> Vec<Vec<f32>> {
        let len = r.length.max(0);
        let mut out = vec![vec![0.0f32; len as usize]; channels];
        let RegionContent::Audio { gain_db, .. } = r.content else { return out };
        let law = FadeLaw::Curve(self.prefs.editing.crossfade_curve.clamp(-1.0, 1.0));
        let curve = |x: f32| law.gain(x);
        for p in pass_pieces(r, f, r.start, r.length, self.crossfade_frames()) {
            let Some(c) = self.clips.get(&p.clip) else { continue };
            let g = volume_gain(gain_db + p.gain_db);
            for tl in p.start.max(r.start)..p.end.min(r.start + len) {
                let src = p.offset + (tl - p.start);
                if src < 0 || src as usize >= c.frames() {
                    continue;
                }
                let mut gain = g;
                if p.fade_in.1 > 0 && tl < p.fade_in.0 + p.fade_in.1 {
                    gain *= curve((tl - p.fade_in.0) as f32 / p.fade_in.1 as f32);
                }
                if p.fade_out.1 > 0 && tl >= p.fade_out.0 {
                    gain *= curve((p.fade_out.0 + p.fade_out.1 - tl) as f32 / p.fade_out.1 as f32);
                }
                for (ch, o) in out.iter_mut().enumerate() {
                    o[(tl - r.start) as usize] += c.channels[ch.min(c.channels.len() - 1)][src as usize] * gain;
                }
            }
        }
        out
    }

    /// The take folder `region` and the track it is on.
    fn folder_mut(&mut self, region: Id) -> Option<(Id, &mut Region)> {
        let (track, idx) = self.find_region(region)?;
        let r = &mut self.project.track_mut(track)?.regions[idx];
        r.takes.is_some().then_some((track, r))
    }

    /// Applies `f` to a folder and refreshes its track. `f` also gets what to add to a
    /// timeline position to get the folder's take time. A MIDI folder's content is saved into
    /// its take first and reloaded after, so `f` sees and sets which take plays.
    pub(super) fn edit_folder(&mut self, region: Id, f: impl FnOnce(&mut TakeFolder, i64)) {
        let Some((track, r)) = self.folder_mut(region) else { return };
        crate::edit::takes::midi::save_midi_take(r);
        let to_take_time = take_time_shift(r);
        let newest = {
            let folder = r.takes.as_mut().unwrap();
            f(folder, to_take_time);
            folder.store_comp();
            folder.takes.last().map(|t| t.clip)
        };
        // The folder's own clip follows its newest take (see `TakeFolder`).
        if let (Some(c), RegionContent::Audio { clip, .. }) = (newest, &mut r.content) {
            *clip = c;
        }
        crate::edit::takes::midi::load_midi_take(r);
        self.push_regions(track);
        self.dirty = true;
    }

    /// Whether `region` is a MIDI take folder.
    pub(super) fn is_midi_folder(&self, region: Id) -> bool {
        self.region(region).map(|r| r.takes.is_some() && matches!(r.content, RegionContent::Midi { .. })).unwrap_or(false)
    }

    /// The take folders that follow `region`'s take choices and comp edits, as in Logic: when
    /// its track is in a group that is on and links Editing (Selection), the folders that
    /// overlap it in time on the group's other tracks.
    fn comp_partners(&self, region: Id) -> Vec<Id> {
        let (Some((track, _)), Some(r)) = (self.find_region(region), self.region(region)) else { return Vec::new() };
        let (a, b) = (r.start, r.end());
        let peers = self.group_peers(track, |g| g.editing);
        let regions = peers.iter().filter_map(|&t| self.project.track(t)).flat_map(|t| t.regions.iter());
        regions.filter(|o| o.takes.is_some() && o.start < b && o.end() > a).map(|o| o.id).collect()
    }

    /// `region` and the folders that follow it, each with its take that matches `take` of
    /// `region` (see `TakeFolder::matching_take`; None where it has none, or for no take).
    fn linked_folders(&self, region: Id, take: Option<Id>) -> Vec<(Id, Option<Id>)> {
        let Some(f) = self.region(region).and_then(|r| r.takes.as_ref()) else { return Vec::new() };
        let mut out = vec![(region, take)];
        for p in self.comp_partners(region) {
            let other = self.region(p).and_then(|r| r.takes.as_ref());
            out.push((p, take.zip(other).and_then(|(t, o)| f.matching_take(t, o))));
        }
        out
    }

    /// Applies a comp list edit (create, duplicate, switch, rename, delete) to `region` and the
    /// folders that follow it.
    fn edit_linked_comps(&mut self, region: Id, f: impl Fn(&mut TakeFolder)) {
        for (r, _) in self.linked_folders(region, None) {
            self.edit_folder(r, |folder, _| f(folder));
        }
    }

    /// Quick Swipe Comping: `take` is heard over timeline `start..end` of the folder (None
    /// silences that part of the comp). A MIDI folder plays whole takes, so it switches to
    /// `take`, as clicking a take does. Grouped folders follow with their matching take (one
    /// without a match is left alone).
    pub fn set_comp_range(&mut self, region: Id, take: Option<Id>, start: i64, end: i64) {
        for (r, t) in self.linked_folders(region, take) {
            if take.is_none() || t.is_some() {
                self.set_comp_range_alone(r, t, start, end);
            }
        }
    }

    /// `set_comp_range` on this folder only.
    fn set_comp_range_alone(&mut self, region: Id, take: Option<Id>, start: i64, end: i64) {
        if self.is_midi_folder(region) {
            if let Some(t) = take {
                self.select_take_alone(region, t);
            }
            return;
        }
        self.edit_folder(region, |f, shift| {
            let (a, b) = (start.min(end) + shift, start.max(end) + shift);
            match take {
                Some(t) => f.select(t, a, b),
                None => f.clear(a, b),
            }
        });
    }

    /// Create New Comp: a fresh comp (the newest take wherever it has audio) is heard. This
    /// and the other comp list edits reach grouped folders too.
    pub fn create_comp(&mut self, region: Id) {
        self.edit_linked_comps(region, |f| f.create_comp());
    }

    /// Duplicate Comp: a copy of the comp heard becomes the one heard.
    pub fn duplicate_comp(&mut self, region: Id) {
        self.edit_linked_comps(region, |f| f.duplicate_comp());
    }

    /// Switches the folder to comp `index` (its take folder menu's comp list).
    pub fn select_comp(&mut self, region: Id, index: usize) {
        self.edit_linked_comps(region, |f| f.switch_comp(index));
    }

    pub fn rename_comp(&mut self, region: Id, index: usize, name: String) {
        self.edit_linked_comps(region, |f| f.rename_comp(index, name.clone()));
    }

    /// Delete Comp (the last one stays).
    pub fn delete_comp(&mut self, region: Id, index: usize) {
        self.edit_linked_comps(region, |f| f.delete_comp(index));
    }

    /// Makes `take` the one heard throughout the folder, and the matching take throughout the
    /// grouped folders that follow it.
    pub fn select_take(&mut self, region: Id, take: Id) {
        for (r, t) in self.linked_folders(region, Some(take)) {
            if let Some(t) = t {
                self.select_take_alone(r, t);
            }
        }
    }

    fn select_take_alone(&mut self, region: Id, take: Id) {
        self.edit_folder(region, |f, _| f.select_whole(take));
    }

    // ---- comp sections ----------------------------------------------------------------------

    /// The comp section of `region` heard at timeline `at` (in take time), and what to add to
    /// a timeline position to get the folder's take time.
    fn comp_section(&self, region: Id, at: i64) -> Option<(CompSegment, i64)> {
        let r = self.region(region)?;
        let shift = take_time_shift(r);
        Some((r.takes.as_ref()?.section_at(at + shift)?, shift))
    }

    /// Drags the comp edge at timeline `at` to `to` (`TakeFolder::move_edge`): a border moves
    /// both sections, an outer edge lengthens or shortens its own. Grouped folders with an
    /// edge there follow.
    pub fn move_comp_edge(&mut self, region: Id, at: i64, to: i64) {
        for (r, _) in self.linked_folders(region, None) {
            if !self.is_midi_folder(r) {
                self.edit_folder(r, |f, shift| f.move_edge(at + shift, to + shift));
            }
        }
    }

    /// Removes the comp section heard at timeline `at`: that stretch falls silent, in grouped
    /// folders too.
    pub fn remove_comp_section(&mut self, region: Id, at: i64) {
        if let Some((s, shift)) = self.comp_section(region, at) {
            self.set_comp_range(region, None, s.start - shift, s.end - shift);
        }
    }

    /// Moves the comp section heard at timeline `at` to the take `delta` rows up (negative) or
    /// down that has audio there (Logic's alternating between lanes); where that take's audio
    /// stops, the section keeps its own take. Grouped folders follow with their matching take.
    pub fn move_comp_section(&mut self, region: Id, at: i64, delta: i64) {
        let Some((s, shift)) = self.comp_section(region, at) else { return };
        let to = self.region(region).and_then(|r| r.takes.as_ref()).and_then(|f| f.neighbour_take(&s, delta));
        if let Some(t) = to {
            self.set_comp_range(region, Some(t), s.start - shift, s.end - shift);
        }
    }

    /// Previous Take (`delta` -1) and Next Take (1): each folder in `regions`, and the grouped
    /// folders that follow it, switches to the take that many places from the one heard. A
    /// folder already moved as another's partner isn't moved again.
    pub fn step_take(&mut self, regions: &[Id], delta: i64) {
        let mut done = Vec::new();
        for &region in regions {
            if done.contains(&region) {
                continue;
            }
            let next = self.region(region).and_then(|r| r.takes.as_ref()).and_then(|f| f.stepped_take(delta));
            for (r, t) in self.linked_folders(region, next) {
                done.push(r);
                if let Some(t) = t {
                    self.select_take_alone(r, t);
                }
            }
        }
    }

    pub fn rename_take(&mut self, region: Id, take: Id, name: String) {
        self.edit_folder(region, |f, _| {
            if let Some(t) = f.take_mut(take) {
                t.name = name;
            }
        });
    }

    /// Removes one take; the comp loses the parts it played.
    pub fn delete_take(&mut self, region: Id, take: Id) {
        self.edit_folder(region, |f, _| {
            if f.takes.len() > 1 {
                f.remove_take(take);
            }
        });
    }

    /// Delete Unused Takes: keeps only the takes the comp uses.
    pub fn delete_unused_takes(&mut self, region: Id) {
        self.edit_folder(region, |f, _| f.remove_unused());
    }

    /// Marks a take good or bad (or neither).
    pub fn set_take_rating(&mut self, region: Id, take: Id, rating: TakeRating) {
        self.edit_folder(region, |f, _| {
            if let Some(t) = f.take_mut(take) {
                t.rating = rating;
            }
        });
    }

    /// A take's own colour (None: the folder's).
    pub fn set_take_color(&mut self, region: Id, take: Id, color: Option<String>) {
        self.edit_folder(region, |f, _| {
            if let Some(t) = f.take_mut(take) {
                t.color = color;
            }
        });
    }

    /// Delete Bad Takes: removes the takes marked bad; the comp loses the parts they played. A
    /// folder of nothing but bad takes keeps its newest.
    pub fn delete_bad_takes(&mut self, region: Id) {
        self.edit_folder(region, |f, _| {
            let bad: Vec<Id> = f.takes.iter().filter(|t| t.rating == TakeRating::Bad).map(|t| t.id).collect();
            for id in bad.iter().take(bad.len().min(f.takes.len().saturating_sub(1))) {
                f.remove_take(*id);
            }
        });
    }
}

/// What to add to a timeline position to get folder `r`'s take time.
fn take_time_shift(r: &Region) -> i64 {
    match r.content {
        RegionContent::Audio { offset, .. } => offset - r.start,
        RegionContent::Midi { .. } => r.takes.as_ref().map_or(0, |f| f.offset) - r.start,
    }
}
