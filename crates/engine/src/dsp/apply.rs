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
        self.reclaim_all();
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
        self.reclaim_all();
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
        self.reclaim_all();
        self.flush_home_notes();
    }

    /// `flush_all_notes` on the audio thread outside a command (an overload, a block given up
    /// on), which mustn't wait for a render thread: a strip with one still in it keeps its notes
    /// until it's home, and the Stop that follows an overload ends them.
    pub(super) fn flush_notes_now(&mut self) {
        self.recall_all(false);
        self.flush_home_notes();
    }

    /// `flush_all_notes` for the strips the callback plays the source of: at the cycle's wrap,
    /// a render thread flushes a lent strip's as its own course wraps (see `ahead/`).
    pub(super) fn flush_home_notes(&mut self) {
        for t in &mut self.tracks {
            if !t.ahead.as_ref().is_some_and(|a| a.lent()) {
                t.flush_notes = true;
                t.chase_cc = true;
            }
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
        self.reclaim_all();
        for t in &mut self.tracks {
            t.chase_cc = true;
        }
    }

    /// After a jump in time, plug-in automation is resent even where the value didn't change,
    /// since the plug-in may have been moved by hand in between.
    pub(super) fn resend_automation(&mut self) {
        self.reclaim_all();
        for t in &mut self.tracks {
            t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
        }
        self.master_auto.sent = [f32::NAN; MAX_AUTO_SENT];
    }

    /// Ends a Stop's drain (see `stopping`) now: the transport stops where it was stopped.
    pub(super) fn finish_stop(&mut self) {
        if let Some((at, _)) = self.stopping.take() {
            self.pos = at;
            self.playing = false;
            self.draining = false;
        }
    }

    /// The transport as the session is told it: stopped, while a Stop's drain runs on.
    fn shown_transport(&self) -> (i64, bool) {
        self.stopping.map_or((self.pos, self.playing), |(at, _)| (at, false))
    }

    /// Publishes the position and whether the transport plays and records, for the session.
    pub(super) fn publish_transport(&self) {
        let (pos, playing) = self.shown_transport();
        self.status.pos.store(pos, Ordering::Relaxed);
        self.status.playing.store(playing, Ordering::Relaxed);
        self.status.recording.store(self.recording, Ordering::Relaxed);
    }

    /// Takes one command in, the strips it changes home (`reclaim_for`). Arms of more than a few
    /// lines call the methods below, which follow in the same order.
    pub(super) fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { from } => self.start_transport(from, false),
            Cmd::Record { from, punch } => {
                self.punch = punch;
                self.start_transport(from, true);
            }
            Cmd::Stop => self.stop_transport(),
            Cmd::Locate(p) => self.locate(p),
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
            Cmd::RemoveTrack(id) => self.remove_track(id),
            Cmd::Order(ids) => {
                self.tracks.sort_by_key(|t| ids.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
                self.discard(Garbage::Ids(ids));
                self.resolve_routes();
            }
            Cmd::Params(id, p) => self.set_params(id, p),
            Cmd::MasterVolume(v) => self.master_volume = v,
            Cmd::Regions(id, r) => {
                if let Some(t) = self.track_mut(id) {
                    let old = std::mem::replace(&mut t.regions, r);
                    t.flush_notes = true;
                    t.chase_cc = true;
                    self.discard(Garbage::Regions(old));
                }
            }
            Cmd::Insert { track, slot, insert } => self.set_insert(track, slot, insert),
            Cmd::ReorderInserts { track, order } => self.reorder_inserts(track, order),
            Cmd::Instrument { track, insert } => self.set_instrument(track, insert),
            Cmd::Bypass { track, uid, bypass } => self.set_bypass(track, uid, bypass),
            Cmd::RecordSink { track, sink } => {
                let old = self.track_mut(track).and_then(|t| std::mem::replace(&mut t.rec, sink));
                if let Some(o) = old {
                    self.discard(Garbage::Sink(o));
                }
            }
            Cmd::AutoMonitor(on) => self.auto_monitor = on,
            Cmd::Analyzer(t) => self.analyzer_target = t,
            Cmd::MidiTarget(t) => {
                if t != self.midi_target {
                    self.release_live_notes();
                    self.midi_target = t;
                }
            }
            Cmd::Automation(MASTER_ID, lanes) => self.set_master_lanes(lanes),
            Cmd::Automation(id, lanes) => self.set_lanes(id, lanes),
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
            Cmd::Pdc { track, params, line, feed_line } => self.set_pdc(track, params, line, feed_line),
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
            Cmd::InstrumentData { track, uid, data } => self.swap_instrument_data(track, uid, data),
            Cmd::FxParam { track, uid, id, value } => self.set_fx_param(track, uid, id, value),
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
            Cmd::VstGate { track, uid, gate } => self.set_vst_gate(track, uid, gate),
            Cmd::Chase { notes, controllers } => self.chase = (notes, controllers),
            Cmd::Ahead(shared, lead) => {
                self.reclaim_all();
                self.ahead.lead_secs = lead;
                if let Some(old) = std::mem::replace(&mut self.ahead.shared, shared) {
                    self.discard(Garbage::Ahead(old));
                }
            }
            Cmd::KeepLive(ids) => self.ahead.keep_live = ids,
            Cmd::AheadCtls(ctls) => {
                // Shared, not moved out: the list goes back to the session, which drops what no
                // strip took.
                for (id, ctl) in &ctls {
                    if let Some(t) = self.track_mut(*id) {
                        if t.ahead.is_none() {
                            t.ahead = Some(ctl.clone());
                        }
                    }
                }
                self.discard(Garbage::AheadCtls(ctls));
            }
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
            Cmd::Back { token, track, still_out } => self.take_back(token, track, still_out),
        }
    }

    /// Play (or Record, with `punch` already set) from `from`.
    fn start_transport(&mut self, from: i64, recording: bool) {
        self.finish_stop();
        if from != self.pos {
            self.reset_effects();
        }
        self.pos = from;
        self.playing = true;
        self.recording = recording;
        self.resend_automation();
        self.chase_all_cc();
    }

    fn stop_transport(&mut self) {
        // The audio still in the delay compensation comes out as at the end of a render:
        // the transport runs on for it, starting nothing new, so automation follows it
        // to the end instead of leaving it at the static levels.
        if self.playing && self.stopping.is_none() && self.pdc.total > 0 && !self.offline {
            self.stopping = Some((self.pos, self.pdc.total as i64));
            self.draining = true;
        } else {
            self.finish_stop();
            self.playing = false;
        }
        self.recording = false;
        self.punch_range = None;
        self.capture = None;
        self.flush_all_notes();
    }

    fn locate(&mut self, p: i64) {
        self.finish_stop();
        if p != self.pos {
            self.reset_effects();
        }
        self.pos = p;
        self.flush_all_notes();
        self.resend_automation();
    }

    fn remove_track(&mut self, id: Id) {
        if let Some(i) = self.tracks.iter().position(|t| t.id == id) {
            let t = self.tracks.remove(i);
            if t.stand_in {
                // Whatever strip the track gets later isn't the one a thread holds.
                self.status.stand_in_gone(t.held_token);
            }
            self.discard(Garbage::Track(t));
        }
        self.resolve_routes();
    }

    /// A track's mix parameters. Routes are only worked out again when one changed.
    fn set_params(&mut self, id: Id, p: TrackParams) {
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

    /// Puts `insert` (or nothing) in an insert slot of a track or Stereo Out.
    fn set_insert(&mut self, track: Id, slot: usize, insert: Option<InsertDsp>) {
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

    /// Slot `i` takes what was in slot `order[i]`.
    fn reorder_inserts(&mut self, track: Id, order: [u8; INSERT_SLOTS]) {
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

    fn set_instrument(&mut self, track: Id, insert: Option<InsertDsp>) {
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

    /// Bypasses the insert or instrument `uid`, or brings it back.
    fn set_bypass(&mut self, track: Id, uid: Id, bypass: bool) {
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

    /// A track's automation lanes (Stereo Out's are `set_master_lanes`).
    fn set_lanes(&mut self, id: Id, lanes: Arc<Vec<LaneDsp>>) {
        if let Some(t) = self.track_mut(id) {
            let old = std::mem::replace(&mut t.automation, lanes);
            t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
            self.discard(Garbage::Automation(old));
        } else {
            self.discard(Garbage::Automation(lanes));
        }
    }

    /// A track's delay compensation, and the delay lines for it and for what feeds it. A line
    /// that isn't taken goes back as garbage, as does what a new one replaced.
    fn set_pdc(&mut self, track: Id, params: PdcParams, line: Option<Box<DelayLine>>, feed_line: Option<Box<DelayLine>>) {
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

    fn swap_instrument_data(&mut self, track: Id, uid: Id, data: crate::instrument::InstrumentData) {
        let old = match self.track_mut(track).and_then(|t| t.instrument.as_mut()).filter(|i| i.uid == uid) {
            Some(InsertDsp { plugin: Plugin::Instrument(i), .. }) => i.swap_data(data),
            // It's gone by now: the data goes straight back.
            _ => Some(data),
        };
        if let Some(o) = old {
            self.discard(Garbage::InstrumentData(o));
        }
    }

    /// A parameter of the built-in effect or instrument `uid`.
    fn set_fx_param(&mut self, track: Id, uid: Id, id: u32, value: f32) {
        if let Some(b) = self.builtin_mut(track, uid) {
            b.set(id, value);
        } else if let Some(InsertDsp { plugin: Plugin::Instrument(i), .. }) = self.track_mut(track).and_then(|t| t.instrument.as_mut()).filter(|i| i.uid == uid)
        {
            i.set(id, value);
        }
    }

    fn set_vst_gate(&mut self, track: Id, uid: Id, gate: VstGate) {
        let sits_out = self.track_mut(track).is_some_and(|t| t.left_out);
        if let Some(InsertDsp { plugin: Plugin::Vst(p), bypass, .. }) = self.insert_mut(track, uid) {
            match gate {
                // A bypassed plug-in isn't running, and neither is one on a strip sitting
                // out, so nothing would finish the fade.
                VstGate::Close if *bypass || sits_out => p.shut_gate(),
                VstGate::Close => p.close_gate(),
                VstGate::Shut => p.shut_gate(),
                VstGate::Open(settle) => p.open_gate(settle),
            }
        }
    }

    /// A plug-in that stopped responding is back (see `Cmd::Back`): the track's new strip takes
    /// its stand-in's place, and the strips that sat out come back.
    fn take_back(&mut self, token: u64, track: Option<Box<TrackDsp>>, still_out: Vec<Id>) {
        if let Some(mut new) = track {
            // Fed by a plug-in still not responding, it sits out with the rest of those.
            if still_out.binary_search(&new.id).is_ok() {
                new.sit_out();
            }
            // Only in place of the stand-in for that strip: a track deleted and brought
            // back meanwhile plays a strip of its own, which stays.
            match self.tracks.iter().position(|t| t.id == new.id && t.stand_in && t.held_token == token) {
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
