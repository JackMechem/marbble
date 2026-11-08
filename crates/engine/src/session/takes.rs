//! The session side of take folders and Autopunch: turning a cycle or punch recording into
//! takes, and the take folder edits (Quick Swipe Comping, Flatten, Pack, Unpack and the rest).
//!
//! The comp logic itself is in `crate::takes`. Playback needs nothing here: `regions_dsp`
//! expands each folder into its comp pieces, so bounces and exports hear the comp too.

use super::*;
use crate::takes::{flat_pieces, pass_pieces, RecPlan};

/// Which recorded MIDI is kept and where its region goes: `lo..hi` on the timeline. Notes
/// that start outside the punch range are dropped and none outlasts the punch-out; in cycle
/// mode a note held across the loop point ends at the cycle end.
pub(super) struct MidiWindow {
    pub lo: i64,
    pub hi: i64,
    wrap: i64,
    cut: i64,
    keep: Option<(i64, i64)>,
}

impl MidiWindow {
    pub fn keeps(&self, pos: i64) -> bool {
        self.keep.map(|(a, b)| pos >= a && pos < b).unwrap_or(true)
    }

    /// Where a note that went down at `on` and up at `off` ends.
    pub fn note_end(&self, on: i64, off: i64) -> i64 {
        let end = if off >= on {
            off
        } else if self.wrap < i64::MAX {
            self.wrap
        } else {
            on
        };
        end.min(self.cut)
    }
}

/// The MIDI window of a recording that started at `punch` and stopped at `stop`.
pub(super) fn midi_window(plan: Option<RecPlan>, punch: i64, stop: i64) -> MidiWindow {
    let plain = MidiWindow { lo: punch, hi: stop, wrap: i64::MAX, cut: i64::MAX, keep: None };
    let Some(plan) = plan else { return plain };
    match (plan.cycle, plan.punch) {
        (Some((_, ce)), p) => match plan.cycle_window() {
            Some((w0, w1, _)) => MidiWindow { lo: w0, hi: w1, wrap: ce, cut: if p.is_some() { w1 } else { i64::MAX }, keep: p.map(|_| (w0, w1)) },
            None => plain,
        },
        (None, Some((a, b))) => MidiWindow { lo: punch.max(a), hi: stop.min(b), wrap: i64::MAX, cut: b, keep: Some((a, b)) },
        (None, None) => plain,
    }
}

impl Session {
    /// How a recording starting at `punch` (transport from `from`) is set up: the plan for
    /// cutting it into takes afterwards, the range where record-enabled tracks fall silent
    /// (None: throughout) and the engine positions input is captured between.
    #[allow(clippy::type_complexity)] // Three named in the doc comment, used once at the one call.
    pub(super) fn record_plan(&self, from: i64, punch: i64, cycle: Option<(i64, i64)>) -> Result<(RecPlan, Option<(i64, i64)>, (i64, i64))> {
        let plan = RecPlan {
            from,
            cycle,
            punch: self.project.punch.range(),
            latency: self.record_latency_frames(),
            midi_latency: self.midi_latency_frames(),
            min_take: self.tempo_map().samples_per_beat_at(punch as f64).round() as i64,
        };
        let Some(capture) = plan.capture(punch) else {
            return Err(anyhow!(if cycle.is_some() { "The Autopunch range is outside the cycle" } else { "The playhead is past the punch-out point" }));
        };
        let quiet = match (plan.punch, cycle) {
            (Some(_), Some(_)) => plan.cycle_window().map(|(a, b, _)| (a, b)),
            (p, None) => p,
            (None, Some(_)) => None,
        };
        Ok((plan, quiet, capture))
    }

    /// A finished audio recording, as `finish_recording` made it (`plain`, from the file's
    /// `frames` captured from engine position `start`), cut to what the plan says: cycle passes
    /// become a take folder (or more takes in the folder already there), an autopunch take is
    /// trimmed to the punch range. None when there's nothing new to place. `recording` is the
    /// same for every track recorded at once (see `Take::pass`).
    pub(super) fn shape_take(&mut self, plan: Option<RecPlan>, track: Id, plain: Region, start: i64, frames: i64, recording: Id) -> Option<Region> {
        let Some(plan) = plan else { return Some(plain) };
        let RegionContent::Audio { clip, .. } = plain.content else { return Some(plain) };
        if plan.cycle.is_none() {
            if plan.punch.is_none() {
                return Some(plain);
            }
            let (at, length, offset) = plan.punch_span(start, frames)?;
            return Some(self.trimmed(plain, at, length, offset));
        }
        let (fs, len, takes) = plan.cycle_takes(frames)?;
        // Recording the same cycle again adds takes to the folder there, as in Logic.
        let existing = self.project.track(track)?.regions.iter().position(|r| {
            r.takes.is_some() && r.start == fs && r.length == len && !r.is_looped() && matches!(r.content, RegionContent::Audio { offset: 0, .. })
        });
        if takes.is_empty() || (takes.len() == 1 && existing.is_none()) {
            let &(offset, a, b) = takes.first()?;
            return Some(self.trimmed(plain, fs + a, b - a, offset + a));
        }
        let mut folder = existing.and_then(|i| self.project.track(track).unwrap().regions[i].takes.clone()).unwrap_or_default();
        let numbered = folder.takes.len();
        for (k, (offset, a, b)) in takes.into_iter().enumerate() {
            let id = self.project.alloc_id();
            folder.takes.push(Take {
                id,
                name: format!("Take {}", numbered + k + 1),
                clip,
                offset,
                start: a,
                end: b,
                gain_db: 0.0,
                notes: Vec::new(),
                cc: Vec::new(),
                pass: Some((recording, k as u32)),
            });
        }
        folder.default_comp();
        folder.store_comp();
        if let Some(i) = existing {
            let r = &mut self.project.track_mut(track)?.regions[i];
            if let RegionContent::Audio { clip: c, .. } = &mut r.content {
                *c = clip;
            }
            r.takes = Some(folder);
            return None;
        }
        let fade = self.default_fade(len);
        Some(Region {
            start: fs,
            length: len,
            content: RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
            takes: Some(folder),
            ..plain
        })
    }

