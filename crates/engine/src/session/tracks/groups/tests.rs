//! Groups on the session's side: what each link passes on through `update_track_linked`.

use super::*;

fn session() -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

fn audio(s: &mut Session) -> Id {
    s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()
}

fn volume(s: &mut Session, id: Id, db: f32) {
    s.update_track_linked(id, TrackPatch { volume_db: Some(db), ..Default::default() });
}

fn level(s: &Session, id: Id) -> f32 {
    s.project.track(id).unwrap().volume_db
}

#[test]
fn a_fader_pulled_to_the_bottom_and_back_keeps_the_group_offsets() {
    let mut s = session();
    let (a, b) = (audio(&mut s), audio(&mut s));
    s.update_track(b, TrackPatch { volume_db: Some(-20.0), ..Default::default() });
    s.create_group(&[a, b]);
    for db in [-30.0, -80.0, -96.0, -60.0, 0.0] {
        volume(&mut s, a, db);
    }
    assert_eq!(level(&s, b), -20.0);
}

#[test]
fn a_member_moved_alone_keeps_its_new_offset() {
    let mut s = session();
    let (a, b) = (audio(&mut s), audio(&mut s));
    let g = s.create_group(&[a, b]);
    volume(&mut s, a, -96.0);
    // With the group off, B moves alone; back on, A carries it from there.
    let mut off = s.project.group(g).unwrap().clone();
    off.enabled = false;
    s.update_group(off.clone());
    volume(&mut s, b, -50.0);
    s.update_group(Group { enabled: true, ..off });
    volume(&mut s, a, -86.0);
    assert_eq!(level(&s, b), -40.0);
}

#[test]
fn pan_keeps_its_offset_past_hard_left() {
    let mut s = session();
    let (a, b) = (audio(&mut s), audio(&mut s));
    let g = s.create_group(&[a, b]);
    s.update_group(Group { pan: true, ..s.project.group(g).unwrap().clone() });
    s.update_track(b, TrackPatch { pan: Some(0.5), ..Default::default() });
    for pan in [-0.8, -1.0, 0.0] {
        s.update_track_linked(a, TrackPatch { pan: Some(pan), ..Default::default() });
    }
    assert_eq!(s.project.track(b).unwrap().pan, 0.5);
}

#[test]
fn switches_pass_the_same_state_to_the_group() {
    let mut s = session();
    let (a, b, c) = (audio(&mut s), audio(&mut s), audio(&mut s));
    s.create_group(&[a, b]);
    s.update_track_linked(a, TrackPatch { mute: Some(true), solo: Some(true), armed: Some(true), ..Default::default() });
    let t = s.project.track(b).unwrap();
    assert!(t.mute && t.solo && t.armed);
    assert!(!s.project.track(c).unwrap().mute, "not in the group");
}

#[test]
fn the_clutch_suspends_every_group_and_keeps_each_ones_switch() {
    let mut s = session();
    let (a, b) = (audio(&mut s), audio(&mut s));
    let g = s.create_group(&[a, b]);
    s.set_groups_clutched(true);
    s.update_track_linked(a, TrackPatch { mute: Some(true), ..Default::default() });
    volume(&mut s, a, -10.0);
    assert!(!s.project.track(b).unwrap().mute);
    assert_eq!(level(&s, b), 0.0);
    assert!(s.project.group(g).unwrap().enabled, "the group's own switch is left alone");
    s.set_groups_clutched(false);
    volume(&mut s, a, -12.0);
    assert_eq!(level(&s, b), -2.0);
}
