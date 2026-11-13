//! Before a file the session stopped using is deleted (a discarded recording, an unfrozen track's
//! freeze file): whether anything can still need it. That is the project and its undo history in
//! memory, and every document in the project folder: project.json, its autosave and .bak, the
//! backups, the other alternatives and the recording journals.
//!
//! The documents are read as projects and their clips' paths compared as files, never as text:
//! relative to the project folder or absolute, folders resolved, either separator, Unicode
//! composed or decomposed (macOS writes names decomposed), any case. A document that can't be
//! read means nothing is deleted.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use icu_normalizer::ComposingNormalizerBorrowed;

use crate::model::Project;
use crate::session::project::alternatives::ALTERNATIVES_DIR;
use crate::session::project::backups::BACKUPS_DIR;
use crate::session::project::recovery::{read_journal, JOURNAL_SUFFIX};
use crate::session::Session;

/// The files something names, to check a file against before deleting it.
pub(crate) struct NamedFiles {
    keys: HashSet<String>,
}

impl NamedFiles {
    /// The files named by `projects` (in memory) and by the documents in the project folder
    /// `dir`. Errs when a document or a folder of them can't be read.
    pub(crate) fn find(dir: &Path, projects: &[&Project]) -> Result<NamedFiles> {
        let mut paths: HashSet<String> = projects.iter().flat_map(|p| &p.clips).map(|c| c.path.clone()).collect();
        documents(dir, 0, &mut paths)?;
        for sub in [BACKUPS_DIR, ALTERNATIVES_DIR] {
            if dir.join(sub).exists() {
                documents(&dir.join(sub), 2, &mut paths)?;
            }
        }
        let dir = canonical(dir);
        Ok(NamedFiles { keys: paths.iter().map(|p| key(&canonical(&resolve(&dir, p)))).collect() })
    }

    /// Whether the file at `path` is one of them.
    pub(crate) fn names(&self, path: &Path) -> bool {
        self.keys.contains(&key(&canonical(path)))
    }
}

impl Session {
    /// What still names a file of this project: its documents on disk and, unless `closing` (when
    /// the undo history goes), the project and its undo and redo steps. None, logged, when a
    /// document can't be read: then nothing may be deleted.
    pub(crate) fn named_files(&self, closing: bool) -> Option<NamedFiles> {
        let history = self.undo_stack.iter().chain(&self.redo_stack).map(|e| &e.project);
        let projects: Vec<&Project> = if closing { Vec::new() } else { std::iter::once(&self.project).chain(history).collect() };
        NamedFiles::find(&self.dir, &projects).map_err(|e| log::warn!("keeping files the project may still need: {e:#}")).ok()
    }
}

/// Adds the file paths the documents in `dir` (and its folders, `depth` deep) name to `out`.
fn documents(dir: &Path, depth: usize, out: &mut HashSet<String>) -> Result<()> {
    for e in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let p = e.with_context(|| format!("reading {}", dir.display()))?.path();
        if p.is_dir() {
            if depth > 0 {
                documents(&p, depth - 1, out)?;
            }
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.ends_with(JOURNAL_SUFFIX) {
            out.extend(read_journal(&p).with_context(|| format!("reading {}", p.display()))?.takes.into_iter().map(|t| t.path));
        } else if p.extension().is_some_and(|x| x == "json" || x == "bak") {
            let bytes = std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?;
            let doc: Project = serde_json::from_slice(&bytes).with_context(|| format!("reading {}", p.display()))?;
            out.extend(doc.clips.into_iter().map(|c| c.path));
        }
    }
    Ok(())
}

/// A clip's path as a document has it, relative to the project folder `dir` or absolute, with
/// either separator.
fn resolve(dir: &Path, path: &str) -> PathBuf {
    let p = path.replace('\\', "/");
    let drive = p.as_bytes().get(1) == Some(&b':') && p.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if p.starts_with('/') || drive || Path::new(&p).is_absolute() {
        // As opening a project finds it: in the projects folder's new name.
        crate::library::old_name::relocated(Path::new(&p)).unwrap_or_else(|| PathBuf::from(p))
    } else {
        dir.join(p)
    }
}

/// `path` with its folders resolved (symbolic links, `..`), as far as they exist.
fn canonical(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    match (path.parent().and_then(|d| std::fs::canonicalize(d).ok()), path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

/// What two spellings of one file have in common: one separator, composed Unicode, lower case.
fn key(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    ComposingNormalizerBorrowed::new_nfc().normalize(&p).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_file_spelled_differently_has_one_key() {
        let dir = Path::new("/p/Song.marbble");
        let a = key(&resolve(dir, "Freeze Files/Café Freeze.wav"));
        assert_eq!(a, key(&resolve(dir, "Freeze Files\\Café Freeze.wav")), "a Windows document read elsewhere");
        assert_eq!(a, key(&resolve(dir, "/p/Song.marbble/Freeze Files/Cafe\u{301} Freeze.wav")), "absolute and decomposed");
        assert_eq!(key(Path::new("C:\\p\\a.wav")), key(&resolve(dir, "C:/p/a.wav")), "and the other way round");
        assert_ne!(a, key(&resolve(dir, "Freeze Files/Cafe Freeze.wav")));
    }
}
