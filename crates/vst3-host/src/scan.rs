//! Finding VST3 bundles on disk.

use std::path::{Path, PathBuf};

use crate::module::{load_module, PluginDescriptor};

pub fn default_search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(v) = std::env::var_os("VST3_PATH") {
        out.extend(std::env::split_paths(&v));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(h) = &home {
            out.push(h.join(".vst3"));
            out.push(h.join(".nix-profile/lib/vst3"));
        }
        out.push("/usr/lib/vst3".into());
        out.push("/usr/local/lib/vst3".into());
        out.push("/run/current-system/sw/lib/vst3".into());
        if let Ok(user) = std::env::var("USER") {
            out.push(format!("/etc/profiles/per-user/{user}/lib/vst3").into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = &home {
            out.push(h.join("Library/Audio/Plug-Ins/VST3"));
        }
        out.push("/Library/Audio/Plug-Ins/VST3".into());
    }
    #[cfg(target_os = "windows")]
    {
        let common = std::env::var_os("CommonProgramFiles").map(PathBuf::from).unwrap_or_else(|| "C:\\Program Files\\Common Files".into());
        out.push(common.join("VST3"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            out.push(PathBuf::from(l).join("Programs/Common/VST3"));
        }
    }
    let _ = home;
    out.dedup();
    out
}

/// Recursively finds `*.vst3` bundles (directories or single files) without descending into them.
pub fn find_bundles(paths: &[PathBuf]) -> Vec<PathBuf> {
    plugin_api::find_files(paths, "vst3")
}

/// Loads a bundle in-process and lists its classes. Prefer running this in a child process so a
/// misbehaving plugin cannot take the host down during a scan.
pub fn scan_bundle(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    Ok(load_module(path)?.descriptors())
}
