//! Autosave and crash recovery of a project's unsaved work: the autosave written every few
//! minutes beside project.json, finding a project folder with work to recover, and opening or
//! discarding it. Recordings cut short by a crash are in `recovery.rs`.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;

use crate::model::Project;
use crate::session::*;

/// An autosave waiting to be written. Taking it (on the main thread, where plug-in states can
/// be read) is cheap; [`AutosaveJob::write`] does the disk work and can run on any thread.
pub struct AutosaveJob {
    path: PathBuf,
    text: String,
    /// Recording journals whose takes this autosave holds: deleted once it is written.
    journals: Vec<PathBuf>,
}

impl AutosaveJob {
    /// Writes the autosave atomically (temporary file, synced, then renamed over the old one),
    /// then deletes the recording journals it supersedes.
    pub fn write(self) -> Result<PathBuf> {
        crate::session::project::recovery::write_atomic(&self.path, self.text.as_bytes(), true)?;
        for j in &self.journals {
            let _ = std::fs::remove_file(j);
        }
        Ok(self.path)
    }
}

/// A project folder holding unsaved work: an autosave newer than its saved document, or audio
/// from a recording that never finished.
#[derive(Debug, Clone, Serialize)]
pub struct Recovery {
    pub path: String,
    pub name: String,
    /// When the work was autosaved (or, with only a recording to recover, when that started),
    /// in seconds since the Unix epoch.
    pub autosaved: u64,
    /// When project.json was last saved; None if the project was never saved.
    pub saved: Option<u64>,
    /// Whether there is a readable autosave newer than project.json.
    pub has_autosave: bool,
    /// Audio recorded when Marbble quit, recovered along with the project.
    pub recording: Option<crate::session::RecordingRecovery>,
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Whether `dir` has unsaved work to recover: a readable autosave newer than project.json, or
/// audio left by an interrupted recording.
pub fn recovery_info(dir: &Path) -> Option<Recovery> {
    let saved = modified(&dir.join(PROJECT_FILE));
    let auto = modified(&dir.join(AUTOSAVE_FILE)).filter(|a| saved.map(|s| s < *a).unwrap_or(true)).filter(|_| {
        // A damaged autosave can't be recovered; opening the project falls back past it.
        let ok = std::fs::read(dir.join(AUTOSAVE_FILE)).ok().map(|b| serde_json::from_slice::<Project>(&b).is_ok()).unwrap_or(false);
        if !ok {
            log::warn!("the autosave in {} is damaged; not offering it", dir.display());
        }
        ok
    });
    let recording = crate::session::project::recovery::recording_info(dir);
    if auto.is_none() && recording.is_none() {
        return None;
    }
    Some(Recovery {
        path: dir.to_string_lossy().to_string(),
        name: dir.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        autosaved: auto.map(secs).unwrap_or(0).max(recording.as_ref().map(|r| r.started).unwrap_or(0)),
        saved: saved.map(secs),
        has_autosave: auto.is_some(),
        recording,
    })
}

/// Deletes the autosave in `dir`, if any, and the journals of interrupted recordings (their
/// audio files stay, repaired).
pub fn discard_autosave(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(AUTOSAVE_FILE));
    crate::session::project::recovery::discard_journals(dir, None);
}

impl Session {
    /// The next autosave, once Settings › General's interval has passed with unsaved changes,
    /// or right after a recording, bounce or freeze (`request_autosave`). Call it from the
    /// main thread now and then and hand the job to a worker thread.
    pub fn autosave_job(&mut self) -> Option<AutosaveJob> {
        let mins = self.prefs.general.autosave_minutes;
        let due = mins > 0 && self.autosaved_at.elapsed() >= Duration::from_secs(mins as u64 * 60);
        if !self.dirty || self.rec_punch.is_some() || !(due || self.autosave_requested()) {
            return None;
        }
        self.autosave_soon = false;
        self.autosaved_at = Instant::now();
        self.autosave_doc()
    }

    /// Autosaves right away (if anything changed since the last autosave).
    pub fn autosave_now(&mut self) -> Result<Option<PathBuf>> {
        match self.autosave_doc() {
            Some(job) => job.write().map(Some),
            None => Ok(None),
        }
    }

