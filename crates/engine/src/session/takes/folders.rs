//! The take folder commands: Flatten, Flatten and Merge, Unpack (to new or existing tracks), Export
//! Active Take and Pack.

use crate::edit::takes::pieces::flat_pieces;
use crate::session::*;

impl Session {
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
        crate::audio_file::decode::write_wav(&path, rate, &audio)?;
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
        crate::edit::takes::midi::save_midi_take(r);
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
                        Region { id, name: t.name.clone(), muted: true, content: RegionContent::midi(notes, cc), takes: None, loop_end: None, ..r.clone() };
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
        crate::edit::takes::midi::save_midi_take(r);
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
            folder.takes.push(Take { id, name: r.name.clone(), clip, offset: offset - at, start: at, end: at + r.length, gain_db, ..Default::default() });
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

    /// Flatten for a MIDI take folder: it becomes a plain region of the take it plays.
    fn flatten_midi_folder(&mut self, region: Id) -> Vec<Id> {
        let Some((track, idx)) = self.find_region(region) else { return Vec::new() };
        self.project.track_mut(track).unwrap().regions[idx].takes = None;
        self.push_regions(track);
        self.dirty = true;
        vec![region]
    }
}
