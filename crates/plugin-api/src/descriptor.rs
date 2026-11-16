//! A plug-in as a scan found it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The plug-in format a descriptor's file is in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginFormat {
    /// Scan caches from before CLAP hosting have no format: they list VST3s.
    #[default]
    Vst3,
    Clap,
}

impl PluginFormat {
    /// The format of a plug-in file, from its extension.
    pub fn of(path: &std::path::Path) -> Option<PluginFormat> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "vst3" => Some(PluginFormat::Vst3),
            "clap" => Some(PluginFormat::Clap),
            _ => None,
        }
    }
}

/// One plug-in (a VST3 class or a CLAP plug-in) in a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    /// Stable identifier: `<file path>|<cid>`.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub path: PathBuf,
    /// The plug-in's id within its file: a VST3 class id in hex, or a CLAP plug-in id.
    pub cid: String,
    /// e.g. "Fx|Reverb" or "Instrument|Synth" (a CLAP plug-in's features are put in these terms).
    pub subcategories: String,
    pub is_instrument: bool,
    #[serde(default)]
    pub format: PluginFormat,
}

impl PluginDescriptor {
    /// Top-level grouping used by the plug-in menus, mirroring Logic's category submenus.
    pub fn category(&self) -> String {
        let mut parts = self.subcategories.split('|').filter(|s| !s.is_empty() && *s != "Fx");
        parts.next().unwrap_or("Effect").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_descriptor_without_a_format_is_a_vst3() {
        let old = r#"{"id":"/a/B.vst3|00","name":"B","vendor":"","version":"","path":"/a/B.vst3","cid":"00","subcategories":"Fx","is_instrument":false}"#;
        let d: PluginDescriptor = serde_json::from_str(old).unwrap();
        assert_eq!(d.format, PluginFormat::Vst3);
        let clap = r#"{"id":"/a/B.clap|b","name":"B","vendor":"","version":"","path":"/a/B.clap","cid":"b","subcategories":"Fx","is_instrument":false,"format":"clap"}"#;
        assert_eq!(serde_json::from_str::<PluginDescriptor>(clap).unwrap().format, PluginFormat::Clap);
    }

    #[test]
    fn the_format_comes_from_the_extension() {
        assert_eq!(PluginFormat::of("/x/Comp.CLAP".as_ref()), Some(PluginFormat::Clap));
        assert_eq!(PluginFormat::of("/x/Comp.vst3".as_ref()), Some(PluginFormat::Vst3));
        assert_eq!(PluginFormat::of("/x/Comp.so".as_ref()), None);
    }
}
