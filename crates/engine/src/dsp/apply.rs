//! How [`Cmd`]s take effect on the audio thread: [`Engine::apply`], and the helpers it and block
//! processing use to find a strip or insert, or to silence, flush or re-chase notes and automation.
//!
//! Each command swaps prepared state in and sends what it replaced back as [`Garbage`]. Everything
//! here runs on the audio thread and must not allocate.

use super::*;

impl Engine {
    pub(super) fn discard(&self, g: Garbage) {
        // The ring is bounded; if it is somehow full, leaking beats blocking the audio thread.
        if let Err(rtrb::PushError::Full(g)) = self.garbage.borrow_mut().push(g) {
            std::mem::forget(g);
        }
    }

    fn track_mut(&mut self, id: Id) -> Option<&mut Box<TrackDsp>> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Finds where each track's output and sends go. Receivers come after their senders in the
    /// list (see `Project::process_order`); a route pointing backwards is ignored, and an
    /// output then plays straight to Stereo Out.
    pub(super) fn resolve_routes(&mut self) {
        for i in 0..self.tracks.len() {
            let find = |id: Id| self.tracks.iter().position(|t| t.id == id).filter(|&j| j > i);
            let out = self.tracks[i].params.output.and_then(find);
            let mut sends = [None; SEND_SLOTS];
            for (k, s) in self.tracks[i].params.sends.iter().enumerate() {
                sends[k] = s.and_then(|s| find(s.bus));
            }
            let fold = self.tracks[i].params.folder.and_then(find);
            // Its instrument plays earlier in the list.
            let source = self.tracks[i].params.instrument_output.and_then(|o| self.tracks[..i].iter().position(|t| t.id == o.track));
            let t = &mut self.tracks[i];
            t.out_idx = out;
            t.send_idx = sends;
            t.fold_idx = fold;
            t.feed.source = source;
        }
        self.resolve_keys();
        let out = self.tracks.iter().filter(|t| t.left_out).count();
        self.status.left_out.store(out as u32, Ordering::Relaxed);
    }

    /// Finds each insert's side-chain source. A key must come from a track played
    /// earlier in the list (Stereo Out's plug-ins can be keyed by any track); others hear
    /// nothing.
    fn resolve_keys(&mut self) {
        // Whatever moved, the task graph needs looking at again before it's trusted.
        self.parallel.routes_changed();
        for t in &mut self.tracks {
            t.keyed = false;
            t.keyed_pre = false;
        }
        let n = self.tracks.len();
        for i in 0..=n {
            let count = if i < n { self.tracks[i].inserts.len() } else { self.master_inserts.len() };
            for k in 0..count {
                let slots = if i < n { &self.tracks[i].inserts } else { &self.master_inserts };
                let Some(ins) = &slots[k] else { continue };
                let KeyRoute { source, pre, .. } = ins.key;
                let idx = source.and_then(|id| self.tracks[..i].iter().position(|t| t.id == id));
                let slots = if i < n { &mut self.tracks[i].inserts } else { &mut self.master_inserts };
                if let Some(ins) = &mut slots[k] {
                    ins.key.idx = idx;
                }
                if let Some(j) = idx {
                    if pre {
                        self.tracks[j].keyed_pre = true;
                    } else {
                        self.tracks[j].keyed = true;
                    }
                }
            }
        }
    }

    /// Clears every built-in effect's state (envelopes, delay lines, tails), after a jump in
    /// time and around offline renders, so nothing from before rings into what plays next.
    pub(super) fn reset_effects(&mut self) {
        let slots = self.tracks.iter_mut().flat_map(|t| t.inserts.iter_mut()).chain(self.master_inserts.iter_mut());
        for ins in slots.flatten() {
            if let Plugin::Builtin(b) = &mut ins.plugin {
                b.reset();
            }
        }
    }

    /// The insert `uid` on a track's (or Stereo Out's) inserts.
    fn insert_mut(&mut self, track: Id, uid: Id) -> Option<&mut InsertDsp> {
        let slots = if track == MASTER_ID { &mut self.master_inserts } else { &mut self.track_mut(track)?.inserts };
        slots.iter_mut().flatten().find(|i| i.uid == uid)
    }

    /// The built-in effect `uid` on a track's (or Stereo Out's) inserts.
    fn builtin_mut(&mut self, track: Id, uid: Id) -> Option<&mut BuiltinDsp> {
        self.insert_mut(track, uid).and_then(|i| match &mut i.plugin {
            Plugin::Builtin(b) => Some(&mut **b),
            _ => None,
        })
    }

