//! What runs on a timer: `idle` on the main thread every 30 ms (plug-in controllers, latency changes,
//! `Garbage`, automation) and `tick`, which builds the [`Tick`] the UI draws from, with the warnings
//! for engine errors, overloads and misbehaving plug-ins.

use super::*;

impl Session {
    /// Main-thread housekeeping: plugin controller idle, freeing replaced DSP objects, keeping
    /// MIDI inputs connected, automation (plug-in editors following it, writes from them) and
    /// bringing the engine up to date after commands for it were dropped (`resync_engine`).
    /// Returns true if the set of open editors changed or automation was written, when the UI
    /// needs a new snapshot.
    pub fn idle(&mut self) -> bool {
        self.mark_unresponsive();
        self.sync_parallel();
        let before = self.open_editors().len();
        self.resync_engine();
        let mut latency_changed = false;
        for c in self.controllers.values_mut() {
            c.idle();
            latency_changed |= c.take_latency_changed();
        }
        // Asked as well: a plug-in may change its latency well before it says so.
        latency_changed |= self.latency_changed();
        if latency_changed {
            self.update_pdc();
        }
        while let Ok(g) = self.garbage_rx.pop() {
            self.sampler_garbage(g);
        }
        // Editors hidden while their plug-in wasn't responding, whose instance is gone now.
        vst3_host::close_stranded_editors();
        self.sampler_idle();
        let written = self.auto_idle();
        let stepped = self.vst_undo_idle();
        self.state_copies_idle();
        let controlled = self.controller_idle();
        self.feedback_idle();
        self.bring_back();
        before != self.open_editors().len() || written || latency_changed || controlled || stepped
    }

    /// Plays a MIDI message on the current MIDI target track (the selected instrument track).
    pub fn send_midi(&self, data: [u8; 3]) {
        self.midi.inject(data);
    }

    /// Name of the track live MIDI currently plays, if any.
    pub fn midi_target_name(&self) -> Option<String> {
        self.midi_target_id().and_then(|id| self.project.track(id)).map(|t| t.name.clone())
    }

    /// Connects MIDI ports that have appeared since the last call. Runs every 3 s in the app.
    pub fn refresh_midi(&mut self) {
        self.midi.refresh();
        self.midi_out.refresh();
    }

    pub fn midi_inputs(&self) -> Vec<String> {
        self.midi.connected()
    }

    /// Builds the [`Tick`], taking what has accumulated since the last one. The `ui-tick` thread calls this.
    pub fn tick(&mut self) -> Tick {
        self.check_overload();
        self.check_left_out();
        // Keep the newest FFT_SIZE analyzer samples.
        let fresh = self.analyzer_rx.slots();
        let got = fresh > 0;
        if let Ok(chunk) = self.analyzer_rx.read_chunk(fresh) {
            let n = self.analyzer_buf.len();
            let (a, b) = chunk.as_slices();
            let incoming: Vec<f32> = a.iter().chain(b.iter()).copied().collect();
            chunk.commit_all();
            if incoming.len() >= n {
                self.analyzer_buf.copy_from_slice(&incoming[incoming.len() - n..]);
            } else {
                self.analyzer_buf.rotate_left(incoming.len());
                self.analyzer_buf[n - incoming.len()..].copy_from_slice(&incoming);
            }
        }
        let spectrum = match (self.analyzer_target, got) {
            (Some((_, uid)), true) => Some((uid, crate::eq::spectrum(&self.analyzer_buf, self.sample_rate()))),
            _ => None,
        };
        let meters: Vec<(Id, f32, f32)> = self
            .meters
            .iter()
            .map(|(&id, m)| {
                let (l, r) = m.take();
                (id, l, r)
            })
            .collect();
        let master = self.status.master.take();
        let peak_holds = self.hold_peaks(&meters, master);
        let n_in = self.audio_status.input_channels as usize;
        Tick {
            pos: self.position(),
            playing: self.is_playing(),
            recording: self.status.recording.load(Ordering::Relaxed),
            cpu: f32::from_bits(self.status.cpu_load.load(Ordering::Relaxed)),
            cpu_peak: f32::from_bits(self.status.cpu_peak.load(Ordering::Relaxed)),
            late_blocks: self.status.late_blocks.load(Ordering::Relaxed),
            threads: self.status.threads.load(Ordering::Relaxed),
            meters,
            master,
            inputs: self.status.inputs[..n_in.min(MAX_INPUTS)].iter().map(|m| m.take().0).collect(),
            previews: self.recorder.previews(),
            midi_activity: self.status.midi_activity.load(Ordering::Relaxed),
            overruns: self.status.overruns.load(Ordering::Relaxed),
            latency: self.latency(),
            spectrum,
            warning: self.warning.take().or_else(|| self.recorder.take_errors()).or_else(|| self.engine_error_warning()).or_else(|| self.bad_audio_warning()),
            fx: self.fx_ticks(),
            peak_holds,
            gain_reduction: self.gain_reductions(),
            learn: self.learning(),
            learned: self.take_learned(),
            midi_in: self.midi_since_last_tick(),
        }
    }

