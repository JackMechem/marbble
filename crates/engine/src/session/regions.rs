//! Region edits: move, resize, split, join, loop, paste, gain, fades, transpose, quantize, reverse,
//! and importing audio files.
//!
//! Each method changes `self.project` and pushes the track's regions to the engine; the Tauri
//! command around it takes the undo checkpoint. More specific edits have modules of their own, among
//! them `region_moves.rs`, `range_edit.rs`, `crossfades.rs`, `midi_content.rs` and `arrange_cmds.rs`.

use super::*;

impl Session {
    pub(super) fn find_region(&self, id: Id) -> Option<(Id, usize)> {
        self.project.tracks.iter().find_map(|t| t.regions.iter().position(|r| r.id == id).map(|i| (t.id, i)))
    }

    /// Moves a region in time and optionally onto another track of the same kind.
    pub fn move_region(&mut self, id: Id, start: i64, track: Option<Id>) {
        let Some((from, idx)) = self.find_region(id) else { return };
        let pairs = self.crossfade_pairs(&[id]);
        let to = track.unwrap_or(from);
        let kind_ok = match (self.project.track(from), self.project.track(to)) {
            (Some(a), Some(b)) => a.kind == b.kind,
            _ => false,
        };
        let to = if kind_ok { to } else { from };
        // Moved regions come to the front, so they win where they now overlap.
        let mut r = self.project.track_mut(from).unwrap().regions.remove(idx);
        r.move_to(start.max(0));
        self.project.track_mut(to).unwrap().regions.push(r);
        self.push_regions(from);
        if to != from {
            self.push_regions(to);
        }
        self.dirty = true;
        self.auto_crossfade(&[id], pairs);
    }

    /// Trims a region's edges. `start`/`length` are timeline samples. A loop keeps its end and
    /// repeats the new window; it goes when the region now reaches past it.
    pub fn resize_region(&mut self, id: Id, start: i64, length: i64) {
        let Some((track, idx)) = self.find_region(id) else { return };
        let pairs = self.crossfade_pairs(&[id]);
        let clips = &self.clips;
        let t = self.project.track_mut(track).unwrap();
        let r = &mut t.regions[idx];
        let delta = start - r.start;
        let mut start = start;
        let mut length = length.max(1);
        if let RegionContent::Audio { clip, offset, .. } = &mut r.content {
            // A take folder's offset is in take time, which runs as far as its takes do.
            let frames = match &r.takes {
                Some(f) => f.takes.iter().map(|t| t.end).max().unwrap_or(1),
                None => clips.get(clip).map(|c| c.frames() as i64).unwrap_or(i64::MAX),
            };
            let new_off = (*offset + delta).clamp(0, frames - 1);
            start = r.start + (new_off - *offset);
            length = length.min(frames - new_off);
            *offset = new_off;
        } else if r.takes.is_some() {
            // A MIDI take folder keeps its takes whole: it shows the take's notes in its new span.
            crate::takes::retrim_midi_folder(r, start.max(0), length);
        } else if let RegionContent::Midi { notes, cc } = &mut r.content {
            crate::midi::trim_front(notes, cc, delta);
        }
        r.start = start.max(0);
        r.length = length;
        if !r.is_looped() {
            r.loop_end = None;
        }
        self.push_regions(track);
        self.dirty = true;
        self.auto_crossfade(&[id], pairs);
    }

