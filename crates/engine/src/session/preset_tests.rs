//! The preset a slot or Channel EQ shows as loaded: set by loading a preset, undone with it,
//! saved with the project, and kept in step when a user preset is renamed or deleted. (Saving a
//! user preset writes to the user's folder, so `fx_check` and `instrument_check` cover that.)

use super::*;
use crate::audio_io::NULL_OUTPUT;
use crate::presets::CHANNEL_EQ;

const TRE: &str = "builtin:tre";

fn session() -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

fn factory(name: &str) -> Option<PresetRef> {
    Some(PresetRef { name: name.into(), factory: true })
}

fn user(name: &str) -> Option<PresetRef> {
    Some(PresetRef { name: name.into(), factory: false })
}

#[test]
fn loading_a_preset_records_it_in_the_slot_and_undo_takes_it_back() {
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    assert_eq!(s.preset_ref(t, usize::MAX), None, "a new instrument has loaded no preset");
    s.checkpoint("Load Preset", None, false);
    s.load_preset(t, usize::MAX, TRE, "Funk Pluck", true).unwrap();
    assert_eq!(s.preset_ref(t, usize::MAX), factory("Funk Pluck").as_ref());
    // The snapshot the UI gets carries it.
    let ui = s.project.for_ui();
    assert_eq!(ui.track(t).unwrap().instrument.as_ref().unwrap().preset, factory("Funk Pluck"));
    // A preset that doesn't load leaves the one before.
    assert!(s.load_preset(t, usize::MAX, TRE, "No Such Preset", true).is_err());
    assert_eq!(s.preset_ref(t, usize::MAX), factory("Funk Pluck").as_ref());
    s.undo();
    assert_eq!(s.preset_ref(t, usize::MAX), None);
}

#[test]
fn channel_eq_presets_are_recorded_per_strip() {
    let mut s = session();
    let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let name = crate::presets::eq_factory()[1].0;
    s.load_preset(t, EQ_SLOT, CHANNEL_EQ, name, true).unwrap();
    s.load_preset(MASTER_ID, EQ_SLOT, CHANNEL_EQ, "Flat", true).unwrap();
    assert_eq!(s.preset_ref(t, EQ_SLOT), factory(name).as_ref());
    assert_eq!(s.preset_ref(MASTER_ID, EQ_SLOT), factory("Flat").as_ref());
}

#[test]
fn the_loaded_preset_is_saved_with_the_project() {
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    s.load_preset(t, usize::MAX, TRE, "Funk Pluck", true).unwrap();
    s.load_preset(t, EQ_SLOT, CHANNEL_EQ, "Flat", true).unwrap();
    let text = serde_json::to_string(&s.project).unwrap();
    let back: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(back.track(t).unwrap().instrument.as_ref().unwrap().preset, factory("Funk Pluck"));
    assert_eq!(back.track(t).unwrap().eq_preset, factory("Flat"));
    // Nothing loaded, nothing written: older versions read the file as before.
    let fresh = serde_json::to_value(&session().project).unwrap();
    assert!(fresh["master"].get("eq_preset").is_none());
}

#[test]
fn renaming_or_deleting_a_user_preset_follows_in_the_project() {
    let mut p = Project::new("p", 48000);
    let slot = |preset| PluginSlot {
        uid: 1,
        plugin_id: TRE.into(),
        name: "Tre".into(),
        bypass: false,
        state: None,
        params: Vec::new(),
        sidechain: None,
        sidechain_pre: false,
        preset,
    };
    p.master.inserts[0] = Some(PluginSlot { plugin_id: "builtin:compressor".into(), ..slot(user("Mine")) });
    p.master.eq_preset = user("Mine");
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    let mut track = s.project.track(t).unwrap().clone();
    track.instrument = Some(slot(user("Mine")));
    track.inserts[0] = Some(slot(factory("Mine")));
    p.tracks.push(track);

    assert!(p.retarget_user_preset(TRE, "Mine", Some("Yours")));
    let t0 = &p.tracks[0];
    assert_eq!(t0.instrument.as_ref().unwrap().preset, user("Yours"));
    assert_eq!(t0.inserts[0].as_ref().unwrap().preset, factory("Mine"), "a factory preset of the same name stays");
    assert_eq!(p.master.inserts[0].as_ref().unwrap().preset, user("Mine"), "another effect's stays");
    assert_eq!(p.master.eq_preset, user("Mine"));

    assert!(p.retarget_user_preset(CHANNEL_EQ, "Mine", None));
    assert_eq!(p.master.eq_preset, None);
    assert!(!p.retarget_user_preset(TRE, "Mine", None), "nothing refers to it any more");
    assert!(p.retarget_user_preset(TRE, "Yours", None));
    assert_eq!(p.tracks[0].instrument.as_ref().unwrap().preset, None);
}

#[test]
fn projects_from_before_read_with_no_preset_loaded() {
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    s.load_preset(t, usize::MAX, TRE, "Funk Pluck", true).unwrap();
    s.load_preset(t, EQ_SLOT, CHANNEL_EQ, "Flat", true).unwrap();
    let mut v = serde_json::to_value(&s.project).unwrap();
    assert!(v["tracks"][0]["instrument"].as_object_mut().unwrap().remove("preset").is_some());
    assert!(v["tracks"][0].as_object_mut().unwrap().remove("eq_preset").is_some());
    let p: Project = serde_json::from_value(v).unwrap();
    assert_eq!(p.track(t).unwrap().instrument.as_ref().unwrap().preset, None);
    assert_eq!(p.track(t).unwrap().eq_preset, None);
}
