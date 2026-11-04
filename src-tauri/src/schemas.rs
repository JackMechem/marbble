//! GSettings schemas for GTK's file chooser (Linux).
//!
//! GTK's file chooser reads its settings from the `org.gtk.Settings.FileChooser` schema, and
//! GLib aborts the whole process when a schema isn't installed. A packaged build finds it (the
//! Nix wrapper, the AppImage and the distribution's GTK put it on the search path), and so does
//! the Nix dev shell, but a binary started some other way can miss it: File › Save As then
//! killed the app. So:
//!
//! - At startup, before anything uses GSettings, `ensure_file_chooser_schema` looks for the
//!   schema where GLib will. If it isn't there but is in a data directory known when Marbble was
//!   built (the dev shell's `XDG_DATA_DIRS`), that directory is added to `XDG_DATA_DIRS`.
//! - Before each file dialog the frontend asks `file_dialog_problem` (src/fileDialog.ts), which
//!   looks the schema up in GLib's own list; when it is missing, the frontend shows what to do
//!   instead of opening the dialog.
//!
//! Other platforms have no GSettings: there the check always passes.

/// Why a file dialog can't open, or None when it can.
#[tauri::command]
pub fn file_dialog_problem() -> Option<String> {
    #[cfg(target_os = "linux")]
    return linux::problem();
    #[cfg(not(target_os = "linux"))]
    None
}

#[cfg(target_os = "linux")]
pub use linux::ensure_file_chooser_schema;

