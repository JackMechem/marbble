//! Recording into Marbble Sampler: a take from an input becomes a zone, trimmed of the silence
//! around it.

use super::*;

impl Session {
    /// Starts recording `input` into the sampler on `track`, through the recorder and the input
    /// every audio track uses, whatever the transport is doing.
    pub fn sampler_record_start(&mut self, track: Id, input: InputSel) -> Result<()> {
        self.sampler_slot(track)?;
        self.check_unfrozen(&[track])?;
        if self.sampler.take.is_some() {
            return Err(anyhow!("A sampler is already recording"));
        }
        if self.rec_punch.is_some() {
            return Err(anyhow!("Stop the song's recording first"));
        }
        if input.channels() == 0 {
            return Err(anyhow!("Choose an input to record from"));
        }
        let sr = self.sample_rate() as u32;
        let depth = self.project.bit_depth.unwrap_or(self.prefs.recording.bit_depth);
        let dir = self.dir.join(SAMPLES_DIR);
        let path = (1..).map(|i| dir.join(format!("Recording {i}.wav"))).find(|p| !p.exists()).unwrap_or_else(|| dir.join("Recording.wav"));
        let rec = self.sampler.recorder.get_or_insert_with(Recorder::new).clone();
        let sink = rec.begin(track, path.clone(), input.channels(), sr, 0, depth)?;
        self.send(Cmd::InputTap { input, sink: Some(sink) });
        self.sampler.take = Some(LiveTake { track, path });
        Ok(())
    }

    /// How the recording's going, for the panel.
    pub fn sampler_recording(&self) -> Option<SamplerRecording> {
        let take = self.sampler.take.as_ref()?;
        let rec = self.sampler.recorder.as_ref()?;
        let p = rec.previews().into_iter().find(|t| t.track == take.track)?;
        let peak = p.peaks.iter().fold(0.0f32, |m, v| m.max(*v));
        Some(SamplerRecording { track: take.track, frames: p.frames as u64, secs: p.frames as f64 / self.sample_rate(), peak })
    }

    fn sampler_stop_take(&mut self) -> Option<(LiveTake, Option<FinishedTake>)> {
        let take = self.sampler.take.take()?;
        self.send(Cmd::InputTap { input: InputSel::None, sink: None });
        self.wait_for_engine();
        let finished = self.sampler.recorder.as_ref().map(|r| r.finish_all()).and_then(|mut f| f.pop()).map(|f| f.1);
        Some((take, finished))
    }

    /// Stops and throws the take away.
    pub fn sampler_record_cancel(&mut self) {
        if let Some((take, _)) = self.sampler_stop_take() {
            let _ = std::fs::remove_file(&take.path);
        }
    }

    /// Stops and makes the take playable: a zone over the whole keyboard (or just `at_key`) with
    /// its pitch at C3. `trim` moves its start and end in past the silence. Returns the zone.
    pub fn sampler_record_stop(&mut self, trim: bool, at_key: Option<u8>) -> Result<u64> {
        let (take, finished) = self.sampler_stop_take().ok_or_else(|| anyhow!("Nothing is recording"))?;
        let finished = finished.ok_or_else(|| anyhow!("The recording was lost"))?;
        if let Some(e) = finished.error {
            self.warning = Some(e);
        }
        let channels = finished.channels;
        let frames = channels.first().map_or(0, |c| c.len());
        let peak = channels.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()));
        if frames == 0 || peak < SILENT {
            let _ = std::fs::remove_file(&take.path);
            return Err(anyhow!(if frames == 0 {
                "Nothing came in from the input. Check Settings › Audio."
            } else {
                "The input was silent, so nothing was kept. Check the input and its level."
            }));
        }
        let rate = self.sample_rate() as u32;
        self.sampler.pool.insert(&take.path, rate, &channels).map_err(|e| anyhow!("The recording is {}", e.text()))?;
        let mut state = self.sampler_state(take.track)?;
        let rel = project_relative(take.path.strip_prefix(&self.dir).unwrap_or(&take.path));
        let name = take.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Recording".into());
        let mut z = Zone::new(state.alloc_id(), name, rel, CONVERT_ROOT);
        if let Some(k) = at_key {
            z.key_lo = k;
            z.key_hi = k;
            z.root = k;
        }
        if trim {
            let (a, b) = trim_points(&channels, rate);
            z.start = a;
            z.end = b;
        }
        let id = z.id;
        state.zones.push(z);
        self.sampler_store(take.track, &state)?;
        Ok(id)
    }

    /// Lets a check push input through the engine, as an audio device would, with `channels`
    /// interleaved. The null device has no input of its own.
    pub fn feed_input_for_checks(&self, input: &[f32], channels: usize) {
        let frames = input.len() / channels.max(1);
        let mut out = vec![0.0f32; frames * 2];
        self.engine.lock().process(input, channels, &mut out, 2);
    }
}
