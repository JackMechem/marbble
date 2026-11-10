//! The Channel EQ as an Audio FX insert against the session: an older project's fixed Channel EQs
//! open as inserts and play sample for sample as before, the EQ thumbnail's insert, the
//! analyzer following an insert, and controller assignments learnt for the fixed EQ.

use super::*;
use crate::controllers::{Assignment, AssignmentFile, Controller, CtlKind, CtlTarget, EqField};
use crate::eq::{coefficients, EqState};
use crate::fx::channel_eq as ceq;

const LEN: usize = 24_000;
/// The port controller messages pretend to come from, with the numbers ALSA appends.
const PORT: &str = "Test Controller:Test Controller MIDI 1 20:0";

/// A session on ALSA's null device in a project folder of its own.
fn session(tag: &str) -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-channel-eq-{tag}-{}.marbble", std::process::id()));
    let _ = std::fs::remove_dir_all(&s.dir);
    std::fs::create_dir_all(s.dir.join(AUDIO_DIR)).unwrap();
    s
}

fn noise(len: usize, seed: u32) -> Vec<f32> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32) - 0.5
        })
        .collect()
}

/// An audio track playing stereo noise from 0, without fades, so nothing but the strip shapes it.
fn noise_track(s: &mut Session) -> Id {
    let rate = s.sample_rate() as u32;
    let track = s.add_track(TrackKind::Audio, InputSel::Stereo(0), None, None).unwrap();
    let path = s.dir.join(AUDIO_DIR).join("noise.wav");
    crate::decode::write_wav(&path, rate, &[noise(LEN, 1), noise(LEN, 2)]).unwrap();
    s.import_audio(&path, track, 0).unwrap();
    for r in &mut s.project.track_mut(track).unwrap().regions {
        if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
            (*fade_in, *fade_out) = (0, 0);
        }
    }
    s.push_regions(track);
    track
}

fn render(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, LEN as i64, 0, None, &mut |_| {}).unwrap()
}

fn plugin_ids(s: &Session, track: Id) -> Vec<Option<String>> {
    let inserts = if track == MASTER_ID { &s.project.master.inserts } else { &s.project.track(track).unwrap().inserts };
    inserts.iter().map(|x| x.as_ref().map(|x| x.plugin_id.clone())).collect()
}

fn same_bits(a: &[Vec<f32>], b: &[Vec<f32>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits()))
}

/// Changes the saved project.json as `edit` says, as if an older version had written it.
fn rewrite(dir: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let file = dir.join(PROJECT_FILE);
    let mut doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    edit(&mut doc);
    std::fs::write(&file, doc.to_string()).unwrap();
}

