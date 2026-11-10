//! Turning an audio region into a sampler track (Convert to New Sampler Track), whole or sliced at
//! its transients.

use super::*;

impl Session {
    /// Logic's Convert to Sampler Track: makes a new instrument track below with a sampler playing
    /// this audio region, and a MIDI region in its place, and mutes the original. With
    /// `slices` (a sensitivity, 0..1) the region is cut at its transients, one key each, and the
    /// MIDI plays them back in time; otherwise one note plays the whole region at its own pitch.
    pub fn convert_region_to_sampler(&mut self, region: Id, slices: Option<f32>) -> Result<Id> {
        let (track, idx) = self.find_region(region).ok_or_else(|| anyhow!("no such region"))?;
        self.check_unfrozen(&[track])?;
        let r = self.project.track(track).unwrap().regions[idx].clone();
        let RegionContent::Audio { clip, offset, gain_db, .. } = r.content else { return Err(anyhow!("Only audio regions convert to a sampler")) };
        let c = self.clips.get(&clip).cloned().ok_or_else(|| anyhow!("{}'s audio file is missing", r.name))?;
        // Normally a copy of its file; one that's gone comes from what's in memory.
        let rel = if c.path.is_file() {
            adopt_file(&self.dir, Path::new(""), &c.path)?
        } else {
            let dest = self.dir.join(SAMPLES_DIR).join(format!("{}.wav", r.name.replace(['/', '\\'], " ")));
            crate::audio_file::decode::write_wav(&dest, c.sample_rate, &c.channels)?;
            dest.strip_prefix(&self.dir).unwrap_or(&dest).to_string_lossy().to_string()
        };
        let buf = self.sampler.pool.get(&self.dir.join(&rel)).map_err(|e| anyhow!("{}: {}", r.name, e.text()))?;
        let at = self.project.tracks.iter().position(|t| t.id == track).map_or(self.project.tracks.len(), |i| i + 1);
        let new = self.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.to_string()), Some(at))?;
        if let Some(t) = self.project.track_mut(new) {
            t.name = format!("{} Sampler", r.name);
        }
        let mut state = SamplerState::parse(None);
        let end = (offset + r.length).max(offset + 1) as u64;
        let mut z = Zone::new(state.alloc_id(), r.name.clone(), rel, CONVERT_ROOT);
        z.start = offset.max(0) as u64;
        z.end = end;
        z.gain_db = gain_db;
        let velocity = 127;
        let mut notes = Vec::new();
        if let Some(sensitivity) = slices {
            let zones = self.slice_zones(&mut state, &z, &buf, sensitivity);
            for s in &zones {
                let note_start = s.start as i64 - offset;
                notes.push(Note { start: note_start, length: (s.end - s.start) as i64, pitch: s.root, velocity, muted: false });
            }
            state.zones = zones;
        } else {
            notes.push(Note { start: 0, length: r.length, pitch: CONVERT_ROOT, velocity, muted: false });
            state.zones.push(z);
        }
        self.sampler_store(new, &state)?;
        let midi = self.create_midi_region(new, r.start, r.length)?;
        self.set_midi_content(vec![MidiContent { region: midi, notes, cc: Vec::new(), start: None, length: None }]);
        if let Some(t) = self.project.track_mut(track) {
            t.regions[idx].muted = true;
        }
        self.push_regions(track);
        self.dirty = true;
        Ok(new)
    }
}
