//! Track On/Off on the session's side: delay compensation, stacks, undo, saving, recording, renders.

use super::*;

fn session() -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

fn audio(s: &mut Session) -> Id {
    s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()
}

fn set_off(s: &mut Session, id: Id, off: bool) {
    s.update_track(id, TrackPatch { off: Some(off), ..Default::default() });
}

#[test]
fn latency_leaves_delay_compensation_while_off() {
    let mut s = session();
    let (a, b) = (audio(&mut s), audio(&mut s));
    s.set_plugin(a, 0, "builtin:linear-phase-eq").unwrap();
    let total = s.pdc_total();
    assert!(total > 0);
    assert!(s.pdc_report().strips.iter().any(|x| x.track == b && x.delay == total), "the other track waits for it");
    set_off(&mut s, a, true);
    assert_eq!(s.pdc_total(), 0);
    assert!(s.pdc_report().strips.is_empty(), "nobody waits, nothing is reported");
    set_off(&mut s, a, false);
    assert_eq!(s.pdc_total(), total);
}

#[test]
fn a_stacks_off_reaches_its_subtracks() {
    for summing in [false, true] {
        let mut s = session();
        let (a, b) = (audio(&mut s), audio(&mut s));
        let stack = s.create_stack(&[a, b], summing).unwrap();
        let params = |s: &Session, id: Id| Session::track_params(&s.project, s.project.track(id).unwrap());
        set_off(&mut s, stack, true);
        assert!(s.project.is_off(a) && params(&s, a).off && params(&s, stack).off);
        assert!(!s.project.track(a).unwrap().off, "the subtrack's own switch is untouched");
        set_off(&mut s, a, true);
        set_off(&mut s, stack, false);
        assert!(s.project.is_off(a) && !s.project.is_off(b));
        assert!(params(&s, a).off && !params(&s, b).off);
    }
}

#[test]
fn undo_and_redo_put_back_the_state_in_their_snapshot() {
    let mut s = session();
    let a = audio(&mut s);
    s.set_plugin(a, 0, "builtin:linear-phase-eq").unwrap();
    let total = s.pdc_total();
    // No undo step of its own, as for Mute.
    let steps = s.undo_stack.len();
    set_off(&mut s, a, true);
    assert_eq!(s.undo_stack.len(), steps);
    s.checkpoint("Rename", None, false);
    s.update_track(a, TrackPatch { name: Some("x".into()), ..Default::default() });
    set_off(&mut s, a, false);
    s.undo();
    assert!(s.project.track(a).unwrap().off);
    assert_eq!(s.pdc_total(), 0, "the engine follows");
    s.redo();
    assert!(!s.project.track(a).unwrap().off);
    assert_eq!(s.pdc_total(), total);
}

#[test]
fn duplicates_and_saved_projects_keep_it() {
    let mut s = session();
    let a = audio(&mut s);
    set_off(&mut s, a, true);
    let copy = s.duplicate_tracks(&[a], true)[0];
    assert!(s.project.track(copy).unwrap().off);
    let json = serde_json::to_string(&s.project).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert!(back.track(a).unwrap().off);
}

#[test]
fn off_tracks_dont_record_and_arent_switched_during_a_take() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("marbble-off-{}", std::process::id()));
    s.dir = dir.join("Off.marbble");
    let (a, b) = (audio(&mut s), audio(&mut s));
    s.update_track(a, TrackPatch { armed: Some(true), ..Default::default() });
    set_off(&mut s, a, true);
    assert!(s.project.track(a).unwrap().armed, "it keeps its R button");
    assert!(s.record().unwrap_err().to_string().contains("off"));
    assert!(!s.is_recording());
    assert!(s.off_refused().is_none());

    s.update_track(b, TrackPatch { armed: Some(true), ..Default::default() });
    s.record().unwrap();
    assert!(s.is_recording());
    assert!(s.off_refused().is_some());
    set_off(&mut s, b, true);
    set_off(&mut s, a, false);
    assert!(!s.project.track(b).unwrap().off && s.project.track(a).unwrap().off, "unchanged during a take");
    s.stop();
    assert!(s.off_refused().is_none());
    set_off(&mut s, b, true);
    assert!(s.project.track(b).unwrap().off);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_undo_that_turns_a_latent_track_on_during_a_take_leaves_the_take_where_it_was_played() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("marbble-off-undo-{}", std::process::id()));
    s.dir = dir.join("OffUndo.marbble");
    let (a, b) = (audio(&mut s), audio(&mut s));
    s.set_plugin(a, 0, "builtin:linear-phase-eq").unwrap();
    s.update_track(b, TrackPatch { armed: Some(true), ..Default::default() });
    s.checkpoint("Rename", None, false);
    s.update_track(b, TrackPatch { name: Some("Vox".into()), ..Default::default() });
    set_off(&mut s, a, true);
    assert_eq!(s.pdc_total(), 0);
    let from = 48_000;
    s.locate(from);
    s.record().unwrap();
    let planned = s.rec_plan.unwrap().latency;
    let wait_past = |s: &Session, pos: i64| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while s.position() < pos && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    wait_past(&s, from + 4800);
    // Past the take's own step, to the rename, when the latent track was on.
    assert_eq!(s.undo().as_deref(), Some("Record"));
    assert_eq!(s.undo().as_deref(), Some("Rename"));
    assert!(!s.project.track(a).unwrap().off && s.pdc_total() > 0, "the latent track is back on");
    wait_past(&s, from + 9600);
    s.stop();
    let take = s.project.track(b).unwrap().regions.first().cloned().expect("a take");
    assert_eq!(take.start, from - planned, "placed with the compensation it was played with ({} frames, now {})", planned, s.record_latency_frames());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn single_track_renders_refuse_an_off_track_and_export_skips_it() {
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let r = s.create_midi_region(t, 0, 4800).unwrap();
    assert_eq!(s.export_targets(), vec![t]);
    set_off(&mut s, t, true);
    assert!(s.export_targets().is_empty());
    assert!(s.render_freeze(&[t], &mut |_| {}).is_err());
    assert!(s.render_in_place(&[t], &[], &BipOptions::default(), &mut |_| {}).is_err());
    assert!(s.export_region(r, Path::new("/nonexistent/x.wav"), &RenderOptions::default(), &mut |_| {}).unwrap_err().to_string().contains("off"));
}