    /// `plain` moved to `start..start + length`, from clip frame `offset`, with fresh fades.
    fn trimmed(&self, plain: Region, start: i64, length: i64, offset: i64) -> Region {
        let fade = self.default_fade(length);
        let mut r = Region { start, length, ..plain };
        if let RegionContent::Audio { offset: o, fade_in, fade_out, .. } = &mut r.content {
            *o = offset;
            *fade_in = fade;
            *fade_out = fade;
        }
        r
    }

    /// What a take folder's first pass plays (frames x channels), as `render_region` does for
    /// plain audio: the comp with its crossfades, the folder's gain and fades.
    pub(super) fn render_comp(&self, r: &Region, f: &TakeFolder, channels: usize) -> Vec<Vec<f32>> {
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
    fn edit_folder(&mut self, region: Id, f: impl FnOnce(&mut TakeFolder, i64)) {
        let Some((track, r)) = self.folder_mut(region) else { return };
        crate::takes::save_midi_take(r);
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
        crate::takes::load_midi_take(r);
        self.push_regions(track);
        self.dirty = true;
    }

    /// Whether `region` is a MIDI take folder.
    fn is_midi_folder(&self, region: Id) -> bool {
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

    /// Flatten: replaces the folder with plain regions, one per stretch of its comp (a looping
    /// folder flattens every pass). Returns the new regions.
    pub fn flatten_take_folder(&mut self, region: Id) -> Vec<Id> {
        if self.is_midi_folder(region) {
            return self.flatten_midi_folder(region);
        }
        let Some((track, idx)) = self.find_region(region) else { return Vec::new() };
        let r = self.project.track(track).unwrap().regions[idx].clone();
        let made = self.flat_regions(&r);
        let ids = made.iter().map(|r| r.id).collect();
        let t = self.project.track_mut(track).unwrap();
        t.regions.splice(idx..idx + 1, made);
        self.push_regions(track);
        self.dirty = true;
        ids
    }

    /// The plain regions an audio folder's comp flattens to, one per stretch (every pass of a
    /// looping folder), with ids of their own.
    fn flat_regions(&mut self, r: &Region) -> Vec<Region> {
        let Some(folder) = r.takes.clone() else { return Vec::new() };
        let RegionContent::Audio { gain_db, .. } = r.content else { return Vec::new() };
        let mut made = Vec::new();
        for pass in r.real_copies() {
            let (pass_start, pass_end) = (pass.start, pass.start + pass.length);
            let (fi, fo, shape) = match pass.content {
                RegionContent::Audio { fade_in, fade_out, fade_shape, .. } => (fade_in, fade_out, fade_shape),
                _ => (0, 0, FadeShape::default()),
            };
            for (start, length, take, offset) in flat_pieces(&pass, &folder) {
                let t = folder.take(take).unwrap();
                let small = self.default_fade(length);
                let fade_in = if start == pass_start { fi.min(length) } else { small };
                let fade_out = if start + length == pass_end { fo.min(length - fade_in) } else { small };
                // The folder's fade shapes go with its own fades, on the outer pieces.
                let mut fade_shape = FadeShape::default();
                if start == pass_start {
                    (fade_shape.in_curve, fade_shape.in_type) = (shape.in_curve, shape.in_type);
                }
                if start + length == pass_end {
                    (fade_shape.out_curve, fade_shape.out_type) = (shape.out_curve, shape.out_type);
                }
                let id = self.project.alloc_id();
                made.push(Region {
                    id,
                    name: r.name.clone(),
                    start,
                    length,
                    muted: r.muted,
                    color: r.color.clone(),
                    content: RegionContent::Audio { clip: t.clip, offset, gain_db: gain_db + t.gain_db, fade_in, fade_out, fade_shape },
                    loop_end: None,
                    takes: None,
                    params: Default::default(),
                    pattern: None,
                });
            }
        }
        made
    }

    /// Flatten and Merge: renders the comp into a new audio file, one region in the folder's
    /// place (keeping its gain, fades and loop). Returns it.
    pub fn merge_take_folder(&mut self, region: Id) -> Result<Vec<Id>> {
        if self.is_midi_folder(region) {
            return Ok(self.flatten_midi_folder(region));
        }
        let Some((track, idx)) = self.find_region(region) else { return Ok(Vec::new()) };
        let r = self.project.track(track).unwrap().regions[idx].clone();
        let Some(folder) = r.takes.clone() else { return Ok(Vec::new()) };
        let RegionContent::Audio { offset, gain_db, fade_in, fade_out, fade_shape, .. } = r.content else { return Ok(Vec::new()) };
        // Rendered without the folder's gain and fades, which stay adjustable on the region.
        let plain = Region {
            content: RegionContent::Audio { clip: 0, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() },
            loop_end: None,
            ..r.clone()
        };
        let channels = folder.takes.iter().filter_map(|t| self.clips.get(&t.clip)).map(|c| c.channels.len().min(2)).max().unwrap_or(1);
        let audio = self.render_comp(&plain, &folder, channels);
        let rate = self.sample_rate() as u32;
        let path = self.unique_audio_path(&format!("{} Comp", r.name));
        crate::decode::write_wav(&path, rate, &audio)?;
        let clip = self.add_clip(&path, Clip::from_channels(path.clone(), rate, audio));
        let id = self.project.alloc_id();
        let merged = Region { id, content: RegionContent::Audio { clip, offset: 0, gain_db, fade_in, fade_out, fade_shape }, takes: None, ..r };
        self.project.track_mut(track).unwrap().regions[idx] = merged;
        self.push_regions(track);
        self.dirty = true;
        Ok(vec![id])
    }

    /// Unpack to New Tracks: every take goes onto a new track below (with the same settings),
    /// muted so the song sounds the same, and the folder is flattened in place. Returns the
    /// new tracks.
    pub fn unpack_take_folder(&mut self, region: Id) -> Vec<Id> {
        let Some((track, takes)) = self.unpacked_takes(region) else { return Vec::new() };
        let mut made = Vec::new();
        // Each new track lands right below the folder's, so go newest first to keep the order.
        for (name, take) in takes.into_iter().rev() {
            let Some(&dst) = self.duplicate_tracks(&[track], false).first() else { continue };
            if let Some(d) = self.project.track_mut(dst) {
                d.name = name;
                d.regions.extend(take);
            }
            self.push_regions(dst);
            made.push(dst);
        }
        made.reverse();
        self.flatten_take_folder(region);
        made
    }

    /// Unpack to Existing Tracks: every take goes onto a track below the folder's, one per
    /// track in order (the next tracks of the same kind, skipping hidden and frozen ones),
    /// muted, and the folder is flattened in place. Where there aren't enough tracks, new ones
    /// with the folder track's settings are added below the last. Returns the tracks used.
    pub fn unpack_to_existing_tracks(&mut self, region: Id) -> Vec<Id> {
        let Some((track, takes)) = self.unpacked_takes(region) else { return Vec::new() };
        let Some(pos) = self.project.tracks.iter().position(|t| t.id == track) else { return Vec::new() };
        let kind = self.project.tracks[pos].kind;
        let mut used: Vec<Id> =
            self.project.tracks[pos + 1..].iter().filter(|t| t.kind == kind && !t.hidden && t.frozen.is_none()).map(|t| t.id).take(takes.len()).collect();
        // The tracks still needed, after the last one used and in its stack.
        let anchor = used.last().copied().unwrap_or(track);
        let parent = self.project.track(anchor).and_then(|t| t.parent);
        let mut added = Vec::new();
        for (name, _) in &takes[used.len()..] {
            let Some(&dst) = self.duplicate_tracks(&[track], false).first() else { continue };
            if let Some(d) = self.project.track_mut(dst) {
                d.name = name.clone();
            }
            added.push(dst);
        }
        if !added.is_empty() {
            let mut order: Vec<Id> = self.project.tracks.iter().map(|t| t.id).filter(|id| !added.contains(id)).collect();
            let at = order.iter().position(|&id| id == anchor).map(|i| i + 1 + self.project.descendants(anchor).len()).unwrap_or(order.len());
            order.splice(at..at, added.iter().copied());
            let parents: Vec<(Id, Option<Id>)> = added.iter().map(|&id| (id, parent)).collect();
            self.move_tracks(order, &parents);
            used.extend(added);
        }
        for (&dst, (_, take)) in used.iter().zip(takes) {
            if let Some(d) = self.project.track_mut(dst) {
                d.regions.extend(take);
            }
            self.push_regions(dst);
        }
        self.flatten_take_folder(region);
        used
    }

    /// What Unpack puts on tracks, for the folder `region` and the track it's on: each take
    /// (oldest first) with its name and its part inside the folder as a muted region (none for
    /// an audio take with no audio there).
    #[allow(clippy::type_complexity)] // Named in the doc comment; used by the two Unpacks.
    fn unpacked_takes(&mut self, region: Id) -> Option<(Id, Vec<(String, Option<Region>)>)> {
        let (track, idx) = self.find_region(region)?;
        let r = &mut self.project.track_mut(track)?.regions[idx];
        crate::takes::save_midi_take(r);
        let r = r.clone();
        let folder = r.takes.clone()?;
        let mut out = Vec::new();
        match r.content {
            RegionContent::Audio { offset: tt0, gain_db, .. } => {
                for t in &folder.takes {
                    let (a, b) = (t.start.max(tt0), t.end.min(tt0 + r.length));
                    if a >= b {
                        out.push((t.name.clone(), None));
                        continue;
                    }
                    let id = self.project.alloc_id();
                    let length = b - a;
                    let fade = self.default_fade(length);
                    let take = Region {
                        id,
                        name: t.name.clone(),
                        start: r.start + a - tt0,
                        length,
                        muted: true,
                        color: r.color.clone(),
                        content: RegionContent::Audio {
                            clip: t.clip,
                            offset: t.offset + a,
                            gain_db: gain_db + t.gain_db,
                            fade_in: fade,
                            fade_out: fade,
                            fade_shape: FadeShape::default(),
                        },
                        loop_end: None,
                        takes: None,
                        params: Default::default(),
                        pattern: None,
                    };
                    out.push((t.name.clone(), Some(take)));
                }
            }
            RegionContent::Midi { .. } => {
                for t in &folder.takes {
                    let (notes, cc) = folder.midi_window(t.id, folder.offset, r.length);
                    let id = self.project.alloc_id();
                    let take =
                        Region { id, name: t.name.clone(), muted: true, content: RegionContent::Midi { notes, cc }, takes: None, loop_end: None, ..r.clone() };
                    out.push((t.name.clone(), Some(take)));
                }
            }
        }
        Some((track, out))
    }

    /// Export Active Take to New Track: a copy of what the folder plays (the take heard, or the
    /// comp flattened) on a new track below with the folder track's settings. The copy is
    /// muted so the song sounds the same, and the folder is left as it is. With `remove` (Move
    /// Active Take to New Track) the copy plays instead: the take or comp leaves the folder,
    /// which is muted (or goes, when that was all it had). Returns the new track.
    pub fn export_active_take(&mut self, region: Id, remove: bool) -> Vec<Id> {
        let Some((track, idx)) = self.find_region(region) else { return Vec::new() };
        let r = &mut self.project.track_mut(track).unwrap().regions[idx];
        crate::takes::save_midi_take(r);
        let r = r.clone();
        let Some(folder) = r.takes.clone() else { return Vec::new() };
        let midi = matches!(r.content, RegionContent::Midi { .. });
        // What's heard: a whole take (always, in a MIDI folder) or a comp.
        let whole = if midi { folder.midi_take() } else { folder.whole_take() };
        let name = whole.and_then(|t| folder.take(t)).map(|t| t.name.clone()).unwrap_or_else(|| folder.comp_name());
        let mut copy = if midi {
            let id = self.project.alloc_id();
            vec![Region { id, takes: None, ..r.clone() }]
        } else {
            self.flat_regions(&r)
        };
        for c in &mut copy {
            c.name = name.clone();
            c.muted = !remove || r.muted;
        }
        let Some(&dst) = self.duplicate_tracks(&[track], false).first() else { return Vec::new() };
        if let Some(d) = self.project.track_mut(dst) {
            d.name = name;
            d.regions = copy;
        }
        self.push_regions(dst);
        if remove {
            if whole.is_some() && folder.takes.len() < 2 {
                self.project.track_mut(track).unwrap().regions.retain(|x| x.id != region);
                self.push_regions(track);
            } else {
                self.edit_folder(region, |f, _| match whole {
                    // Another take takes its place: the newest, or the comp made of the rest.
                    Some(t) => {
                        f.remove_take(t);
                        match f.takes.last().map(|t| t.id) {
                            Some(newest) if midi => f.select_whole(newest),
                            Some(_) if f.comp.is_empty() => f.default_comp(),
                            _ => {}
                        }
                    }
                    // The last comp makes way for a fresh one.
                    None if f.comps.len() < 2 => {
                        let old = f.current;
                        f.create_comp();
                        f.delete_comp(old);
                    }
                    None => f.delete_comp(f.current),
                });
                if let Some(x) = self.region_mut(region) {
                    x.muted = true;
                }
                self.push_regions(track);
            }
        }
        self.dirty = true;
        vec![dst]
    }

    /// Pack Take Folder: turns audio regions into a take folder spanning them, one take each.
    /// Regions on several tracks (takes recorded one per track) go into a folder on the topmost
    /// of them, taken in track order, top first, so the take numbers follow the old tracks. The
    /// last take (the newest region on one track, the bottom track's otherwise) starts out
    /// heard. The other tracks keep their regions that weren't packed. Returns the folder.
    pub fn pack_take_folder(&mut self, ids: &[Id]) -> Result<Id> {
        let mut tracks: Vec<Id> = Vec::new();
        let mut picked: Vec<Region> = Vec::new();
        for t in &self.project.tracks {
            for r in t.regions.iter().filter(|r| ids.contains(&r.id)) {
                if !tracks.contains(&t.id) {
                    tracks.push(t.id);
                }
                picked.push(r.clone());
            }
        }
        let Some(&track) = tracks.first() else { return Err(anyhow!("No regions to pack")) };
        if picked.len() < 2 || picked.iter().any(|r| r.takes.is_some() || !matches!(r.content, RegionContent::Audio { .. })) {
            return Err(anyhow!("Select two or more audio regions to pack into a take folder"));
        }
        let fs = picked.iter().map(|r| r.start).min().unwrap();
        let fe = picked.iter().map(|r| r.start + r.length).max().unwrap();
        let mut folder = TakeFolder::new();
        for r in &picked {
            let RegionContent::Audio { clip, offset, gain_db, .. } = r.content else { continue };
            let at = r.start - fs;
            let id = self.project.alloc_id();
            folder.takes.push(Take {
                id,
                name: r.name.clone(),
                clip,
                offset: offset - at,
                start: at,
                end: at + r.length,
                gain_db,
                notes: Vec::new(),
                cc: Vec::new(),
                pass: None,
            });
        }
        folder.default_comp();
        let newest = picked.last().unwrap();
        let clip = folder.takes.last().map(|t| t.clip).unwrap_or(0);
        let id = self.project.alloc_id();
        let fade = self.default_fade(fe - fs);
        let packed = Region {
            id,
            name: newest.name.clone(),
            start: fs,
            length: fe - fs,
            muted: false,
            color: newest.color.clone(),
            content: RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
            loop_end: None,
            takes: Some(folder),
            params: Default::default(),
            pattern: None,
        };
        for &other in &tracks[1..] {
            self.project.track_mut(other).unwrap().regions.retain(|r| !ids.contains(&r.id));
            self.push_regions(other);
        }
        let t = self.project.track_mut(track).unwrap();
        let at = t.regions.iter().position(|r| ids.contains(&r.id)).unwrap_or(t.regions.len());
        t.regions.retain(|r| !ids.contains(&r.id));
        t.regions.insert(at.min(t.regions.len()), packed);
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }
}

/// What to add to a timeline position to get folder `r`'s take time.
fn take_time_shift(r: &Region) -> i64 {
    match r.content {
        RegionContent::Audio { offset, .. } => offset - r.start,
        RegionContent::Midi { .. } => r.takes.as_ref().map_or(0, |f| f.offset) - r.start,
    }
}

/// The notes and controller events of one recorded MIDI pass (`events`, engine positions, in
/// order), relative to the region start `start`: note-ons paired with their note-offs, notes
/// still held ending at `stop` (or as the window says), and only what the window keeps.
pub(super) fn midi_take(events: &[(i64, [u8; 3])], w: &MidiWindow, start: i64, stop: i64) -> (Vec<Note>, Vec<CcEvent>) {
    let mut held: [Option<(i64, u8)>; 128] = [None; 128];
    let mut notes = Vec::new();
    let mut cc = Vec::new();
    for &(pos, d) in events {
        let status = d[0] & 0xF0;
        let pitch = d[1] as usize & 127;
        if let Some((kind, value)) = CcKind::from_message(d) {
            if w.keeps(pos) {
                cc.push(CcEvent { start: pos - start, kind, value });
            }
        } else if status == 0x90 && d[2] > 0 {
            if w.keeps(pos) {
                held[pitch] = Some((pos, d[2]));
            }
        } else if status == 0x80 || status == 0x90 {
            if let Some((on, vel)) = held[pitch].take() {
                notes.push(Note { start: on - start, length: (w.note_end(on, pos) - on).max(1), pitch: pitch as u8, velocity: vel, muted: false });
            }
        }
    }
    for (pitch, h) in held.iter().enumerate() {
        if let Some((on, vel)) = h {
            notes.push(Note { start: on - start, length: (w.note_end(*on, stop) - on).max(1), pitch: pitch as u8, velocity: *vel, muted: false });
        }
    }
    notes.sort_by_key(|n| n.start);
    cc.sort_by_key(|c| c.start);
    (notes, cc)
}

impl Session {
    /// Places a MIDI recording made with "Create take folders" (Settings › Recording › MIDI):
    /// `passes` (each pass's notes and controllers, relative to `start`) over `start..end`.
    /// Recording over a MIDI take folder adds the passes to it as new takes; recording over
    /// plain MIDI regions packs them into a new folder with the passes; otherwise several
    /// passes make a new folder and a single one a plain region. The newest take is heard.
    pub(super) fn place_midi_takes(&mut self, track: Id, start: i64, end: i64, passes: Vec<(Vec<Note>, Vec<CcEvent>)>) {
        let Some(t) = self.project.track(track) else { return };
        let name = t.name.clone();
        let overlaps = |r: &Region| matches!(r.content, RegionContent::Midi { .. }) && !r.is_looped() && r.start < end && r.start + r.length > start;
        let folder_at = t.regions.iter().position(|r| r.takes.is_some() && overlaps(r));
        let packed: Vec<Region> =
            if folder_at.is_some() { Vec::new() } else { t.regions.iter().filter(|r| r.takes.is_none() && overlaps(r)).cloned().collect() };
        if folder_at.is_none() && packed.is_empty() && passes.len() < 2 {
            let Some((notes, cc)) = passes.into_iter().next() else { return };
            let id = self.project.alloc_id();
            let region = Region {
                id,
                name,
                start,
                length: end - start,
                muted: false,
                color: None,
                content: RegionContent::Midi { notes, cc },
                loop_end: None,
                takes: None,
                params: Default::default(),
                pattern: None,
            };
            self.project.track_mut(track).unwrap().regions.push(region);
            return;
        }
        // The folder: the one recorded over, or a new one where the old regions were.
        let mut folder = match folder_at {
            Some(i) => {
                let r = &mut self.project.track_mut(track).unwrap().regions[i];
                crate::takes::save_midi_take(r);
                r.clone()
            }
            None => {
                let id = self.project.alloc_id();
                let content = RegionContent::Midi { notes: Vec::new(), cc: Vec::new() };
                let color = packed.first().and_then(|r| r.color.clone());
                Region {
                    id,
                    name: name.clone(),
                    start,
                    length: end - start,
                    muted: false,
                    color,
                    content,
                    loop_end: None,
                    takes: Some(TakeFolder::new()),
                    params: Default::default(),
                    pattern: None,
                }
            }
        };
        // It covers the recording and everything packed into it.
        let lo = packed.iter().map(|r| r.start).chain([start, folder.start]).min().unwrap();
        let hi = packed.iter().map(|r| r.start + r.length).chain([end, folder.start + folder.length]).max().unwrap();
        let f = folder.takes.as_mut().unwrap();
        // Take time at timeline position p is p + to_tt; the folder never starts at negative take time.
        let mut to_tt = f.offset - folder.start;
        if lo + to_tt < 0 {
            f.shift(-(lo + to_tt));
            to_tt = f.offset - folder.start;
        }
        f.offset = lo + to_tt;
        let add = |f: &mut TakeFolder, id: Id, at: i64, len: i64, notes: &[Note], cc: &[CcEvent]| {
            let name = format!("Take {}", f.takes.len() + 1);
            let tt = at + to_tt;
            let notes = notes.iter().map(|n| Note { start: n.start + tt, ..*n }).collect();
            let cc = cc.iter().map(|c| CcEvent { start: c.start + tt, ..*c }).collect();
            f.takes.push(Take { id, name, clip: 0, offset: 0, start: tt, end: tt + len, gain_db: 0.0, notes, cc, pass: None });
        };
        for r in &packed {
            if let RegionContent::Midi { notes, cc } = &r.content {
                let id = self.project.alloc_id();
                add(f, id, r.start, r.length, notes, cc);
            }
        }
        for (notes, cc) in &passes {
            let id = self.project.alloc_id();
            add(f, id, start, end - start, notes, cc);
        }
        let newest = f.takes.last().map(|t| t.id).unwrap();
        f.select_whole(newest);
        f.store_comp();
        folder.start = lo;
        folder.length = hi - lo;
        crate::takes::load_midi_take(&mut folder);
        let t = self.project.track_mut(track).unwrap();
        match folder_at {
            Some(i) => t.regions[i] = folder,
            None => {
                let at = t.regions.iter().position(|r| packed.iter().any(|p| p.id == r.id)).unwrap_or(t.regions.len());
                t.regions.retain(|r| !packed.iter().any(|p| p.id == r.id));
                t.regions.insert(at.min(t.regions.len()), folder);
            }
        }
    }

    /// Flatten for a MIDI take folder: it becomes a plain region of the take it plays.
    fn flatten_midi_folder(&mut self, region: Id) -> Vec<Id> {
        let Some((track, idx)) = self.find_region(region) else { return Vec::new() };
        self.project.track_mut(track).unwrap().regions[idx].takes = None;
        self.push_regions(track);
        self.dirty = true;
        vec![region]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes_of(s: &Session, region: Id) -> Vec<u8> {
        match &s.region(region).unwrap().content {
            RegionContent::Midi { notes, .. } => notes.iter().map(|n| n.pitch).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn midi_recordings_become_take_folders() {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("marbble-midi-takes-{}", std::process::id()));
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let pass = |pitch: u8, at: i64| (vec![Note { start: at, length: 100, pitch, velocity: 100, muted: false }], Vec::new());
        // One pass over nothing: a plain region.
        s.place_midi_takes(t, 0, 1000, vec![pass(60, 10)]);
        let plain = s.project.track(t).unwrap().regions[0].id;
        assert!(s.region(plain).unwrap().takes.is_none());
        // Two passes over it: the region is packed with them, the newest heard.
        s.place_midi_takes(t, 500, 1500, vec![pass(62, 20), pass(64, 30)]);
        let regions = &s.project.track(t).unwrap().regions;
        assert_eq!(regions.len(), 1);
        let folder = regions[0].id;
        let f = regions[0].takes.clone().unwrap();
        assert_eq!((regions[0].start, regions[0].length, f.takes.len()), (0, 1500, 3));
        assert_eq!(notes_of(&s, folder), vec![64]);
        assert!(matches!(&s.region(folder).unwrap().content, RegionContent::Midi { notes, .. } if notes[0].start == 530));
        // Recording over the folder adds a take.
        s.place_midi_takes(t, 0, 1000, vec![pass(65, 0)]);
        assert_eq!(s.region(folder).unwrap().takes.as_ref().unwrap().takes.len(), 4);
        assert_eq!(notes_of(&s, folder), vec![65]);
        // Choosing a take plays it, whole; a swipe on a MIDI folder does the same.
        let ids: Vec<Id> = s.region(folder).unwrap().takes.as_ref().unwrap().takes.iter().map(|k| k.id).collect();
        s.select_take(folder, ids[0]);
        assert_eq!(notes_of(&s, folder), vec![60]);
        s.set_comp_range(folder, Some(ids[1]), 600, 700);
        assert_eq!(notes_of(&s, folder), vec![62]);
        // Edits to the content belong to the take heard.
        if let RegionContent::Midi { notes, .. } = &mut s.region_mut(folder).unwrap().content {
            notes[0].pitch = 70;
        }
        s.select_take(folder, ids[0]);
        s.select_take(folder, ids[1]);
        assert_eq!(notes_of(&s, folder), vec![70]);
        // Unpack: a muted track per take, the folder flattened to the take heard.
        let tracks = s.unpack_take_folder(folder);
        assert_eq!(tracks.len(), 4);
        assert!(s.region(folder).unwrap().takes.is_none());
        assert_eq!(notes_of(&s, folder), vec![70]);
        let first = &s.project.track(tracks[0]).unwrap().regions[0];
        assert!(first.muted && matches!(&first.content, RegionContent::Midi { notes, .. } if notes[0].pitch == 60));
    }

    /// A session on the null device with a 10 s audio clip, and `tracks` audio tracks.
    fn audio_session(tracks: usize) -> (Session, Vec<Id>, Id) {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("marbble-audio-takes-{}", std::process::id()));
        let ids = (0..tracks).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
        let path = s.dir.join("Audio Files").join("ramp.wav");
        let ramp: Vec<f32> = (0..480_000).map(|i| i as f32 / 480_000.0).collect();
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![ramp]));
        (s, ids, clip)
    }

    fn add_audio(s: &mut Session, track: Id, clip: Id, name: &str, start: i64, length: i64, offset: i64) -> Id {
        let id = s.project.alloc_id();
        let content = RegionContent::Audio { clip, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() };
        let r = Region {
            id,
            name: name.into(),
            start,
            length,
            muted: false,
            color: None,
            content,
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        };
        s.project.track_mut(track).unwrap().regions.push(r);
        s.push_regions(track);
        id
    }

    #[test]
    fn pack_gathers_takes_from_several_tracks() {
        let (mut s, t, clip) = audio_session(4);
        // One take per track, the bottom one recorded first on its track's timeline; the third
        // track's region stays where it is.
        let a = add_audio(&mut s, t[0], clip, "Overhead 1", 48_000, 96_000, 0);
        let b = add_audio(&mut s, t[1], clip, "Overhead 2", 24_000, 96_000, 1_000);
        let keep = add_audio(&mut s, t[1], clip, "Fill", 400_000, 10_000, 0);
        let c = add_audio(&mut s, t[3], clip, "Overhead 3", 48_000, 120_000, 2_000);
        let other = add_audio(&mut s, t[2], clip, "Kick", 0, 10_000, 0);
        s.checkpoint("Pack Take Folder", None, false);
        // The order of the ids doesn't matter: tracks go top first.
        let folder = s.pack_take_folder(&[c, a, b]).unwrap();
        let (track, _) = s.find_region(folder).unwrap();
        assert_eq!(track, t[0], "the folder is on the topmost track");
        let r = s.region(folder).unwrap().clone();
        assert_eq!((r.start, r.length), (24_000, 168_000 - 24_000));
        let f = r.takes.clone().unwrap();
        let names: Vec<&str> = f.takes.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["Overhead 1", "Overhead 2", "Overhead 3"]);
        // Each take keeps its own place: take time 0 is the folder start.
        let spans: Vec<(i64, i64, i64)> = f.takes.iter().map(|k| (k.start, k.end, k.offset)).collect();
        assert_eq!(spans, [(24_000, 120_000, -24_000), (0, 96_000, 1_000), (24_000, 144_000, 2_000 - 24_000)]);
        assert_eq!(f.whole_take(), None, "the last take is heard, the others fill in before it");
        assert_eq!(f.comp.last().unwrap().take, f.takes[2].id);
        // The other tracks lose only what was packed.
        assert_eq!(s.project.track(t[1]).unwrap().regions.iter().map(|r| r.id).collect::<Vec<_>>(), [keep]);
        assert!(s.project.track(t[3]).unwrap().regions.is_empty());
        assert_eq!(s.project.track(t[2]).unwrap().regions[0].id, other);
        // One undo step puts every region back on its own track.
        s.undo();
        for (track, id) in [(t[0], a), (t[1], b), (t[3], c)] {
            assert_eq!(s.find_region(id).map(|(tr, _)| tr), Some(track));
        }
        assert!(s.region(folder).is_none());
        // MIDI mixed in is refused, wherever it is.
        let m = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        s.place_midi_takes(m, 0, 1000, vec![(Vec::new(), Vec::new())]);
        let midi = s.project.track(m).unwrap().regions[0].id;
        assert!(s.pack_take_folder(&[a, midi]).is_err());
        assert!(s.pack_take_folder(&[a]).is_err());
    }

