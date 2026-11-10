//! The project's copy of each running VST3's state (`PluginSlot::state`), kept recent.
//!
//! A running plug-in's state lives in the plug-in, and is read from it to save, copy or rebuild.
//! The copy is what Marbble falls back on when it can't ask: while a processing thread is left in
//! the plug-in (`session/engine/left_out.rs`), saving, copying, a rebuild of its track, and an undo or redo
//! that brings the track back all take the copy. So it must never be far behind:
//! - A plug-in loaded without a state (just added) has its state read at the next `idle`.
//! - With its editor open, the undo watch (`editor_undo.rs`) reads the state after every edit and
//!   once the host's changes go quiet; each of those reads is the copy too.
//! - Otherwise, a change (the host setting a parameter: automation following, a controller
//!   assignment, `load_state`; `restartComponent`; edits the plug-in sends itself) makes the copy
//!   due once it has been quiet for 300 ms, read at most once a second and 1 % of the time. An
//!   `idle` with nothing changed asks no plug-in anything, and one that keeps changing (automation
//!   playing) is read once it stops.
//! - A save, an autosave, an undo or redo, a checkpoint and a rebuild capture every state anyway
//!   (`capture_plugin_states`), and the copy takes what they read (`adopt_states`). A state loaded
//!   into the plug-in (undoing an edit made in its editor) is the copy as it stands.
//! - `restore` keeps the copy of a plug-in that stays loaded, rather than the step's older one.
//!
//! A plug-in that stops responding between a change and its read keeps the copy from before it.
//! Built-in effects and instruments keep their settings in the slot itself (`params`, and the
//! Sampler's `state`), so they need none of this.

use crate::session::*;

/// How long a plug-in's changes have to pause before its state is read.
const QUIET: Duration = Duration::from_millis(300);
/// How long the read waits for edits to reach a processor that isn't taking them (`getState`
/// reads the processor's side).
const DRAIN_WAIT: Duration = Duration::from_millis(300);
/// At most this often, and spending at most 1 % of the time on a heavy plug-in.
const MIN_GAP: Duration = Duration::from_secs(1);
const SHARE: u32 = 100;

/// How current the project's copy of a running VST3's state is.
#[derive(Debug, Clone)]
pub(crate) struct StateCopy {
    /// `PluginController::host_changes` as of the copy.
    seen_host: u64,
    /// When the copy is to be read again, the plug-in having changed since (or it having none).
    due: Option<Instant>,
    read_at: Option<Instant>,
    read_took: Duration,
    /// How many times `getState` has been called for it here, for headless checks.
    reads: u32,
}

impl StateCopy {
    /// For an instance just loaded: `now` when it has no state to copy yet.
    fn new(seen_host: u64, now: Option<Instant>) -> StateCopy {
        StateCopy { seen_host, due: now, read_at: None, read_took: Duration::ZERO, reads: 0 }
    }

    /// The plug-in changed: the copy is due once that has been quiet.
    fn changed(&mut self, now: Instant) {
        self.due = Some(now + QUIET);
    }

    /// Whether to read the state now. `drained`: edits sent to the plug-in have reached its
    /// processor.
    fn due(&self, now: Instant, drained: bool) -> bool {
        self.due.is_some_and(|d| now >= d && (drained || now >= d + DRAIN_WAIT))
            && self.read_at.is_none_or(|t| now.duration_since(t) >= MIN_GAP.max(self.read_took * SHARE))
    }

    /// The copy was just read here.
    fn read(&mut self, now: Instant, took: Duration, seen_host: u64) {
        self.read_at = Some(now);
        self.read_took = took;
        self.reads += 1;
        self.current(seen_host);
    }

    /// The copy is the plug-in's state as of `seen_host`.
    fn current(&mut self, seen_host: u64) {
        self.seen_host = seen_host;
        self.due = None;
    }
}

impl Session {
    /// Starts keeping the copy of an instance's state, just loaded with `slot`'s.
    pub(crate) fn track_state_copy(&mut self, slot: &PluginSlot, c: &PluginController) {
        let now = slot.state.is_none().then(Instant::now);
        self.state_copies.insert(slot.uid, StateCopy::new(c.host_changes(), now));
    }

    /// Edits came from a plug-in: with its editor closed, its copy is due (with it open, the undo
    /// watch reads the state).
    pub(crate) fn plugin_changed(&mut self, uid: Id) {
        if self.vst_watches.contains_key(&uid) {
            return;
        }
        if let Some(e) = self.state_copies.get_mut(&uid) {
            e.changed(Instant::now());
        }
    }

