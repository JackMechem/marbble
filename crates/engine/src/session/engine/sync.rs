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
mod tests {
    use super::*;

    /// A session on the null device, whose callback plays block after block as fast as it can.
    fn session() -> Session {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        assert!(s.audio.is_some(), "the null device runs");
        s.wait_for_engine();
        s.warning = None;
        s
    }

    fn engine_volume(s: &Session) -> f32 {
        s.engine.lock().master_volume_for_tests()
    }

    fn dropped(s: &Session) -> u64 {
        s.status.cmds_dropped.load(Ordering::Relaxed)
    }

    /// Two and a half queues of commands while the callback is held off for 200 ms, as by a long
    /// buffer: the main thread waits for room, and every one is applied, in order.
    #[test]
    fn a_burst_longer_than_the_queue_waits_for_room() {
        let mut s = session();
        let engine = s.engine.clone();
        let (held_tx, held) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _callback_held_off = engine.lock();
            held_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        held.recv().unwrap();
        let before = s.cmds_sent;
        let t = Instant::now();
        for k in 0..10_000 {
            s.send(Cmd::MasterVolume(k as f32));
        }
        assert!(t.elapsed() >= Duration::from_millis(150), "it waited for the engine");
        holder.join().unwrap();
        s.wait_for_engine();
        assert_eq!(s.cmds_sent - before, 10_000);
        assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
        assert_eq!(engine_volume(&s), 9999.0);
        assert_eq!(dropped(&s), 0);
        assert!(!s.engine_resync && s.warning.is_none());
    }

    /// With no device, each command is applied as it's sent, however many there are.
    #[test]
    fn without_a_device_commands_apply_at_once() {
        let mut s = session();
        s.audio = None;
        for k in 0..10_000 {
            s.send(Cmd::MasterVolume(k as f32));
            assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
        }
        assert_eq!(engine_volume(&s), 9999.0);
        assert_eq!(dropped(&s), 0);
        assert!(!s.engine_resync && s.warning.is_none());
    }

    /// A callback that takes nothing for longer than `SEND_WAIT`: the edit that finds the queue
    /// full waits that long, is dropped, counted and warned about, and those after it are dropped
    /// at once. When the engine takes commands again it plays what was queued, out of step with the
    /// project, until `idle` stops the transport and rebuilds it from the project.
    #[test]
    fn an_engine_taking_nothing_is_rebuilt_once_it_does() {
        let mut s = session();
        s.set_master_volume(-6.0);
        s.play();
        s.wait_for_engine();
        assert!(s.is_playing());
        let engine = s.engine.clone();
        let callback_held_off = engine.lock();
        for _ in 0..s.cmd_tx.slots() {
            s.send(Cmd::MasterVolume(0.5));
        }
        let t = Instant::now();
        s.set_master_volume(-12.0);
        assert!(t.elapsed() >= SEND_WAIT);
        assert_eq!(dropped(&s), 1);
        assert!(s.engine_resync);
        assert!(s.warning.take().is_some_and(|w| w.contains("stopped taking changes")));
        let t = Instant::now();
        for _ in 0..100 {
            s.set_master_volume(-18.0);
        }
        assert!(t.elapsed() < SEND_WAIT / 4, "later commands don't wait: {:?}", t.elapsed());
        assert_eq!(dropped(&s), 101);
        // Not while the engine has yet to take what's queued.
        s.resync_engine();
        assert!(s.engine_resync && s.is_playing());

        drop(callback_held_off);
        s.wait_for_engine();
        assert_eq!(engine_volume(&s), 0.5, "the engine is out of step with the project");
        // As the app's idle timer does: an idle may send commands of its own first, and the
        // rebuild waits for the engine to take them.
        let idle_until = |s: &mut Session, done: fn(&Session) -> bool| {
            for _ in 0..100 {
                s.idle();
                s.wait_for_engine();
                if done(s) {
                    return;
                }
            }
            panic!("idle didn't get there");
        };
        idle_until(&mut s, |s| !s.is_playing());
        assert!(s.engine_resync, "first the transport stops");
        let w = s.warning.take();
        assert!(w.as_deref().is_some_and(|w| w.contains("Playback stopped")), "{w:?}");
        idle_until(&mut s, |s| !s.engine_resync);
        assert_eq!(engine_volume(&s), volume_gain(-18.0));

        // And commands go as before.
        s.set_master_volume(-3.0);
        s.wait_for_engine();
        assert_eq!(engine_volume(&s), volume_gain(-3.0));
        assert_eq!(dropped(&s), 101);
        assert!(s.warning.is_none());
    }

    /// A command the engine has yet to take, as when the device stops before its next callback:
    /// pushed straight onto the queue, with nothing to drain it.
    fn queue(s: &mut Session, cmd: Cmd) {
        assert!(s.audio.is_none());
        assert!(s.cmd_tx.push(cmd).is_ok());
        s.cmds_sent += 1;
    }

    /// A new track the engine hasn't taken yet: in the project, its strip still in the queue.
    fn queue_new_track(s: &mut Session) -> Id {
        let first = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let mut t = s.project.track(first).unwrap().clone();
        t.id = s.project.alloc_id();
        s.project.tracks.push(t.clone());
        let strip = s.build_track(&t);
        queue(s, Cmd::AddTrack(strip, usize::MAX));
        t.id
    }

    /// The engine is rebuilt (reopening the device at another rate, opening a project) while
    /// commands for the old graph are still queued, as when the device has yet to call back: they
    /// are applied to that graph before the swap, not on top of the new one.
    #[test]
    fn a_rebuild_applies_what_is_queued_to_the_old_graph() {
        let mut s = session();
        s.audio = None;
        // The same project: the queued strip would be a second one for its track.
        let id = queue_new_track(&mut s);
        s.rebuild_graph();
        assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
        let ids = s.engine.lock().track_ids_for_tests();
        assert_eq!(ids.iter().filter(|&&t| t == id).count(), 1, "one strip per track: {ids:?}");
        assert_eq!(ids.len(), s.project.tracks.len());

        // Another project: the old one's strip would play in it, under an id of the new one's.
        queue_new_track(&mut s);
        s.project = Project::new("Other", s.project.sample_rate);
        s.rebuild_graph();
        assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent);
        assert_eq!(s.engine.lock().track_ids_for_tests(), Vec::<Id>::new());
    }

    /// Reopened, the device takes what the old one didn't, in order, and the count of commands
    /// done still matches those sent, so waiting for the engine waits for what's sent afterwards.
    #[test]
    fn reopening_the_device_keeps_the_queue_and_its_count() {
        let mut s = session();
        s.audio = None;
        let id = queue_new_track(&mut s);
        s.restart_audio();
        assert!(s.audio.is_some());
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(s.status.cmds_done.load(Ordering::Acquire), s.cmds_sent, "the queue was taken and counted");
        assert!(s.engine.lock().track_ids_for_tests().contains(&id));
        s.set_master_volume(-9.0);
        s.wait_for_engine();
        assert_eq!(engine_volume(&s), volume_gain(-9.0));
    }
}
