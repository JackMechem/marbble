//! Keeping VST3 states current: when they are read, and how undo steps and saves share them.

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