    /// `st` is the plug-in's state now, read or loaded by someone else: it's the copy.
    pub(crate) fn keep_state(&mut self, uid: Id, st: &PluginState) {
        let Some(slot) = crate::session::project::recovery::slots_mut(&mut self.project).find(|s| s.uid == uid) else { return };
        slot.state = Some(encode_state(st));
        let seen_host = self.controllers.get(&uid).map(|c| c.host_changes());
        if let (Some(e), Some(h)) = (self.state_copies.get_mut(&uid), seen_host) {
            e.current(h);
        }
    }

    /// Takes the states of the running VST3s from `doc`. With `read`, it's the project with every
    /// state just captured (`capture_plugin_states`), so the copies are current; a plug-in that
    /// may not be called wasn't asked, and has its copy in `doc` already. Without, it's the
    /// project `restore` replaces, whose copies are as current as they were.
    pub(crate) fn adopt_states(&mut self, doc: &Project, read: bool) {
        let states: HashMap<Id, (&str, &Arc<str>)> =
            crate::session::project::recovery::slots(doc).filter_map(|s| Some((s.uid, (s.plugin_id.as_str(), s.state.as_ref()?)))).collect();
        for slot in crate::session::project::recovery::slots_mut(&mut self.project) {
            let Some(c) = self.controllers.get(&slot.uid) else { continue };
            // An instance id undo freed may have come back as another plug-in.
            let Some(&(_, st)) = states.get(&slot.uid).filter(|(id, _)| *id == slot.plugin_id) else { continue };
            if slot.state.as_ref() != Some(st) {
                slot.state = Some(st.clone());
            }
            if read && c.responsive() {
                if let Some(e) = self.state_copies.get_mut(&slot.uid) {
                    e.current(c.host_changes());
                }
            }
        }
    }

    /// From `idle`: reads the state of each plug-in whose copy is due. Nothing for one with its
    /// editor open, which the undo watch reads, or one that may not be called.
    pub(crate) fn state_copies_idle(&mut self) {
        let ctrls = &self.controllers;
        self.state_copies.retain(|uid, _| ctrls.contains_key(uid));
        let now = Instant::now();
        let mut read = Vec::new();
        for (&uid, e) in &mut self.state_copies {
            let Some(c) = self.controllers.get_mut(&uid) else { continue };
            if !c.responsive() || self.vst_watches.contains_key(&uid) {
                continue;
            }
            let host = c.host_changes();
            if c.take_params_restarted() || host != e.seen_host {
                e.seen_host = host;
                e.changed(now);
            }
            if !e.due(now, !c.edits_pending()) {
                continue;
            }
            let start = Instant::now();
            let Some(st) = c.save_state() else { continue };
            e.read(now, start.elapsed(), c.host_changes());
            read.push((uid, st));
        }
        for (uid, st) in read {
            if let Some(slot) = crate::session::project::recovery::slots_mut(&mut self.project).find(|s| s.uid == uid) {
                slot.state = Some(encode_state(&st));
            }
        }
    }

