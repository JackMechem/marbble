//! The session's side of the wire to the engine: sending [`Cmd`]s (and waiting for room, or for
//! the engine to apply them), and bringing the engine in step with a changed project. What the
//! engine is sent is built in `playback.rs`; the audio device is in `device.rs`.
//!
//! Every edit ends up here. `sync_globals`, `push_all_params`, `sync_tracks` and `push_regions`
//! are how a changed project reaches the audio thread; `undo.rs` and `files.rs` call them too.

use crate::session::*;

/// How long `send` waits for room in a full command queue before it gives up on a command:
/// longer than any device buffer, and than a block given up on in a stuck plug-in takes to end.
const SEND_WAIT: Duration = Duration::from_millis(1000);

impl Session {
    /// Queues a command for the audio thread. Should the queue (4096 commands) be full, the main
    /// thread waits for the engine to make room (`send_when_room`); the audio thread never waits.
    pub(crate) fn send(&mut self, cmd: Cmd) {
        self.graph_dirty |= crate::session::engine::threads::changes_routes(&cmd);
        match self.cmd_tx.push(cmd) {
            Ok(()) => self.cmds_sent += 1,
            Err(rtrb::PushError::Full(cmd)) => self.send_when_room(cmd),
        }
        if self.audio.is_none() {
            // No device is running, so drain the queue here to keep the graph current.
            self.engine.lock().process(&[], 0, &mut [], 0);
        }
    }

