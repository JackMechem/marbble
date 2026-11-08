//! The project on disk: new, save and open. A project is a `.marbble` folder holding `project.json`
//! and its audio.
//!
//! Autosave and recovery are in `options.rs` and `recovery.rs`, backups in `backups.rs`, project
//! alternatives in `alternatives.rs`.

use super::*;

impl Session {
    pub fn project_path(&self) -> PathBuf {
        self.dir.clone()
    }

    pub fn new_project(&mut self) {
        if self.rec_punch.is_some() {
            self.stop();
        }
        self.sampler_record_cancel();
        self.send(Cmd::Stop);
        self.close_recordings();
        self.controllers.clear();
        self.vst_watches.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.auto = Default::default();
        self.project = Project::new("Untitled", self.sample_rate() as u32);
        self.project.click = self.prefs.click;
        self.dir = untitled_dir(&self.projects_root());
        self.saved = false;
        self.dirty = false;
        self.forget_unsaved();
        self.clips.clear();
        self.clips_unreadable.clear();
        self.selected = None;
        self.locate(0);
        self.rebuild_graph();
        self.sampler.pool.forget_unused();
    }

    /// Forgets every undo and redo step. The Project Chooser builds a new project's template
    /// out of ordinary edits, which shouldn't be undoable back to an empty project.
    pub fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Saves into `dir` (a `.marbble` folder, or an older `.fermata` one). Saving somewhere new copies the audio files.
    pub fn save(&mut self, dir: Option<PathBuf>) -> Result<PathBuf> {
        let target = dir.unwrap_or_else(|| self.dir.clone());
        let target = crate::old_name::project_path(target);
        let in_place = target == self.dir;
        std::fs::create_dir_all(target.join(AUDIO_DIR))?;
        self.ensure_freeze_files();
        if target != self.dir {
            // Every file the project uses ends up inside the new folder: its own files keep
            // their subfolder (Audio Files, Freeze Files), files from elsewhere (a custom
            // Recording Folder, say) go into Audio Files, and a name already taken gets a number.
            let mut taken = std::collections::HashSet::new();
            let mut copied: HashMap<PathBuf, String> = HashMap::new();
            for c in &mut self.project.clips {
                let src = if Path::new(&c.path).is_absolute() { PathBuf::from(&c.path) } else { self.dir.join(&c.path) };
                if src.starts_with(&target) {
                    continue;
                }
                // Clips sharing a file share the copy.
                if let Some(rel) = copied.get(&src) {
                    c.path = rel.clone();
                    continue;
                }
                let folder = match src.strip_prefix(&self.dir).ok().and_then(|rel| rel.parent()) {
                    Some(sub) if !sub.as_os_str().is_empty() => sub.to_path_buf(),
                    _ => PathBuf::from(AUDIO_DIR),
                };
                let rel = unique_name(&target, &folder, &src, &mut taken);
                if src.exists() {
                    std::fs::create_dir_all(target.join(&folder))?;
                    std::fs::copy(&src, target.join(&rel)).with_context(|| format!("copying {}", src.display()))?;
                }
                c.path = rel.to_string_lossy().to_string();
                copied.insert(src, c.path.clone());
            }
            self.sampler_copy_samples(&target)?;
        }
        self.project.name = target.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        // The first alternative is named after the project, and keeps that name (alternatives.rs).
        if self.project.alternative.is_empty() {
            self.project.alternative = self.project.name.clone();
        }
        let doc = self.capture_plugin_states();
        self.write_project(&target, serde_json::to_string_pretty(&doc)?.as_bytes())?;
        self.saved_doc(&doc);
        // The saved file supersedes any autosave, here and where the project used to be.
        let _ = std::fs::remove_file(target.join(AUTOSAVE_FILE));
        let _ = std::fs::remove_file(self.dir.join(AUTOSAVE_FILE));
        self.autosave_hash = 0;
        self.dir = target.clone();
        self.saved = true;
        self.dirty = false;
        // Discarded recordings the saved project (and its undo history) no longer needs go now.
        if in_place {
            self.purge_discarded(false);
        }
        Ok(target)
    }

    /// Opens a project. A missing or damaged project.json falls back to the autosave, then to
    /// the backup of the previous save, with a warning (see `session/recovery.rs`).
    pub fn open(&mut self, dir: &Path) -> Result<()> {
        self.open_checked(dir)
    }

    /// Opens the project document `file` inside the project folder `dir`.
    pub(super) fn open_from(&mut self, dir: &Path, file: &str) -> Result<()> {
        let dir = if dir.is_file() { dir.parent().unwrap_or(dir).to_path_buf() } else { dir.to_path_buf() };
        let text = std::fs::read_to_string(dir.join(file)).with_context(|| format!("reading {}", dir.display()))?;
        let mut project: Project = serde_json::from_str(&text)?;
        // Files from the projects folder's old name are found under its new one.
        for c in &mut project.clips {
            if let Some(p) = Path::new(&c.path).is_absolute().then(|| crate::old_name::relocated(Path::new(&c.path))).flatten() {
                c.path = p.to_string_lossy().to_string();
            }
        }
        if self.rec_punch.is_some() {
            self.stop();
        }
        self.sampler_record_cancel();
        self.send(Cmd::Stop);
        self.close_recordings();
        // Instance ids restart in every project, so old controllers must not leak state into
        // the new one.
        self.controllers.clear();
        self.vst_watches.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.auto = Default::default();
        self.forget_unsaved();
        let project_rate = project.sample_rate;
        self.project = project;
        self.dir = dir;
        // Only what its tracks can play: clips nothing uses any more wait on disk (clip_loading.rs).
        self.load_project_clips();
        self.saved = true;
        self.dirty = false;
        self.selected = self.project.tracks.first().map(|t| t.id);
        self.locate(0);
        if project_rate != self.audio_status.sample_rate && self.audio_config.sample_rate != project_rate {
            // Switch the device to the project's rate, as Logic does.
            self.audio_config.sample_rate = project_rate;
            self.restart_audio();
        }
        self.project.sample_rate = self.sample_rate() as u32;
        self.project.prune_routes();
        self.rebuild_graph();
        let mut missing: Vec<&str> = self.missing_plugins().iter().map(|s| s.name.as_str()).collect();
        missing.sort_unstable();
        missing.dedup();
        if !missing.is_empty() {
            self.warning = Some(format!("Not installed: {}. Their settings are kept with the project.", missing.join(", ")));
        }
        self.sampler.pool.forget_unused();
        self.sampler_warn_missing();
        Ok(())
    }
}
