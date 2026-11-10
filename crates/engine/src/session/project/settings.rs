//! The session side of Settings and Project Settings: applying app preferences, the projects and
//! recordings folders, project options (click, bit depth, notes), and the MIDI ports Settings lists.
//! Autosave is in `autosave.rs`; where a recording lands over existing regions is in
//! `session/takes/placement.rs`.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::dsp::{ClickParams, Cmd};
use crate::midi::input::{MidiLogEntry, MidiPort};
use crate::model::Id;
use crate::prefs::{BitDepth, ClickSettings, Prefs};
use crate::session::{default_projects_root, untitled_dir, Session, AUDIO_DIR};

impl Session {
    /// Applies app-wide settings. Safe to call at any time; only what changed is touched.
    pub fn set_prefs(&mut self, prefs: Prefs) {
        let old = std::mem::replace(&mut self.prefs, prefs);
        let p = &self.prefs;
        if old.midi.disabled_inputs != p.midi.disabled_inputs {
            self.midi.set_disabled(p.midi.disabled_inputs.clone());
        }
        if old.midi.disabled_outputs != p.midi.disabled_outputs {
            self.midi_out.set_disabled(p.midi.disabled_outputs.clone());
        }
        self.midi.set_velocity(p.midi.velocity, p.midi.fixed_velocity);
        let limit = p.general.undo_limit.max(1);
        if self.undo_stack.len() > limit {
            let extra = self.undo_stack.len() - limit;
            self.undo_stack.drain(..extra);
        }
        // An untouched new project follows the new defaults.
        let pristine = !self.saved && !self.dirty && self.project.tracks.is_empty() && self.project.clips.is_empty();
        if pristine {
            self.project.click = self.prefs.click;
            if old.general.project_folder != self.prefs.general.project_folder && !self.dir.exists() {
                self.dir = untitled_dir(&self.projects_root());
            }
        }
        let thru = self.prefs.midi.thru;
        self.send(Cmd::MidiThru(thru));
        let (notes, controllers) = (self.prefs.midi.chase_notes, self.prefs.midi.chase_controllers);
        self.send(Cmd::Chase { notes, controllers });
        self.send(Cmd::Click(ClickParams::from_settings(&self.project.click)));
        // Settings › Audio › Plug-in Delay Compensation and Low Latency Mode.
        self.update_pdc();
        // Crossfade Time and Curve shape take folder comps and loop seams as they play.
        if (old.editing.crossfade_ms, old.editing.crossfade_curve) != (self.prefs.editing.crossfade_ms, self.prefs.editing.crossfade_curve) {
            let ids: Vec<Id> = self.project.tracks.iter().map(|t| t.id).collect();
            ids.into_iter().for_each(|t| self.push_regions(t));
        }
    }

    /// Where new projects go.
    pub fn projects_root(&self) -> PathBuf {
        self.prefs.general.project_folder.as_deref().filter(|f| !f.trim().is_empty()).map(PathBuf::from).unwrap_or_else(default_projects_root)
    }

    /// Renames the projects folder from before the rename, `~/Music/Fermata`, to `~/Music/Marbble`
    /// (see [`crate::library::old_name::rename_projects`] for when it refuses). An unsaved project that
    /// would have been saved in it is saved in the new folder instead. Returns the path prefixes
    /// that stored paths (settings, recent projects) should be moved by.
    pub fn rename_old_projects_folder(&mut self) -> Result<Vec<(PathBuf, PathBuf)>> {
        let (old, new) =
            crate::library::old_name::projects_rename().ok_or_else(|| anyhow!("There is no ~/Music/Fermata to rename, or ~/Music/Marbble exists already."))?;
        self.rename_projects_folder(&old, &new)
    }