#[test]
fn an_old_projects_fixed_eqs_play_as_inserts_sample_for_sample() {
    let mut s = session("render");
    let track = noise_track(&mut s);
    // A plug-in already in the first slot, which the EQ goes ahead of. The Gain at its defaults
    // passes the signal bit for bit.
    s.set_plugin(track, 0, "builtin:gain").unwrap();
    let dry = render(&mut s);
    assert!(dry[0].iter().any(|v| v.abs() > 0.1), "the noise plays");

    let mut strip = ChannelEq::default();
    strip.bands[0].on = true;
    strip.bands[0].freq = 120.0;
    strip.bands[0].slope = 48;
    strip.bands[3].gain = -6.5;
    strip.bands[3].q = 2.0;
    strip.bands[6].gain = 4.0;
    strip.output_db = -2.0;
    let mut master = ChannelEq::default();
    master.bands[2].gain = 3.0;
    master.bands[7].on = true;
    master.bands[7].freq = 6000.0;
    let dir = s.save(None).unwrap();
    rewrite(&dir, |doc| {
        doc["tracks"][0]["eq"] = serde_json::to_value(&strip).unwrap();
        doc["tracks"][0]["eq_preset"] = serde_json::json!({ "name": "Mine", "factory": false });
        doc["master"]["eq"] = serde_json::to_value(&master).unwrap();
    });
    s.open(&dir).unwrap();
    assert_eq!(plugin_ids(&s, track)[..3], [Some(ceq::ID.to_string()), Some("builtin:gain".to_string()), None]);
    assert_eq!(plugin_ids(&s, MASTER_ID)[0].as_deref(), Some(ceq::ID));
    assert_eq!(s.project.track(track).unwrap().inserts[0].as_ref().unwrap().preset, Some(PresetRef { name: "Mine".into(), factory: false }));
    assert!(s.warning.is_none());
    let wet = render(&mut s);

    // What the fixed EQs did: the strip's on its own signal, ahead of its plug-ins, and Stereo
    // Out's on the mix. Fader, pan and Stereo Out are at unity, so the EQs come out the same
    // applied to the dry render.
    let mut want = dry.clone();
    let sr = s.sample_rate();
    for eq in [&strip, &master] {
        let c = coefficients(eq, sr);
        let mut st = EqState::default();
        let (l, r) = want.split_at_mut(1);
        st.process(&c, &mut l[0], &mut r[0]);
    }
    assert!(!same_bits(&want, &dry));
    assert!(same_bits(&wet, &want), "the inserts play as the fixed EQs did");

    // Saved, the project has the inserts and not the old EQs, and plays the same reopened.
    s.save(None).unwrap();
    let text = std::fs::read_to_string(dir.join(PROJECT_FILE)).unwrap();
    assert!(!text.contains("\"eq\"") && !text.contains("eq_preset"));
    s.open(&dir).unwrap();
    assert_eq!(plugin_ids(&s, track)[..2], [Some(ceq::ID.to_string()), Some("builtin:gain".to_string())]);
    assert!(same_bits(&render(&mut s), &wet));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_full_strip_loses_its_eq_and_says_so() {
    let mut s = session("full");
    let track = noise_track(&mut s);
    for k in 0..INSERT_SLOTS {
        s.set_plugin(track, k, "builtin:gain").unwrap();
    }
    let mut eq = ChannelEq::default();
    eq.bands[4].gain = 5.0;
    let dir = s.save(None).unwrap();
    rewrite(&dir, |doc| doc["tracks"][0]["eq"] = serde_json::to_value(&eq).unwrap());
    s.open(&dir).unwrap();
    assert!(plugin_ids(&s, track).iter().all(|p| p.as_deref() == Some("builtin:gain")));
    assert!(s.warning.as_deref().is_some_and(|w| w.contains("Audio 1")), "{:?}", s.warning);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_eq_thumbnail_inserts_one_at_the_top_and_the_analyzer_follows_it() {
    let mut s = session("thumb");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    assert!(s.project.track(t).unwrap().inserts.iter().all(|x| x.is_none()), "a new track has no Channel EQ");
    assert!(s.first_channel_eq(t).is_none());
    s.set_plugin(t, 0, "builtin:compressor").unwrap();
    s.set_plugin(t, 1, "builtin:gain").unwrap();
    s.checkpoint("Insert Channel EQ", None, true);
    let uid = s.insert_channel_eq(t).unwrap();
    assert_eq!(plugin_ids(&s, t)[..4], [Some(ceq::ID.to_string()), Some("builtin:compressor".to_string()), Some("builtin:gain".to_string()), None]);
    assert_eq!(s.first_channel_eq(t).map(|x| x.uid), Some(uid));
    // A second one further down isn't the first.
    s.set_plugin(t, 5, ceq::ID).unwrap();
    assert_eq!(s.first_channel_eq(t).map(|x| x.uid), Some(uid));

    s.set_analyzer(Some(uid));
    assert_eq!(s.analyzer_target, Some((t, uid)));
    let m = s.insert_channel_eq(MASTER_ID).unwrap();
    s.set_analyzer(Some(m));
    assert_eq!(s.analyzer_target, Some((MASTER_ID, m)));
    s.set_analyzer(Some(987_654));
    assert_eq!(s.analyzer_target, None, "an instance that isn't there feeds nothing");

    s.undo();
    s.undo();
    assert_eq!(plugin_ids(&s, t)[..2], [Some("builtin:compressor".to_string()), Some("builtin:gain".to_string())]);
}

/// The analyzer's insert belongs to its project. Another one (here a new one, whose ids start
/// over, so that its first track and Channel EQ take the old ones' ids) feeds nothing until an
/// editor asks; nor does a project opened, even the same one again.
#[test]
fn another_project_starts_with_the_analyzer_off() {
    let mut s = session("analyzer-new");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let uid = s.insert_channel_eq(t).unwrap();
    s.set_analyzer(Some(uid));
    s.new_project();
    s.dir = std::env::temp_dir().join(format!("marbble-channel-eq-analyzer-other-{}.marbble", std::process::id()));
    let track = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let other = s.insert_channel_eq(track).unwrap();
    assert_eq!((track, other), (t, uid), "the new project's ids are the old one's");
    assert_eq!(s.analyzer_target, None);

    s.set_analyzer(Some(other));
    s.save(None).unwrap();
    let dir = s.dir.clone();
    s.open(&dir).unwrap();
    assert_eq!(s.analyzer_target, None, "after opening");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn assignments_to_the_fixed_eq_drive_the_first_channel_eq_insert() {
    let mut s = session("ctl");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let learnt = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let cc = |number: u8| Controller { port: crate::controllers::port_key(PORT).into(), channel: 0, kind: CtlKind::Cc { number } };
    let file = s.dir.join("controller-assignments.json");
    let assignments = vec![
        Assignment::new(cc(20), CtlTarget::Eq { track: t, band: 3, field: EqField::Gain }),
        Assignment::new(cc(21), CtlTarget::EqOutput { track: t }),
        Assignment::new(cc(22), CtlTarget::Eq { track: t, band: 0, field: EqField::On }),
        Assignment::new(cc(23), CtlTarget::Eq { track: learnt, band: 2, field: EqField::Freq }),
    ];
    std::fs::write(&file, serde_json::to_string(&AssignmentFile { assignments }).unwrap()).unwrap();
    s.load_controller_assignments(file);
    let send = |s: &mut Session, number: u8, value: u8| {
        s.inject_midi_from(PORT, &[0xB0, number, value]);
        s.idle();
    };

    // Without a Channel EQ on the strip, nothing to drive.
    send(&mut s, 20, 127);
    assert!(s.project.track(t).unwrap().inserts.iter().all(|x| x.is_none()));
    assert!(s.controller_assignments().iter().all(|r| !r.available));

    let uid = s.insert_channel_eq(t).unwrap();
    s.set_plugin(t, 4, ceq::ID).unwrap();
    let second = s.project.track(t).unwrap().inserts[4].as_ref().unwrap().uid;
    let value = |s: &Session, uid: Id, id: u32| s.builtin_params(uid).iter().find(|p| p.0 == id).unwrap().1;
    send(&mut s, 20, 127);
    assert_eq!(value(&s, uid, ceq::gain(3)), 24.0);
    send(&mut s, 21, 0);
    assert_eq!(value(&s, uid, ceq::OUTPUT), -24.0);
    send(&mut s, 22, 127);
    assert_eq!(value(&s, uid, ceq::on(0)), 1.0, "a band's switch toggles");
    assert_eq!(value(&s, second, ceq::gain(3)), 0.0, "only the first Channel EQ");
    let rows = s.controller_assignments();
    assert!(rows[0].available && rows[0].name.ends_with("Channel EQ › Bell 2 Gain"), "{}", rows[0].name);

    // Opening an older project keeps a flat EQ that an assignment drives, so it has something
    // to drive.
    let dir = s.save(None).unwrap();
    rewrite(&dir, |doc| doc["tracks"][1]["eq"] = serde_json::to_value(ChannelEq::default()).unwrap());
    s.open(&dir).unwrap();
    assert_eq!(plugin_ids(&s, learnt)[0].as_deref(), Some(ceq::ID));
    let _ = std::fs::remove_dir_all(&dir);
}