    /// Built-in instruments stop dead, release tails included (offline renders start and end
    /// with this, so one render's tail never reaches the next). VST3 instruments get note-offs
    /// from `flush_all_notes`, as before.
    pub(super) fn silence_instruments(&mut self) {
        for t in &mut self.tracks {
            if let Some(InsertDsp { plugin: Plugin::Instrument(i), .. }) = &mut t.instrument {
                i.reset();
            }
        }
    }

    /// The MIDI a track's instrument got in the last block.
    #[cfg(test)]
    pub(crate) fn midi_sent(&self, track: Id) -> &[MidiEvent] {
        self.tracks.iter().find(|t| t.id == track).map_or(&[], |t| &t.midi)
    }

    pub(super) fn flush_all_notes(&mut self) {
        for t in &mut self.tracks {
            t.flush_notes = true;
            t.chase_cc = true;
        }
    }

    /// Live MIDI is about to stop reaching the target, so the note-offs of the keys held on it
    /// never would. Only that track, and only if a key is down: what the regions play has nothing
    /// to do with where live MIDI goes.
    fn release_live_notes(&mut self) {
        let Some(id) = self.midi_target else { return };
        if let Some(t) = self.track_mut(id) {
            if t.live_held.contains(&true) {
                t.adopt_live_notes();
                t.flush_notes = true;
                t.chase_cc = true;
            }
        }
    }

    pub(super) fn chase_all_cc(&mut self) {
        for t in &mut self.tracks {
            t.chase_cc = true;
        }
    }

    /// After a jump in time, plug-in automation is resent even where the value didn't change,
    /// since the plug-in may have been moved by hand in between.
    pub(super) fn resend_automation(&mut self) {
        for t in &mut self.tracks {
            t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
        }
        self.master_auto.sent = [f32::NAN; MAX_AUTO_SENT];
    }

