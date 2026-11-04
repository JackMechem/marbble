//! Marbble was called Fermata up to 0.1.0-alpha.5. This is the one place that knows the old
//! name: projects saved as `.fermata` folders still open and save in place, [`migrate`] moves
//! what the old name left on disk to the new one, once, at launch, and [`rename_projects`] renames
//! the old projects folder when the user asks (Settings › General).

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

/// The projects folder from before the rename and the name it would take: `~/Music/Fermata` and
/// `~/Music/Marbble`, while the first is a folder and nothing has the second name.
pub fn projects_rename() -> Option<(PathBuf, PathBuf)> {
    rename_offer(&home())
}

fn rename_offer(home: &Path) -> Option<(PathBuf, PathBuf)> {
    let music = home.join("Music");
    let (old, new) = (music.join("Fermata"), music.join("Marbble"));
    // `symlink_metadata`, so a link named Marbble counts as taken even when it points nowhere.
    (old.is_dir() && new.symlink_metadata().is_err()).then_some((old, new))
}

/// `path` after `old` was renamed to `new`: Some when it is `old` or lies inside it.
pub fn moved(path: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(old).ok()?;
    Some(if rest.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rest) })
}

/// [`moved`] for a path stored as text, trying each `(old, new)` pair in turn.
pub fn moved_str(path: &str, pairs: &[(PathBuf, PathBuf)]) -> Option<String> {
    pairs.iter().find_map(|(old, new)| moved(Path::new(path), old, new)).map(|p| p.to_string_lossy().to_string())
}

/// The settings that name folders (Project Folder, Recording Folder), moved along with a renamed
/// projects folder. Returns whether any changed.
pub fn rebase_prefs(prefs: &mut crate::prefs::Prefs, pairs: &[(PathBuf, PathBuf)]) -> bool {
    let mut changed = false;
    for f in [&mut prefs.general.project_folder, &mut prefs.recording.folder].into_iter().flatten() {
        if let Some(m) = moved_str(f, pairs) {
            *f = m;
            changed = true;
        }
    }
    changed
}

/// Renames the old projects folder `old` to `new`, once nothing could be lost by it: refuses,
/// touching nothing, when `old` isn't a folder, when anything is called `new` already, or when
/// `open` (the open project's folder, if it is on disk) lies inside `old`. The move is a plain
/// rename, so it fails rather than copies when the two are on different file systems.
///
/// Returns the pairs of (old, new) path prefixes that paths stored elsewhere (recent projects,
/// settings) should be rewritten by: the plain one, and the canonical one when it differs (a
/// symlinked `~/Music`).
pub fn rename_projects(old: &Path, new: &Path, open: Option<&Path>) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if !old.is_dir() {
        return Err(format!("{} isn't there any more.", old.display()));
    }
    if new.symlink_metadata().is_ok() {
        return Err(format!("{} exists already, so nothing was moved.", new.display()));
    }
    let canonical_old = std::fs::canonicalize(old).unwrap_or_else(|_| old.to_path_buf());
    if let Some(open) = open {
        let canonical_open = std::fs::canonicalize(open).unwrap_or_else(|_| open.to_path_buf());
        if open.starts_with(old) || canonical_open.starts_with(&canonical_old) {
            let name = open.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            return Err(format!("“{name}” is open from {}. Open a project from somewhere else, or start a new one, and try again.", old.display()));
        }
    }
    std::fs::rename(old, new).map_err(|e| format!("Could not rename {} to {}: {e}. Nothing was moved.", old.display(), new.display()))?;
    log::info!("renamed {} to {}", old.display(), new.display());
    let mut pairs = vec![(old.to_path_buf(), new.to_path_buf())];
    if canonical_old != old {
        let canonical_new = std::fs::canonicalize(new).unwrap_or_else(|_| new.to_path_buf());
        pairs.push((canonical_old, canonical_new));
    }
    Ok(pairs)
}

/// Where an audio file a project names by its full path is now: a file that was inside
/// `~/Music/Fermata` and is missing there is looked for at the same place in `~/Music/Marbble`,
/// for projects that use files from another project after the folder was renamed.
pub fn relocated(path: &Path) -> Option<PathBuf> {
    let music = home().join("Music");
    relocated_in(path, &music.join("Fermata"), &music.join("Marbble"))
}

