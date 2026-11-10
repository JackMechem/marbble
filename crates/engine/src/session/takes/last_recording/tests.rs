//! Discard Last Recording, and marking takes good or bad.

use super::*;

fn session(tag: &str) -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::env::temp_dir().join(format!("marbble-last-rec-{tag}-{}.marbble", std::process::id()));
    let _ = std::fs::remove_dir_all(&s.dir);
    s
}

/// Places a recording as `finish_recording` does, one undo step: `frames` of audio from
/// `start` on each of `tracks`, played from `from`. Its files are written, empty.
fn record_onto(s: &mut Session, from: i64, start: i64, frames: usize, tracks: &[Id]) {
    s.checkpoint("Record", None, false);
    s.rec_punch = Some(start);
    s.rec_plan = None;
    s.rec_play_from = from;
    let dir = s.recording_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let takes = tracks
        .iter()
        .map(|&t| {
            let path = s.next_take_path(&format!("T{t}"));
            std::fs::write(&path, b"RIFF").unwrap();
            (t, FinishedTake { path, start, channels: vec![vec![0.1; frames]], error: None })
        })
        .collect();
    s.place_recording(takes, 0);
}

fn takes(s: &Session, track: Id) -> Vec<Take> {
    s.project.track(track).unwrap().regions.iter().flat_map(|r| r.takes.iter().flat_map(|f| f.takes.clone())).collect()
}

fn spans(s: &Session, track: Id) -> Vec<(i64, i64, bool)> {
    s.project.track(track).unwrap().regions.iter().map(|r| (r.start, r.length, r.takes.is_some())).collect()
}

#[test]
fn discard_puts_every_track_back_and_returns_the_playhead() {
    let mut s = session("discard");
    let oh = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let kick = s.add_track(TrackKind::Audio, InputSel::Mono(1), None, None).unwrap();
    record_onto(&mut s, 0, 1000, 1000, &[oh, kick]);
    record_onto(&mut s, 500, 1000, 1000, &[oh, kick]);
    assert_eq!(takes(&s, oh).len(), 2);
    let clips = s.project.clips.len();
    s.locate(3000);
    s.discard_last_recording().unwrap();
    for t in [oh, kick] {
        assert_eq!(spans(&s, t), [(1000, 1000, false)]);
    }
    assert_eq!(s.position(), 500);
    // Its files leave the project, kept on disk while Undo can bring them back.
    assert_eq!(s.project.clips.len(), clips - 2);
    assert_eq!(s.discarded.len(), 2);
    assert!(s.discarded.iter().all(|(_, p)| p.exists()));
    // One undo step brings the take back on both tracks.
    s.undo();
    for t in [oh, kick] {
        assert_eq!(takes(&s, t).len(), 2);
    }
    assert_eq!(s.project.clips.len(), clips);
    s.redo();
    // Nothing left to discard.
    assert!(s.discard_last_recording().is_err());
}

#[test]
fn discard_after_an_edit_takes_out_only_the_recording() {
    let mut s = session("edited");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    record_onto(&mut s, 0, 0, 1000, &[t]);
    let old = takes(&s, t);
    assert!(old.is_empty());
    record_onto(&mut s, 0, 500, 1000, &[t]);
    let folder = s.project.track(t).unwrap().regions[0].id;
    s.checkpoint("Rename", None, false);
    s.rename_region(folder, "Kit".into());
    s.discard_last_recording().unwrap();
    let regions = &s.project.track(t).unwrap().regions;
    assert_eq!((regions.len(), regions[0].name.as_str()), (1, "Kit"));
    let f = regions[0].takes.as_ref().unwrap();
    // The older take is left, heard where it has audio.
    assert_eq!((f.takes.len(), f.comp.len()), (1, 1));
    assert!(matches!(regions[0].content, RegionContent::Audio { clip, .. } if clip == f.takes[0].clip));
    // A take packed onto another track since goes from the folder there.
    let other = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    record_onto(&mut s, 0, 3000, 1000, &[other]);
    let ids = [s.project.track(t).unwrap().regions[0].id, s.project.track(other).unwrap().regions[0].id];
    s.flatten_take_folder(ids[0]);
    let flat = s.project.track(t).unwrap().regions[0].id;
    s.pack_take_folder(&[flat, ids[1]]).unwrap();
    s.discard_last_recording().unwrap();
    assert_eq!(takes(&s, t).len(), 1);
    assert!(s.project.track(other).unwrap().regions.is_empty());
    // A recording placed on a new track takes the track with it.
    s.prefs.recording.audio_overlap = AudioOverlap::NewTrack;
    let n = s.project.tracks.len();
    record_onto(&mut s, 0, 0, 1000, &[t]);
    assert_eq!(s.project.tracks.len(), n + 1);
    s.discard_last_recording().unwrap();
    assert_eq!(s.project.tracks.len(), n);
}

