//! Freeze files on disk. A freeze file the session stops playing (the track unfrozen, or the
//! freeze undone) isn't deleted then: the saved project, a backup or an undo or redo step may
//! still play it. It is noted instead, and deleted when the project is saved or closed once
//! nothing names it any more ([`Session::named_files`]), as Discard Last Recording's files are.
//! Nothing else in Freeze Files is ever deleted.

use std::collections::HashSet;

use super::*;

impl Session {
    /// Notes the freeze files of `before` that the project no longer plays (an undo, redo or
    /// abandoned edit across a freeze).
    pub(crate) fn note_unfrozen(&mut self, before: &Project) {
        let frozen = |p: &Project| p.tracks.iter().flat_map(|t| t.frozen.into_iter().chain(t.frozen_output)).map(|f| f.clip).collect::<HashSet<Id>>();
        let now = frozen(&self.project);
        for clip in frozen(before).difference(&now) {
            if let Some(c) = before.clips.iter().find(|c| c.id == *clip) {
                self.note_unused_freeze_file(self.clip_path(&c.path));
            }
        }
    }

    pub(super) fn note_unused_freeze_file(&mut self, path: PathBuf) {
        if !self.unused_freeze_files.contains(&path) {
            self.unused_freeze_files.push(path);
        }
    }

    /// Deletes the noted freeze files that nothing names any more. `closing`: the project is being
    /// closed, so its undo history is going and only the documents on disk count; files they
    /// still name are kept for good.
    pub(crate) fn purge_freeze_files(&mut self, closing: bool) {
        if self.unused_freeze_files.is_empty() {
            return;
        }
        let named = self.named_files(closing);
        let folder = self.dir.join(FREEZE_DIR);
        for path in std::mem::take(&mut self.unused_freeze_files) {
            // Only this project's renders: after Save As, the old folder's stay as they are.
            if path.parent() != Some(folder.as_path()) || !path.extension().is_some_and(|x| x == "wav") {
                continue;
            }
            if named.as_ref().is_none_or(|n| n.names(&path)) {
                if !closing {
                    self.unused_freeze_files.push(path);
                }
                continue;
            }
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("couldn't delete the unused freeze file {}: {e}", path.display());
                }
            }
        }
    }

    /// Where a render made before a Save As goes: it is moved into the project's own Freeze Files
    /// folder (named after `name`), so the project never plays a file in its old folder.
    pub(super) fn adopt_render(&self, path: PathBuf, name: &str) -> Result<PathBuf> {
        if path.parent() == Some(self.dir.join(FREEZE_DIR).as_path()) {
            return Ok(path);
        }
        let to = self.freeze_path(name);
        std::fs::create_dir_all(self.dir.join(FREEZE_DIR))?;
        if std::fs::rename(&path, &to).is_err() {
            std::fs::copy(&path, &to).with_context(|| format!("copying {}", path.display()))?;
            let _ = std::fs::remove_file(&path);
        }
        Ok(to)
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
