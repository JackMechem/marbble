//! Multi-output instruments: their auxes' routes and solo, deleting and copying them, and what they
//! play.

use super::*;

fn session() -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

/// An instrument track and an aux that says it's on the instrument's output 3.
fn wired(s: &mut Session) -> (Id, Id) {
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let aux = s.create_aux(None).unwrap();
    s.project.track_mut(aux).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 3 });
    s.sync_tracks();
    (inst, aux)
}

fn params(s: &Session, id: Id) -> TrackParams {
    Session::track_params(&s.project, s.project.track(id).unwrap())
}

#[test]
fn the_instrument_plays_before_the_aux_and_it_is_a_route() {
    let mut s = session();
    // The aux comes first in the list, the instrument after it.
    let aux = s.create_aux(None).unwrap();
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.project.track_mut(aux).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 1 });
    s.sync_tracks();
    let order = s.project.process_order();
    let at = |id: Id| order.iter().position(|&x| x == id).unwrap();
    assert!(at(inst) < at(aux));
    assert_eq!(s.project.feeds(inst), vec![aux]);
    assert!(s.project.reaches(inst, aux) && !s.project.reaches(aux, inst));
    assert_eq!(params(&s, aux).instrument_output, Some(InstrumentOutput { track: inst, bus: 1 }));
}

#[test]
fn a_key_from_the_aux_cant_reach_its_own_instrument() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    s.set_plugin(inst, 0, "builtin:compressor").unwrap();
    assert!(s.set_sidechain(inst, 0, Some(aux)).is_err(), "that would be a feedback loop");
}

#[test]
fn solo_follows_the_output_one_way() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    let other = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(inst, TrackPatch { solo: Some(true), ..Default::default() });
    assert!(params(&s, aux).solo, "a soloed instrument keeps its auxes");
    assert!(!params(&s, other).solo);
    s.update_track(inst, TrackPatch { solo: Some(false), ..Default::default() });
    s.update_track(aux, TrackPatch { solo: Some(true), ..Default::default() });
    assert!(!params(&s, inst).solo, "a soloed aux doesn't bring the instrument's strip with it");
}

#[test]
fn deleting_the_instrument_keeps_the_aux_and_undo_rewires_it() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    s.set_plugin(aux, 0, "builtin:gain").unwrap();
    s.checkpoint("Delete", None, true);
    s.delete_track(inst);
    let a = s.project.track(aux).unwrap();
    assert_eq!(a.instrument_output, None);
    assert!(a.inserts[0].is_some(), "its plug-ins stay");
    assert_eq!(params(&s, aux).instrument_output, None);
    s.undo();
    assert_eq!(s.project.track(aux).unwrap().instrument_output, Some(InstrumentOutput { track: inst, bus: 3 }));
    assert_eq!(params(&s, aux).instrument_output, Some(InstrumentOutput { track: inst, bus: 3 }));
    assert_eq!(s.project.feeds(inst), vec![aux]);
    s.redo();
    assert_eq!(s.project.track(aux).unwrap().instrument_output, None);
}

#[test]
fn deleting_the_aux_leaves_the_instrument_and_undo_brings_it_back() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    s.checkpoint("Delete", None, true);
    s.delete_track(aux);
    assert!(s.project.feeds(inst).is_empty());
    s.undo();
    assert_eq!(s.project.feeds(inst), vec![aux]);
    assert_eq!(s.project.track(aux).unwrap().kind, TrackKind::Aux);
}

#[test]
fn unused_tracks_go_together() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    // The instrument has content: its aux is in use.
    s.create_midi_region(inst, 0, 4800).unwrap();
    assert_eq!(s.delete_unused_tracks(), 0);
    let (inst2, aux2) = wired(&mut s);
    assert_eq!(s.delete_unused_tracks(), 2);
    assert!(s.project.track(inst2).is_none() && s.project.track(aux2).is_none());
    assert!(s.project.track(inst).is_some() && s.project.track(aux).is_some());
}

#[test]
fn copies_of_the_instrument_start_with_no_auxes_and_copies_of_the_aux_share_the_output() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    let copy = s.duplicate_tracks(&[inst], true)[0];
    assert!(s.project.feeds(copy).is_empty());
    assert_eq!(s.project.feeds(inst), vec![aux]);
    let aux2 = s.duplicate_tracks(&[aux], true)[0];
    assert_eq!(s.project.track(aux2).unwrap().instrument_output, s.project.track(aux).unwrap().instrument_output);
    assert_ne!(s.project.track(aux2).unwrap().bus, s.project.track(aux).unwrap().bus);
}

