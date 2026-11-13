//! Freeze: the guard on frozen tracks' regions, and an instrument's output auxes frozen with it
//! (`outputs.rs`). What the outputs sound like through a real multi-output plug-in is
//! `multiout_freeze_check`'s.

use crate::audio_file::clip::Clip;
use crate::audio_io::AudioConfig;
use crate::session::tests::scratch_session;
use crate::session::RegionMove;
use crate::session::*;

fn session_with_two_tracks() -> (Session, Id, Id, Id) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let a = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
    let id = s.project.alloc_id();
    let content = RegionContent::midi(Vec::new(), Vec::new());
    s.project.track_mut(a).unwrap().regions.push(Region {
        id,
        name: "M".into(),
        start: 0,
        length: 1000,
        muted: false,
        color: None,
        content,
        loop_end: None,
        takes: None,
        params: Default::default(),
        pattern: None,
    });
    (s, a, b, id)
}

fn place(s: &Session, id: Id) -> Vec<Id> {
    s.project.tracks.iter().filter(|t| t.regions.iter().any(|r| r.id == id)).map(|t| t.id).collect()
}

#[test]
fn a_region_moved_onto_a_frozen_track_stays_where_it_was() {
    let (mut s, a, b, id) = session_with_two_tracks();
    s.project.track_mut(b).unwrap().frozen = Some(FreezeInfo { clip: 0, start: 0 });
    s.checkpoint("Move Region", None, false);
    s.move_regions(&[RegionMove { id, start: 0, track: Some(b) }]);
    assert!(s.frozen_guard().is_err());
    assert_eq!(place(&s, id), vec![a]);
    assert!(s.undo_stack.is_empty(), "nothing changed, so no undo step");
}

#[test]
fn a_region_moved_off_a_frozen_track_isnt_doubled() {
    let (mut s, a, b, id) = session_with_two_tracks();
    s.project.track_mut(a).unwrap().frozen = Some(FreezeInfo { clip: 0, start: 0 });
    s.checkpoint("Move Region", None, false);
    s.move_regions(&[RegionMove { id, start: 0, track: Some(b) }]);
    assert!(s.frozen_guard().is_err());
    assert_eq!(place(&s, id), vec![a]);
}

/// An instrument track with a note to freeze, and an aux that says it's on the instrument's output
/// 3 (Marbble Synth has none, so it renders silence there).
pub(super) fn instrument_with_output_aux(s: &mut Session) -> (Id, Id) {
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let region = s.create_midi_region(inst, 0, 4800).unwrap();
    let note = Note { start: 0, length: 2400, pitch: 60, velocity: 100, muted: false };
    s.set_midi_content(vec![MidiContent { region, notes: vec![note], cc: vec![], start: None, length: None }]);
    let aux = s.create_aux(None).unwrap();
    s.project.track_mut(aux).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 3 });
    s.sync_tracks();
    (inst, aux)
}

pub(super) fn file_of(s: &Session, f: Option<FreezeInfo>) -> Option<PathBuf> {
    let clip = s.project.clips.iter().find(|c| Some(c.id) == f.map(|f| f.clip))?;
    Some(s.dir.join(&clip.path))
}

#[test]
fn an_instruments_output_auxes_freeze_with_it_and_undo_follows() {
    let (mut s, scratch) = scratch_session("freeze-outputs");
    let (inst, aux) = instrument_with_output_aux(&mut s);
    s.checkpoint("Freeze Track", None, true);
    s.freeze_tracks(&[inst], &mut |_| {}).unwrap();
    let frozen = s.project.track(aux).unwrap().frozen_output;
    assert_eq!(
        frozen.map(|f| f.start),
        s.project.track(inst).unwrap().frozen.map(|f| f.start),
        "the aux plays its output's render, from where the instrument's starts"
    );
    let file = file_of(&s, frozen).expect("the render is one of the project's clips");
    assert!(file.is_file() && file.starts_with(s.dir.join("Freeze Files")));

    s.checkpoint("Unfreeze Track", None, true);
    s.unfreeze_track(inst).unwrap();
    assert_eq!(s.project.track(aux).unwrap().frozen_output, None, "unfrozen, the aux is on the live output again");
    assert!(file.is_file(), "its render stays on disk for Undo");

    s.undo();
    assert_eq!(s.project.track(aux).unwrap().frozen_output, frozen);
    s.undo();
    assert_eq!(s.project.track(aux).unwrap().frozen_output, None, "undoing the freeze puts the live output back");
    s.clear_history();
    s.save(None).unwrap();
    assert!(!file.exists(), "and once nothing can bring the freeze back, saving deletes its render");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn an_aux_loses_the_render_with_its_instrument() {
    let (mut s, scratch) = scratch_session("freeze-outputs-gone");
    let (inst, aux) = instrument_with_output_aux(&mut s);
    s.freeze_tracks(&[inst], &mut |_| {}).unwrap();
    s.checkpoint("Delete Track", None, true);
    s.delete_track(inst);
    let a = s.project.track(aux).unwrap();
    assert_eq!((a.instrument_output, a.frozen_output), (None, None), "an ordinary aux again");
    s.undo();
    assert!(s.project.track(aux).unwrap().frozen_output.is_some());
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn an_aux_plays_its_frozen_output_through_its_own_strip() {
    let (mut s, scratch) = scratch_session("freeze-outputs-play");
    let (inst, aux) = instrument_with_output_aux(&mut s);
    s.freeze_tracks(&[inst], &mut |_| {}).unwrap();
    // A known render in place of the synth's silence on output 3.
    let sr = s.project.sample_rate;
    let sine: Vec<f32> = (0..4800).map(|i| 0.5 * (i as f32 * 0.07).sin()).collect();
    let path = s.dir.join("Freeze Files/known.wav");
    crate::audio_file::decode::write_wav(&path, sr, &[sine.clone(), sine.clone()]).unwrap();
    let clip = s.add_clip(&path, Clip::from_channels(path.clone(), sr, vec![sine.clone(), sine.clone()]));
    s.project.track_mut(aux).unwrap().frozen_output = Some(FreezeInfo { clip, start: 0 });
    s.push_regions(aux);

    let aux_alone = |s: &mut Session| {
        let iso = s.isolate_track(aux, true, true);
        s.render(0, 4800, 0, Some(iso), &mut |_| {}).unwrap()
    };
    let off_by = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
    let out = aux_alone(&mut s);
    assert!(off_by(&out[0], &sine) < 1e-6 && off_by(&out[1], &sine) < 1e-6, "the aux plays the render, sample for sample");
    s.update_track(aux, TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    let lower = aux_alone(&mut s);
    let half: Vec<f32> = sine.iter().map(|x| x * 10f32.powf(-6.0 / 20.0)).collect();
    assert!(off_by(&lower[0], &half) < 1e-5, "through its own fader");
    s.update_track(aux, TrackPatch { volume_db: Some(0.0), ..Default::default() });
    s.update_track(inst, TrackPatch { mute: Some(true), ..Default::default() });
    assert!(off_by(&aux_alone(&mut s)[0], &sine) < 1e-6, "the instrument's mute doesn't reach it, as live");
    s.update_track(inst, TrackPatch { mute: Some(false), off: Some(true), ..Default::default() });
    assert!(aux_alone(&mut s)[0].iter().all(|x| *x == 0.0), "its Off switch does");
    let _ = std::fs::remove_dir_all(&scratch);
}
