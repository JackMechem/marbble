//! The audio device on the session's side: restarting it, a new configuration, locking memory,
//! and the latencies that recordings and the Latency window are compensated by.

use crate::session::*;

impl Session {
    pub fn sample_rate(&self) -> f64 {
        self.engine_rate
    }

    /// The output device's IO thread as the workers take it (its workgroup and time-constraint
    /// policy on macOS), while the device runs. `IoThread::suggested_threads` is the system's
    /// suggestion for how many threads to play on beside it.
    pub fn io_thread(&self) -> Option<&audio_io::IoThread> {
        self.audio.as_ref().map(|a| a.io_thread())
    }

    /// Closes and reopens the device with the current config (Reset Audio Engine, a new config).
    /// After restarts by itself that gave up, it starts watching the device afresh.
    pub fn restart_audio(&mut self) {
        self.device_watch.reset();
        self.reopen_audio();
    }

    /// Reopens the device when its streams said it changed or went away, once things settle and
    /// within limits (`audio_io/watch.rs`), and says so in a toast. If the chosen device is gone,
    /// the system default takes its place, as when it's missing at startup. True when the device
    /// was reopened, for a new snapshot of `audio_status`.
    pub(crate) fn device_idle(&mut self) -> bool {
        use audio_io::watch::Step;
        let now = Instant::now();
        if let Some(io) = &self.audio {
            let (events, others) = io.events().counts();
            let (seen_events, seen_others) = self.device_events_seen;
            if others != seen_others {
                log::warn!("audio stream reported {} error(s), the last {:?}", others.wrapping_sub(seen_others), io.events().last_other());
            }
            if events != seen_events {
                self.device_watch.event(now);
            }
            self.device_events_seen = (events, others);
        }
        match self.device_watch.poll(now) {
            Step::Wait => false,
            Step::GiveUp => {
                log::error!("audio device: gave up restarting it");
                let what = if self.audio.is_some() { "The audio device kept dropping out" } else { "No audio output could be opened" };
                self.warning.get_or_insert(format!(
                    "{what}, so Marbble stopped restarting it. Choose a device in Settings › Audio, or click Reset Audio Engine there."
                ));
                false
            }
            Step::Restart => {
                // A take carries on through a moved system output, and the device reopens once
                // it ends (`Reason::waits_for_take`). A worse event meanwhile replaces the reason.
                let pending = self.audio.as_ref().and_then(|io| io.events().reason());
                if self.is_recording() && pending.is_some_and(|r| r.waits_for_take()) {
                    return false;
                }
                let reason = self.audio.as_ref().and_then(|io| io.events().take_reason());
                let old = self.audio_status.output_name.clone();
                log::warn!("audio device: {reason:?} on {old:?}, reopening");
                // A recording whose device went away, or whose device changed under it, ends here
                // with what it has rather than carrying on into the next device.
                if self.is_recording() {
                    self.stop();
                }
                self.reopen_audio();
                let ok = self.audio_status.running;
                self.device_watch.restarted(now, ok);
                let msg = match (ok, reason) {
                    (true, Some(r)) => {
                        let name = self.audio_status.output_name.as_deref().unwrap_or("the output device");
                        Some(format!("Audio restarted on \u{201c}{name}\u{201d} because {}.", r.because(old.as_deref())))
                    }
                    // A retry that worked: the toast for its failure said what happened.
                    (true, None) => Some("Audio is back.".into()),
                    (false, Some(r)) => Some(format!(
                        "Audio stopped because {}, and no other output could be opened. Marbble keeps trying for a few seconds; or choose a device in Settings › Audio.",
                        r.because(old.as_deref())
                    )),
                    (false, None) => None,
                };
                if let Some(m) = msg {
                    self.warning.get_or_insert(m);
                }
                true
            }
        }
    }

