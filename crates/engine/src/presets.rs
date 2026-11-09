//! Presets for the built-in effects (the Channel EQ among them) and the built-in instruments,
//! shared by every project.
//!
//! - Factory presets ship with the code: `fx::factory_presets` for the effects and
//!   `InstrumentInfo::presets` for the instruments.
//! - User presets are files, `<config dir>/presets/<effect>/<name>.json` (the effect id with
//!   anything unsafe in a file name replaced, so "builtin:deesser" becomes "builtin_deesser").
//!   Each holds its name, effect id and format version alongside the settings, and is written
//!   to a temporary file first and renamed into place, so a crash never leaves half a preset.
//! - The Channel EQ's user presets from before it was an insert hold its settings as bands
//!   (`ChannelEq`); reading one turns them into the effect's values.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::ChannelEq;

/// The Channel EQ's plug-in id, which its presets are filed under.
pub const CHANNEL_EQ: &str = crate::fx::channel_eq::ID;
const VERSION: u32 = 1;
const MAX_NAME: usize = 64;

/// A preset as saved (and as the UI gets it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub effect: String,
    #[serde(default)]
    pub version: u32,
    /// A built-in effect's values: (parameter id, plain value). Unlisted ones are defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<(u32, f32)>,
    /// The settings of a Channel EQ preset saved before the Channel EQ was an insert. Only read:
    /// [`read_user`] turns it into `values`.
    #[serde(rename = "eq", default, skip_serializing)]
    pub legacy_eq: Option<ChannelEq>,
    /// An instrument's settings that aren't numbers, as its `PluginSlot::state` holds them. None
    /// leaves what's there alone (a factory preset that shapes a sampler's sound keeps its zones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// The folder a preset saved with its files came from, which `state`'s paths are relative to.
    #[serde(skip)]
    pub base: Option<PathBuf>,
}

/// The presets offered for one effect.
#[derive(Debug, Clone, Serialize, Default)]
pub struct PresetList {
    pub factory: Vec<String>,
    pub user: Vec<String>,
}

/// Where user presets live: `presets` in the app's config folder.
pub fn user_root() -> PathBuf {
    crate::plugin_db::config_dir().join("presets")
}

/// A preset name made safe to use as a file name: trimmed, without path separators or other
/// characters file systems refuse, at most 64 characters.
pub fn clean_name(name: &str) -> Result<String> {
    let cleaned: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let mut out = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    out = out.trim_start_matches('.').trim().to_string();
    if out.chars().count() > MAX_NAME {
        out = out.chars().take(MAX_NAME).collect::<String>().trim_end().to_string();
    }
    if out.is_empty() {
        return Err(anyhow!("Type a name for the preset"));
    }
    Ok(out)
}

/// Where instruments saved with their samples live: a folder for each under the user's Music
/// folder, next to the projects folder and easy to find, back up or share. It's created on first
/// save.
pub fn bundle_root() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    home.join("Music").join("Marbble Instruments")
}

/// The name of the document in an instrument's folder.
pub const BUNDLE_DOC: &str = "instrument.json";

/// Whether an effect's user presets are folders with their files in them, not single files.
fn is_bundle(effect: &str) -> bool {
    effect == crate::instrument::sampler::ID
}

/// An instrument's folder, for a name already cleaned.
pub fn bundle_dir(name: &str) -> PathBuf {
    bundle_root().join(name)
}

fn effect_dir(root: &Path, effect: &str) -> PathBuf {
    if is_bundle(effect) {
        return bundle_root();
    }
    let safe: String = effect.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    root.join(safe)
}

fn file(root: &Path, effect: &str, name: &str) -> PathBuf {
    if is_bundle(effect) {
        return bundle_dir(name).join(BUNDLE_DOC);
    }
    effect_dir(root, effect).join(format!("{name}.json"))
}

