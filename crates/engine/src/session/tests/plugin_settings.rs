//! Pasting a plug-in's settings into another instance of it (`session/plugins/settings.rs`), which
//! the panel's Paste Settings and Compare both use: what comes along, what is refused, and undo.

use crate::instrument::sampler::ID as SAMPLER;
use crate::instrument::tre::params::param;
use crate::session::mixer::strip_settings::tests::{plays_from_the_project, sampler_track};
use crate::session::tests::scratch_session;
use crate::session::*;

const TRE: &str = "builtin:tre";

fn tre_track(s: &mut Session) -> Id {
    s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap()
}

fn instrument(s: &Session, t: Id) -> &PluginSlot {
    s.project.track(t).unwrap().instrument.as_ref().unwrap()
}

#[test]
fn settings_go_into_another_instance_as_one_undo_step() {
    let (mut s, scratch) = scratch_session("settings-paste");
    let (a, b) = (tre_track(&mut s), tre_track(&mut s));
    s.load_preset(a, usize::MAX, TRE, "Funk Pluck", true).unwrap();
    s.set_builtin_param(a, usize::MAX, param::CUTOFF, 1234.0).unwrap();
    let copied = s.copy_plugin(a, usize::MAX).unwrap();
    let b_uid = instrument(&s, b).uid;

    s.checkpoint("Paste Settings", None, false);
    s.paste_plugin_settings(b, usize::MAX, &copied).unwrap();
    let pasted = instrument(&s, b);
    assert_eq!(pasted.uid, b_uid, "the instance stays");
    assert_eq!(s.builtin_params(b_uid), s.builtin_params(copied.uid), "every value comes along");
    assert_eq!(pasted.preset, copied.preset, "and the preset its box compares with");

    s.undo();
    assert_eq!(instrument(&s, b).preset, None);
    assert_ne!(s.builtin_params(b_uid), s.builtin_params(copied.uid));
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn settings_only_go_into_the_same_built_in_plug_in() {
    let (mut s, scratch) = scratch_session("settings-refused");
    let t = tre_track(&mut s);
    s.set_plugin(t, 0, "builtin:deesser").unwrap();
    let eq = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.set_plugin(eq, 0, crate::library::presets::CHANNEL_EQ).unwrap();
    let deesser = s.copy_plugin(t, 0).unwrap();
    assert!(s.paste_plugin_settings(eq, 0, &deesser).is_err(), "another plug-in's settings");
    assert!(s.paste_plugin_settings(t, usize::MAX, &deesser).is_err(), "an effect's into an instrument");
    assert!(s.paste_plugin_settings(eq, 1, &deesser).is_err(), "an empty slot");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_samplers_zones_come_along_and_round_trip_unchanged() {
    let (mut s, scratch) = scratch_session("settings-sampler");
    let wav = scratch.join("originals/Keys.wav");
    std::fs::create_dir_all(wav.parent().unwrap()).unwrap();
    crate::audio_file::decode::write_wav(&wav, 48000, &[(0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()]).unwrap();
    let dir = s.dir.clone();
    let from = sampler_track(&mut s, &dir, &wav);
    let to = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();

    // Compare puts back what it copied: the same zones, word for word, so the panel sees no edit.
    let before = instrument(&s, from).state.clone();
    let copied = s.copy_plugin(from, usize::MAX).unwrap();
    s.paste_plugin_settings(from, usize::MAX, &copied).unwrap();
    assert_eq!(instrument(&s, from).state, before);

    s.paste_plugin_settings(to, usize::MAX, &copied).unwrap();
    plays_from_the_project(&s, to);
    assert_eq!(std::fs::read_dir(dir.join("Samples")).unwrap().count(), 1, "nothing copied within a project");

    // An empty sampler's settings empty it.
    let empty = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();
    let nothing = s.copy_plugin(empty, usize::MAX).unwrap();
    s.paste_plugin_settings(to, usize::MAX, &nothing).unwrap();
    assert!(s.sampler_view(to).unwrap().zones.is_empty());
    let _ = std::fs::remove_dir_all(&scratch);
}
