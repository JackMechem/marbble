//! Bringing audio and MIDI into the project as regions: importing audio files (copied into the
//! project's Audio Files and decoded), new empty MIDI regions, and the clips' peaks and waveforms.

use crate::session::*;

impl Session {
    /// An empty MIDI region on an instrument track.
    pub fn create_midi_region(&mut self, track: Id, start: i64, length: i64) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        match self.project.track(track) {
            Some(t) if t.kind == TrackKind::Instrument => {}
            _ => return Err(anyhow!("MIDI regions go on software instrument tracks")),
        }
        let id = self.project.alloc_id();
        let name = self.project.track(track).unwrap().name.clone();
        self.project.track_mut(track).unwrap().regions.push(Region::midi(id, name, start.max(0), length.max(1), Vec::new(), Vec::new()));
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }

    /// Decodes a file for the project. A WAV already at the project rate is referenced where it
    /// is; anything else (other formats or rates) is converted into the project's Audio Files.
    fn prepare_import(&self, path: &Path) -> Result<(PathBuf, Clip)> {
        let d = crate::audio_file::decode::decode(path)?;
        let rate = self.sample_rate() as u32;
        if d.is_wav && d.sample_rate == rate {
            return Ok((path.to_path_buf(), Clip::from_channels(path.to_path_buf(), rate, d.channels)));
        }
        let channels = crate::audio_file::decode::resample(d.channels, d.sample_rate, rate)?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Import".into());
        let dest = self.unique_audio_path(&stem);
        crate::audio_file::decode::write_wav(&dest, rate, &channels)?;
        Ok((dest.clone(), Clip::from_channels(dest, rate, channels)))
    }

    /// An unused file name in the project's Audio Files folder.
    pub(crate) fn unique_audio_path(&self, stem: &str) -> PathBuf {
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

    pub(crate) fn add_clip(&mut self, path: &Path, clip: Clip) -> Id {
        let id = self.project.alloc_id();
        let rel = path.strip_prefix(&self.dir).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string_lossy().to_string());
        self.clips.insert(id, Arc::new(clip));
        self.project.clips.push(ClipRef { id, path: rel });
        id
    }

    /// Copies or converts an audio file into the project and puts a region for it on an audio track at
    /// `start`. Returns the region's id.
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
        t.regions.push(Region {
            id,
            name,
            start: start.max(0),
            length: frames,
            muted: false,
            color: None,
            content: RegionContent::Audio { clip: clip_id, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        });
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }

    /// Imports files dropped onto the project. With `track` set (an audio track), the first file
    /// lands on it and the rest on new tracks below; otherwise every file gets a new track,
    /// named after the file, inserted at `index` (the end when None). Returns per-file errors.
    pub fn import_audio_files(&mut self, paths: &[PathBuf], track: Option<Id>, start: i64, index: Option<usize>) -> Vec<String> {
        let mut errors = Vec::new();
        let mut files = paths.iter().filter(|p| crate::audio_file::decode::is_audio_file(p) || p.is_file());
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
                    t.regions.push(Region {
                        id: rid,
                        name,
                        start: start.max(0),
                        length: frames,
                        muted: false,
                        color: None,
                        content: RegionContent::Audio {
                            clip: clip_id,
                            offset: 0,
                            gain_db: 0.0,
                            fade_in: fade,
                            fade_out: fade,
                            fade_shape: FadeShape::default(),
                        },
                        loop_end: None,
                        takes: None,
                        params: Default::default(),
                        pattern: None,
                    });
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