#[test]
fn only_an_aux_takes_an_instrument_output_and_only_from_an_instrument() {
    let mut p = Project::new("x", 48000);
    let mk = |p: &mut Project, kind: TrackKind| {
        let id = p.alloc_id();
        p.tracks.push(
            serde_json::from_value(serde_json::json!({
                "id": id, "kind": kind, "name": "t", "color": "", "volume_db": 0.0, "pan": 0.0, "mute": false,
                "solo": false, "armed": false, "monitor": false, "input": { "type": "none" },
            }))
            .unwrap(),
        );
        id
    };
    let (inst, audio, aux, aux2, aux3) =
        (mk(&mut p, TrackKind::Instrument), mk(&mut p, TrackKind::Audio), mk(&mut p, TrackKind::Aux), mk(&mut p, TrackKind::Aux), mk(&mut p, TrackKind::Aux));
    p.track_mut(aux).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 2 });
    p.track_mut(aux2).unwrap().instrument_output = Some(InstrumentOutput { track: audio, bus: 2 });
    p.track_mut(aux3).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 0 });
    p.track_mut(audio).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 1 });
    p.prune_routes();
    assert!(p.track(aux).unwrap().instrument_output.is_some());
    assert!(p.track(aux2).unwrap().instrument_output.is_none() && p.track(aux3).unwrap().instrument_output.is_none());
    assert!(p.track(audio).unwrap().instrument_output.is_none());
}

#[test]
fn an_old_project_loads_without_it_and_a_new_one_keeps_it() {
    let mut s = session();
    let (_, aux) = wired(&mut s);
    let json = serde_json::to_string(&s.project).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.track(aux).unwrap().instrument_output, s.project.track(aux).unwrap().instrument_output);
    let mut old = serde_json::to_value(&s.project).unwrap();
    for t in old["tracks"].as_array_mut().unwrap() {
        t.as_object_mut().unwrap().remove("instrument_output");
    }
    let back: Project = serde_json::from_value(old).unwrap();
    assert!(back.tracks.iter().all(|t| t.instrument_output.is_none()));
    // Nothing is written for tracks that don't use it.
    let plain = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    assert!(!serde_json::to_string(s.project.track(plain).unwrap()).unwrap().contains("instrument_output"));
}

#[test]
fn a_new_aux_needs_a_loaded_instrument_with_extra_outputs() {
    let mut s = session();
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    // The built-in synth has one output.
    let steps = s.project.tracks.len();
    assert!(s.add_output_aux(inst).unwrap_err().to_string().contains("extra outputs"));
    assert!(s.add_output_aux(audio).is_err());
    assert_eq!(s.project.tracks.len(), steps, "nothing was added");
    assert!(s.instrument_outputs().is_empty());
}

#[test]
fn freeze_and_bounce_in_place_refuse_and_say_why() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    s.create_midi_region(inst, 0, 4800).unwrap();
    for e in [s.render_freeze(&[inst], &mut |_| {}).err(), s.render_in_place(&[inst], &[], &BipOptions::default(), &mut |_| {}).err()] {
        let e = e.expect("refused").to_string();
        assert!(e.contains("extra outputs") && e.contains("delete those auxes"), "{e}");
    }
    s.delete_track(aux);
    assert!(s.check_no_output_auxes(&[inst], "Freeze").is_ok());
}

#[test]
fn an_exported_aux_covers_its_instrument_and_plays_it() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    assert!(s.export_targets().is_empty());
    s.create_midi_region(inst, 0, 4800).unwrap();
    assert_eq!(s.export_targets(), vec![inst, aux]);
    let iso = s.isolate_track(aux, true, true);
    assert_eq!(iso.keys, vec![inst], "the instrument plays, unheard, for the aux's input");
    s.update_track(inst, TrackPatch { off: Some(true), ..Default::default() });
    assert!(s.export_targets().is_empty(), "an Off instrument leaves its aux with nothing");
}

#[test]
fn an_off_instrument_leaves_timing_to_the_others() {
    let mut s = session();
    let (inst, aux) = wired(&mut s);
    s.set_plugin(inst, 0, "builtin:linear-phase-eq").unwrap();
    let total = s.pdc_total();
    assert!(total > 0);
    assert!(s.pdc_report().strips.iter().any(|x| x.track == aux && x.delay == total), "the aux waits for the main output's inserts");
    s.update_track(inst, TrackPatch { off: Some(true), ..Default::default() });
    assert_eq!(s.pdc_total(), 0);
}
