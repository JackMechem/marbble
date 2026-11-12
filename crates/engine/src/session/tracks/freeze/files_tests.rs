//! Freeze files on disk (`files.rs`): kept while the saved project, a document or the undo
//! history may play them, deleted once nothing can, and nothing else in Freeze Files touched.

use super::tests::{file_of, instrument_with_output_aux};
use crate::session::tests::scratch_session;
use crate::session::*;

/// An instrument track with a note, frozen, as a project saved with backups off (a save keeps the
/// version it replaces as project.json.bak).
fn frozen_project(tag: &str) -> (Session, PathBuf, Id, PathBuf) {
    let (mut s, scratch) = scratch_session(tag);
    s.prefs.general.backups = 0;
    let (inst, _) = instrument_with_output_aux(&mut s);
    s.checkpoint("Freeze Track", None, true);
    s.freeze_tracks(&[inst], &mut |_| {}).unwrap();
    let file = file_of(&s, s.project.track(inst).unwrap().frozen).unwrap();
    s.save(None).unwrap();
    (s, scratch, inst, file)
}

/// Whether `track` plays what it plays live: its freeze file when frozen.
fn sounds(s: &mut Session, track: Id) -> bool {
    let iso = s.isolate_track(track, true, true);
    s.render(0, 4800, 0, Some(iso), &mut |_| {}).unwrap()[0].iter().any(|x| x.abs() > 1e-4)
}

fn unfreeze(s: &mut Session, track: Id) {
    s.checkpoint("Unfreeze Track", None, true);
    s.unfreeze_track(track).unwrap();
}

/// Rewrites the saved project.json with `from` replaced by `to`, as another computer may write it.
fn edit_saved(s: &Session, from: &str, to: &str) {
    let path = s.dir.join("project.json");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from));
    std::fs::write(&path, text.replace(from, to)).unwrap();
}

