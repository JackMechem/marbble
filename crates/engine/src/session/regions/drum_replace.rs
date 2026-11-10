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
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::edit::drum_replace::{DrumKind, DrumTiming};

    const RATE: i64 = 48000;

    /// A session with an audio track playing kicks (the made sample) at `hits`, at their
    /// levels, in one region from bar 2 (`RATE * 2`).
    fn session(hits: &[(usize, f32)]) -> (Session, Id, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("marbble-drum-replace-{}-{}", std::process::id(), hits.len()));
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let kick = DrumKind::Kick.sample(RATE as u32);
        let mut buf = vec![0.0f32; RATE as usize * 3];
        for &(a, g) in hits {
            for (i, v) in kick.iter().enumerate().take(buf.len() - a) {
                buf[a + i] += v * g;
            }
        }
        let path = s.dir.join("Audio Files").join("kick.wav");
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), RATE as u32, vec![buf]));
        let id = s.project.alloc_id();
        let content = RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
        s.project.track_mut(t).unwrap().regions.push(Region {
            id,
            name: "Kick In".into(),
            start: RATE * 2,
            length: RATE * 3,
            muted: false,
            color: None,
            content,
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        });
        s.push_regions(t);
        (s, t, id)
    }

    fn midi(s: &Session, t: Id) -> (Region, Vec<Note>) {
        let r = s.project.track(t).unwrap().regions[0].clone();
        let RegionContent::Midi { notes, .. } = &r.content else { panic!("not MIDI") };
        let n = notes.clone();
        (r, n)
    }

    #[test]
    fn replace_makes_a_sampler_track_below_with_a_note_per_hit_and_undoes_in_one_step() {
        let hits = [(4800, 1.0), (28800, 0.25), (52800, 0.6), (76800, 0.02)];
        let (mut s, t, _) = session(&hits);
        let settings = DrumSettings { threshold_db: -30.0, ..Default::default() };
        s.checkpoint("Drum Replacement", None, false);
        let new = s.drum_replace(t, &[], &settings).unwrap();
        let pos = |id: Id| s.project.tracks.iter().position(|x| x.id == id).unwrap();
        assert_eq!(pos(new), pos(t) + 1);
        let nt = s.project.track(new).unwrap();
        assert_eq!((nt.kind, nt.name.as_str()), (TrackKind::Instrument, "Kick Replacement"));
        let slot = nt.instrument.as_ref().unwrap();
        assert_eq!(slot.plugin_id, SAMPLER);
        let zones = SamplerState::parse(slot.state.as_deref()).zones;
        assert_eq!(zones.len(), 1);
        assert!(zones[0].one_shot && zones[0].key_lo == 36 && zones[0].key_hi == 36 && zones[0].root == 36);
        assert!(s.dir.join(&zones[0].file).is_file(), "the kick sample was written into the project");
        // The -34 dB hit is under the threshold; the others land on their attacks.
        let (r, n) = midi(&s, new);
        assert_eq!((r.start, r.length), (RATE * 2, RATE * 3));
        assert_eq!(n.len(), 3, "{n:?}");
        for (note, &(a, _)) in n.iter().zip(&hits) {
            assert!((note.start - a as i64).abs() <= 2, "{note:?} vs {a}");
            assert_eq!(note.pitch, 36);
        }
        assert_eq!(n.iter().map(|x| x.velocity).collect::<Vec<_>>().first(), Some(&127));
        assert!(n[1].velocity < n[2].velocity && n[2].velocity < n[0].velocity, "{n:?}");
        assert!(s.project.track(t).unwrap().mute, "Replace mutes the original");
        s.undo();
        assert!(s.project.track(new).is_none() && !s.project.track(t).unwrap().mute, "one undo takes it all back");
        s.redo();
        assert!(s.project.track(new).is_some() && s.project.track(t).unwrap().mute);
    }

    #[test]
    fn doubling_keeps_the_original_and_takes_the_selected_regions_only() {
        let (mut s, t, id) = session(&[(4800, 1.0), (52800, 0.6)]);
        // Split, so only the second half is chosen.
        let pieces = s.split_regions_at(&[id], &[RATE * 2 + 30000]);
        let second = pieces.into_iter().find(|&p| p != id).unwrap();
        let settings = DrumSettings { instrument: DrumKind::Snare, mode: DrumMode::Double, timing: DrumTiming::Peak, offset_ms: 1.0, ..Default::default() };
        let new = s.drum_replace(t, &[second], &settings).unwrap();
        assert!(!s.project.track(t).unwrap().mute);
        let (r, n) = midi(&s, new);
        assert_eq!(r.start, RATE * 2 + 30000);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].pitch, n[0].velocity), (38, 127));
        // At the peak, a millisecond late: a few ms after the attack at 52800 from the region.
        let from_attack = n[0].start - (52800 - 30000);
        assert!(from_attack > 48 && from_attack < 48 * 20, "{n:?}");
        assert_eq!(s.project.track(new).unwrap().name, "Snare Double");
    }

    #[test]
    fn nothing_past_the_threshold_or_no_audio_track_is_refused() {
        let (mut s, t, _) = session(&[(4800, 0.01)]);
        let settings = DrumSettings { threshold_db: -20.0, ..Default::default() };
        assert!(s.drum_replace(t, &[], &settings).is_err());
        let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        assert!(s.drum_replace(inst, &[], &settings).is_err());
        assert_eq!(s.project.tracks.len(), 2, "no track was made");
    }

    #[test]
    fn a_take_folder_is_read_through_its_comp() {
        let (mut s, t, id) = session(&[(4800, 1.0), (52800, 1.0)]);
        // A second take, silent but for one kick at 100000, heard from take time 40000 on.
        let other = DrumKind::Kick.sample(RATE as u32);
        let mut buf = vec![0.0f32; RATE as usize * 3];
        buf[100000..100000 + other.len()].copy_from_slice(&other);
        let path = s.dir.join("Audio Files").join("kick 2.wav");
        let clip2 = s.add_clip(&path, Clip::from_channels(path.clone(), RATE as u32, vec![buf]));
        let r = s.project.track_mut(t).unwrap().regions.iter_mut().find(|r| r.id == id).unwrap();
        let RegionContent::Audio { clip, .. } = r.content else { panic!() };
        let take = |id, clip| crate::model::Take {
            id,
            name: String::new(),
            clip,
            offset: 0,
            start: 0,
            end: RATE * 3,
            gain_db: 0.0,
            notes: Vec::new(),
            cc: Vec::new(),
            ..Default::default()
        };
        r.takes = Some(TakeFolder {
            takes: vec![take(901, clip), take(902, clip2)],
            comp: vec![CompSegment { start: 0, end: 40000, take: 901 }, CompSegment { start: 40000, end: RATE * 3, take: 902 }],
            comps: Vec::new(),
            current: 0,
            offset: 0,
        });
        let sources = s.drum_sources(t, &[]).unwrap();
        assert_eq!(sources.len(), 2);
        let hits = find_hits(&sources, RATE as u32);
        // The first take's first kick, then the second take's: its 52800 isn't heard.
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!((hits[0].pos - (RATE * 2 + 4800)).abs() <= 2 && (hits[1].pos - (RATE * 2 + 100000)).abs() <= 2, "{hits:?}");
    }
}