    /// How many times the state of a plug-in with its editor closed has been read for the
    /// project's copy, for headless checks.
    #[doc(hidden)]
    pub fn vst_copy_reads(&self, uid: Id) -> u32 {
        self.state_copies.get(&uid).map_or(0, |e| e.reads)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_plug_in_loaded_without_a_state_is_read_at_once() {
        let s = Instant::now();
        assert!(StateCopy::new(0, Some(s)).due(s, true));
        assert!(!StateCopy::new(0, None).due(at(s, 60_000), true), "loaded with one, it has its copy");
    }

    #[test]
    fn a_change_is_read_once_it_has_been_quiet() {
        let s = Instant::now();
        let mut e = StateCopy::new(0, None);
        for i in 0..50 {
            e.changed(at(s, i * 100));
            assert!(!e.due(at(s, i * 100 + 200), true), "not while it keeps changing");
        }
        assert!(e.due(at(s, 49 * 100) + QUIET, true));
        e.read(at(s, 49 * 100) + QUIET, Duration::from_micros(50), 7);
        assert!(!e.due(at(s, 60_000), true), "and not again until it changes");
        assert_eq!((e.reads, e.seen_host), (1, 7));
    }

    #[test]
    fn a_read_waits_for_edits_to_reach_the_processor_but_not_forever() {
        let s = Instant::now();
        let mut e = StateCopy::new(0, None);
        e.changed(s);
        assert!(!e.due(s + QUIET, false));
        assert!(e.due(s + QUIET, true));
        assert!(e.due(s + QUIET + DRAIN_WAIT, false));
    }

    #[test]
    fn reads_are_at_most_once_a_second_and_one_percent_of_the_time() {
        let s = Instant::now();
        let mut e = StateCopy::new(0, None);
        e.read(s, Duration::from_millis(1), 0);
        e.changed(s);
        assert!(!e.due(at(s, 900), true));
        assert!(e.due(at(s, 1000), true));
        // A heavy plug-in, 30 ms a read: every 3 s.
        e.read(at(s, 1000), Duration::from_millis(30), 0);
        e.changed(at(s, 1000));
        assert!(!e.due(at(s, 3900), true));
        assert!(e.due(at(s, 4000), true));
    }

    #[test]
    fn a_state_read_elsewhere_is_the_copy() {
        let s = Instant::now();
        let mut e = StateCopy::new(0, Some(s));
        e.current(3);
        assert!(!e.due(at(s, 60_000), true));
        assert_eq!((e.reads, e.seen_host), (0, 3), "and it's no read here");
    }

    fn session() -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    /// Only a running VST3 takes a state from a captured document: a built-in keeps its settings
    /// in its slot, and a plug-in that isn't loaded (missing, frozen) keeps the state it has.
    #[test]
    fn only_running_vst3s_adopt_captured_states() {
        let mut s = session();
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.set_plugin(t, 0, "builtin:compressor").unwrap();
        s.project.track_mut(t).unwrap().inserts[1] = Some(PluginSlot {
            uid: s.project.alloc_id(),
            plugin_id: "/missing.vst3|ABC".into(),
            name: "Missing".into(),
            bypass: false,
            state: Some("AAAA:".into()),
            params: Vec::new(),
            sidechain: None,
            sidechain_pre: false,
            preset: None,
        });
        let before = s.project.clone();
        let mut doc = s.project.clone();
        for slot in crate::session::project::recovery::slots_mut(&mut doc) {
            slot.state = Some("BBBB:".into());
        }
        s.adopt_states(&doc, true);
        let states = |p: &Project| p.track(t).unwrap().inserts.iter().flatten().map(|s| s.state.clone()).collect::<Vec<_>>();
        assert_eq!(states(&s.project), states(&before));
    }

    fn slot(uid: Id, plugin_id: &str, state: &str) -> PluginSlot {
        PluginSlot {
            uid,
            plugin_id: plugin_id.into(),
            name: "Heavy".into(),
            bypass: false,
            state: Some(state.into()),
            params: Vec::new(),
            sidechain: None,
            sidechain_pre: false,
            preset: None,
        }
    }

    /// A state can be megabytes: the undo steps and UI snapshots that copy the project share it
    /// rather than copying it, and it's written as the plain string it always was.
    #[test]
    fn states_are_shared_by_undo_steps_and_snapshots_and_saved_as_before() {
        let mut s = session();
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let big = format!("{}:", "QUJD".repeat(256 * 1024));
        let uid = s.project.alloc_id();
        s.project.track_mut(t).unwrap().inserts[0] = Some(slot(uid, "/missing.vst3|ABC", &big));
        let state = |s: &Session| s.project.track(t).unwrap().inserts[0].as_ref().unwrap().state.clone().unwrap();
        for i in 0..20 {
            s.checkpoint("Tempo", None, false);
            s.set_tempo(100.0 + i as f64);
        }
        let ui = s.project.for_ui();
        assert!(ui.track(t).unwrap().inserts[0].as_ref().unwrap().state.is_none(), "the UI never sees it");
        let st = state(&s);
        assert_eq!(Arc::strong_count(&st), 1 + 20 + 1, "the project, each step and this one share it");
        s.undo();
        assert!(Arc::ptr_eq(&state(&s), &st), "and an undo puts back the same one");

        let json = serde_json::to_value(s.project.track(t).unwrap().inserts[0].as_ref().unwrap()).unwrap();
        assert_eq!(json["state"].as_str(), Some(big.as_str()));
        let read: PluginSlot = serde_json::from_str(r#"{"uid": 7, "plugin_id": "/x.vst3|ABC", "name": "X", "state": "AAAA:BBBB"}"#).unwrap();
        assert_eq!(read.state.as_deref(), Some("AAAA:BBBB"));
        let none: PluginSlot = serde_json::from_str(r#"{"uid": 7, "plugin_id": "/x.vst3|ABC", "name": "X"}"#).unwrap();
        assert!(none.state.is_none() && serde_json::to_value(&none).unwrap().get("state").is_none());
    }

    /// The Sampler's fingerprint for its panel is the hash of its zones' text, as it was.
    #[test]
    fn the_sampler_fingerprint_is_unchanged() {
        use std::hash::{Hash, Hasher};
        let mut p = Project::new("t", 48000);
        let mut s = session();
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        p.tracks = vec![s.project.track(t).unwrap().clone()];
        p.tracks[0].instrument = Some(slot(5, crate::instrument::sampler::ID, "{\"zones\":[]}"));
        let mut h = std::collections::hash_map::DefaultHasher::new();
        String::from("{\"zones\":[]}").hash(&mut h);
        let ui = p.for_ui();
        assert_eq!(ui.tracks[0].instrument.as_ref().unwrap().state.as_deref(), Some(format!("{:x}", h.finish()).as_str()));
    }
}
