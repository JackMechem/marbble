//! The session's side of the wire to the engine: sending [`Cmd`]s, restarting the audio device,
//! and turning the project's tracks and regions into what the engine plays (`track_params`,
//! `regions_dsp`, `build_track`, `rebuild_graph`).
//!
//! Every edit ends up here. `sync_globals`, `push_all_params`, `sync_tracks` and `push_regions`
//! are how a changed project reaches the audio thread; `undo.rs` and `files.rs` call them too.

use super::*;

/// How long `send` waits for room in a full command queue before it gives up on a command:
/// longer than any device buffer, and than a block given up on in a stuck plug-in takes to end.
const SEND_WAIT: Duration = Duration::from_millis(1000);

impl Session {
    /// Queues a command for the audio thread. Should the queue (4096 commands) be full, the main
    /// thread waits for the engine to make room (`send_when_room`); the audio thread never waits.
    pub(super) fn send(&mut self, cmd: Cmd) {
        self.graph_dirty |= threads::changes_routes(&cmd);
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
    pub(super) fn resync_engine(&mut self) {
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
    pub(super) fn wait_for_engine(&self) {
        let deadline = Instant::now() + Duration::from_millis(1000);
        while self.status.cmds_done.load(Ordering::Relaxed) < self.cmds_sent && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn sample_rate(&self) -> f64 {
        self.engine_rate
    }

    /// Closes and reopens the device with the current config.
    pub fn restart_audio(&mut self) {
        self.audio = None;
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
        // The command counter restarts with the engine's view of it.
        self.cmds_sent = self.status.cmds_done.load(Ordering::Relaxed);
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
        self.memory_lock = crate::memlock::apply(self.audio_config.lock_memory);
    }

    /// Round trip from what the performer heard to when their playing reached the engine:
    /// output latency + input latency + frames waiting in the duplex ring, plus the user's
    /// manual Recording Delay.
    pub(super) fn record_latency_frames(&self) -> i64 {
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
    pub(super) fn midi_latency_frames(&self) -> i64 {
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

    pub(super) fn sync_globals(&mut self) {
        let p = &self.project;
        let cmds = [
            Cmd::Tempo(Arc::new(crate::tempo::TempoMap::from_project(p))),
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
        let tempo = crate::tempo::TempoMap::from_project(&self.project);
        if self.regions_tempo.as_ref() != Some(&tempo) {
            self.regions_tempo = Some(tempo);
            self.push_tempo_regions();
        }
    }

    /// What the engine plays for a track, with its stacks applied: a folder's fader scales its
    /// subtracks, a muted stack mutes them, and solo reaches both down (a soloed stack's
    /// subtracks) and up (the stacks a soloed subtrack plays through).
    pub(super) fn track_params(p: &Project, t: &Track) -> TrackParams {
        let ancestors = p.ancestors(t.id);
        let mut volume_db = t.volume_db;
        let mut mute = t.mute;
        let mut solo = t.solo;
        let mut stack_db = 0.0;
        let mut stack_mute = false;
        let mut bus_mute = false;
        for a in ancestors.iter().filter_map(|id| p.track(*id)) {
            if a.kind == TrackKind::Folder {
                volume_db += a.volume_db;
                stack_db += a.volume_db;
            } else {
                bus_mute |= a.mute;
            }
            mute |= a.mute;
            stack_mute |= a.mute;
            solo |= a.solo;
        }
        // A VCA offsets the fader without moving it and mutes with it; like a folder's, both
        // still apply when automation drives the track's own volume and mute.
        let (vca_db, vca_mute) = crate::vca::offset(&p.vcas, t.vca.filter(|_| crate::vca::can_assign(t.kind)));
        volume_db += vca_db;
        stack_db += vca_db;
        mute |= vca_mute;
        stack_mute |= vca_mute;
        if t.kind.is_stack() {
            solo |= p.descendants(t.id).iter().filter_map(|id| p.track(*id)).any(|d| d.solo);
        }
        // Solo follows the routing too: a soloed track keeps the auxes it feeds audible, and a
        // soloed aux keeps the tracks feeding it audible. An aux on an instrument's extra output
        // doesn't need the instrument's strip audible, so that stays out of the solo.
        if !solo && p.tracks.iter().any(|y| y.solo) {
            let soloed = |y: &Track| y.solo || p.ancestors(y.id).iter().filter_map(|a| p.track(*a)).any(|a| a.solo);
            solo = p
                .tracks
                .iter()
                .filter(|y| y.id != t.id)
                .any(|y| (soloed(y) && p.reaches(y.id, t.id)) || (y.kind == TrackKind::Aux && y.solo && p.reaches_past_fader(t.id, y.id)));
        }
        // Summing stacks and auxes are buses; a folder passes its subtracks on to wherever it
        // plays. An explicit output overrides the stack.
        let output = p.output_of(t.id);
        let mut sends = [None; SEND_SLOTS];
        if t.kind != TrackKind::Folder {
            for (k, s) in t.sends.iter().take(SEND_SLOTS).enumerate() {
                if let Some(s) = s.filter(|s| s.bus != t.id && p.is_aux(s.bus)) {
                    let gain = if s.muted { 0.0 } else { volume_gain(s.level_db) };
                    sends[k] = Some(SendParams { bus: s.bus, gain, pre_fader: s.pre_fader });
                }
            }
        }
        TrackParams {
            volume: volume_gain(volume_db.max(-96.0)),
            pan: t.pan,
            mute,
            solo,
            armed: t.armed && t.kind.can_record(),
            monitor: t.monitor,
            input: t.input,
            output,
            sends,
            stack_gain: volume_gain(stack_db.max(-96.0)),
            stack_mute,
            folder: ancestors.iter().copied().find(|id| p.track(*id).map(|a| a.kind == TrackKind::Folder).unwrap_or(false)),
            own_gain: volume_gain(t.volume_db.max(-96.0)),
            own_mute: t.mute,
            bus_mute,
            vca_gain: volume_gain(vca_db.max(-96.0)),
            vca_mute,
            off: p.is_off(t.id),
            instrument_output: t.instrument_output,
        }
    }

    /// Sends every track's settings (stacks make one track's change reach others).
    pub(super) fn push_all_params(&mut self) {
        let params: Vec<(Id, TrackParams)> = self.project.tracks.iter().map(|t| (t.id, Self::track_params(&self.project, t))).collect();
        for (id, p) in params {
            self.send(Cmd::Params(id, p));
        }
        // Routing may have changed, and with it the delays that line the paths up.
        self.update_pdc();
    }

    /// After tracks are added, removed, moved or restacked: keeps subtracks under their stack
    /// and tells the engine the processing order and routing.
    pub(super) fn sync_tracks(&mut self) {
        self.project.prune_routes();
        self.project.normalize_tree();
        self.send(Cmd::Order(self.project.process_order()));
        self.push_all_params();
        self.update_midi_target();
    }

    /// Builds the playback list for a track. Audio regions don't stack: a newer region (later in
    /// the list) hides whatever older audio it covers, like Logic's "No Overlap" playback, but
    /// without destroying the older regions.
    ///
    /// A looping region plays as one entry per pass. Loop repetitions sit beneath every region
    /// body (they are laid down first), so a loop dragged over another region never hides it,
    /// as in Logic where the region covers the loop. The fade-in shapes the start of the first
    /// pass and the fade-out the end of the last one, at the loop end.
    ///
    /// Two regions that crossfade (see [`crate::fades::crossfades`]) both play across the
    /// crossfade, the outgoing one past its end and the incoming one from before its start;
    /// outside it, the earlier one is heard before and the later one after, whichever is newer.
    pub(super) fn regions_dsp(&self, t: &Track) -> Arc<Vec<RegionDsp>> {
        if let Some(f) = self.freeze_dsp(t) {
            return f;
        }
        struct Seg {
            region: Id,
            start: i64,
            end: i64,
            offset: i64,
            gain: f32,
            clip: Arc<Clip>,
            fade_in: (i64, i64),
            fade_out: (i64, i64),
            laws: [FadeLaw; 2],
        }
        let mut segs: Vec<Seg> = Vec::new();
        let mut out = Vec::new();
        // Cuts away the older audio under `rs..re`, except a crossfade partner's (`keep`).
        let cut = |segs: &mut Vec<Seg>, rs: i64, re: i64, keep: &[Id]| {
            let mut kept = Vec::with_capacity(segs.len() + 1);
            for sg in segs.drain(..) {
                if sg.end <= rs || sg.start >= re || keep.contains(&sg.region) {
                    kept.push(sg);
                    continue;
                }
                if sg.start < rs {
                    kept.push(Seg { start: sg.start, end: rs, clip: sg.clip.clone(), laws: sg.laws.clone(), ..sg });
                }
                if sg.end > re {
                    kept.push(Seg { start: re, offset: sg.offset + (re - sg.start), ..sg });
                }
            }
            *segs = kept;
        };
        // Lays a new piece of audio over what's there.
        let lay = |segs: &mut Vec<Seg>, new: Seg, keep: &[Id]| {
            cut(segs, new.start, new.end, keep);
            segs.push(new);
        };
        // Regions with playback parameters (see `session/region_params.rs`).
        let map = self.params_map(t);
        let delayed = map.as_ref().and_then(|m| Self::delay_audio(t, m));
        let regions = delayed.as_deref().unwrap_or(&t.regions);
        // Pattern regions play the notes their patterns make (see `session/patterns.rs`).
        let patterns = self.realize_patterns(regions);
        let regions = patterns.as_deref().unwrap_or(regions);
        // Settings › Editing › Crossfade Time and Curve, for comp boundaries and loop seams.
        let xfade = self.crossfade_frames();
        let comp_law = FadeLaw::Curve(self.prefs.editing.crossfade_curve.clamp(-1.0, 1.0));
        let clip_frames = |c: Id| self.clips.get(&c).map(|c| c.frames() as i64);
        let crossfades = crate::fades::crossfades(regions, clip_frames);
        // Loop repetitions first, then the region bodies, so bodies always win.
        for repeats in [true, false] {
            for r in regions.iter().filter(|r| !r.muted) {
                let passes = r.passes();
                let picked = if repeats { &passes[1..] } else { &passes[..1] };
                match &r.content {
                    // A take folder plays its comp: each pass covers what's under it, then its
                    // pieces go in side by side so their crossfades overlap. The folder's own
                    // fades take its shape, the comp's crossfades the Crossfade Curve. Crossfades
                    // with neighbours replace the folder's own fades at those ends.
                    RegionContent::Audio { gain_db, fade_in, fade_out, fade_shape, .. } if r.takes.is_some() => {
                        let folder = r.takes.as_ref().unwrap();
                        let last = passes.last().map(|p| p.1).unwrap_or(r.length);
                        let fi = (*fade_in).clamp(0, r.length);
                        let fo = if r.is_looped() { (*fade_out).clamp(0, last) } else { (*fade_out).clamp(0, r.length - fi) };
                        let x_in = crossfades.iter().find(|x| x.in_region == r.id);
                        let x_out = crossfades.iter().find(|x| x.out_region == r.id);
                        let own_in = x_in.map(|x| (x.fade_in, x.laws().1)).unwrap_or(((r.start, fi), FadeLaw::fade_in(fade_shape)));
                        let own_out = x_out.map(|x| (x.fade_out, x.laws().0)).unwrap_or(((r.end() - fo, fo), FadeLaw::fade_out(fade_shape)));
                        let keep: Vec<Id> = x_in.map(|x| x.out_region).into_iter().chain(x_out.map(|x| x.in_region)).collect();
                        for &(ps, pl) in picked {
                            let mut edges = crate::takes::PassEdges::default();
                            if let (Some(x), true) = (x_in, ps == r.start) {
                                (edges.lead, edges.fade_in) = (ps - x.fade_in.0, Some(x.fade_in));
                            }
                            if let (Some(x), true) = (x_out, ps + pl >= r.end()) {
                                (edges.tail, edges.fade_out) = (x.fade_out.0 + x.fade_out.1 - (ps + pl), Some(x.fade_out));
                            }
                            cut(&mut segs, ps - edges.lead.max(0), ps + pl + edges.tail.max(0), &keep);
                            for p in crate::takes::pass_pieces_edged(r, folder, ps, pl, xfade, edges) {
                                let Some(c) = self.clips.get(&p.clip) else { continue };
                                let gain = volume_gain(*gain_db + p.gain_db);
                                let law_in = if own_in.0 .1 > 0 && p.fade_in == own_in.0 { own_in.1.clone() } else { comp_law.clone() };
                                let law_out = if own_out.0 .1 > 0 && p.fade_out == own_out.0 { own_out.1.clone() } else { comp_law.clone() };
                                segs.push(Seg {
                                    region: r.id,
                                    start: p.start,
                                    end: p.end,
                                    offset: p.offset,
                                    gain,
                                    clip: c.clone(),
                                    fade_in: p.fade_in,
                                    fade_out: p.fade_out,
                                    laws: [law_in, law_out],
                                });
                            }
                        }
                    }
                    RegionContent::Audio { clip, offset, gain_db, fade_in, fade_out, fade_shape } => {
                        let Some(c) = self.clips.get(clip) else { continue };
                        let (end, last) = (r.end(), passes.last().map(|p| p.1).unwrap_or(r.length));
                        let fi = (*fade_in).clamp(0, r.length);
                        let fo = if r.is_looped() { (*fade_out).clamp(0, last) } else { (*fade_out).clamp(0, r.length - fi) };
                        // The first pass fades in and the last fades out, with the region's own
                        // fades or a crossfade, which stretches the region to cover it.
                        let mut first_in = ((r.start, fi), FadeLaw::fade_in(fade_shape));
                        let mut last_out = ((end - fo, fo), FadeLaw::fade_out(fade_shape));
                        let mut keep = Vec::new();
                        if let Some(x) = crossfades.iter().find(|x| x.in_region == r.id) {
                            first_in = (x.fade_in, x.laws().1);
                            keep.push(x.out_region);
                        }
                        if let Some(x) = crossfades.iter().find(|x| x.out_region == r.id) {
                            last_out = (x.fade_out, x.laws().0);
                            keep.push(x.in_region);
                        }
                        // Repetitions that crossfade overlap each other at the seams.
                        let seams = crate::fades::loop_seams(r, clip_frames, xfade);
                        if !seams.is_empty() {
                            keep.push(r.id);
                        }
                        let seam_law = FadeLaw::Curve(crate::fades::DEFAULT_CURVE);
                        let (first, n) = (r.start, passes.len());
                        for &(ps, pl) in picked {
                            let k = passes.iter().position(|p| p.0 == ps).unwrap_or(0);
                            let (fin, law_in) = match (ps == first, k.checked_sub(1).and_then(|j| seams.get(j))) {
                                (true, _) => first_in.clone(),
                                (false, Some(z)) => (z.1, seam_law.clone()),
                                (false, None) => ((ps, 0), FadeLaw::default()),
                            };
                            let (fout, law_out) = match (k + 1 == n, seams.get(k)) {
                                (true, _) => last_out.clone(),
                                (false, Some(z)) => (z.0, seam_law.clone()),
                                (false, None) => ((ps + pl, 0), FadeLaw::default()),
                            };
                            let (s, e) = (ps.min(fin.0), (ps + pl).max(fout.0 + fout.1));
                            let seg = Seg {
                                region: r.id,
                                start: s,
                                end: e,
                                offset: *offset + (s - ps),
                                gain: volume_gain(*gain_db),
                                clip: c.clone(),
                                fade_in: fin,
                                fade_out: fout,
                                laws: [law_in, law_out],
                            };
                            lay(&mut segs, seg, &keep);
                        }
                    }
                    RegionContent::Midi { .. } if !r.params.is_default() => {
                        out.extend(Self::midi_passes(r, picked, map.as_ref().unwrap()));
                    }
                    RegionContent::Midi { notes, cc } => {
                        if picked.is_empty() {
                            continue;
                        }
                        let mut n: Vec<Note> = notes.iter().filter(|n| !n.muted).copied().collect();
                        n.sort_by_key(|n| n.start);
                        let mut c = cc.clone();
                        c.sort_by_key(|c| c.start);
                        // Every pass shares one note list and one controller list.
                        let (n, c) = (Arc::new(n), Arc::new(c));
                        for &(ps, pl) in picked {
                            out.push(RegionDsp::Midi { start: ps, length: pl, notes: n.clone(), cc: c.clone() });
                        }
                    }
                }
            }
        }
        segs.sort_by_key(|s| s.start);
        out.extend(segs.into_iter().map(|s| RegionDsp::Audio {
            start: s.start,
            length: s.end - s.start,
            offset: s.offset,
            gain: s.gain,
            clip: s.clip,
            fade_in: s.fade_in,
            fade_out: s.fade_out,
            laws: s.laws,
        }));
        Arc::new(out)
    }

    pub(super) fn push_regions(&mut self, track: Id) {
        // An undo, a switched alternative or a paste may bring back a clip that isn't loaded.
        let clips = self.project.track(track).map(|t| t.clips()).unwrap_or_default();
        self.load_clips(clips);
        if let Some(t) = self.project.track(track) {
            let r = self.regions_dsp(t);
            self.send(Cmd::Regions(track, r));
        }
    }

    pub(super) fn instantiate_slot(&mut self, slot: &PluginSlot) -> Result<InsertDsp> {
        let sr = self.sample_rate();
        // Undo frees instance ids for reuse: one that held a built-in effect may come back as
        // another kind of plug-in, which mustn't inherit its latency or meters.
        if !builtin::is_builtin(&slot.plugin_id) {
            self.fx.remove(&slot.uid);
        }
        let plugin = if let Some(info) = crate::fx::find(&slot.plugin_id) {
            self.instantiate_builtin(&info, slot)
        } else if let Some(info) = crate::instrument::find(&slot.plugin_id) {
            self.instantiate_instrument(&info, slot)
        } else {
            let desc = self.find_plugin(&slot.plugin_id).ok_or_else(|| anyhow!("plug-in not found: {}", slot.name))?.clone();
            let (proc, ctrl) = vst3_host::instantiate_keyed(&desc, sr, MAX_BLOCK, slot.sidechain.is_some()).map_err(|e| anyhow!(e))?;
            if let Some(st) = &slot.state {
                ctrl.load_state(&decode_state(st));
            }
            self.track_state_copy(slot, &ctrl);
            self.controllers.insert(slot.uid, ctrl);
            Plugin::Vst(Box::new(proc))
        };
        Ok(InsertDsp { uid: slot.uid, bypass: slot.bypass, plugin, key: KeyRoute::new(slot.sidechain, slot.sidechain_pre) })
    }

    pub(super) fn build_track(&mut self, t: &Track) -> Box<TrackDsp> {
        let meter = self.meters.entry(t.id).or_default().clone();
        let mut dsp = TrackDsp::new(t.id, t.kind, Self::track_params(&self.project, t), meter);
        self.load_clips(t.clips());
        dsp.set_regions(self.regions_dsp(t));
        dsp.set_automation(lanes_dsp(&t.automation), t.auto_mode);
        // A new engine track starts without delay compensation; update_pdc sends it.
        self.pdc_sent.remove(&t.id);
        if t.frozen.is_some() {
            // A frozen track plays its freeze file; its plug-ins stay unloaded.
            return Box::new(dsp);
        }
        if let Some(slot) = &t.instrument {
            match self.instantiate_slot(slot) {
                Ok(i) => dsp.set_instrument(Some(i)),
                Err(e) => log::error!("{}: {e}", t.name),
            }
        }
        for (i, slot) in t.inserts.iter().enumerate() {
            if let Some(slot) = slot {
                match self.instantiate_slot(slot) {
                    Ok(ins) => dsp.set_insert(i, Some(ins)),
                    Err(e) => log::error!("{}: {e}", t.name),
                }
            }
        }
        Box::new(dsp)
    }

    /// Recreates every track and plugin from the project document.
    pub(super) fn rebuild_graph(&mut self) {
        // Every command dropped is made good by this, unless the engine has yet to apply some sent
        // before it, which would undo part of what this sets up.
        self.engine_resync &= !self.cmd_queue_empty();
        // The plug-ins threads are still left in keep their editors hidden, untold.
        self.mark_unresponsive();
        let snapshot = self.capture_plugin_states();
        self.adopt_states(&snapshot, true);
        for c in self.controllers.values_mut() {
            c.close_editor();
        }
        let old_controllers = std::mem::take(&mut self.controllers);
        self.fx.clear();
        let tracks: Vec<Track> = snapshot.tracks.clone();
        let mut built: Vec<Box<TrackDsp>> = tracks.iter().map(|t| self.build_track(t)).collect();
        // The engine plays tracks in this order; routes only point forwards.
        let order = snapshot.process_order();
        built.sort_by_key(|t| order.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
        let mut master = Vec::new();
        for slot in &snapshot.master.inserts {
            master.push(slot.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok()));
        }
        let sr = self.project.sample_rate as f64;
        // The one lock outside an offline render: a swap, with the old graph dropped after.
        let old = self.engine.lock().replace_graph(sr, built, master);
        self.engine_rate = sr;
        self.graph_dirty = true;
        drop(old);
        drop(old_controllers);
        // Another project (or the same one reloaded): its strips start without clips.
        self.peak_holds.reset(None);
        self.sync_master_automation();
        self.sync_globals();
    }

    /// The project with each plugin's current state embedded (for saving or rebuilding). A plug-in
    /// a processing thread is left in (`session/left_out.rs`) isn't asked, as it may never answer:
    /// it keeps the state the project last had for it.
    pub(super) fn capture_plugin_states(&self) -> Project {
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

    pub(super) fn midi_target_id(&self) -> Option<Id> {
        let sel = self.selected.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Instrument).unwrap_or(false));
        sel.or_else(|| self.project.tracks.iter().find(|t| t.kind == TrackKind::Instrument && t.armed && !self.project.is_off(t.id)).map(|t| t.id))
    }

    pub(super) fn update_midi_target(&mut self) {
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
}