#[test]
fn closing_an_unsaved_unfreeze_keeps_the_saved_projects_freeze_file() {
    let (mut s, scratch, inst, file) = frozen_project("freeze-files-close");
    unfreeze(&mut s, inst);
    let dir = s.dir.clone();
    s.new_project();
    assert!(file.is_file(), "the saved project still plays it");
    s.open(&dir).unwrap();
    assert!(s.is_frozen(inst));
    assert!(sounds(&mut s, inst), "reopened, the frozen track plays its file");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn unfreeze_undo_redo_keeps_the_file_while_undo_needs_it() {
    let (mut s, scratch, inst, file) = frozen_project("freeze-files-redo");
    unfreeze(&mut s, inst);
    s.save(None).unwrap();
    assert!(file.is_file(), "saved unfrozen, the file stays: Undo can freeze the track again");
    s.undo();
    assert!(s.is_frozen(inst) && sounds(&mut s, inst), "undone, the track plays its freeze file");
    s.redo();
    s.save(None).unwrap();
    assert!(file.is_file(), "redone, Undo needs it still");
    // Saved again after an edit, neither project.json nor its .bak names it.
    s.update_track(inst, TrackPatch { name: Some("Lead".into()), ..Default::default() });
    s.save(None).unwrap();
    assert!(file.is_file(), "Undo needs it still");
    let dir = s.dir.clone();
    s.new_project();
    assert!(!file.exists(), "closing ends Undo, and nothing else needs it");
    s.open(&dir).unwrap();
    assert!(!s.is_frozen(inst) && sounds(&mut s, inst), "the project plays live");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn an_undone_freeze_leaves_no_file_behind() {
    let (mut s, scratch) = scratch_session("freeze-files-undo");
    let (inst, _) = instrument_with_output_aux(&mut s);
    s.save(None).unwrap();
    s.checkpoint("Freeze Track", None, true);
    s.freeze_tracks(&[inst], &mut |_| {}).unwrap();
    let file = file_of(&s, s.project.track(inst).unwrap().frozen).unwrap();
    s.undo();
    s.save(None).unwrap();
    assert!(file.is_file(), "Redo can still freeze the track");
    s.redo();
    assert!(s.is_frozen(inst) && sounds(&mut s, inst), "redone, it plays its freeze file");
    s.undo();
    // A new edit ends Redo.
    s.checkpoint("Track Name", None, false);
    s.update_track(inst, TrackPatch { name: Some("Lead".into()), ..Default::default() });
    s.save(None).unwrap();
    assert!(!file.exists(), "nothing can bring the freeze back, so saving deletes the file");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn other_files_in_freeze_files_are_never_deleted() {
    let (mut s, scratch, inst, file) = frozen_project("freeze-files-stray");
    // A file of the user's, and one left by an earlier version that nothing names.
    let mine = s.dir.join("Freeze Files/My Notes.txt");
    let old = s.dir.join("Freeze Files/Old Freeze.wav");
    std::fs::write(&mine, "notes").unwrap();
    std::fs::copy(&file, &old).unwrap();
    unfreeze(&mut s, inst);
    s.clear_history();
    s.save(None).unwrap();
    s.new_project();
    assert!(mine.is_file() && old.is_file());
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_document_naming_the_file_another_way_keeps_it() {
    // With the other separator, as written on Windows.
    let (mut s, scratch, inst, file) = frozen_project("freeze-files-backslash");
    edit_saved(&s, "\"Freeze Files/", "\"Freeze Files\\\\");
    unfreeze(&mut s, inst);
    s.new_project();
    assert!(file.is_file(), "a Windows-style path names it");
    let _ = std::fs::remove_dir_all(&scratch);

    // By its absolute path.
    let (mut s, scratch, inst, file) = frozen_project("freeze-files-absolute");
    let absolute = serde_json::to_string(&s.dir.join("Freeze Files/").to_string_lossy()).unwrap();
    edit_saved(&s, "\"Freeze Files/", absolute.trim_end_matches('"'));
    unfreeze(&mut s, inst);
    s.new_project();
    assert!(file.is_file(), "an absolute path names it");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_decomposed_name_on_disk_is_the_composed_one_in_a_document() {
    let (mut s, scratch) = scratch_session("freeze-files-nfd");
    s.prefs.general.backups = 0;
    let (inst, _) = instrument_with_output_aux(&mut s);
    s.update_track(inst, TrackPatch { name: Some("Caf\u{e9}".into()), ..Default::default() });
    s.freeze_tracks(&[inst], &mut |_| {}).unwrap();
    // The file named decomposed on disk and in the project, as macOS names files.
    let clip = s.project.track(inst).unwrap().frozen.unwrap().clip;
    let decomposed = "Freeze Files/Cafe\u{301} Freeze.wav";
    let c = s.project.clips.iter_mut().find(|c| c.id == clip).unwrap();
    std::fs::rename(s.dir.join(&c.path), s.dir.join(decomposed)).unwrap();
    c.path = decomposed.into();
    s.save(None).unwrap();
    // project.json, written elsewhere, names it composed.
    edit_saved(&s, decomposed, "Freeze Files/Caf\u{e9} Freeze.wav");
    unfreeze(&mut s, inst);
    s.new_project();
    assert!(scratch.join("Song.marbble").join(decomposed).is_file(), "project.json names it");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn an_unreadable_document_means_nothing_is_deleted() {
    let (mut s, scratch, inst, file) = frozen_project("freeze-files-bad-doc");
    unfreeze(&mut s, inst);
    s.clear_history();
    s.save(None).unwrap();
    assert!(file.is_file(), "project.json.bak, the frozen version, names it");
    let bak = s.dir.join("project.json.bak");
    std::fs::write(&bak, "not a project").unwrap();
    s.save(None).unwrap();
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "not a project");
    assert!(file.is_file(), "a document that can't be read might name it");
    std::fs::remove_file(&bak).unwrap();
    s.save(None).unwrap();
    assert!(!file.exists(), "once every document is read, it goes");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_freeze_placed_after_save_as_plays_from_the_new_folder() {
    let (mut s, scratch) = scratch_session("freeze-files-save-as");
    let (inst, aux) = instrument_with_output_aux(&mut s);
    s.save(None).unwrap();
    let old = s.dir.clone();
    let renders = s.render_freeze(&[inst], &mut |_| {}).unwrap();
    let new = s.save(Some(scratch.join("Copy.marbble"))).unwrap();
    s.checkpoint("Freeze Track", None, true);
    s.place_freeze(renders).unwrap();
    let files = [file_of(&s, s.project.track(inst).unwrap().frozen).unwrap(), file_of(&s, s.project.track(aux).unwrap().frozen_output).unwrap()];
    assert!(files.iter().all(|f| f.is_file() && f.starts_with(new.join("Freeze Files"))), "{files:?}");
    assert!(std::fs::read_dir(old.join("Freeze Files")).map_or(true, |mut d| d.next().is_none()), "none left in the old folder");
    s.save(None).unwrap();
    s.open(&old).unwrap();
    s.new_project();
    s.open(&new).unwrap();
    assert!(s.is_frozen(inst) && sounds(&mut s, inst), "the copy plays its freeze file");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_freeze_rendered_in_a_closed_project_isnt_placed_in_the_next() {
    let (mut s, scratch) = scratch_session("freeze-files-closed");
    let (inst, _) = instrument_with_output_aux(&mut s);
    s.save(None).unwrap();
    let renders = s.render_freeze(&[inst], &mut |_| {}).unwrap();
    let dir = s.dir.clone();
    s.new_project();
    s.open(&dir).unwrap();
    assert!(s.place_freeze(renders).is_err());
    assert!(!s.is_frozen(inst));
    assert!(std::fs::read_dir(dir.join("Freeze Files")).unwrap().next().is_none(), "its files are gone");
    let _ = std::fs::remove_dir_all(&scratch);
}