    pub(super) fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { from } => {
                if from != self.pos {
                    self.reset_effects();
                }
                self.pos = from;
                self.playing = true;
                self.recording = false;
                self.resend_automation();
                self.chase_all_cc();
            }
            Cmd::Record { from, punch } => {
                if from != self.pos {
                    self.reset_effects();
                }
                self.pos = from;
                self.punch = punch;
                self.playing = true;
                self.recording = true;
                self.resend_automation();
                self.chase_all_cc();
            }
            Cmd::Stop => {
                self.playing = false;
                self.recording = false;
                self.punch_range = None;
                self.capture = None;
                self.flush_all_notes();
            }
            Cmd::Locate(p) => {
                if p != self.pos {
                    self.reset_effects();
                }
                self.pos = p;
                self.flush_all_notes();
                self.resend_automation();
            }
            Cmd::Tempo(map) => {
                let old = std::mem::replace(&mut self.tempo, map);
                self.discard(Garbage::Tempo(old));
            }
            Cmd::Cycle { enabled, start, end } => self.cycle = (enabled && end > start, start, end),
            Cmd::Metronome { on, count_in } => {
                self.metronome = on;
                self.count_in = count_in;
            }
            Cmd::AddTrack(t, idx) => {
                let idx = idx.min(self.tracks.len());
                self.tracks.insert(idx, t);
                self.resolve_routes();
            }
            Cmd::RemoveTrack(id) => {
                if let Some(i) = self.tracks.iter().position(|t| t.id == id) {
                    let t = self.tracks.remove(i);
                    self.discard(Garbage::Track(t));
                }
                self.resolve_routes();
            }
            Cmd::Order(ids) => {
                self.tracks.sort_by_key(|t| ids.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
                self.discard(Garbage::Ids(ids));
                self.resolve_routes();
            }
            Cmd::Params(id, p) => {
                if let Some(t) = self.track_mut(id) {
                    let bus = |s: &Option<SendParams>| s.map(|s| s.bus);
                    let rerouted = t.params.output != p.output
                        || t.params.folder != p.folder
                        || t.params.instrument_output != p.instrument_output
                        || t.params.sends.iter().map(bus).ne(p.sends.iter().map(bus));
                    t.params = p;
                    if rerouted {
                        self.resolve_routes();
                    }
                }
            }
            Cmd::MasterVolume(v) => self.master_volume = v,
            Cmd::Regions(id, r) => {
                if let Some(t) = self.track_mut(id) {
                    let old = std::mem::replace(&mut t.regions, r);
                    t.flush_notes = true;
                    t.chase_cc = true;
                    self.discard(Garbage::Regions(old));
                }
            }
            Cmd::Insert { track, slot, insert } => {
                let old = if track == MASTER_ID {
                    self.master_auto.sent = [f32::NAN; MAX_AUTO_SENT];
                    self.master_inserts.get_mut(slot).and_then(|s| std::mem::replace(s, insert))
                } else {
                    // A new instance starts at its own values: automation is sent to it afresh,
                    // even if its lanes came before it (an undo restores lanes first).
                    self.track_mut(track).and_then(|t| {
                        t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                        t.inserts.get_mut(slot).and_then(|s| std::mem::replace(s, insert))
                    })
                };
                if let Some(o) = old {
                    self.discard(Garbage::Insert(o));
                }
                self.resolve_keys();
            }
            Cmd::ReorderInserts { track, order } => {
                let slots = if track == MASTER_ID { Some(&mut self.master_inserts) } else { self.track_mut(track).map(|t| &mut t.inserts) };
                if let Some(slots) = slots {
                    // Moved out and back in through a fixed array: nothing is allocated.
                    let mut taken: [Option<InsertDsp>; INSERT_SLOTS] = std::array::from_fn(|i| slots.get_mut(i).and_then(Option::take));
                    for (i, &from) in order.iter().enumerate() {
                        if let Some(s) = slots.get_mut(i) {
                            *s = taken[from as usize].take();
                        }
                    }
                }
                self.resolve_keys();
            }
            Cmd::Instrument { track, insert } => {
                let old = self.track_mut(track).and_then(|t| {
                    t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                    // The new one hasn't heard the notes being held.
                    t.flush_notes = true;
                    t.chase_cc = true;
                    std::mem::replace(&mut t.instrument, insert)
                });
                if let Some(o) = old {
                    self.discard(Garbage::Insert(o));
                }
            }
            Cmd::Bypass { track, uid, bypass } => {
                let set = |s: &mut Option<InsertDsp>| {
                    if let Some(i) = s {
                        if i.uid == uid {
                            i.bypass = bypass;
                        }
                    }
                };
                if track == MASTER_ID {
                    self.master_inserts.iter_mut().for_each(set);
                } else if let Some(t) = self.track_mut(track) {
                    t.inserts.iter_mut().for_each(set);
                    set(&mut t.instrument);
                    // While bypassed it hears no note-offs: what it holds ends now, or it would
                    // come back still holding them.
                    if let Some(InsertDsp { uid: u, plugin: Plugin::Instrument(i), .. }) = &mut t.instrument {
                        if bypass && *u == uid {
                            i.all_notes_off();
                        }
                    }
                }
            }
            Cmd::RecordSink { track, sink } => {
                let old = self.track_mut(track).and_then(|t| std::mem::replace(&mut t.rec, sink));
                if let Some(o) = old {
                    self.discard(Garbage::Sink(o));
                }
            }
            Cmd::AutoMonitor(on) => self.auto_monitor = on,
            Cmd::Eq(id, c) => {
                if id == MASTER_ID {
                    if !c.active {
                        self.master_eq_state.reset();
                    }
                    self.master_eq = c;
                } else if let Some(t) = self.track_mut(id) {
                    t.set_eq(c);
                }
            }
            Cmd::Analyzer(t) => self.analyzer_target = t,
            Cmd::MidiTarget(t) => {
                if t != self.midi_target {
                    self.release_live_notes();
                    self.midi_target = t;
                }
            }
            Cmd::Automation(MASTER_ID, lanes) => self.set_master_lanes(lanes),
            Cmd::Automation(id, lanes) => {
                if let Some(t) = self.track_mut(id) {
                    let old = std::mem::replace(&mut t.automation, lanes);
                    t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                    self.discard(Garbage::Automation(old));
                } else {
                    self.discard(Garbage::Automation(lanes));
                }
            }
            Cmd::AutoMode(MASTER_ID, mode) => {
                self.master_auto.mode = mode;
                self.master_auto.sent = [f32::NAN; MAX_AUTO_SENT];
            }
            Cmd::AutoMode(id, mode) => {
                if let Some(t) = self.track_mut(id) {
                    t.auto_mode = mode;
                    t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                }
            }
            Cmd::AutoTouch { track: MASTER_ID, target, on } => self.set_master_touched(target, on),
            Cmd::AutoTouch { track, target, on } => {
                if let Some(t) = self.track_mut(track) {
                    t.set_touched(target, on);
                }
            }
            Cmd::Click(c) => self.click_params = c,
            Cmd::MidiThru(on) => {
                if on != self.midi_thru {
                    self.release_live_notes();
                    self.midi_thru = on;
                }
            }
            Cmd::Pdc { track, params, line, feed_line } => {
                let old_feed = match self.track_mut(track) {
                    Some(t) => t.feed.set_line(feed_line),
                    None => feed_line,
                };
                if let Some(o) = old_feed {
                    self.discard(Garbage::Delay(o));
                }
                let old = match self.track_mut(track) {
                    Some(t) => {
                        t.pdc = params;
                        match line {
                            Some(mut l) => {
                                // The taps glide from where they read now: keep that audio.
                                if let Some(old) = &t.delay {
                                    l.take_history(old);
                                }
                                t.delay.replace(l)
                            }
                            None => None,
                        }
                    }
                    None => line,
                };
                if let Some(o) = old {
                    self.discard(Garbage::Delay(o));
                }
            }
            Cmd::PdcGlobal(g) => self.pdc = g,
            Cmd::Autopunch { range, capture } => {
                self.punch_range = range;
                self.capture = Some(capture);
            }
            Cmd::InputTap { input, sink } => {
                let old = std::mem::replace(&mut self.input_tap, sink.map(|s| (input, s)));
                if let Some((_, s)) = old {
                    self.discard(Garbage::Sink(s));
                }
            }
            Cmd::InstrumentData { track, uid, data } => {
                let old = match self.track_mut(track).and_then(|t| t.instrument.as_mut()).filter(|i| i.uid == uid) {
                    Some(InsertDsp { plugin: Plugin::Instrument(i), .. }) => i.swap_data(data),
                    // It's gone by now: the data goes straight back.
                    _ => Some(data),
                };
                if let Some(o) = old {
                    self.discard(Garbage::InstrumentData(o));
                }
            }
            Cmd::FxParam { track, uid, id, value } => {
                if let Some(b) = self.builtin_mut(track, uid) {
                    b.set(id, value);
                } else if let Some(InsertDsp { plugin: Plugin::Instrument(i), .. }) =
                    self.track_mut(track).and_then(|t| t.instrument.as_mut()).filter(|i| i.uid == uid)
                {
                    i.set(id, value);
                }
            }
            Cmd::Sidechain { track, uid, source } => {
                if let Some(i) = self.insert_mut(track, uid) {
                    i.key.source = source;
                }
                self.resolve_keys();
            }
            Cmd::SidechainPre { track, uid, pre } => {
                if let Some(i) = self.insert_mut(track, uid) {
                    i.key.pre = pre;
                }
                self.resolve_keys();
            }
            Cmd::VstGate { track, uid, gate } => {
                if let Some(InsertDsp { plugin: Plugin::Vst(p), bypass, .. }) = self.insert_mut(track, uid) {
                    match gate {
                        // A bypassed plug-in isn't running, so nothing would finish the fade.
                        VstGate::Close if *bypass => p.shut_gate(),
                        VstGate::Close => p.close_gate(),
                        VstGate::Shut => p.shut_gate(),
                        VstGate::Open(settle) => p.open_gate(settle),
                    }
                }
            }
            Cmd::Chase { notes, controllers } => self.chase = (notes, controllers),
            Cmd::Workers(w) => {
                // Strips their threads have let go of go back first, for the session to bring back.
                self.release_held();
                if let Some(old) = self.parallel.set_workers(w) {
                    self.discard(Garbage::Workers(old));
                }
            }
            Cmd::Graph(g) => {
                if let Some(old) = self.parallel.set_graph(g) {
                    self.discard(Garbage::Graph(old));
                }
            }
            Cmd::ParallelTest(t) => self.parallel.test = t,
            Cmd::Back { track, still_out } => {
                if let Some(new) = track {
                    match self.tracks.iter().position(|t| t.id == new.id) {
                        Some(i) => {
                            let old = std::mem::replace(&mut self.tracks[i], new);
                            self.discard(Garbage::Track(old));
                        }
                        None => self.discard(Garbage::Track(new)),
                    }
                }
                let fade = track_off::resume_frames(self.sample_rate);
                for t in &mut self.tracks {
                    if t.left_out && !t.stand_in && still_out.binary_search(&t.id).is_err() {
                        t.come_back(fade);
                    }
                }
                self.discard(Garbage::Ids(still_out));
                self.resolve_routes();
            }
        }
    }
}
