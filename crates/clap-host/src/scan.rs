//! Finding `.clap` files on disk.

use std::path::{Path, PathBuf};

use plugin_api::PluginDescriptor;

/// `$CLAP_PATH`, then the folders the CLAP standard names for the platform (and NixOS's profiles).
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(v) = std::env::var_os("CLAP_PATH") {
        out.extend(std::env::split_paths(&v));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(h) = &home {
            out.push(h.join(".clap"));
            out.push(h.join(".nix-profile/lib/clap"));
        }
        out.push("/usr/lib/clap".into());
        out.push("/usr/local/lib/clap".into());
        out.push("/run/current-system/sw/lib/clap".into());
        if let Ok(user) = std::env::var("USER") {
            out.push(format!("/etc/profiles/per-user/{user}/lib/clap").into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = &home {
            out.push(h.join("Library/Audio/Plug-Ins/CLAP"));
        }
        out.push("/Library/Audio/Plug-Ins/CLAP".into());
    }
    #[cfg(target_os = "windows")]
    {
        let common = std::env::var_os("CommonProgramFiles").map(PathBuf::from).unwrap_or_else(|| "C:\\Program Files\\Common Files".into());
        out.push(common.join("CLAP"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            out.push(PathBuf::from(l).join("Programs/Common/CLAP"));
        }
    }
    let _ = home;
    out.dedup();
    out
}

/// Loads a `.clap` file in-process and lists its plug-ins. Prefer running this in a child process
/// so a misbehaving plug-in can't take the host down during a scan.
pub fn scan_file(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    Ok(crate::entry::load(path)?.descriptors())
}

/// Recursively finds `.clap` files (or bundles, on macOS) without descending into them.
pub fn find_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    plugin_api::find_files(paths, "clap")
}
