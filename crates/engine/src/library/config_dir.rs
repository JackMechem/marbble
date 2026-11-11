//! Where Marbble keeps its settings folder (settings, recent projects, the plug-in cache, presets,
//! channel strips, templates, controller assignments), and the one-time move from where earlier
//! versions kept it.
//!
//! - `$XDG_CONFIG_HOME/marbble` whenever `XDG_CONFIG_HOME` is set, on every system. On macOS this
//!   is an explicit override (the headless checks use it to keep away from the real folder).
//! - macOS: `~/Library/Application Support/io.github.jackmechem.marbble`, the bundle identifier,
//!   as Tauri's `app_config_dir` and Apple's guidance have it. Up to 0.1.0 it was
//!   `~/.config/marbble`; [`migrate`] moves it.
//! - Windows: `%APPDATA%\marbble`.
//! - Linux and the rest: `~/.config/marbble`.
//!
//! [`location`] is a pure function of the system, the home folder and the environment, so all of
//! this is tested without touching the real home folder.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The settings folder's name everywhere but macOS.
const NAME: &str = "marbble";

/// The kind of system, as far as the settings folder is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
    /// Linux and the other Unix-likes: the XDG layout.
    Other,
}

impl Os {
    /// The system this was built for.
    pub const CURRENT: Os = if cfg!(target_os = "macos") {
        Os::MacOs
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Other
    };
}

/// The settings folder and where earlier versions kept it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// The settings folder.
    pub dir: PathBuf,
    /// The folder from before the rename to Marbble (`fermata` beside the old place, or beside
    /// `dir` when the folder never moved).
    pub old_name: PathBuf,
    /// The folder's earlier place under the same name, when it has moved: `~/.config/marbble` on
    /// macOS.
    pub old_place: Option<PathBuf>,
}

/// Where the settings folder is for `os`, with `home` the home folder and `var` reading an
/// environment variable (empty values count as unset, as the XDG spec says).
pub fn location(os: Os, home: Option<&Path>, var: &dyn Fn(&str) -> Option<OsString>) -> Location {
    let var = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let old_name = crate::library::old_name::OLD_CONFIG;
    if let Some(xdg) = var("XDG_CONFIG_HOME") {
        return Location { dir: xdg.join(NAME), old_name: xdg.join(old_name), old_place: None };
    }
    if os == Os::MacOs {
        if let Some(home) = home {
            let dot_config = home.join(".config");
            return Location {
                dir: home.join("Library/Application Support").join(crate::library::old_name::IDENTIFIER),
                old_name: dot_config.join(old_name),
                old_place: Some(dot_config.join(NAME)),
            };
        }
    }
    let base = var("APPDATA").or_else(|| home.map(|h| h.join(".config"))).unwrap_or_else(|| PathBuf::from("."));
    Location { dir: base.join(NAME), old_name: base.join(old_name), old_place: None }
}

fn env_var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// The settings folder's [`Location`] on this system.
pub fn current() -> Location {
    let home = env_var("HOME").map(PathBuf::from);
    location(Os::CURRENT, home.as_deref(), &env_var)
}

/// Marbble's settings folder (see the module's documentation for where it is).
pub fn config_dir() -> PathBuf {
    current().dir
}

/// Moves the settings folder from where earlier versions kept it, once, at launch: the old name's
/// folder to the old place, then the old place to the new one.
pub fn migrate() {
    migrate_location(&current(), &|a, b| std::fs::rename(a, b));
}

type Rename<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

/// [`migrate`] for `loc`, with `rename` the first try at each move (the tests make it fail, as a
/// rename across volumes does, to exercise the copy).
fn migrate_location(loc: &Location, rename: Rename) {
    match &loc.old_place {
        Some(place) => {
            // As before the move: `fermata` becomes `marbble` only when there is none yet.
            if place.symlink_metadata().is_err() {
                report(&loc.old_name, place, move_dir(&loc.old_name, place, rename));
            }
            report(place, &loc.dir, merge_dir(place, &loc.dir, rename));
        }
        None => {
            if loc.dir.symlink_metadata().is_err() {
                report(&loc.old_name, &loc.dir, move_dir(&loc.old_name, &loc.dir, rename));
            }
        }
    }
}

fn report(from: &Path, to: &Path, result: io::Result<bool>) {
    match result {
        Ok(true) => log::info!("moved the settings folder {} to {}", from.display(), to.display()),
        Ok(false) => {}
        Err(e) => log::warn!("could not move the settings folder {} to {}: {e}", from.display(), to.display()),
    }
}

