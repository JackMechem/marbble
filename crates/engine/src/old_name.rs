//! Marbble was called Fermata up to 0.1.0-alpha.5. This is the one place that knows the old
//! name: projects saved as `.fermata` folders still open and save in place, and [`migrate`] moves
//! what the old name left on disk to the new one, once, at launch.

use std::path::{Path, PathBuf};

/// The extension of a project folder.
pub const EXT: &str = "marbble";
/// The extension project folders had before the rename. They open and save as they are.
pub const OLD_EXT: &str = "fermata";

/// The config folder's name before the rename.
const OLD_CONFIG: &str = "fermata";
/// The app identifier before the rename; the webview keeps its storage in a folder named after it.
const OLD_IDENTIFIER: &str = "studio.fermata.daw";
/// The app identifier, as in tauri.conf.json.
pub const IDENTIFIER: &str = "io.github.jackmechem.marbble";

/// Whether `path` names a project folder, by its extension.
pub fn is_project(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == EXT || e == OLD_EXT)
}

/// `path` as a project folder: kept as it is when it already is one, given `.marbble` otherwise.
pub fn project_path(path: PathBuf) -> PathBuf {
    if is_project(&path) {
        path
    } else {
        path.with_extension(EXT)
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// The projects folder from before the rename, `~/Music/Fermata`, if it's there.
pub fn old_projects_root() -> Option<PathBuf> {
    Some(home().join("Music").join("Fermata")).filter(|p| p.is_dir())
}

/// Moves `from` to `to` when `from` exists and `to` doesn't, so it happens once and never
/// overwrites anything.
fn adopt(from: PathBuf, to: PathBuf) {
    if from.exists() && !to.exists() {
        match std::fs::rename(&from, &to) {
            Ok(()) => log::info!("moved {} to {}", from.display(), to.display()),
            Err(e) => log::warn!("could not move {} to {}: {e}", from.display(), to.display()),
        }
    }
}

/// Moves the config folder (settings, recent projects, plug-in cache, presets), the folder of
/// saved instruments and the webview's storage (window layout and view preferences) from their
/// old names. The projects folder is left where it is, since recent projects point into it;
/// [`crate::session::default_projects_root`] keeps using it.
pub fn migrate() {
    let config = crate::plugin_db::config_dir();
    if let Some(parent) = config.parent() {
        adopt(parent.join(OLD_CONFIG), config.clone());
    }
    let instruments = crate::presets::bundle_root();
    if let Some(parent) = instruments.parent() {
        adopt(parent.join("Fermata Instruments"), instruments.clone());
    }
    for base in webview_bases() {
        adopt(base.join(OLD_IDENTIFIER), base.join(IDENTIFIER));
    }
}

/// The folders the webview keeps per-app storage in, by identifier.
fn webview_bases() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(target_os = "macos") {
        out.push(home().join("Library/WebKit"));
        out.push(home().join("Library/Application Support"));
        out.push(home().join("Library/Caches"));
    } else if cfg!(windows) {
        out.extend(std::env::var_os("LOCALAPPDATA").map(PathBuf::from));
    } else {
        out.push(std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")));
        out.push(std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_project_folders_keep_their_extension() {
        assert_eq!(project_path(PathBuf::from("/m/Song.fermata")), PathBuf::from("/m/Song.fermata"));
        assert_eq!(project_path(PathBuf::from("/m/Song.marbble")), PathBuf::from("/m/Song.marbble"));
        assert_eq!(project_path(PathBuf::from("/m/Song")), PathBuf::from("/m/Song.marbble"));
        assert!(!is_project(Path::new("/m/Song.wav")));
    }

    #[test]
    fn identifier_matches_the_tauri_config() {
        let conf = include_str!("../../../src-tauri/tauri.conf.json");
        assert!(conf.contains(&format!("\"identifier\": \"{IDENTIFIER}\"")));
    }

    #[test]
    fn adopt_moves_once_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("marbble-old-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("old")).unwrap();
        std::fs::write(dir.join("old/a"), "1").unwrap();
        adopt(dir.join("old"), dir.join("new"));
        assert_eq!(std::fs::read_to_string(dir.join("new/a")).unwrap(), "1");
        std::fs::create_dir_all(dir.join("old")).unwrap();
        std::fs::write(dir.join("old/a"), "2").unwrap();
        adopt(dir.join("old"), dir.join("new"));
        assert_eq!(std::fs::read_to_string(dir.join("new/a")).unwrap(), "1");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
