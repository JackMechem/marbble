//! Freeze files on disk. Unfreezing, or undoing a freeze, doesn't delete a track's freeze file:
//! the saved project, a backup or an undo or redo step may still play it. Saving the project and
//! closing it sweep the Freeze Files folder instead, deleting the files nothing names any more, as
//! Discard Last Recording's files go (`Session::purge_discarded`).

use std::collections::HashSet;

use super::*;
use crate::session::project::files::{docs_name, project_docs};

impl Session {
    /// Deletes the files in Freeze Files that nothing can play any more: no clip of the project,
    /// of an undo or redo step, nor of a document in the project folder (project.json, its
    /// autosave, the backups or another alternative). `closing`: the project is being closed,
    /// so its undo history is going and only the documents on disk count.
    pub(crate) fn sweep_freeze_files(&self, closing: bool) {
        let Ok(entries) = std::fs::read_dir(self.dir.join(FREEZE_DIR)) else { return };
        let history = self.undo_stack.iter().chain(&self.redo_stack).map(|e| &e.project);
        let projects: Vec<&Project> = if closing { Vec::new() } else { std::iter::once(&self.project).chain(history).collect() };
        let listed: HashSet<PathBuf> = projects.iter().flat_map(|p| &p.clips).map(|c| self.clip_path(&c.path)).collect();
        let unlisted: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file() && !listed.contains(p)).collect();
        if unlisted.is_empty() {
            return;
        }
        let docs = project_docs(&self.dir);
        for path in unlisted.into_iter().filter(|p| !docs_name(&docs, &self.dir, p)) {
            if let Err(e) = std::fs::remove_file(&path) {
                log::warn!("couldn't delete the unused freeze file {}: {e}", path.display());
            }
        }
    }

    /// Where a clip's file is: its path in the project, relative to the project folder.
    pub(super) fn clip_path(&self, path: &str) -> PathBuf {
        if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.dir.join(path)
        }
    }
}