    fn names(s: &Session) -> Vec<String> {
        s.project.tracks.iter().map(|t| t.name.clone()).collect()
    }

    #[test]
    fn unpack_to_existing_tracks_fills_the_tracks_below() {
        let (mut s, t, clip) = audio_session(3);
        for (k, name) in ["Folder", "Below 1", "Below 2"].iter().enumerate() {
            s.project.track_mut(t[k]).unwrap().name = name.to_string();
        }
        // An instrument track between the two audio tracks is passed over.
        let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, Some(2)).unwrap();
        s.project.track_mut(inst).unwrap().name = "Keys".into();
        let below = add_audio(&mut s, t[1], clip, "Already here", 200_000, 1_000, 0);
        let ids: Vec<Id> = (0..4).map(|k| add_audio(&mut s, t[0], clip, &format!("Take {}", k + 1), 1_000, 10_000, k * 20_000)).collect();
        let folder = s.pack_take_folder(&ids).unwrap();
        s.checkpoint("Unpack Take Folder", None, false);
        let used = s.unpack_to_existing_tracks(folder);
        assert_eq!(used.len(), 4);
        assert_eq!((used[0], used[1]), (t[1], t[2]));
        // Two more tracks, like the folder's, below the last one used.
        assert_eq!(names(&s), ["Folder", "Below 1", "Keys", "Below 2", "Take 3", "Take 4"]);
        for (k, &track) in used.iter().enumerate() {
            let take = s.project.track(track).unwrap().regions.iter().find(|r| r.name == format!("Take {}", k + 1)).unwrap().clone();
            assert!(take.muted);
            assert!(matches!(take.content, RegionContent::Audio { offset, .. } if offset == k as i64 * 20_000));
            assert_eq!((take.start, take.length), (1_000, 10_000));
        }
        // The tracks keep what they had; the folder is flattened to the take heard.
        assert!(s.region(below).is_some());
        let flat = &s.project.track(t[0]).unwrap().regions;
        assert!(flat.len() == 1 && flat[0].takes.is_none() && !flat[0].muted);
        s.undo();
        assert_eq!(names(&s), ["Folder", "Below 1", "Keys", "Below 2"]);
        assert!(s.region(folder).unwrap().takes.is_some());
        assert_eq!(s.project.track(t[1]).unwrap().regions.len(), 1);
    }

    #[test]
    fn export_and_move_the_active_take() {
        let (mut s, t, clip) = audio_session(2);
        s.project.track_mut(t[0]).unwrap().name = "Vocal".into();
        let a = add_audio(&mut s, t[0], clip, "A", 0, 1_000, 0);
        let b = add_audio(&mut s, t[0], clip, "B", 0, 1_000, 5_000);
        let folder = s.pack_take_folder(&[a, b]).unwrap();
        let takes: Vec<Id> = s.region(folder).unwrap().takes.as_ref().unwrap().takes.iter().map(|k| k.id).collect();
        let region_on = |s: &Session, track: Id| s.project.track(track).unwrap().regions.clone();
        // Export: a muted copy of take B on a new track below, the folder as it was.
        s.checkpoint("Export Active Take", None, false);
        let made = s.export_active_take(folder, false);
        assert_eq!(names(&s), ["Vocal", "B", "Audio 2"]);
        let copy = region_on(&s, made[0]);
        assert_eq!(copy.len(), 1);
        assert!(copy[0].muted && copy[0].takes.is_none() && copy[0].name == "B");
        assert!(matches!(copy[0].content, RegionContent::Audio { offset: 5_000, clip: c, .. } if c == clip));
        assert_eq!(s.region(folder).unwrap().takes.as_ref().unwrap().takes.len(), 2);
        s.undo();
        // Move: the copy plays, the folder loses take B, plays A, and is muted.
        s.checkpoint("Move Active Take", None, false);
        let made = s.export_active_take(folder, true);
        assert!(!region_on(&s, made[0])[0].muted);
        let f = s.region(folder).unwrap().takes.clone().unwrap();
        assert_eq!((f.takes.len(), f.whole_take()), (1, Some(takes[0])));
        assert!(s.region(folder).unwrap().muted);
        // Moving the only take left moves the folder out altogether.
        s.export_active_take(folder, true);
        assert!(s.region(folder).is_none());
        assert_eq!(names(&s), ["Vocal", "A", "B", "Audio 2"]);
        s.undo();
        assert_eq!(names(&s), ["Vocal", "Audio 2"]);
        // A comp is exported flattened and named after the comp; moving it leaves a fresh one.
        s.set_comp_range(folder, Some(takes[0]), 0, 500);
        let made = s.export_active_take(folder, false);
        let copy = region_on(&s, made[0]);
        assert_eq!(copy.iter().map(|r| (r.start, r.length, r.name.as_str())).collect::<Vec<_>>(), [(0, 500, "Comp 1"), (500, 500, "Comp 1")]);
        s.export_active_take(folder, true);
        let f = s.region(folder).unwrap().takes.clone().unwrap();
        assert_eq!((f.takes.len(), f.comp_names().len(), f.whole_take()), (2, 1, Some(takes[1])));
    }

    #[test]
    fn export_a_midi_take() {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let pass = |pitch: u8| (vec![Note { start: 10, length: 100, pitch, velocity: 100, muted: false }], Vec::new());
        s.place_midi_takes(t, 0, 1000, vec![pass(60), pass(62)]);
        let folder = s.project.track(t).unwrap().regions[0].id;
        let made = s.export_active_take(folder, false);
        let copy = s.project.track(made[0]).unwrap().regions[0].clone();
        assert!(copy.takes.is_none() && copy.muted);
        assert_eq!(notes_of(&s, copy.id), vec![62]);
        // Unpack to existing tracks: the exported track takes the first take, a new one the second.
        let used = s.unpack_to_existing_tracks(folder);
        assert_eq!(used[0], made[0]);
        assert_eq!(s.project.track(used[1]).unwrap().regions.len(), 1);
        assert_eq!(notes_of(&s, s.project.track(used[1]).unwrap().regions[0].id), vec![62]);
        assert_eq!(notes_of(&s, s.project.track(used[0]).unwrap().regions[1].id), vec![60]);
    }

    #[test]
    fn midi_window_follows_cycle_and_punch() {
        let plan = RecPlan { from: 0, cycle: Some((1000, 2000)), punch: None, latency: 0, midi_latency: 0, min_take: 1 };
        let w = midi_window(Some(plan), 1000, 1500);
        assert_eq!((w.lo, w.hi), (1000, 2000));
        // Held across the loop point: ends at the cycle end.
        assert_eq!(w.note_end(1900, 1100), 2000);
        assert_eq!(w.note_end(1200, 1300), 1300);
        let punched = midi_window(Some(RecPlan { cycle: None, punch: Some((1200, 1600)), ..plan }), 0, 3000);
        assert_eq!((punched.lo, punched.hi), (1200, 1600));
        assert!(!punched.keeps(1100) && punched.keeps(1200) && !punched.keeps(1600));
        assert_eq!(punched.note_end(1500, 1900), 1600);
        let plain = midi_window(None, 500, 900);
        assert_eq!((plain.lo, plain.hi, plain.note_end(600, 700)), (500, 900, 700));
    }
}