    pub(crate) fn autosave_doc(&mut self) -> Option<AutosaveJob> {
        let doc = self.capture_plugin_states();
        let text = serde_json::to_string(&doc).ok()?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.dir, &text).hash(&mut h);
        let hash = h.finish();
        if hash == self.autosave_hash {
            // The autosave on disk already holds these takes.
            for j in std::mem::take(&mut self.unsaved_journals) {
                let _ = std::fs::remove_file(j);
            }
            return None;
        }
        self.autosave_hash = hash;
        self.adopt_states(&doc, true);
        Some(AutosaveJob { path: self.dir.join(AUTOSAVE_FILE), text, journals: std::mem::take(&mut self.unsaved_journals) })
    }

    /// Opens a project from its autosave (or, with only an interrupted recording to recover,
    /// from project.json), then puts back any takes recordings left unfinished. It counts as
    /// edited, so saving writes it back.
    pub fn open_recovered(&mut self, dir: &Path) -> Result<()> {
        let dir = if dir.is_file() { dir.parent().unwrap_or(dir).to_path_buf() } else { dir.to_path_buf() };
        let autosave = recovery_info(&dir).map(|r| r.has_autosave).unwrap_or(false);
        if autosave {
            self.open_first(&dir, &[AUTOSAVE_FILE, PROJECT_FILE, crate::session::BACKUP_FILE])?;
        } else if [PROJECT_FILE, AUTOSAVE_FILE, crate::session::BACKUP_FILE].iter().any(|f| dir.join(f).exists()) {
            self.open_checked(&dir)?;
        } else {
            // A recording in a project that was never saved or autosaved.
            self.new_project();
            self.project.name = dir.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
            self.dir = dir.clone();
        }
        self.saved = self.dir.join(PROJECT_FILE).exists();
        self.recover_recordings();
        self.dirty = true;
        Ok(())
    }

    /// Deletes the current project's autosave and recording journals (the user chose not to
    /// keep the changes).
    pub fn discard_autosave(&mut self) {
        let _ = std::fs::remove_file(self.dir.join(AUTOSAVE_FILE));
        for j in std::mem::take(&mut self.unsaved_journals) {
            let _ = std::fs::remove_file(j);
        }
        crate::session::project::recovery::discard_journals(&self.dir, self.rec_journal.as_deref());
        self.autosave_hash = 0;
        self.autosave_soon = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::scratch_session as session;

    #[test]
    fn autosave_writes_and_recovery_is_detected() {
        let (mut s, scratch) = session("autosave");
        s.prefs.general.autosave_minutes = 1;
        let dir = s.dir.clone();
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        assert!(s.dirty);
        // Not due yet.
        assert!(s.autosave_job().is_none());
        s.autosaved_at = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
        let job = s.autosave_job().expect("autosave due");
        let path = std::thread::spawn(move || job.write()).join().unwrap().unwrap();
        assert_eq!(path, dir.join(AUTOSAVE_FILE));
        assert!(path.exists());
        // Never saved: the autosave is recoverable.
        let r = recovery_info(&dir).expect("recoverable");
        assert!(r.saved.is_none());

        // Saving supersedes it.
        s.save(None).unwrap();
        assert!(!dir.join(AUTOSAVE_FILE).exists());
        assert!(recovery_info(&dir).is_none());

        // A later edit autosaves again and is newer than project.json.
        std::thread::sleep(Duration::from_millis(20));
        s.set_tempo(97.0);
        s.dirty = true;
        assert!(s.autosave_now().unwrap().is_some());
        // Nothing new since: nothing written.
        assert!(s.autosave_now().unwrap().is_none());
        let r = recovery_info(&dir).expect("newer autosave");
        assert!(r.saved.is_some());

        // Recovering brings back the unsaved tempo, marked as edited.
        s.open(&dir).unwrap();
        assert_eq!(s.project.tempo, 120.0);
        s.open_recovered(&dir).unwrap();
        assert_eq!(s.project.tempo, 97.0);
        assert!(s.dirty && s.saved);
        s.discard_autosave();
        assert!(recovery_info(&dir).is_none());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn autosave_off_or_clean_does_nothing() {
        let (mut s, scratch) = session("off");
        s.prefs.general.autosave_minutes = 0;
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.autosaved_at = Instant::now().checked_sub(Duration::from_secs(3600)).unwrap();
        assert!(s.autosave_job().is_none());
        s.prefs.general.autosave_minutes = 1;
        s.dirty = false;
        assert!(s.autosave_job().is_none());
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