/// Moves the folder `from` to `to`, which mustn't exist: a rename, or, when that fails (another
/// volume), a copy to a temporary name beside `to` that is renamed to `to` once complete, so an
/// interrupted move leaves `from` whole and `to` absent. Returns whether anything moved.
fn move_dir(from: &Path, to: &Path, rename: Rename) -> io::Result<bool> {
    if !from.is_dir() || to.symlink_metadata().is_ok() {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    move_entry(from, to, rename)?;
    Ok(true)
}

/// Moves `from` to `to` (neither checked): a rename, else a copy to a partial name and a rename.
fn move_entry(from: &Path, to: &Path, rename: Rename) -> io::Result<()> {
    if rename(from, to).is_ok() {
        return Ok(());
    }
    let mut partial = to.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    remove(&partial)?; // left by an interrupted copy
    copy_tree(from, &partial)?;
    std::fs::rename(&partial, to)?;
    if let Err(e) = remove(from) {
        log::warn!("moved {} to {}, but could not remove the old copy: {e}", from.display(), to.display());
    }
    Ok(())
}

/// [`move_dir`] when `to` doesn't exist; when it does (macOS keeps other things for the app in
/// that folder), moves each entry of `from` that `to` doesn't have, never overwriting, and removes
/// `from` once it's empty. Each entry moves whole, so an interrupted merge picks up where it left off.
fn merge_dir(from: &Path, to: &Path, rename: Rename) -> io::Result<bool> {
    if !from.is_dir() {
        return Ok(false);
    }
    if to.symlink_metadata().is_err() {
        return move_dir(from, to, rename);
    }
    let mut moved = false;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let target = to.join(&name);
        if target.symlink_metadata().is_ok() {
            continue;
        }
        move_entry(&entry.path(), &target, rename)?;
        moved = true;
    }
    if std::fs::read_dir(from)?.next().is_none() {
        std::fs::remove_dir(from)?;
    } else {
        log::info!("{} still holds files that {} has too; it is no longer used", from.display(), to.display());
    }
    Ok(moved)
}

