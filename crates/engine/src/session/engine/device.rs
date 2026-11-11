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

    /// Closes and reopens the device with the current config.
    pub fn restart_audio(&mut self) {
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
