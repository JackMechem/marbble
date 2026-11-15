//! Finding plug-in files on disk.

use std::path::{Path, PathBuf};

/// Recursively finds files or bundles with the extension `ext` (a `.vst3` bundle, a `.clap` file
/// or bundle) under `paths`, without descending into them. A name reachable by several paths
/// (NixOS's profile symlinks) is listed once.
pub fn find_files(paths: &[PathBuf], ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        walk(p, ext, &mut out, 0);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| {
        let key = p.file_name().map(|n| n.to_os_string());
        seen.insert(key)
    });
    out.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    out
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            out.push(p);
        } else if p.is_dir() {
            walk(&p, ext, out, depth + 1);
        }
    }
}