/// The user presets saved for an effect, by name.
pub fn list_user(root: &Path, effect: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(effect_dir(root, effect)) else { return Vec::new() };
    if is_bundle(effect) {
        let mut names: Vec<String> =
            rd.flatten().filter(|e| e.path().join(BUNDLE_DOC).is_file()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        return names;
    }
    let mut names: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().map(|x| x == "json").unwrap_or(false)).then(|| p.file_stem().map(|s| s.to_string_lossy().to_string())).flatten()
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

pub fn read_user(root: &Path, effect: &str, name: &str) -> Result<Preset> {
    let name = clean_name(name)?;
    let path = file(root, effect, &name);
    let text = std::fs::read_to_string(&path).with_context(|| format!("No preset named \"{name}\""))?;
    let mut p: Preset = serde_json::from_str(&text).with_context(|| format!("The preset \"{name}\" can't be read"))?;
    if let Some(eq) = p.legacy_eq.take() {
        p.values = crate::fx::channel_eq::values_of(&eq);
    }
    // The file name is what the list shows, whatever the file says.
    p.base = is_bundle(effect).then(|| bundle_dir(&name));
    p.name = name;
    Ok(p)
}

/// Saves a user preset (named by `p.name`, cleaned). Refuses to replace an existing one unless
/// `overwrite`. Returns the name it was saved under.
pub fn write_user(root: &Path, p: &Preset, overwrite: bool) -> Result<String> {
    let name = clean_name(&p.name)?;
    let path = file(root, &p.effect, &name);
    if path.exists() && !overwrite {
        return Err(anyhow!("A preset named \"{name}\" already exists"));
    }
    std::fs::create_dir_all(path.parent().unwrap_or(root))?;
    let doc = Preset { name: name.clone(), version: VERSION, ..p.clone() };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&doc)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(name)
}

pub fn delete_user(root: &Path, effect: &str, name: &str) -> Result<()> {
    let name = clean_name(name)?;
    if is_bundle(effect) {
        // It's listed only if this file is there, so this can't take an unrelated folder with it.
        file(root, effect, &name).is_file().then_some(()).with_context(|| format!("No preset named \"{name}\""))?;
        return std::fs::remove_dir_all(bundle_dir(&name)).with_context(|| format!("Couldn't delete \"{name}\""));
    }
    std::fs::remove_file(file(root, effect, &name)).with_context(|| format!("No preset named \"{name}\""))
}

/// Renames a user preset; refuses a name that is already taken.
pub fn rename_user(root: &Path, effect: &str, old: &str, new: &str) -> Result<String> {
    let mut p = read_user(root, effect, old)?;
    let new_name = clean_name(new)?;
    if new_name == p.name {
        return Ok(new_name);
    }
    // Case-only renames are the same file on some systems.
    let same_file = new_name.to_lowercase() == p.name.to_lowercase();
    if !same_file && file(root, effect, &new_name).exists() {
        return Err(anyhow!("A preset named \"{new_name}\" already exists"));
    }
    if is_bundle(effect) {
        // The folder moves with its samples. A case-only rename goes through a third name.
        let (from, to) = (bundle_dir(&p.name), bundle_dir(&new_name));
        let step = bundle_root().join(format!(".renaming {new_name}"));
        std::fs::rename(&from, &step)?;
        std::fs::rename(&step, &to)?;
        p.name = new_name.clone();
        write_user(root, &p, true)?;
        return Ok(new_name);
    }
    let old_name = std::mem::replace(&mut p.name, new_name);
    let saved = write_user(root, &p, same_file)?;
    if !same_file {
        std::fs::remove_file(file(root, effect, &old_name))?;
    }
    Ok(saved)
}

/// The factory presets of an effect, complete.
pub fn factory(effect: &str) -> Vec<Preset> {
    let preset = |name: &str, values: &[(u32, f32)]| Preset {
        name: name.into(),
        effect: effect.into(),
        version: VERSION,
        values: values.to_vec(),
        legacy_eq: None,
        state: None,
        base: None,
    };
    // Every built-in effect and instrument starts with its defaults, as Logic's "#default".
    if let Some(info) = crate::instrument::find(effect) {
        let own = info.presets.iter().map(|f| preset(f.name, f.values));
        return std::iter::once(preset(DEFAULT, &[])).chain(own).collect();
    }
    // The Channel EQ's own first preset, "Flat", is its defaults (as it was before it was an insert).
    let default = crate::fx::find(effect).filter(|_| effect != CHANNEL_EQ).map(|_| preset(DEFAULT, &[]));
    default.into_iter().chain(crate::fx::factory_presets(effect).iter().map(|f| preset(f.name, f.values))).collect()
}

