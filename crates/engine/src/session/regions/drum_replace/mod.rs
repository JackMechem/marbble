//! Drum Replacement and Doubling on the session's side: what an audio track plays, as parts of
//! clips for `crate::edit::drum_replace` to find hits in, and the new sampler track it makes.

use crate::edit::drum_replace::{find_hits, kept, notes, DrumMode, DrumSettings, DrumSource};
use crate::instrument::sampler::state::{SamplerState, Zone};
use crate::instrument::sampler::ID as SAMPLER;
use crate::session::plugins::sampler::project_relative;
use crate::session::*;

/// Where the made drum samples go, under the project's Samples folder.
const DRUMS_DIR: &str = "Drum Replacement";

impl Session {
    /// The regions of audio track `track` that Drum Replacement works on: those of `regions` on
    /// it, or (when none are) every region on it that isn't muted.
    fn drum_regions(&self, track: Id, regions: &[Id]) -> Result<Vec<Region>> {
        let t = self.project.track(track).filter(|t| t.kind == TrackKind::Audio).ok_or_else(|| anyhow!("Drum Replacement works on an audio track"))?;
        let chosen: Vec<Region> = t.regions.iter().filter(|r| regions.contains(&r.id)).cloned().collect();
        let all = if chosen.is_empty() { t.regions.iter().filter(|r| !r.muted).cloned().collect() } else { chosen };
        Ok(all.into_iter().filter(|r| matches!(r.content, RegionContent::Audio { .. })).collect())
    }

    /// What the track plays in those regions, as parts of clips: each pass of a looped region,
    /// and each comp segment of a take folder (crossfades aside). Shared, so the hits can be
    /// found without holding the session.
    pub fn drum_sources(&self, track: Id, regions: &[Id]) -> Result<Vec<DrumSource>> {
        let mut out = Vec::new();
        for r in self.drum_regions(track, regions)? {
            let RegionContent::Audio { clip, offset, gain_db, .. } = r.content else { continue };
            let gain = 10f32.powf(gain_db / 20.0);
            for (start, length) in r.passes() {
                match &r.takes {
                    None => {
                        if let Some(c) = self.clips.get(&clip) {
                            out.push(DrumSource { clip: c.clone(), from: offset, start, length, gain });
                        }
                    }
                    // In take time, the pass covers offset..offset + length.
                    Some(folder) => {
                        for seg in &folder.comp {
                            let (a, b) = (seg.start.max(offset), seg.end.min(offset + length));
                            let Some(take) = folder.takes.iter().find(|t| t.id == seg.take) else { continue };
                            let Some(c) = self.clips.get(&take.clip) else { continue };
                            if b > a {
                                let gain = gain * 10f32.powf(take.gain_db / 20.0);
                                out.push(DrumSource { clip: c.clone(), from: take.offset + a, start: start + a - offset, length: b - a, gain });
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Drum Replacement or Doubling: a new software instrument track below `track` with Marbble
    /// Sampler playing the chosen drum, and a MIDI region over the regions' span with a note at
    /// each hit past the threshold. Replace mutes `track`. Returns the new track.
    pub fn drum_replace(&mut self, track: Id, regions: &[Id], s: &DrumSettings) -> Result<Id> {
        let used = self.drum_regions(track, regions)?;
        let (Some(start), Some(end)) = (used.iter().map(|r| r.start).min(), used.iter().map(|r| r.end()).max()) else {
            return Err(anyhow!("There's no audio on that track"));
        };
        let rate = self.sample_rate() as u32;
        let hits = kept(&find_hits(&self.drum_sources(track, regions)?, rate), s.threshold_db);
        if hits.is_empty() {
            return Err(anyhow!("No hits reach the threshold. Lower it to find softer ones."));
        }
        let kind = s.instrument;
        // The drum's sample, made once per project.
        let path = self.dir.join(SAMPLES_DIR).join(DRUMS_DIR).join(format!("{}.wav", kind.name()));
        if !path.is_file() {
            crate::audio_file::decode::write_wav(&path, rate, &[kind.sample(rate)])?;
        }
        let rel = project_relative(path.strip_prefix(&self.dir).unwrap_or(&path));
        self.sampler.pool.get(&path).map_err(|e| anyhow!("{}: {}", kind.name(), e.text()))?;

        let at = self.project.tracks.iter().position(|t| t.id == track).map_or(self.project.tracks.len(), |i| i + 1);
        let parent = self.project.track(track).and_then(|t| t.parent);
        let new = self.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.to_string()), Some(at))?;
        let name = format!("{} {}", kind.name(), if s.mode == DrumMode::Replace { "Replacement" } else { "Double" });
        if let Some(t) = self.project.track_mut(new) {
            t.name = name;
            t.parent = parent;
        }
        let mut state = SamplerState::parse(None);
        let note = kind.note();
        let mut z = Zone::new(state.alloc_id(), kind.name().to_string(), rel, note);
        z.key_lo = note;
        z.key_hi = note;
        z.one_shot = true;
        state.zones.push(z);
        self.sampler_store(new, &state)?;
        let midi = self.create_midi_region(new, start, end - start)?;
        let notes = notes(&hits, s, start, end - start, rate);
        self.set_midi_content(vec![MidiContent { region: midi, notes, cc: Vec::new(), start: None, length: None }]);
        if s.mode == DrumMode::Replace {
            self.update_track(track, TrackPatch { mute: Some(true), ..Default::default() });
        }
        self.sync_tracks();
        self.dirty = true;
        Ok(new)
    }
}

#[cfg(test)]
mod tests;