    /// What hardware MIDI inputs sent since the last call, from the activity log.
    fn midi_since_last_tick(&mut self) -> Vec<[u8; 3]> {
        let log = self.midi.recent();
        let fresh: Vec<[u8; 3]> = log.iter().filter(|e| e.seq >= self.midi_seen).map(|e| e.data).collect();
        if let Some(last) = log.last() {
            self.midi_seen = last.seq + 1;
        }
        fresh
    }

    /// A warning when the audio callback had to silence blocks because the engine panicked
    /// (the panic message itself goes to the log). Reported at most every few seconds.
    pub(super) fn engine_error_warning(&mut self) -> Option<String> {
        let n = self.status.engine_errors.load(Ordering::Relaxed);
        if n == self.engine_errors_seen || self.engine_error_at.elapsed() < Duration::from_secs(5) {
            return None;
        }
        self.engine_errors_seen = n;
        self.engine_error_at = Instant::now();
        Some("The audio engine hit an internal error and skipped some audio. Please report it, with the terminal output.".into())
    }

    /// Like Logic's System Overload: when the engine had to skip audio to keep up, playback
    /// (and recording) stops and the user hears why. A plug-in that stopped responding on a
    /// processing thread only costs its track and what that feeds, so playback goes on without
    /// them, and `check_left_out` says so.
    fn check_overload(&mut self) {
        let n = self.status.overloads.load(Ordering::Relaxed);
        if n == self.overloads_seen {
            return;
        }
        self.overloads_seen = n;
        if self.status.overload_left_out.load(Ordering::Relaxed) {
            return;
        }
        let ms = f32::from_bits(self.status.overload_secs.load(Ordering::Relaxed)) * 1000.0;
        let track = self.project.track(self.status.overload_track.load(Ordering::Relaxed)).map(|t| t.name.clone());
        let was_running = self.is_playing() || self.is_recording();
        if was_running {
            self.stop();
        }
        let what = if was_running { "so playback stopped" } else { "so it skipped a moment of audio" };
        let detail = match (self.status.overload_stuck.load(Ordering::Relaxed), track) {
            (true, Some(t)) => format!("A processing thread stayed in \"{t}\" for over {ms:.0} ms, and is left out until the audio engine is reset."),
            (true, None) => format!("A processing thread didn't finish within {ms:.0} ms, and is left out until the audio engine is reset."),
            (false, Some(t)) => format!("A buffer took {ms:.0} ms; \"{t}\" took longest."),
            (false, None) => format!("A buffer took {ms:.0} ms."),
        };
        log::warn!(
            "overload: {detail} ({} threads, {} late blocks so far)",
            self.status.threads.load(Ordering::Relaxed),
            self.status.late_blocks.load(Ordering::Relaxed)
        );
        let msg = format!("System overload: the audio engine couldn't keep up, {what}. {detail} Try a larger buffer size in Settings › Audio, or bypass or freeze heavy plug-ins.");
        self.warning.get_or_insert(msg);
    }

    /// Names the plug-in the engine caught putting out NaNs or a runaway level (at most every
    /// 5 seconds while it keeps doing it).
    fn bad_audio_warning(&mut self) -> Option<String> {
        let n = self.status.bad_audio.load(Ordering::Relaxed);
        if n == self.bad_audio_seen || self.bad_audio_at.elapsed() < Duration::from_secs(5) {
            return None;
        }
        self.bad_audio_seen = n;
        self.bad_audio_at = Instant::now();
        let uid = self.status.bad_audio_uid.load(Ordering::Relaxed);
        let p = &self.project;
        let found = p
            .tracks
            .iter()
            .find_map(|t| t.inserts.iter().flatten().chain(t.instrument.iter()).find(|s| s.uid == uid).map(|s| (s.name.clone(), t.name.clone())));
        let found = found.or_else(|| p.master.inserts.iter().flatten().find(|s| s.uid == uid).map(|s| (s.name.clone(), "Stereo Out".to_string())));
        let (plugin, track) = found.unwrap_or_else(|| ("A plug-in".into(), "a track".into()));
        Some(format!("{plugin} on {track} put out invalid audio (NaN or a runaway level), so Marbble muted it to protect the rest of the mix. If it keeps happening, bypass it or reset its settings."))
    }
}
