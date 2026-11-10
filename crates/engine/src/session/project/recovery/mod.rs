//! Crash safety for project files and recordings.
//!
//! - Project files are written atomically (temporary file, sync, rename), and saving keeps the
//!   previous project.json as project.json.bak. Opening falls back from a missing or damaged
//!   project.json to the autosave, then to the backup, and says so.
//! - Recordings: when recording starts, a small journal (`recording-<ms>.journal.json`) goes
//!   into the project folder, saying which files are being recorded for which tracks, from
//!   where, and how the recording is cut into takes. The recorder keeps those files valid as
//!   it goes (`recorder.rs`). After a crash, the journal is what lets the next launch repair
//!   the files and put the takes back where they were recorded. A clean stop hands the journal
//!   to the autosave that follows right away; once the takes are in a file on disk (that
//!   autosave, or a save), the journal is deleted.
//! - The autosave runs as soon as a recording, Bounce in Place, Freeze or import finishes,
//!   rather than waiting for the interval (Settings › General).

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::audio_io::recorder::FinishedTake;
use crate::edit::takes::recording::RecPlan;
use crate::model::{Id, InputSel, PluginSlot, Project, TrackKind};
use crate::session::{Session, AUDIO_DIR, AUTOSAVE_FILE, PROJECT_FILE};

/// The previous project.json, kept by every save.
pub const BACKUP_FILE: &str = "project.json.bak";
const JOURNAL_PREFIX: &str = "recording-";
const JOURNAL_SUFFIX: &str = ".journal.json";