fn relocated_in(path: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    if path.exists() {
        return None;
    }
    moved(path, old, new).filter(|p| p.exists())
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

    /// A fresh folder under the system temp dir, never the real `~/Music`.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("marbble-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_rename_is_offered_only_with_an_old_folder_and_no_new_one() {
        let home = temp("offer");
        assert_eq!(rename_offer(&home), None, "no ~/Music/Fermata");
        std::fs::create_dir_all(home.join("Music/Fermata")).unwrap();
        assert_eq!(rename_offer(&home), Some((home.join("Music/Fermata"), home.join("Music/Marbble"))));
        std::fs::write(home.join("Music/Marbble"), "a file").unwrap();
        assert_eq!(rename_offer(&home), None, "something has the new name");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn renaming_moves_everything_and_refuses_what_could_lose_data() {
        let music = temp("rename");
        let (old, new) = (music.join("Fermata"), music.join("Marbble"));
        std::fs::create_dir_all(old.join("Song.fermata/Audio Files")).unwrap();
        std::fs::write(old.join("Song.fermata/project.json"), "{}").unwrap();
        std::fs::write(old.join("Song.fermata/Audio Files/a.wav"), "RIFF").unwrap();

        // A project open from the folder holds it.
        let err = rename_projects(&old, &new, Some(&old.join("Song.fermata"))).unwrap_err();
        assert!(err.contains("Song"), "{err}");
        assert!(old.join("Song.fermata/project.json").exists() && !new.exists());

        // So does anything with the new name; nothing is overwritten.
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("keep"), "mine").unwrap();
        assert!(rename_projects(&old, &new, None).is_err());
        assert_eq!(std::fs::read_to_string(new.join("keep")).unwrap(), "mine");
        assert!(old.join("Song.fermata/project.json").exists());
        std::fs::remove_dir_all(&new).unwrap();

        // A project open from elsewhere doesn't.
        let pairs = rename_projects(&old, &new, Some(&music.join("Other.marbble"))).unwrap();
        assert!(!old.exists());
        assert_eq!(std::fs::read_to_string(new.join("Song.fermata/Audio Files/a.wav")).unwrap(), "RIFF");
        assert_eq!(pairs[0], (old.clone(), new.clone()));

        // Once moved, there is nothing to rename.
        assert!(rename_projects(&old, &new, None).is_err());
        std::fs::remove_dir_all(&music).unwrap();
    }

    #[test]
    fn stored_paths_follow_the_folder() {
        let pairs = vec![
            (PathBuf::from("/h/Music/Fermata"), PathBuf::from("/h/Music/Marbble")),
            (PathBuf::from("/data/Music/Fermata"), PathBuf::from("/data/Music/Marbble")),
        ];
        assert_eq!(moved_str("/h/Music/Fermata/Song.fermata", &pairs).as_deref(), Some("/h/Music/Marbble/Song.fermata"));
        assert_eq!(moved_str("/data/Music/Fermata/Song.fermata", &pairs).as_deref(), Some("/data/Music/Marbble/Song.fermata"));
        assert_eq!(moved_str("/h/Music/Fermata", &pairs).as_deref(), Some("/h/Music/Marbble"));
        // Only whole path components match.
        assert_eq!(moved_str("/h/Music/Fermata Instruments/x", &pairs), None);
        assert_eq!(moved_str("/elsewhere/Song.marbble", &pairs), None);

        let mut prefs = crate::prefs::Prefs::default();
        assert!(!rebase_prefs(&mut prefs, &pairs), "unset folders stay unset");
        prefs.general.project_folder = Some("/h/Music/Fermata".into());
        prefs.recording.folder = Some("/h/Music/Fermata/Recordings".into());
        assert!(rebase_prefs(&mut prefs, &pairs));
        assert_eq!(prefs.general.project_folder.as_deref(), Some("/h/Music/Marbble"));
        assert_eq!(prefs.recording.folder.as_deref(), Some("/h/Music/Marbble/Recordings"));
    }

    #[test]
    fn missing_files_are_found_in_the_renamed_folder() {
        let music = temp("relocate");
        let (old, new) = (music.join("Fermata"), music.join("Marbble"));
        std::fs::create_dir_all(new.join("Song.fermata")).unwrap();
        std::fs::write(new.join("Song.fermata/a.wav"), "RIFF").unwrap();
        assert_eq!(relocated_in(&old.join("Song.fermata/a.wav"), &old, &new), Some(new.join("Song.fermata/a.wav")));
        assert_eq!(relocated_in(&old.join("Song.fermata/b.wav"), &old, &new), None, "missing in both");
        // A file still at its old place is used where it is.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("c.wav"), "RIFF").unwrap();
        assert_eq!(relocated_in(&old.join("c.wav"), &old, &new), None);
        std::fs::remove_dir_all(&music).unwrap();
    }
}