/// The factory preset that puts a built-in effect back to its defaults ("Flat" for the Channel EQ).
pub const DEFAULT: &str = "Default";

/// Lists an effect's factory and user presets.
pub fn list(root: &Path, effect: &str) -> PresetList {
    PresetList { factory: factory(effect).into_iter().map(|p| p.name).collect(), user: list_user(root, effect) }
}

/// Finds a factory or user preset.
pub fn get(root: &Path, effect: &str, name: &str, factory_preset: bool) -> Result<Preset> {
    if factory_preset {
        factory(effect).into_iter().find(|p| p.name == name).ok_or_else(|| anyhow!("No factory preset named \"{name}\""))
    } else {
        read_user(root, effect, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("marbble-presets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn user_presets_round_trip() {
        let root = temp_root("rt");
        let p = Preset {
            name: "My: Kick/Gate?".into(),
            effect: "builtin:noise-gate".into(),
            version: 0,
            values: vec![(0, -31.5), (4, 120.0)],
            legacy_eq: None,
            state: None,
            base: None,
        };
        let name = write_user(&root, &p, false).unwrap();
        assert_eq!(name, "My Kick Gate");
        assert!(root.join("builtin_noise-gate").join("My Kick Gate.json").exists());
        assert!(write_user(&root, &p, false).is_err(), "an existing preset isn't replaced silently");
        write_user(&root, &Preset { values: vec![(0, -20.0)], ..p.clone() }, true).unwrap();
        assert_eq!(list(&root, "builtin:noise-gate").user, vec!["My Kick Gate".to_string()]);
        let back = get(&root, "builtin:noise-gate", "My Kick Gate", false).unwrap();
        assert_eq!((back.values.clone(), back.version), (vec![(0, -20.0)], VERSION));
        rename_user(&root, "builtin:noise-gate", "My Kick Gate", "Kick").unwrap();
        assert_eq!(list_user(&root, "builtin:noise-gate"), vec!["Kick".to_string()]);
        delete_user(&root, "builtin:noise-gate", "Kick").unwrap();
        assert!(list_user(&root, "builtin:noise-gate").is_empty());
        // A Channel EQ preset from before it was an insert reads as the effect's values.
        let dir = root.join("builtin_channel-eq");
        std::fs::create_dir_all(&dir).unwrap();
        let mut eq = ChannelEq::default();
        eq.bands[6].gain = 4.0;
        let old = serde_json::json!({ "name": "Bright", "effect": CHANNEL_EQ, "version": 1, "eq": eq });
        std::fs::write(dir.join("Bright.json"), old.to_string()).unwrap();
        let back = get(&root, CHANNEL_EQ, "Bright", false).unwrap();
        assert!(back.legacy_eq.is_none() && back.values.contains(&(crate::fx::channel_eq::gain(6), 4.0)));
        assert!(!serde_json::to_string(&back).unwrap().contains("bands"), "written back without the old form");
        assert!(clean_name(" ..  ").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn factory_presets_are_valid() {
        assert!(factory(CHANNEL_EQ).len() >= 8 && factory(CHANNEL_EQ)[0].name == "Flat");
        assert!(factory("builtin:noise-gate").len() >= 6 && factory("builtin:deesser").len() >= 6);
        assert_eq!(factory("builtin:deesser")[0].name, DEFAULT);
        for e in crate::fx::registry() {
            for p in factory(e.id) {
                for (id, v) in &p.values {
                    let spec = e.param(*id).unwrap_or_else(|| panic!("{}: {} sets unknown {id}", e.id, p.name));
                    assert!(*v >= spec.min && *v <= spec.max, "{}: {} {} out of range", e.id, p.name, spec.name);
                }
            }
        }
    }
}