    pub fn delete_regions(&mut self, ids: &[Id]) {
        let mut touched = Vec::new();
        for t in &mut self.project.tracks {
            let before = t.regions.len();
            t.regions.retain(|r| !ids.contains(&r.id));
            if t.regions.len() != before {
                touched.push(t.id);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    pub fn set_region_muted(&mut self, id: Id, muted: bool) {
        let Some((track, idx)) = self.find_region(id) else { return };
        self.project.track_mut(track).unwrap().regions[idx].muted = muted;
        self.push_regions(track);
        self.dirty = true;
    }

    pub fn rename_region(&mut self, id: Id, name: String) {
        if let Some((track, idx)) = self.find_region(id) {
            self.project.track_mut(track).unwrap().regions[idx].name = name;
            self.dirty = true;
        }
    }

    /// Splits the given regions (every region if empty) at `pos`. A loop that `pos` falls in is
    /// turned into real copies first, as Logic does.
    pub fn split_at(&mut self, pos: i64, region_ids: &[Id]) {
        let looped: Vec<Id> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.regions.iter())
            .filter(|r| (region_ids.is_empty() || region_ids.contains(&r.id)) && r.is_looped() && pos > r.start && pos < r.end())
            .map(|r| r.id)
            .collect();
        let mut picked = region_ids.to_vec();
        if !looped.is_empty() {
            let copies = self.convert_loops(&looped);
            if !picked.is_empty() {
                picked.extend(copies);
            }
        }
        let region_ids = &picked[..];
        let mut targets = Vec::new();
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for (ri, r) in t.regions.iter().enumerate() {
                let picked = region_ids.is_empty() || region_ids.contains(&r.id);
                if picked && pos > r.start && pos < r.start + r.length {
                    // Snap Edits to Zero Crossings cuts each region at its own crossing.
                    targets.push((ti, ri, self.zero_cut(r, pos)));
                }
            }
        }
        let mut touched = Vec::new();
        for (ti, ri, pos) in targets {
            let new_id = self.project.alloc_id();
            let t = &mut self.project.tracks[ti];
            let r = &mut t.regions[ri];
            let cut = pos - r.start;
            let mut right = r.clone();
            right.id = new_id;
            right.start = pos;
            right.length = r.length - cut;
            match (&mut right.content, &mut r.content) {
                (RegionContent::Audio { offset, .. }, _) => *offset += cut,
                (RegionContent::Midi { notes, cc }, RegionContent::Midi { notes: left, cc: left_cc }) => {
                    let ((ln, lc), (rn, rc)) = crate::midi::split(notes, cc, cut);
                    (*left, *left_cc, *notes, *cc) = (ln, lc, rn, rc);
                    // The right half of a MIDI take folder starts later in its takes.
                    if let Some(f) = &mut right.takes {
                        f.offset += cut;
                    }
                }
                _ => {}
            }
            r.length = cut;
            t.regions.push(right);
            touched.push(t.id);
        }
        touched.dedup();
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    pub(super) fn region_mut(&mut self, id: Id) -> Option<&mut Region> {
        let (track, idx) = self.find_region(id)?;
        self.project.track_mut(track).map(|t| &mut t.regions[idx])
    }

    /// Applies `f` to each region and refreshes the tracks they're on.
    pub(super) fn edit_regions(&mut self, ids: &[Id], mut f: impl FnMut(&mut Region, &HashMap<Id, Arc<Clip>>)) {
        let mut touched = Vec::new();
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let clips = &self.clips;
            let t = self.project.track_mut(track).unwrap();
            f(&mut t.regions[idx], clips);
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    pub(super) fn region(&self, id: Id) -> Option<&Region> {
        let (track, idx) = self.find_region(id)?;
        self.project.track(track).map(|t| &t.regions[idx])
    }

    /// Loops a region up to `loop_end` (a timeline sample), or stops it looping when that is
    /// None or doesn't reach past the region's end (dragging the loop back to the region).
    pub fn set_region_loop(&mut self, id: Id, loop_end: Option<i64>) {
        self.edit_regions(&[id], |r, _| {
            r.loop_end = loop_end;
            if !r.is_looped() {
                r.loop_end = None;
            }
        });
    }

    /// Logic's Loop switch. On, each region loops up to the next region on its track, or for
    /// four times its length when nothing follows it; off removes the loop.
    pub fn set_regions_looped(&mut self, ids: &[Id], on: bool) {
        let mut ends = Vec::new();
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let t = self.project.track(track).unwrap();
            let r = &t.regions[idx];
            let own = r.start + r.length;
            let next = t.regions.iter().filter(|o| o.id != id && o.start >= own).map(|o| o.start).min();
            ends.push((id, on.then(|| next.unwrap_or(r.start + 4 * r.length))));
        }
        for (id, end) in ends {
            self.set_region_loop(id, end);
        }
    }

    /// Convert Loops to Real Copies: each looping region becomes one plain region per pass.
    /// The original stays as the first pass; the copies go to the front of the track's list,
    /// beneath every other region, which is where the loop played. Returns the new copies.
    pub fn convert_loops(&mut self, ids: &[Id]) -> Vec<Id> {
        let mut made = Vec::new();
        let mut touched = Vec::new();
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let r = self.project.track(track).unwrap().regions[idx].clone();
            if !r.is_looped() {
                continue;
            }
            let mut copies = r.real_copies();
            let first = copies.remove(0);
            for c in &mut copies {
                c.id = self.project.alloc_id();
                made.push(c.id);
            }
            let t = self.project.track_mut(track).unwrap();
            t.regions[idx] = first;
            t.regions.splice(0..0, copies);
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        made
    }

    /// Whether `r` can go on `track`: its kind, and for audio a clip that's loaded.
    pub(super) fn region_fits(&self, track: Id, r: &Region) -> bool {
        match (self.project.track(track).map(|t| t.kind), &r.content) {
            (Some(TrackKind::Audio), RegionContent::Audio { clip, .. }) => self.clips.contains_key(clip),
            (Some(TrackKind::Instrument), RegionContent::Midi { .. }) => true,
            _ => false,
        }
    }

    /// Places copies of `regions` (each with the track it goes on), shifted by `offset`.
    /// Regions that don't fit their track's kind are skipped. Returns the new ids.
    pub fn paste_regions(&mut self, items: &[(Id, Region)], offset: i64) -> Vec<Id> {
        let mut made = Vec::new();
        let mut touched = Vec::new();
        for (track, r) in items {
            if !self.region_fits(*track, r) {
                continue;
            }
            let id = self.project.alloc_id();
            let mut copy = Region { id, ..r.clone() };
            copy.move_to((r.start + offset).max(0));
            self.project.track_mut(*track).unwrap().regions.push(copy);
            made.push(id);
            if !touched.contains(track) {
                touched.push(*track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        self.auto_crossfade(&made, Vec::new());
        made
    }

    /// Repeats the selection `times` times straight after itself, as Logic's Repeat Regions.
    pub fn repeat_regions(&mut self, ids: &[Id], times: usize) -> Vec<Id> {
        let items: Vec<(Id, Region)> = ids.iter().filter_map(|id| self.find_region(*id)).map(|(t, i)| (t, self.project.track(t).unwrap().regions[i].clone())).collect();
        let (Some(lo), Some(hi)) = (items.iter().map(|(_, r)| r.start).min(), items.iter().map(|(_, r)| r.end()).max()) else { return Vec::new() };
        let mut made = Vec::new();
        for k in 1..=times.clamp(1, 256) as i64 {
            made.extend(self.paste_regions(&items, (hi - lo) * k));
        }
        made
    }

    /// Sets (`db`) or changes (`delta`) region gain.
    pub fn set_region_gain(&mut self, ids: &[Id], db: Option<f32>, delta: Option<f32>) {
        self.edit_regions(ids, |r, _| {
            if let RegionContent::Audio { gain_db, .. } = &mut r.content {
                let v = db.unwrap_or(*gain_db) + delta.unwrap_or(0.0);
                *gain_db = v.clamp(-96.0, 30.0);
            }
        });
    }

    /// Sets each audio region's gain so its loudest sample reaches 0 dBFS.
    pub fn normalize_regions(&mut self, ids: &[Id]) {
        self.edit_regions(ids, |r, clips| {
            let len = r.length;
            if let RegionContent::Audio { clip, offset, gain_db, .. } = &mut r.content {
                let Some(c) = clips.get(clip) else { return };
                let (a, b) = ((*offset).max(0) as usize, ((*offset + len).max(0) as usize).min(c.frames()));
                let peak = c.channels.iter().flat_map(|ch| ch[a.min(b)..b].iter()).fold(0.0f32, |m, s| m.max(s.abs()));
                if peak > 1e-6 {
                    *gain_db = (-20.0 * peak.log10()).clamp(-96.0, 30.0);
                }
            }
        });
    }

    /// Sets fade lengths (frames); None leaves that end alone.
    pub fn set_region_fades(&mut self, ids: &[Id], fade_in: Option<i64>, fade_out: Option<i64>) {
        self.edit_regions(ids, |r, _| {
            let len = r.length;
            if let RegionContent::Audio { fade_in: fi, fade_out: fo, .. } = &mut r.content {
                if let Some(v) = fade_in {
                    *fi = v.clamp(0, len);
                }
                if let Some(v) = fade_out {
                    *fo = v.clamp(0, len);
                }
                // The two can't overlap.
                if *fi + *fo > len {
                    if fade_in.is_some() {
                        *fo = len - *fi;
                    } else {
                        *fi = len - *fo;
                    }
                }
            }
        });
    }

    pub fn set_region_color(&mut self, ids: &[Id], color: Option<String>) {
        self.edit_regions(ids, |r, _| r.color = color.clone());
    }

    pub fn transpose_regions(&mut self, ids: &[Id], semitones: i32) {
        self.edit_regions(ids, |r, _| {
            if let RegionContent::Midi { notes, .. } = &mut r.content {
                for n in notes.iter_mut() {
                    n.pitch = (n.pitch as i32 + semitones).clamp(0, 127) as u8;
                }
            }
        });
    }

    /// Moves note starts to the nearest `grid` step on the timeline.
    /// Quantizes note starts to `grid` samples at the base tempo (a note value), counted in
    /// musical time from each bar line, so the grid follows tempo and signature changes.
    pub fn quantize_regions(&mut self, ids: &[Id], grid: i64) {
        if grid <= 0 {
            return;
        }
        let map = self.tempo_map();
        let mut step = grid as f64 / (60.0 * self.project.sample_rate as f64 / self.project.tempo);
        // The grid arrives rounded to whole samples; use the note value it stands for.
        let per_whole = 4.0 / step;
        if (per_whole - per_whole.round()).abs() < 0.01 * per_whole {
            step = 4.0 / per_whole.round();
        }
        self.edit_regions(ids, |r, _| {
            let rs = r.start;
            if let RegionContent::Midi { notes, .. } = &mut r.content {
                for n in notes.iter_mut() {
                    let at = (rs + n.start) as f64;
                    let q = map.sample_at(map.snap_q(map.beats_at(at), step)).round() as i64;
                    n.start = (q - rs).max(0);
                }
                notes.sort_by_key(|n| n.start);
            }
        });
    }

    /// An empty MIDI region on an instrument track.
    pub fn create_midi_region(&mut self, track: Id, start: i64, length: i64) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        match self.project.track(track) {
            Some(t) if t.kind == TrackKind::Instrument => {}
            _ => return Err(anyhow!("MIDI regions go on software instrument tracks")),
        }
        let id = self.project.alloc_id();
        let name = self.project.track(track).unwrap().name.clone();
        self.project.track_mut(track).unwrap().regions.push(Region { id, name, start: start.max(0), length: length.max(1), muted: false, color: None, content: RegionContent::Midi { notes: Vec::new(), cc: Vec::new() }, loop_end: None, takes: None, params: Default::default(), pattern: None });
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }

    /// Moves regions onto new tracks with their tracks' settings (one per source track).
    pub fn regions_to_new_track(&mut self, ids: &[Id]) -> Vec<Id> {
        let mut by_track: Vec<(Id, Vec<Id>)> = Vec::new();
        for &id in ids {
            let Some((t, _)) = self.find_region(id) else { continue };
            match by_track.iter_mut().find(|(x, _)| *x == t) {
                Some((_, v)) => v.push(id),
                None => by_track.push((t, vec![id])),
            }
        }
        let mut made = Vec::new();
        for (src, regs) in by_track {
            let Some(&dst) = self.duplicate_tracks(&[src], false).first() else { continue };
            for id in regs {
                let start = self.find_region(id).map(|(t, i)| self.project.track(t).unwrap().regions[i].start).unwrap_or(0);
                self.move_region(id, start, Some(dst));
            }
            made.push(dst);
        }
        made
    }

    /// The audio a region plays, rendered with its gain and fades (frames x channels).
    pub(super) fn render_region(&self, r: &Region, channels: usize) -> Vec<Vec<f32>> {
        if let Some(f) = &r.takes {
            return self.render_comp(r, f, channels);
        }
        let mut out = vec![vec![0.0f32; r.length.max(0) as usize]; channels];
        let RegionContent::Audio { clip, offset, gain_db, fade_in, fade_out, fade_shape } = &r.content else { return out };
        let Some(c) = self.clips.get(clip) else { return out };
        let g = volume_gain(*gain_db);
        let (fi, fo) = (*fade_in, *fade_out);
        let (law_in, law_out) = (FadeLaw::Curve(fade_shape.in_curve), FadeLaw::Curve(fade_shape.out_curve));
        for i in 0..r.length.max(0) {
            let src = offset + i;
            if src < 0 || src as usize >= c.frames() {
                continue;
            }
            let mut gain = g;
            if fi > 0 && i < fi {
                gain *= law_in.gain(i as f32 / fi as f32);
            }
            if fo > 0 && i >= r.length - fo {
                gain *= law_out.gain((r.length - i) as f32 / fo as f32);
            }
            for (ch, o) in out.iter_mut().enumerate() {
                let s = c.channels[ch.min(c.channels.len() - 1)][src as usize];
                o[i as usize] = s * gain;
            }
        }
        out
    }

    fn region_channels(&self, r: &Region) -> usize {
        match &r.content {
            RegionContent::Audio { clip, .. } => self.clips.get(clip).map(|c| c.channels.len().min(2)).unwrap_or(1),
            _ => 0,
        }
    }

    /// Replaces each audio region with a reversed copy of the audio it plays.
    pub fn reverse_regions(&mut self, ids: &[Id]) -> Result<()> {
        let rate = self.sample_rate() as u32;
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let r = self.project.track(track).unwrap().regions[idx].clone();
            let RegionContent::Audio { clip, offset, gain_db, fade_in, fade_out, fade_shape } = r.content else { continue };
            // Render without gain and fades, which stay adjustable on the new region.
            let plain = Region { content: RegionContent::Audio { clip, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() }, ..r.clone() };
            let mut audio = self.render_region(&plain, self.region_channels(&r));
            audio.iter_mut().for_each(|c| c.reverse());
            let path = self.unique_audio_path(&format!("{} Reversed", r.name));
            crate::decode::write_wav(&path, rate, &audio)?;
            let clip = self.add_clip(&path, Clip::from_channels(path.clone(), rate, audio));
            if let Some(reg) = self.region_mut(id) {
                reg.content = RegionContent::Audio { clip, offset: 0, gain_db, fade_in: fade_out, fade_out: fade_in, fade_shape: fade_shape.reversed() };
                // A reversed take folder is plain audio.
                reg.takes = None;
            }
            self.push_regions(track);
        }
        self.dirty = true;
        Ok(())
    }

    /// Joins the selected regions on each track into one. MIDI notes are merged; audio is
    /// mixed into a new file, as it plays (later regions win where they overlap). Returns the
    /// new regions.
    pub fn join_regions(&mut self, ids: &[Id]) -> Result<Vec<Id>> {
        // Loops join as the copies they play.
        let looped: Vec<Id> = ids.iter().copied().filter(|id| self.region(*id).map(|r| r.is_looped()).unwrap_or(false)).collect();
        let mut ids = ids.to_vec();
        ids.extend(self.convert_loops(&looped));
        let ids = &ids[..];
        let rate = self.sample_rate() as u32;
        let map = self.tempo_map();
        let mut made = Vec::new();
        let tracks: Vec<Id> = self.project.tracks.iter().map(|t| t.id).collect();
        for track in tracks {
            let t = self.project.track(track).unwrap();
            // Joined as they play, with their playback parameters applied.
            let picked: Vec<Region> = t.regions.iter().filter(|r| ids.contains(&r.id)).map(|r| crate::region_params::bake(r, &map)).collect();
            if picked.len() < 2 {
                continue;
            }
            let lo = picked.iter().map(|r| r.start).min().unwrap();
            let hi = picked.iter().map(|r| r.end()).max().unwrap();
            let name = picked.iter().min_by_key(|r| r.start).unwrap().name.clone();
            let content = if t.kind == TrackKind::Instrument {
                let parts = picked.iter().filter_map(|r| match &r.content {
                    RegionContent::Midi { notes, cc } => Some((r.start - lo, notes.as_slice(), cc.as_slice())),
                    _ => None,
                });
                let (notes, cc) = crate::midi::join(parts);
                RegionContent::Midi { notes, cc }
            } else {
                let ch = picked.iter().map(|r| self.region_channels(r)).max().unwrap_or(1).max(1);
                let mix = self.mix_regions(t, &picked, lo, hi, ch);
                let path = self.unique_audio_path(&format!("{name} Joined"));
                crate::decode::write_wav(&path, rate, &mix)?;
                let clip = self.add_clip(&path, Clip::from_channels(path.clone(), rate, mix));
                RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() }
            };
            let id = self.project.alloc_id();
            let color = picked[0].color.clone();
            let t = self.project.track_mut(track).unwrap();
            t.regions.retain(|r| !ids.contains(&r.id));
            t.regions.push(Region { id, name, start: lo, length: hi - lo, muted: false, color, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
            self.push_regions(track);
            made.push(id);
        }
        self.dirty = true;
        Ok(made)
    }

    /// Decodes a file for the project. A WAV already at the project rate is referenced where it
    /// is; anything else (other formats or rates) is converted into the project's Audio Files.
    fn prepare_import(&self, path: &Path) -> Result<(PathBuf, Clip)> {
        let d = crate::decode::decode(path)?;
        let rate = self.sample_rate() as u32;
        if d.is_wav && d.sample_rate == rate {
            return Ok((path.to_path_buf(), Clip::from_channels(path.to_path_buf(), rate, d.channels)));
        }
        let channels = crate::decode::resample(d.channels, d.sample_rate, rate)?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Import".into());
        let dest = self.unique_audio_path(&stem);
        crate::decode::write_wav(&dest, rate, &channels)?;
        Ok((dest.clone(), Clip::from_channels(dest, rate, channels)))
    }

    /// An unused file name in the project's Audio Files folder.
    pub(super) fn unique_audio_path(&self, stem: &str) -> PathBuf {
        let dir = self.dir.join(AUDIO_DIR);
        let mut dest = dir.join(format!("{stem}.wav"));
        for i in 2.. {
            if !dest.exists() {
                break;
            }
            dest = dir.join(format!("{stem} {i}.wav"));
        }
        dest
    }

    pub(super) fn add_clip(&mut self, path: &Path, clip: Clip) -> Id {
        let id = self.project.alloc_id();
        let rel = path.strip_prefix(&self.dir).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string_lossy().to_string());
        self.clips.insert(id, Arc::new(clip));
        self.project.clips.push(ClipRef { id, path: rel });
        id
    }

    pub fn import_audio(&mut self, path: &Path, track: Id, start: i64) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        match self.project.track(track) {
            Some(t) if t.kind == TrackKind::Audio => {}
            Some(_) => return Err(anyhow!("audio files can only go on audio tracks")),
            None => return Err(anyhow!("no such track")),
        }
        let (stored, clip) = self.prepare_import(path)?;
        let frames = clip.frames() as i64;
        let clip_id = self.add_clip(&stored, clip);
        let id = self.project.alloc_id();
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let fade = self.default_fade(frames);
        let t = self.project.track_mut(track).unwrap();
        t.regions.push(Region { id, name, start: start.max(0), length: frames, muted: false, color: None, content: RegionContent::Audio { clip: clip_id, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() }, loop_end: None, takes: None, params: Default::default(), pattern: None });
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }

    /// Imports files dropped onto the project. With `track` set (an audio track), the first file
    /// lands on it and the rest on new tracks below; otherwise every file gets a new track,
    /// named after the file, inserted at `index` (the end when None). Returns per-file errors.
    pub fn import_audio_files(&mut self, paths: &[PathBuf], track: Option<Id>, start: i64, index: Option<usize>) -> Vec<String> {
        let mut errors = Vec::new();
        let mut files = paths.iter().filter(|p| crate::decode::is_audio_file(p) || p.is_file());
        let target = track.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Audio).unwrap_or(false));
        let mut next_index = match (target, index) {
            (Some(id), _) => self.project.tracks.iter().position(|t| t.id == id).map(|i| i + 1).unwrap_or(self.project.tracks.len()),
            (None, Some(i)) => i.min(self.project.tracks.len()),
            (None, None) => self.project.tracks.len(),
        };
        if let Some(id) = target {
            if let Some(first) = files.next() {
                if let Err(e) = self.import_audio(first, id, start) {
                    errors.push(format!("{}: {e}", first.display()));
                }
            }
        }
        for path in files {
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Audio".into());
            match self.prepare_import(path) {
                Ok((stored, clip)) => {
                    let input = if clip.channels.len() > 1 { InputSel::Stereo(0) } else { InputSel::Mono(0) };
                    let frames = clip.frames() as i64;
                    let id = match self.add_track(TrackKind::Audio, input, None, Some(next_index)) {
                        Ok(id) => id,
                        Err(e) => {
                            errors.push(format!("{}: {e}", path.display()));
                            continue;
                        }
                    };
                    next_index += 1;
                    let clip_id = self.add_clip(&stored, clip);
                    let rid = self.project.alloc_id();
                    let fade = self.default_fade(frames);
                    let t = self.project.track_mut(id).unwrap();
                    t.name = name.clone();
                    t.regions.push(Region { id: rid, name, start: start.max(0), length: frames, muted: false, color: None, content: RegionContent::Audio { clip: clip_id, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() }, loop_end: None, takes: None, params: Default::default(), pattern: None });
                    self.push_regions(id);
                    self.select_track(Some(id));
                }
                Err(e) => errors.push(format!("{}: {e}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
            }
        }
        self.dirty = true;
        self.request_autosave();
        errors
    }

    /// The clip's absolute peak level.
    pub fn clip_peak(&self, id: Id) -> Option<f32> {
        self.clips.get(&id).map(|c| c.channels.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs())))
    }

    /// See [`Clip::waveform`].
    pub fn clip_waveform(&self, id: Id) -> Option<Vec<u8>> {
        self.clips.get(&id).map(|c| c.waveform())
    }
}