    /// Closes and reopens the device with the current config.
    fn reopen_audio(&mut self) {
        self.audio = None;
        // What the old device didn't take goes to the engine now, so that the count of commands
        // done keeps up with `cmds_sent` and nothing queued waits on whether the new one starts.
        self.engine.lock().take_queued();
        let (io, status) = audio_io::start(self.engine.clone(), &self.audio_config);
        if !status.running {
            log::error!("audio failed to start: {:?}", status.error);
        }
        let rate = if status.running { status.sample_rate } else { self.audio_config.sample_rate };
        self.audio = io;
        self.audio_status = status;
        // The new streams' events count from zero.
        self.device_events_seen = (0, 0);
        // Whether the workers should be real-time threads depends on the device. They start afresh,
        // promoted for its buffer, with any given up on in a stuck plug-in back.
        self.graph_dirty = true;
        self.threads.renew = true;
        self.pending_pos = None;
        let new_rate = rate != self.project.sample_rate || (self.engine_rate as u32) != rate;
        if new_rate {
            // The project runs at the device's rate, and that's saved with it.
            if rate != self.project.sample_rate {
                self.dirty = true;
            }
            self.project.sample_rate = rate;
        }
        // Tracks left out after a plug-in stopped responding come back with new instances of
        // their plug-ins.
        if new_rate || self.status.left_out.load(Ordering::Relaxed) > 0 {
            self.rebuild_graph();
        }
    }

    /// Takes a new config, reopening the device only if the devices changed.
    pub fn set_audio_config(&mut self, cfg: AudioConfig) {
        let reopen = !cfg.same_devices(&self.audio_config) || self.audio.is_none();
        self.graph_dirty |= cfg.processing_threads != self.audio_config.processing_threads;
        let relock = cfg.lock_memory != self.audio_config.lock_memory;
        self.audio_config = cfg;
        if relock {
            self.apply_memory_lock();
        }
        if reopen {
            self.restart_audio();
        }
        self.send(Cmd::AutoMonitor(self.audio_config.auto_monitor));
        self.sync_parallel();
    }

    /// Locks the process's memory in RAM, or lets it go, as Settings › Audio › Lock audio memory says
    /// (see `memlock.rs`). The app calls it at startup and `set_audio_config` when the switch
    /// changes; sessions in tests and checks leave their memory alone unless they ask.
    pub fn apply_memory_lock(&mut self) {
        self.memory_lock = crate::audio_io::memlock::apply(self.audio_config.lock_memory);
    }

    /// Round trip from what the performer heard to when their playing reached the engine:
    /// output latency + input latency + frames waiting in the duplex ring, plus the user's
    /// manual Recording Delay.
    pub(crate) fn record_latency_frames(&self) -> i64 {
        let sr = self.sample_rate();
        let us = self.status.input_latency_us.load(Ordering::Relaxed) + self.status.output_latency_us.load(Ordering::Relaxed);
        let ring = self.status.ring_frames.load(Ordering::Relaxed) as f64;
        let manual = self.audio_config.recording_delay_ms as f64 * sr / 1000.0;
        // With delay compensation, the playback the performer played along to left Stereo Out
        // that much late too.
        let pdc = self.pdc_global.total as f64;
        (us as f64 * sr / 1e6 + ring + manual + pdc).round() as i64
    }

    /// What recorded MIDI is moved earlier by: it only travels one way, as the performer reacts
    /// to audio that left late (output latency plus the delay compensation).
    pub(crate) fn midi_latency_frames(&self) -> i64 {
        (self.status.output_latency_us.load(Ordering::Relaxed) as f64 * self.sample_rate() / 1e6).round() as i64 + self.pdc_global.total as i64
    }

    /// The input and output latency the device last reported, and the compensation recordings get.
    pub fn latency(&self) -> Latency {
        let sr = self.sample_rate().max(1.0);
        Latency {
            input_ms: self.status.input_latency_us.load(Ordering::Relaxed) as f32 / 1000.0,
            output_ms: self.status.output_latency_us.load(Ordering::Relaxed) as f32 / 1000.0,
            compensation_ms: (self.record_latency_frames() as f64 * 1000.0 / sr) as f32,
        }
    }
}
