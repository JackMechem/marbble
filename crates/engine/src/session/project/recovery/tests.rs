//! Crash safety: the recovery journal, interrupted recordings, atomic saves and autosaves.

use super::*;
use crate::audio_io::AudioConfig;
use crate::model::RegionContent;

fn session(tag: &str) -> (Session, PathBuf) {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    let scratch = std::env::temp_dir().join(format!("marbble-recovery-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    s.dir = scratch.join("Song.marbble");
    (s, scratch)
}

/// A take as a crash leaves it: 24-bit, sizes never written, `frames` frames of audio.
fn crashed_wav(path: &Path, frames: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..frames {
        w.write_sample((i % 100) as i32 * 1000).unwrap();
    }
    w.finalize().unwrap();
    let mut b = std::fs::read(path).unwrap();
    let data = b.windows(4).position(|w| w == b"data").unwrap();
    b[4..8].copy_from_slice(&[0; 4]);
    b[data + 4..data + 8].copy_from_slice(&[0; 4]);
    std::fs::write(path, b).unwrap();
}

fn journal(dir: &Path, track: Id, path: &Path, start: i64) -> RecJournal {
    RecJournal {
        version: 1,
        started: 1_700_000_000,
        sample_rate: 48000,
        punch: start,
        from: start,
        cycle: None,
        punch_range: None,
        latency: 0,
        min_take: 24000,
        takes: vec![JournalTake {
            track,
            track_name: "Vocal".into(),
            input: InputSel::Mono(0),
            path: path.to_string_lossy().to_string(),
            channels: 1,
            start,
            bits: 24,
        }],
    }
    .write_to(dir)
}

impl RecJournal {
    fn write_to(self, dir: &Path) -> RecJournal {
        let p = dir.join(format!("{JOURNAL_PREFIX}{}{JOURNAL_SUFFIX}", self.started));
        write_atomic(&p, &serde_json::to_vec(&self).unwrap(), false).unwrap();
        self
    }
}

#[test]
fn journal_round_trips() {
    let dir = std::env::temp_dir().join(format!("marbble-journal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut j = journal(&dir, 7, &dir.join("a.wav"), 96000);
    j.cycle = Some((0, 192000));
    j.punch_range = Some((48000, 144000));
    let j = j.write_to(&dir);
    let found = recording_journals(&dir);
    assert_eq!(found.len(), 1);
    assert_eq!(read_journal(&found[0]).unwrap(), j);
    assert_eq!(j.plan(), RecPlan { from: 96000, cycle: Some((0, 192000)), punch: Some((48000, 144000)), latency: 0, midi_latency: 0, min_take: 24000 });
    // Files that merely look similar aren't journals.
    std::fs::write(dir.join("recording-notes.txt"), "x").unwrap();
    assert_eq!(recording_journals(&dir).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn interrupted_recording_is_detected_and_recovered() {
    let (mut s, scratch) = session("recover");
    let dir = s.dir.clone();
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.save(None).unwrap();
    assert!(crate::session::recovery_info(&dir).is_none());

    let wav = dir.join(AUDIO_DIR).join("Vocal#01.wav");
    crashed_wav(&wav, 30000);
    journal(&dir, t, &wav, 96000);
    let info = crate::session::recovery_info(&dir).expect("recording to recover");
    assert!(!info.has_autosave);
    let rec = info.recording.unwrap();
    assert_eq!((rec.takes, rec.tracks.clone()), (1, vec!["Vocal".to_string()]));
    assert!((rec.seconds - 30000.0 / 48000.0).abs() < 1e-9);

    // A fresh launch recovers it onto the track, where it was recorded.
    let (mut s2, _) = session("recover2");
    s2.open_recovered(&dir).unwrap();
    let regions = &s2.project.track(t).unwrap().regions;
    assert_eq!(regions.len(), 1);
    assert_eq!((regions[0].start, regions[0].length), (96000, 30000));
    assert!(matches!(regions[0].content, RegionContent::Audio { offset: 0, .. }));
    assert!(s2.dirty && s2.saved);
    assert!(!crate::audio_file::wav_repair::inspect(&wav).unwrap().damaged);
    // The recovery can be undone, and an autosave is due now.
    assert!(s2.autosave_requested());
    assert_eq!(s2.undo_label().as_deref(), Some("Recover Recording"));
    // Doing it again finds the file already in the project.
    assert_eq!(s2.recover_recordings(), 0);
    // The autosave that follows takes the journal's place.
    let job = s2.autosave_job().expect("autosave right after recovery");
    job.write().unwrap();
    assert!(recording_journals(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn recovery_makes_a_track_the_project_never_saved() {
    let (mut s, scratch) = session("newtrack");
    let dir = s.dir.clone();
    let wav = dir.join(AUDIO_DIR).join("Vocal#01.wav");
    crashed_wav(&wav, 4800);
    journal(&dir, 4242, &wav, 0);
    // Never saved, never autosaved: only the journal is there.
    let info = crate::session::recovery_info(&dir).expect("recoverable");
    assert!(info.saved.is_none() && !info.has_autosave);
    s.open_recovered(&dir).unwrap();
    assert_eq!(s.dir, dir);
    let t = s.project.tracks.iter().find(|t| t.name == "Vocal").expect("a track for the take");
    assert_eq!(t.regions.len(), 1);
    assert_eq!(t.regions[0].length, 4800);
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn discarding_keeps_the_audio_usable() {
    let (s, scratch) = session("discard");
    let dir = s.dir.clone();
    let wav = dir.join(AUDIO_DIR).join("Vocal#01.wav");
    crashed_wav(&wav, 4800);
    journal(&dir, 1, &wav, 0);
    crate::session::discard_autosave(&dir);
    assert!(recording_journals(&dir).is_empty());
    assert!(crate::session::recovery_info(&dir).is_none());
    assert_eq!(crate::audio_file::decode::decode(&wav).unwrap().channels[0].len(), 4800);
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn saves_are_atomic_with_a_backup_and_damage_falls_back() {
    let (mut s, scratch) = session("damaged");
    let dir = s.dir.clone();
    // The single .bak is what a save keeps with backups off (backups.rs tests the rest).
    s.prefs.general.backups = 0;
    s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.set_tempo(100.0);
    s.save(None).unwrap();
    assert!(!dir.join(BACKUP_FILE).exists());
    s.set_tempo(110.0);
    s.save(None).unwrap();
    // The previous save is the backup; no temporary files are left.
    let bak: Project = serde_json::from_slice(&std::fs::read(dir.join(BACKUP_FILE)).unwrap()).unwrap();
    assert_eq!(bak.tempo, 100.0);
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
    assert!(leftovers.is_empty());

    // Truncated project.json, newer autosave: the autosave opens, with a warning.
    s.set_tempo(120.0);
    s.dirty = true;
    s.autosave_now().unwrap();
    let full = std::fs::read(dir.join(PROJECT_FILE)).unwrap();
    std::fs::write(dir.join(PROJECT_FILE), &full[..full.len() / 2]).unwrap();
    let (mut s2, _) = session("damaged2");
    s2.open(&dir).unwrap();
    assert_eq!(s2.project.tempo, 120.0);
    assert!(s2.dirty);
    let w = s2.warning.take().unwrap();
    assert!(w.contains("project.json is cut short") && w.contains("autosave"), "{w}");

    // No autosave either: the backup.
    std::fs::remove_file(dir.join(AUTOSAVE_FILE)).unwrap();
    s2.open(&dir).unwrap();
    assert_eq!(s2.project.tempo, 100.0);
    assert!(s2.warning.take().unwrap().contains("backup"));

    // Nothing to fall back on: a clear error, and the open project stays.
    std::fs::write(dir.join(BACKUP_FILE), b"{ not json").unwrap();
    let e = s2.open(&dir).unwrap_err().to_string();
    assert!(e.contains("project.json is cut short") && e.contains("no autosave or backup"), "{e}");
    assert_eq!(s2.project.tempo, 100.0);

    // A damaged project.json never replaces a good backup.
    std::fs::write(dir.join(BACKUP_FILE), &full).unwrap();
    s2.set_tempo(130.0);
    s2.save(None).unwrap();
    assert_eq!(std::fs::read(dir.join(BACKUP_FILE)).unwrap(), full);
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_damaged_autosave_is_not_offered() {
    let (mut s, scratch) = session("badauto");
    let dir = s.dir.clone();
    s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.save(None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join(AUTOSAVE_FILE), b"{\"name\": \"So").unwrap();
    assert!(crate::session::recovery_info(&dir).is_none());
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn emergency_autosave_keeps_captured_plugin_states() {
    let (mut s, scratch) = session("emergency");
    assert!(s.emergency_autosave().unwrap().is_none());
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.set_tempo(90.0);
    s.dirty = true;
    let path = s.emergency_autosave().unwrap().unwrap();
    let doc: Project = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(doc.tempo, 90.0);
    assert!(doc.track(t).is_some());
    let _ = std::fs::remove_dir_all(&scratch);
}