    /// The queue is full: waits up to `SEND_WAIT` for the engine to take commands, freeing what it
    /// hands back meanwhile (were the garbage ring to fill too, the engine would leak it). An engine
    /// that takes none for that long (the device stopped, a callback stuck) doesn't get this one: it
    /// is counted in `EngineStatus::cmds_dropped`, the tick warns, and `idle` rebuilds the engine
    /// from the project once it has taken the rest (`resync_engine`). Until then a command finding
    /// the queue full is dropped at once, so that an edit doesn't hold the UI up for each.
    fn send_when_room(&mut self, mut cmd: Cmd) {
        let deadline = Instant::now() + SEND_WAIT;
        while !self.engine_resync {
            if self.audio.is_none() {
                self.engine.lock().process(&[], 0, &mut [], 0);
            }
            while let Ok(g) = self.garbage_rx.pop() {
                self.sampler_garbage(g);
            }
            match self.cmd_tx.push(cmd) {
                Ok(()) => {
                    self.cmds_sent += 1;
                    return;
                }
                Err(rtrb::PushError::Full(c)) => cmd = c,
            }
            if Instant::now() >= deadline {
                log::error!("the audio engine took no commands for {} ms with its queue full: rebuilding it once it does", SEND_WAIT.as_millis());
                self.engine_resync = true;
                self.warning.get_or_insert_with(|| {
                    "The audio engine stopped taking changes, so what plays may not match the project. It is brought up to date once it catches up, or with Reset Audio Engine in Settings › Audio.".into()
                });
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(cmd);
        self.status.cmds_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// After `send` dropped commands: once the engine has taken every other one, rebuilds it from
    /// the project, as Reset Audio Engine does without reopening the device. A transport running
    /// is stopped first, through the session, so a recording ends with its takes.
    pub(crate) fn resync_engine(&mut self) {
        if !self.engine_resync || !self.cmd_queue_empty() {
            return;
        }
        if self.is_playing() || self.is_recording() {
            // The rebuild waits for the next idle, once the engine has taken the stop.
            self.stop();
            self.warning.get_or_insert_with(|| "Playback stopped to bring the audio engine up to date with changes it missed.".into());
            return;
        }
        log::warn!("rebuilding the audio engine after {} commands for it were dropped", self.status.cmds_dropped.load(Ordering::Relaxed));
        self.rebuild_graph();
    }

    /// Whether the engine has taken every command sent. Once it has, holding the engine's lock
    /// means it has applied them too, as it applies what it takes before letting go.
    fn cmd_queue_empty(&self) -> bool {
        self.cmd_tx.slots() == self.cmd_tx.buffer().capacity()
    }

    /// Blocks until the audio thread has applied every command sent so far.
    pub(crate) fn wait_for_engine(&self) {
        let deadline = Instant::now() + Duration::from_millis(1000);
        while self.status.cmds_done.load(Ordering::Relaxed) < self.cmds_sent && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub(crate) fn sync_globals(&mut self) {
        let p = &self.project;
        let cmds = [
            Cmd::Tempo(Arc::new(crate::model::tempo::TempoMap::from_project(p))),
            Cmd::Cycle { enabled: p.cycle.enabled, start: p.cycle.start, end: p.cycle.end },
            Cmd::Metronome { on: p.metronome, count_in: p.count_in },
            Cmd::MasterVolume(volume_gain(p.master.volume_db)),
            Cmd::AutoMonitor(self.audio_config.auto_monitor),
            Cmd::Analyzer(self.analyzer_target),
            Cmd::Click(ClickParams::from_settings(&p.click)),
            Cmd::MidiThru(self.prefs.midi.thru),
            Cmd::Chase { notes: self.prefs.midi.chase_notes, controllers: self.prefs.midi.chase_controllers },
        ];
        for c in cmds {
            self.send(c);
        }
        self.update_midi_target();
        self.update_pdc();
        // Region quantize and delay in ticks follow the tempo map. Only when it moved: new
        // playback lists release and chase the track's notes.
        let tempo = crate::model::tempo::TempoMap::from_project(&self.project);
        if self.regions_tempo.as_ref() != Some(&tempo) {
            self.regions_tempo = Some(tempo);
            self.push_tempo_regions();
        }
    }

    /// Sends every track's settings (stacks make one track's change reach others).
    pub(crate) fn push_all_params(&mut self) {
        let params: Vec<(Id, TrackParams)> = self.project.tracks.iter().map(|t| (t.id, Self::track_params(&self.project, t))).collect();
        for (id, p) in params {
            self.send(Cmd::Params(id, p));
        }
        // Routing may have changed, and with it the delays that line the paths up.
        self.update_pdc();
    }

    /// After tracks are added, removed, moved or restacked: keeps subtracks under their stack
    /// and tells the engine the processing order and routing.
    pub(crate) fn sync_tracks(&mut self) {
        self.project.prune_routes();
        self.project.normalize_tree();
        self.send(Cmd::Order(self.project.process_order()));
        self.push_all_params();
        self.update_midi_target();
    }

    pub(crate) fn push_regions(&mut self, track: Id) {
        // An undo, a switched alternative or a paste may bring back a clip that isn't loaded.
        let clips = self.project.track(track).map(|t| t.clips()).unwrap_or_default();
        self.load_clips(clips);
        if let Some(t) = self.project.track(track) {
            let r = self.regions_dsp(t);
            self.send(Cmd::Regions(track, r));
        }
    }

    /// The project with each plugin's current state embedded (for saving or rebuilding). A plug-in
    /// a processing thread is left in (`session/engine/left_out.rs`) isn't asked, as it may never answer:
    /// it keeps the state the project last had for it.
    pub(crate) fn capture_plugin_states(&self) -> Project {
        let mut p = self.project.clone();
        let held = self.held();
        let fill = |track: Id, s: &mut Option<PluginSlot>, ctrls: &HashMap<Id, PluginController>| {
            if let Some(slot) = s {
                let c = ctrls.get(&slot.uid).filter(|c| !Self::left_in(&held, track, slot.uid, c.instance()));
                if let Some(st) = c.and_then(|c| c.save_state()) {
                    slot.state = Some(encode_state(&st));
                }
            }
        };
        for t in &mut p.tracks {
            fill(t.id, &mut t.instrument, &self.controllers);
            for s in &mut t.inserts {
                fill(t.id, s, &self.controllers);
            }
        }
        for s in &mut p.master.inserts {
            fill(MASTER_ID, s, &self.controllers);
        }
        p
    }

    pub(crate) fn midi_target_id(&self) -> Option<Id> {
        let sel = self.selected.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Instrument).unwrap_or(false));
        sel.or_else(|| self.project.tracks.iter().find(|t| t.kind == TrackKind::Instrument && t.armed && !self.project.is_off(t.id)).map(|t| t.id))
    }

    pub(crate) fn update_midi_target(&mut self) {
        let target = self.midi_target_id();
        self.send(Cmd::MidiTarget(target));
    }
}

#[cfg(test)]
mod tests;