/// Writes `bytes` to `path` so that a crash leaves either the old file or the new one, never
/// a mix: into a temporary file next to it, then renamed over it. `durable` also syncs the
/// data (and on Unix the folder) so the new file survives a power cut.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8], durable: bool) -> Result<()> {
    let dir = path.parent().ok_or_else(|| anyhow!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        if durable {
            f.sync_all()?;
        }
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    #[cfg(unix)]
    if durable {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

/// Saves project.json in `dir`, first keeping the one there as project.json.bak (when it is
/// readable: a damaged file never replaces a good backup).
pub(crate) fn write_project_file(dir: &Path, bytes: &[u8]) -> Result<()> {
    let path = dir.join(PROJECT_FILE);
    if let Ok(old) = std::fs::read(&path) {
        if old != bytes && serde_json::from_slice::<serde::de::IgnoredAny>(&old).is_ok() {
            if let Err(e) = write_atomic(&dir.join(BACKUP_FILE), &old, false) {
                log::warn!("couldn't keep a backup of {}: {e}", path.display());
            }
        }
    }
    write_atomic(&path, bytes, true)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The crash-recovery journal of one recording: what `finish_recording` would have needed to
/// place its takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecJournal {
    pub version: u32,
    /// When recording started, in seconds since the Unix epoch.
    pub started: u64,
    pub sample_rate: u32,
    /// Where recording started (the punch-in or the left locator).
    pub punch: i64,
    /// Where the transport started (before a count-in or pre-roll).
    pub from: i64,
    /// The cycle, when recording in cycle mode (passes become a take folder).
    pub cycle: Option<(i64, i64)>,
    /// The Autopunch range.
    pub punch_range: Option<(i64, i64)>,
    /// Frames the takes move earlier to line up with what was heard.
    pub latency: i64,
    /// A cycle pass cut short is kept as a take if it got this far.
    pub min_take: i64,
    pub takes: Vec<JournalTake>,
}

/// One file being recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalTake {
    pub track: Id,
    /// To make a track for the take if the recovered project doesn't have this one.
    pub track_name: String,
    pub input: InputSel,
    /// Absolute path of the WAV file.
    pub path: String,
    pub channels: usize,
    /// Engine position of the file's first frame.
    pub start: i64,
    pub bits: u16,
}

impl RecJournal {
    fn plan(&self) -> RecPlan {
        // Recovery finds no MIDI: it was never written down.
        RecPlan { from: self.from, cycle: self.cycle, punch: self.punch_range, latency: self.latency, midi_latency: 0, min_take: self.min_take }
    }
}

/// The recording journals left in a project folder, oldest first.
pub fn recording_journals(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().map(|n| n.to_string_lossy()).map(|n| n.starts_with(JOURNAL_PREFIX) && n.ends_with(JOURNAL_SUFFIX)).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub fn read_journal(path: &Path) -> Result<RecJournal> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

/// Where a journal's take is now: where it was recorded, or in the Audio Files of `dir` when
/// the project folder has moved since.
fn take_path(dir: &Path, t: &JournalTake) -> PathBuf {
    let p = PathBuf::from(&t.path);
    if p.exists() {
        return p;
    }
    match p.file_name() {
        Some(name) if dir.join(AUDIO_DIR).join(name).exists() => dir.join(AUDIO_DIR).join(name),
        _ => p,
    }
}

/// Audio left behind by a recording that never finished, as the recovery sheet shows it.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingRecovery {
    /// When the (latest) interrupted recording started, in seconds since the Unix epoch.
    pub started: u64,
    /// Files with audio in them.
    pub takes: usize,
    /// The longest file, in seconds.
    pub seconds: f64,
    pub tracks: Vec<String>,
}

/// What interrupted recordings in `dir` left to recover, if anything.
pub fn recording_info(dir: &Path) -> Option<RecordingRecovery> {
    let mut info = RecordingRecovery { started: 0, takes: 0, seconds: 0.0, tracks: Vec::new() };
    for j in recording_journals(dir).iter().filter_map(|p| read_journal(p).ok()) {
        for t in &j.takes {
            let Ok(w) = crate::audio_file::wav_repair::inspect(&take_path(dir, t)) else { continue };
            if w.frames == 0 {
                continue;
            }
            info.started = info.started.max(j.started);
            info.takes += 1;
            info.seconds = info.seconds.max(w.frames as f64 / w.sample_rate.max(1) as f64);
            if !info.tracks.contains(&t.track_name) {
                info.tracks.push(t.track_name.clone());
            }
        }
    }
    (info.takes > 0).then_some(info)
}

/// Deletes the recording journals in `dir` (but `keep`), first repairing the files they
/// list, so the audio stays usable by hand (File › Import) even though no region refers to it.
pub fn discard_journals(dir: &Path, keep: Option<&Path>) {
    for p in recording_journals(dir) {
        if Some(p.as_path()) == keep {
            continue;
        }
        if let Ok(j) = read_journal(&p) {
            for t in &j.takes {
                let _ = crate::audio_file::wav_repair::repair(&take_path(dir, t));
            }
        }
        let _ = std::fs::remove_file(&p);
    }
}

/// Every plug-in slot in a project.
pub(crate) fn slots_mut(p: &mut Project) -> impl Iterator<Item = &mut PluginSlot> {
    p.tracks.iter_mut().flat_map(|t| t.instrument.iter_mut().chain(t.inserts.iter_mut().flatten())).chain(p.master.inserts.iter_mut().flatten())
}

/// The same, to read.
pub(crate) fn slots(p: &Project) -> impl Iterator<Item = &PluginSlot> {
    p.tracks.iter().flat_map(|t| t.instrument.iter().chain(t.inserts.iter().flatten())).chain(p.master.inserts.iter().flatten())
}

/// Why a project document couldn't be opened, in words for the user.
fn describe(file: &str, e: &anyhow::Error) -> String {
    if let Some(j) = e.downcast_ref::<serde_json::Error>() {
        return match j.classify() {
            serde_json::error::Category::Eof => format!("{file} is cut short"),
            _ => format!("{file} is damaged ({j})"),
        };
    }
    match e.root_cause().downcast_ref::<std::io::Error>() {
        Some(io) if io.kind() == std::io::ErrorKind::NotFound => format!("{file} is missing"),
        _ => format!("{file} can't be read ({e:#})"),
    }
}

impl Session {
    /// Asks for an autosave at the main loop's next turn, if Settings › General says so: after
    /// work that would be costly to redo.
    pub(crate) fn request_autosave(&mut self) {
        if self.prefs.general.autosave_after_recording {
            self.autosave_soon = true;
        }
    }

    /// Whether an autosave was asked for (by [`Session::request_autosave`]) and not written yet.
    pub(crate) fn autosave_requested(&self) -> bool {
        self.autosave_soon && self.prefs.general.autosave_after_recording
    }

    /// Writes the journal of the recording that just started (call once the recorder has its
    /// takes and `rec_punch`/`rec_plan` are set). With unsaved changes, the project is
    /// autosaved too, so the tracks the takes belong to can be recovered with them.
    pub(crate) fn start_journal(&mut self) {
        let (Some(punch), Some(plan)) = (self.rec_punch, self.rec_plan) else { return };
        if self.prefs.general.autosave_after_recording && (self.autosave_soon || self.dirty) {
            if let Some(job) = self.autosave_doc() {
                self.autosave_soon = false;
                std::thread::spawn(move || {
                    if let Err(e) = job.write() {
                        log::warn!("autosave failed: {e}");
                    }
                });
            }
        }
        let takes: Vec<JournalTake> = self
            .recorder
            .active()
            .into_iter()
            .map(|a| {
                let t = self.project.track(a.track);
                JournalTake {
                    track: a.track,
                    track_name: t.map(|t| t.name.clone()).unwrap_or_default(),
                    input: t.map(|t| t.input).unwrap_or(InputSel::Mono(0)),
                    path: a.path.to_string_lossy().to_string(),
                    channels: a.channels,
                    start: a.start,
                    bits: a.bits,
                }
            })
            .collect();
        if takes.is_empty() {
            return;
        }
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = self.dir.join(format!("{JOURNAL_PREFIX}{millis}{JOURNAL_SUFFIX}"));
        let journal = RecJournal {
            version: 1,
            started: now_secs(),
            sample_rate: self.sample_rate() as u32,
            punch,
            from: plan.from,
            cycle: plan.cycle,
            punch_range: plan.punch,
            latency: plan.latency,
            min_take: plan.min_take,
            takes,
        };
        // Small, and it must be on disk before there's audio worth recovering.
        match serde_json::to_vec_pretty(&journal).map_err(anyhow::Error::from).and_then(|b| write_atomic(&path, &b, true)) {
            Ok(()) => self.rec_journal = Some(path),
            Err(e) => log::warn!("couldn't write the recording journal {}: {e}", path.display()),
        }
    }

    /// After a clean stop: the takes are in the project now, so the journal waits for them to
    /// reach a file on disk, which the autosave asked for here does right away.
    pub(crate) fn recording_finished(&mut self) {
        if let Some(j) = self.rec_journal.take() {
            self.unsaved_journals.push(j);
        }
        self.request_autosave();
    }

    /// Puts takes from recordings that never finished (the journals in the project folder)
    /// back on their tracks, as stopping would have, as one undo step. Files already in the
    /// project are skipped, so this is safe to repeat. Returns how many takes it placed.
    pub(crate) fn recover_recordings(&mut self) -> usize {
        let mut placed = 0;
        let mut checkpointed = false;
        for jpath in recording_journals(&self.dir) {
            let j = match read_journal(&jpath) {
                Ok(j) => j,
                Err(e) => {
                    log::warn!("unreadable recording journal {}: {e}", jpath.display());
                    continue;
                }
            };
            let used: HashSet<PathBuf> =
                self.project.clips.iter().map(|c| if Path::new(&c.path).is_absolute() { PathBuf::from(&c.path) } else { self.dir.join(&c.path) }).collect();
            let mut takes = Vec::new();
            for t in &j.takes {
                let path = take_path(&self.dir, t);
                if used.contains(&path) {
                    continue;
                }
                let info = match crate::audio_file::wav_repair::repair(&path) {
                    Ok(i) if i.frames > 0 => i,
                    Ok(_) => continue,
                    Err(e) => {
                        log::warn!("can't recover {}: {e}", path.display());
                        continue;
                    }
                };
                let decoded = match crate::audio_file::decode::decode(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        log::warn!("can't read the recovered {}: {e}", path.display());
                        continue;
                    }
                };
                if info.sample_rate as f64 != self.sample_rate() {
                    log::warn!("{} was recorded at {} Hz; the project now runs at {}", path.display(), info.sample_rate, self.sample_rate());
                }
                if !checkpointed {
                    self.checkpoint("Recover Recording", None, false);
                    checkpointed = true;
                }
                let track = match self.project.track(t.track) {
                    Some(tr) if tr.kind == TrackKind::Audio => t.track,
                    // The project was last written before this track existed.
                    _ => match self.add_track(TrackKind::Audio, t.input, None, None) {
                        Ok(id) => {
                            if let Some(tr) = self.project.track_mut(id) {
                                tr.name = if t.track_name.is_empty() { "Recovered".into() } else { t.track_name.clone() };
                            }
                            id
                        }
                        Err(e) => {
                            log::warn!("can't make a track for {}: {e}", path.display());
                            continue;
                        }
                    },
                };
                takes.push((track, FinishedTake { path, start: t.start, channels: decoded.channels, error: None }));
            }
            if !takes.is_empty() {
                placed += takes.len();
                self.rec_punch = Some(j.punch);
                self.rec_plan = Some(j.plan());
                self.rec_midi_track = None;
                self.place_recording(takes, j.latency);
            }
            // Gone once the recovered project reaches the disk.
            self.unsaved_journals.push(jpath);
        }
        if placed > 0 {
            self.sync_tracks();
            self.dirty = true;
            self.request_autosave();
        }
        placed
    }

    /// Opens the project in `dir` from project.json, or when that is missing or damaged, from
    /// the autosave or the backup, saying which in the next tick's warning.
    pub(crate) fn open_checked(&mut self, dir: &Path) -> Result<()> {
        let folder = if dir.is_file() { dir.parent().unwrap_or(dir) } else { dir };
        // With backups on there's no .bak, but the newest of the backups (backups.rs).
        let latest = crate::session::project::backups::latest_backup(folder);
        let mut files = vec![PROJECT_FILE, AUTOSAVE_FILE, BACKUP_FILE];
        files.extend(latest.as_deref());
        self.open_first(dir, &files)
    }

    /// Opens the first of `files` in `dir` that can be read. Falling back past the first one
    /// is reported as a warning and leaves the project marked as edited, so saving repairs it.
    pub(crate) fn open_first(&mut self, dir: &Path, files: &[&str]) -> Result<()> {
        let dir = if dir.is_file() { dir.parent().unwrap_or(dir).to_path_buf() } else { dir.to_path_buf() };
        let name = dir.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut problems: Vec<String> = Vec::new();
        for &file in files {
            let path = dir.join(file);
            if !problems.is_empty() && !path.exists() {
                continue;
            }
            match self.open_from(&dir, file) {
                Ok(()) => {
                    if !problems.is_empty() {
                        let what = match file {
                            AUTOSAVE_FILE => "its most recent autosave",
                            BACKUP_FILE => "the backup kept by the save before last (recent changes may be missing)",
                            f if f.starts_with(crate::session::project::backups::BACKUPS_DIR) => "its most recent backup (recent changes may be missing)",
                            _ => file,
                        };
                        let msg = format!("“{name}”: {}, so Marbble opened {what} instead. Save the project to repair it.", problems.join("; "));
                        log::warn!("{msg}");
                        self.warning = Some(msg);
                        self.saved = dir.join(PROJECT_FILE).exists();
                        self.dirty = true;
                    }
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("opening {}: {e:#}", path.display());
                    problems.push(describe(file, &e));
                }
            }
        }
        if problems.len() == 1 && !dir.join(PROJECT_FILE).exists() {
            return Err(anyhow!("“{name}” is not a Marbble project (there's no {PROJECT_FILE} in it)"));
        }
        Err(anyhow!("“{name}” can't be opened: {}, and there's no autosave or backup to fall back on.", problems.join("; ")))
    }

    /// After a save wrote `doc`: the autosave and journals it supersedes go.
    pub(crate) fn saved_doc(&mut self, doc: &Project) {
        self.adopt_states(doc, true);
        for j in std::mem::take(&mut self.unsaved_journals) {
            let _ = std::fs::remove_file(j);
        }
        self.autosave_soon = false;
    }

    /// A new project or another one was opened: what was pending for the old one no longer
    /// applies. (Its journals stay on disk, so its takes are offered for recovery next time,
    /// unless the user chose not to keep them, which deletes them; see `discard_autosave`.)
    pub(crate) fn forget_unsaved(&mut self) {
        self.unsaved_journals.clear();
        self.autosave_soon = false;
    }

    /// Writes the autosave from the project as it stands, with the project's copies of the
    /// plug-in states (`session/plugins/states/`): for a panic hook, where calling into plug-ins
    /// isn't safe. Nothing is written without unsaved changes.
    pub fn emergency_autosave(&self) -> Result<Option<PathBuf>> {
        if !self.dirty {
            return Ok(None);
        }
        let path = self.dir.join(AUTOSAVE_FILE);
        write_atomic(&path, &serde_json::to_vec(&self.project)?, true)?;
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests;