/// Removes a file, link or folder, if there is one.
fn remove(path: &Path) -> io::Result<()> {
    match path.symlink_metadata() {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Copies a file, link or folder (recursively), keeping links as links.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let meta = from.symlink_metadata()?;
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        std::fs::set_permissions(to, meta.permissions())
    } else if meta.file_type().is_symlink() {
        copy_link(from, to)
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

#[cfg(not(unix))]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::copy(from, to).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const ID: &str = "io.github.jackmechem.marbble";

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
        move |k| map.get(k).cloned()
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn macos_uses_application_support_and_remembers_the_old_places() {
        let loc = location(Os::MacOs, Some(Path::new("/Users/a")), &env(&[]));
        assert_eq!(loc.dir, p("/Users/a/Library/Application Support").join(ID));
        assert_eq!(loc.old_place, Some(p("/Users/a/.config/marbble")));
        assert_eq!(loc.old_name, p("/Users/a/.config/fermata"));
    }

    #[test]
    fn xdg_config_home_wins_everywhere() {
        for os in [Os::MacOs, Os::Windows, Os::Other] {
            let loc = location(os, Some(Path::new("/h")), &env(&[("XDG_CONFIG_HOME", "/x"), ("APPDATA", "/w")]));
            assert_eq!(loc, Location { dir: p("/x/marbble"), old_name: p("/x/fermata"), old_place: None }, "{os:?}");
        }
        // Empty counts as unset.
        let loc = location(Os::Other, Some(Path::new("/h")), &env(&[("XDG_CONFIG_HOME", "")]));
        assert_eq!(loc.dir, p("/h/.config/marbble"));
    }

    #[test]
    fn linux_and_windows_are_unchanged() {
        let loc = location(Os::Other, Some(Path::new("/home/a")), &env(&[]));
        assert_eq!(loc, Location { dir: p("/home/a/.config/marbble"), old_name: p("/home/a/.config/fermata"), old_place: None });
        let loc = location(Os::Windows, None, &env(&[("APPDATA", r"C:\Users\a\AppData\Roaming")]));
        assert_eq!(loc.dir, p(r"C:\Users\a\AppData\Roaming").join("marbble"));
        assert_eq!(loc.old_place, None);
        // Without a home folder, the working directory.
        assert_eq!(location(Os::Other, None, &env(&[])).dir, p("./marbble"));
        assert_eq!(location(Os::MacOs, None, &env(&[])).dir, p("./marbble"));
    }

    /// A fresh folder under the system temp dir standing in for the home folder, never the real one.
    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("marbble-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mac(home: &Path) -> Location {
        location(Os::MacOs, Some(home), &env(&[]))
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn fs_rename(a: &Path, b: &Path) -> io::Result<()> {
        std::fs::rename(a, b)
    }

    /// A rename that fails as one across volumes does.
    fn no_rename(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("cross-device link"))
    }

    #[test]
    fn the_old_folder_moves_to_application_support_once() {
        for rename in [fs_rename as fn(&Path, &Path) -> io::Result<()>, no_rename] {
            let home = temp_home("move");
            let loc = mac(&home);
            let old = home.join(".config/marbble");
            write(&old.join("settings.json"), "{\"a\":1}");
            write(&old.join("presets/Compressor/Soft.json"), "{}");
            #[cfg(unix)]
            std::os::unix::fs::symlink("presets", old.join("link")).unwrap();
            migrate_location(&loc, &rename);
            assert_eq!(read(&loc.dir.join("settings.json")), "{\"a\":1}");
            assert_eq!(read(&loc.dir.join("presets/Compressor/Soft.json")), "{}");
            #[cfg(unix)]
            assert_eq!(std::fs::read_link(loc.dir.join("link")).unwrap(), p("presets"));
            assert!(!old.exists(), "the old folder is gone");
            assert!(!home.join("Library/Application Support").join(format!("{ID}.partial")).exists());

            // An old folder that reappears (an older version run since) never overwrites.
            write(&old.join("settings.json"), "{\"a\":2}");
            migrate_location(&loc, &rename);
            assert_eq!(read(&loc.dir.join("settings.json")), "{\"a\":1}");
            std::fs::remove_dir_all(&home).unwrap();
        }
    }

    #[test]
    fn an_interrupted_copy_is_redone() {
        let home = temp_home("partial");
        let loc = mac(&home);
        write(&home.join(".config/marbble/settings.json"), "{}");
        // What a copy cut short leaves: a partial folder, the old one whole and no new one.
        let partial = home.join("Library/Application Support").join(format!("{ID}.partial"));
        write(&partial.join("half"), "");
        migrate_location(&loc, &no_rename);
        assert_eq!(read(&loc.dir.join("settings.json")), "{}");
        assert!(!loc.dir.join("half").exists() && !partial.exists());
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn it_merges_into_a_folder_the_system_made_already() {
        let home = temp_home("merge");
        let loc = mac(&home);
        write(&loc.dir.join("WebKit/data"), "webview");
        write(&loc.dir.join("recent.json"), "new");
        write(&home.join(".config/marbble/recent.json"), "old");
        write(&home.join(".config/marbble/settings.json"), "{}");
        migrate_location(&loc, &no_rename);
        assert_eq!(read(&loc.dir.join("WebKit/data")), "webview");
        assert_eq!(read(&loc.dir.join("recent.json")), "new", "nothing is overwritten");
        assert_eq!(read(&loc.dir.join("settings.json")), "{}");
        assert!(!home.join(".config/marbble/settings.json").exists());
        assert!(home.join(".config/marbble/recent.json").exists(), "what clashed stays where it was");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn the_old_name_moves_on_through_the_old_place() {
        let home = temp_home("oldname");
        let loc = mac(&home);
        write(&home.join(".config/fermata/settings.json"), "fermata");
        migrate_location(&loc, &fs_rename);
        assert_eq!(read(&loc.dir.join("settings.json")), "fermata");
        assert!(!home.join(".config/fermata").exists() && !home.join(".config/marbble").exists());
        std::fs::remove_dir_all(&home).unwrap();

        // With both old folders, `marbble` wins and `fermata` is left alone, as before.
        let home = temp_home("both");
        let loc = mac(&home);
        write(&home.join(".config/fermata/settings.json"), "fermata");
        write(&home.join(".config/marbble/settings.json"), "marbble");
        migrate_location(&loc, &fs_rename);
        assert_eq!(read(&loc.dir.join("settings.json")), "marbble");
        assert!(home.join(".config/fermata/settings.json").exists());
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn linux_still_renames_the_old_name_in_place() {
        let home = temp_home("linux");
        let loc = location(Os::Other, Some(&home), &env(&[]));
        write(&home.join(".config/fermata/settings.json"), "fermata");
        migrate_location(&loc, &fs_rename);
        assert_eq!(read(&home.join(".config/marbble/settings.json")), "fermata");
        assert!(!home.join("Library").exists());
        std::fs::remove_dir_all(&home).unwrap();
    }
}