#[test]
fn takes_are_marked_while_and_after_recording() {
    let mut s = session("mark");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    // Marked while recording: the take lands rated, a one-take folder over nothing.
    s.rec_rating = Some(TakeRating::Bad);
    record_onto(&mut s, 0, 0, 1000, &[t]);
    assert_eq!(spans(&s, t), [(0, 1000, true)]);
    assert_eq!(takes(&s, t).iter().map(|k| k.rating).collect::<Vec<_>>(), [TakeRating::Bad]);
    assert!(matches!(s.project.track(t).unwrap().regions[0].content, RegionContent::Audio { offset: 0, .. }));
    // After recording, the key marks the newest take, and Discard still puts the track back.
    record_onto(&mut s, 0, 0, 1000, &[t]);
    assert!(s.mark_take(TakeRating::Good, &[]));
    assert_eq!(takes(&s, t).iter().map(|k| k.rating).collect::<Vec<_>>(), [TakeRating::Bad, TakeRating::Good]);
    s.discard_last_recording().unwrap();
    assert_eq!(takes(&s, t).len(), 1);
    // With no last recording, the key marks the take heard in the folders given.
    let folder = s.project.track(t).unwrap().regions[0].id;
    assert!(!s.mark_take(TakeRating::Good, &[]));
    assert!(s.mark_take(TakeRating::Good, &[folder]));
    assert_eq!(takes(&s, t)[0].rating, TakeRating::Good);
}

#[test]
fn takes_are_coloured_and_bad_ones_deleted() {
    let mut s = session("colour");
    s.prefs.recording.color_takes = true;
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    for _ in 0..3 {
        record_onto(&mut s, 0, 0, 1000, &[t]);
    }
    let colors: Vec<Option<String>> = takes(&s, t).into_iter().map(|k| k.color).collect();
    assert_eq!(colors, TAKE_COLORS[..3].iter().map(|c| Some(c.to_string())).collect::<Vec<_>>());
    let folder = s.project.track(t).unwrap().regions[0].id;
    let ids: Vec<Id> = takes(&s, t).iter().map(|k| k.id).collect();
    s.set_take_color(folder, ids[0], None);
    s.set_take_rating(folder, ids[0], TakeRating::Bad);
    s.set_take_rating(folder, ids[2], TakeRating::Bad);
    s.delete_bad_takes(folder);
    assert_eq!(takes(&s, t).iter().map(|k| k.id).collect::<Vec<_>>(), [ids[1]]);
    // A folder of nothing but bad takes keeps its newest.
    s.set_take_rating(folder, ids[1], TakeRating::Bad);
    s.delete_bad_takes(folder);
    assert_eq!(takes(&s, t).len(), 1);
}

#[test]
fn discarded_files_go_once_nothing_needs_them() {
    let mut s = session("purge");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    record_onto(&mut s, 0, 0, 1000, &[t]);
    s.discard_last_recording().unwrap();
    let file = s.discarded[0].1.clone();
    // Saved, but Undo could still bring the recording back: the file stays.
    s.save(None).unwrap();
    assert!(file.exists() && s.discarded.len() == 1);
    // Closing the project ends the history; the saved project doesn't name it.
    s.new_project();
    assert!(!file.exists() && s.discarded.is_empty());

    // A saved project that names the file keeps it.
    let mut s = session("purge-kept");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    record_onto(&mut s, 0, 0, 1000, &[t]);
    s.save(None).unwrap();
    s.discard_last_recording().unwrap();
    let file = s.discarded[0].1.clone();
    s.new_project();
    assert!(file.exists());
    // Once nothing in the history needs it, saving deletes it.
    let mut s = session("purge-save");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    record_onto(&mut s, 0, 0, 1000, &[t]);
    s.discard_last_recording().unwrap();
    let file = s.discarded[0].1.clone();
    s.clear_history();
    s.save(None).unwrap();
    assert!(!file.exists() && s.discarded.is_empty());
}