    pub(crate) fn rename_projects_folder(&mut self, old: &Path, new: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
        if self.is_recording() {
            return Err(anyhow!("Stop recording first."));
        }
        // The open project holds the folder once anything of it is on disk (a save, an autosave, a
        // recording).
        let open = self.dir.exists().then(|| self.dir.clone());
        let pairs = crate::library::old_name::rename_projects(old, new, open.as_deref()).map_err(|e| anyhow!(e))?;
        if let Some(dir) = pairs.iter().find_map(|(o, n)| crate::library::old_name::moved(&self.dir, o, n)) {
            self.dir = dir;
        }
        Ok(pairs)
    }

    /// Where new recordings go: the project's Audio Files, or a folder per project inside the
    /// recording folder chosen in Settings › Recording.
    pub(crate) fn recording_dir(&self) -> PathBuf {
        match self.prefs.recording.folder.as_deref().filter(|f| !f.trim().is_empty()) {
            Some(f) => PathBuf::from(f).join(&self.project.name),
            None => self.dir.join(AUDIO_DIR),
        }
    }

    /// The anti-click fade for a new audio region of `length` frames (Settings › Editing).
    pub(crate) fn default_fade(&self, length: i64) -> i64 {
        let ms = self.prefs.editing.fade_ms.clamp(0.0, 1000.0) as f64;
        ((ms * self.sample_rate() / 1000.0).round() as i64).min(length / 2).max(0)
    }

    /// Project Settings › Metronome.
    pub fn set_click(&mut self, click: ClickSettings) {
        self.project.click = click;
        self.send(Cmd::Click(ClickParams::from_settings(&click)));
        self.dirty = true;
    }

    /// Project Settings › Recording: None follows Settings › Recording.
    pub fn set_record_bit_depth(&mut self, depth: Option<BitDepth>) {
        self.project.bit_depth = depth;
        self.dirty = true;
    }

    pub fn set_notes(&mut self, notes: String) {
        self.project.notes = notes;
        self.dirty = true;
    }

    /// MIDI input ports, whether each is in use, and the output paired with it automatically.
    pub fn midi_ports(&self) -> Vec<MidiPort> {
        let outputs = self.midi_out.keys();
        let mut ports = self.midi.ports();
        for p in &mut ports {
            p.auto_output = crate::control::feedback::auto_output(&p.key, &outputs).map(str::to_string);
        }
        ports
    }

    /// The latest messages from hardware MIDI inputs.
    pub fn midi_monitor(&self) -> Vec<MidiLogEntry> {
        self.midi.recent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::scratch_session as session;

    #[test]
    fn renaming_the_old_projects_folder_follows_the_open_project() {
        let (mut s, scratch) = session("rename");
        let (old, new) = (scratch.join("Fermata"), scratch.join("Marbble"));
        std::fs::create_dir_all(old.join("Kept.fermata")).unwrap();
        // A saved project open from the folder holds it.
        s.dir = old.join("Song.marbble");
        s.save(None).unwrap();
        assert!(s.rename_projects_folder(&old, &new).is_err());
        assert!(old.join("Song.marbble/project.json").exists() && !new.exists());
        // An untitled project that isn't on disk yet moves with it, so saving it can't bring the
        // old folder back.
        s.new_project();
        s.dir = old.join("Untitled.marbble");
        s.rename_projects_folder(&old, &new).unwrap();
        assert_eq!(s.dir, new.join("Untitled.marbble"));
        assert!(new.join("Kept.fermata").is_dir() && new.join("Song.marbble/project.json").exists());
        s.save(None).unwrap();
        assert!(!old.exists() && new.join("Untitled.marbble/project.json").exists());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn undo_limit_is_configurable() {
        let (mut s, _scratch) = session("undo");
        let mut p = Prefs::default();
        p.general.undo_limit = 3;
        s.set_prefs(p);
        for i in 0..6 {
            s.checkpoint("Tempo", None, false);
            s.set_tempo(100.0 + i as f64);
        }
        let mut steps = 0;
        while s.undo().is_some() {
            steps += 1;
        }
        assert_eq!(steps, 3);
    }
}