#[cfg(not(target_os = "linux"))]
pub fn ensure_file_chooser_schema() {}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    /// The schema GTK's file chooser can't run without.
    pub const FILE_CHOOSER: &str = "org.gtk.Settings.FileChooser";

    /// What GLib reads when `XDG_DATA_DIRS` is unset or empty.
    const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

    /// The schema directories GLib reads, given the environment it will see: each data directory's
    /// `glib-2.0/schemas`, the user's (`XDG_DATA_HOME`, or `~/.local/share`), then the directories
    /// in `GSETTINGS_SCHEMA_DIR`. Mirrors `initialise_schema_sources` in GLib's gsettingsschema.c.
    pub fn schema_dirs(data_dirs: Option<&str>, data_home: Option<&str>, home: Option<&str>, schema_dir: Option<&str>) -> Vec<PathBuf> {
        let data_dirs = data_dirs.filter(|d| !d.is_empty()).unwrap_or(DEFAULT_DATA_DIRS);
        let mut out: Vec<PathBuf> = data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("glib-2.0/schemas")).collect();
        match (data_home.filter(|d| !d.is_empty()), home) {
            (Some(d), _) => out.push(Path::new(d).join("glib-2.0/schemas")),
            (None, Some(h)) => out.push(Path::new(h).join(".local/share/glib-2.0/schemas")),
            (None, None) => {}
        }
        out.extend(schema_dir.unwrap_or("").split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
        out
    }

    /// Whether one of `dirs` has the compiled schema `id`. Reads each directory on its own, so
    /// GLib's default list (fixed the first time anything asks for it) is left alone.
    pub fn dirs_have(dirs: &[PathBuf], id: &str) -> bool {
        dirs.iter()
            .filter(|d| d.join("gschemas.compiled").is_file())
            .any(|d| gio::SettingsSchemaSource::from_directory(d, None, false).is_ok_and(|s| s.lookup(id, false).is_some()))
    }

    /// The data directories (each holding `glib-2.0/schemas`) in a `:`-separated list.
    fn data_dirs_with(list: &str, id: &str) -> Vec<PathBuf> {
        list.split(':').filter(|d| !d.is_empty()).map(PathBuf::from).filter(|d| dirs_have(&[d.join("glib-2.0/schemas")], id)).collect()
    }

    /// `XDG_DATA_DIRS` with `extra` added at the end, keeping GLib's default when it was unset.
    pub fn data_dirs_plus(current: Option<&str>, extra: &[PathBuf]) -> String {
        let mut out = current.filter(|d| !d.is_empty()).unwrap_or(DEFAULT_DATA_DIRS).to_string();
        for d in extra {
            out.push(':');
            out.push_str(&d.to_string_lossy());
        }
        out
    }

    fn env(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    /// The schema directories GLib will read in this process.
    fn current_dirs() -> Vec<PathBuf> {
        schema_dirs(env("XDG_DATA_DIRS").as_deref(), env("XDG_DATA_HOME").as_deref(), env("HOME").as_deref(), env("GSETTINGS_SCHEMA_DIR").as_deref())
    }

    /// Makes sure GLib will find the file chooser's schema, if it is anywhere Marbble knows of.
    /// Call it from `main` before anything starts a thread or touches GSettings: it may set
    /// `XDG_DATA_DIRS`.
    pub fn ensure_file_chooser_schema() {
        if dirs_have(&current_dirs(), FILE_CHOOSER) {
            return;
        }
        // The data directories of the environment Marbble was built in: the dev shell's, which a
        // debug binary started from elsewhere doesn't have.
        let found: Vec<PathBuf> = option_env!("XDG_DATA_DIRS").map(|l| data_dirs_with(l, FILE_CHOOSER)).unwrap_or_default().into_iter().take(1).collect();
        if found.is_empty() {
            log::warn!("GSettings schema {FILE_CHOOSER} not found: file dialogs are unavailable (see docs/DEVELOPING.md)");
            return;
        }
        let dirs = data_dirs_plus(env("XDG_DATA_DIRS").as_deref(), &found);
        log::info!("GSettings schema {FILE_CHOOSER} found in {}; added to XDG_DATA_DIRS", found[0].display());
        std::env::set_var("XDG_DATA_DIRS", dirs);
    }

    /// Why a file dialog can't open, or None when it can. Asks GLib's own schema list, the one GTK's
    /// file chooser will use.
    pub fn problem() -> Option<String> {
        let ok = gio::SettingsSchemaSource::default().is_some_and(|s| s.lookup(FILE_CHOOSER, true).is_some());
        (!ok).then(|| {
            format!(
                "GTK's settings schema {FILE_CHOOSER} isn't installed, and GTK's file chooser stops the app without it. \
             Install your distribution's GTK 3 package (it ships the schema), or start Marbble from its package or from \
             the Nix dev shell (`nix develop`), which put the schema on XDG_DATA_DIRS."
            )
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn p(s: &str) -> PathBuf {
            PathBuf::from(s)
        }

        #[test]
        fn dirs_follow_glibs_order() {
            assert_eq!(
                schema_dirs(Some("/a/share:/b/share"), None, Some("/home/u"), Some("/x:/y")),
                vec![p("/a/share/glib-2.0/schemas"), p("/b/share/glib-2.0/schemas"), p("/home/u/.local/share/glib-2.0/schemas"), p("/x"), p("/y")]
            );
            // Unset or empty XDG_DATA_DIRS is GLib's default; XDG_DATA_HOME wins over HOME.
            assert_eq!(
                schema_dirs(Some(""), Some("/d"), Some("/home/u"), None),
                vec![p("/usr/local/share/glib-2.0/schemas"), p("/usr/share/glib-2.0/schemas"), p("/d/glib-2.0/schemas")]
            );
            assert_eq!(schema_dirs(None, None, None, None).len(), 2);
        }

        #[test]
        fn adding_a_data_dir_keeps_the_default() {
            assert_eq!(data_dirs_plus(None, &[p("/nix/gtk/share")]), "/usr/local/share:/usr/share:/nix/gtk/share");
            assert_eq!(data_dirs_plus(Some("/a"), &[p("/b")]), "/a:/b");
            assert_eq!(data_dirs_plus(Some("/a"), &[]), "/a");
        }

        #[test]
        fn a_missing_schema_or_directory_is_not_found() {
            let empty = std::env::temp_dir().join(format!("marbble-schemas-{}", std::process::id()));
            std::fs::create_dir_all(&empty).unwrap();
            assert!(!dirs_have(&[empty.clone(), p("/nonexistent/glib-2.0/schemas")], FILE_CHOOSER));
            assert!(!dirs_have(&current_dirs(), "org.example.marbble.NoSuchSchema"));
            assert!(data_dirs_with(&empty.to_string_lossy(), FILE_CHOOSER).is_empty());
            let _ = std::fs::remove_dir(&empty);
        }

        /// Where the tests run (the dev shell, CI's Ubuntu with GTK installed) the schema is on the
        /// search path, so this is the check a file dialog makes, positive.
        #[test]
        fn the_file_chooser_schema_is_found_here() {
            assert!(dirs_have(&current_dirs(), FILE_CHOOSER), "{FILE_CHOOSER} not in {:?}", current_dirs());
            assert_eq!(problem(), None);
        }

        /// Runs one of the ignored tests below in a process of its own, with `env` changed (None
        /// removes a variable): GLib reads its schema list once per process.
        fn in_child(test: &str, env: &[(&str, Option<&str>)]) -> bool {
            let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
            cmd.args([&format!("schemas::linux::tests::{test}"), "--ignored", "--exact", "--test-threads=1"]);
            for (k, v) in env {
                match v {
                    Some(v) => cmd.env(k, v),
                    None => cmd.env_remove(k),
                };
            }
            let out = cmd.output().unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(text.contains("1 passed") || text.contains("1 failed"), "{test} didn't run: {text}");
            out.status.success()
        }

        /// The Save As crash: the schema isn't on the search path. The check says so, which
        /// keeps the frontend from opening the dialog.
        #[test]
        fn without_the_schema_the_check_says_why() {
            let nowhere = Some("/nonexistent-marbble");
            assert!(in_child("child_reports_the_problem", &[("XDG_DATA_DIRS", nowhere), ("XDG_DATA_HOME", nowhere), ("GSETTINGS_SCHEMA_DIR", None)]));
        }

        /// A binary built in the dev shell and started without its XDG_DATA_DIRS (as the audit's
        /// was) finds the schema again at startup. Where the system has it (/usr/share), it is
        /// found there.
        #[test]
        fn startup_finds_the_schema_again() {
            assert!(in_child("child_repairs_the_search_path", &[("XDG_DATA_DIRS", None), ("GSETTINGS_SCHEMA_DIR", None)]));
        }

        #[test]
        #[ignore = "run by without_the_schema_the_check_says_why in a process of its own"]
        fn child_reports_the_problem() {
            assert!(problem().is_some_and(|p| p.contains(FILE_CHOOSER)));
        }

        #[test]
        #[ignore = "run by startup_finds_the_schema_again in a process of its own"]
        fn child_repairs_the_search_path() {
            ensure_file_chooser_schema();
            assert!(dirs_have(&current_dirs(), FILE_CHOOSER));
            assert_eq!(problem(), None);
        }
    }
}
