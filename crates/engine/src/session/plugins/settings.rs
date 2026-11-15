//! A plug-in's settings put into another instance of the same plug-in: Paste Settings (a built-in
//! panel's header, a VST3 slot's right-click menu), and a built-in panel's Compare, which swaps
//! between the settings last loaded or saved and the edited ones (a VST3's Compare is in
//! `compare.rs`). Unlike pasting a plug-in (`presets.rs`), the instance stays: only its
//! parameters, a Sampler's zones and the preset it shows change, or a VST3's state, loaded into the
//! running plug-in.
//!
//! The settings come as a slot from [`Session::copy_plugin`], so they carry a Sampler's files named
//! in full (pasting copies any from another project into this one's Samples folder) and a VST3's
//! state as it was then.

use crate::instrument::sampler::{state::SamplerState, ID as SAMPLER};
use crate::library::presets::Preset;
use crate::session::plugins::builtin::is_builtin;
use crate::session::plugins::sampler::adopt_samples;
use crate::session::*;

impl Session {
    /// Gives the plug-in in `slot` (the instrument slot when `usize::MAX`) of `track` the settings
    /// of `copy`, which must be the same plug-in: its parameters (unlisted ones at their defaults),
    /// a Sampler's zones, and the preset its box shows; a VST3's state. Called after the command's
    /// checkpoint, which becomes the step that undoes it.
    pub fn paste_plugin_settings(&mut self, track: Id, slot: usize, copy: &PluginSlot) -> Result<()> {
        let target = self.slot_get(track, slot).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        if !same_plugin(&target.plugin_id, &copy.plugin_id) {
            return Err(anyhow!("Settings of {} can't go into {}", copy.name, target.name));
        }
        if !is_builtin(&target.plugin_id) {
            return self.paste_vst_settings(track, target.uid, copy);
        }
        let mut copy = copy.clone();
        adopt_samples(&mut copy, &self.dir, &self.dir)?;
        let settings = Preset {
            name: copy.name.clone(),
            effect: copy.plugin_id.clone(),
            version: 0,
            values: copy.params.clone(),
            legacy_eq: None,
            state: zones_or_settings(&copy),
            base: None,
        };
        self.apply_preset(track, slot, &settings)?;
        self.set_preset_ref(track, slot, copy.preset.clone());
        Ok(())
    }

    /// Loads a copy's VST3 state into the running instance `uid` on `track`.
    fn paste_vst_settings(&mut self, track: Id, uid: Id, copy: &PluginSlot) -> Result<()> {
        self.check_unfrozen(&[track])?;
        let state = copy.state.as_deref().ok_or_else(|| anyhow!("The copy of {} has no settings", copy.name))?;
        // An edit, which ends comparing; Compare stays off when the copy is the saved settings.
        self.compare_edited(uid);
        self.load_vst_settings(uid, Arc::new(decode_state(state)));
        Ok(())
    }
}

/// Whether two slots' plug-in ids name the same plug-in. A VST3's id starts with its bundle's
/// path, so one copied on another computer matches by its class id, as `Session::find_plugin` does.
fn same_plugin(a: &str, b: &str) -> bool {
    let class = |id: &str| id.rsplit_once('|').map(|(_, cid)| cid.to_ascii_lowercase());
    a == b || class(a).is_some_and(|c| Some(c) == class(b))
}

/// What a copy holds beyond numbers. A Sampler with no zones holds nothing, which pasting must
/// still take as no zones, where a preset without any would leave the zones there alone.
fn zones_or_settings(copy: &PluginSlot) -> Option<String> {
    match copy.state.as_deref() {
        Some(state) => Some(state.to_string()),
        None if copy.plugin_id == SAMPLER => Some(SamplerState::default().to_json()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::same_plugin;

    #[test]
    fn same_plugin_by_id_or_vst3_class() {
        assert!(same_plugin("builtin:chorus", "builtin:chorus"));
        assert!(!same_plugin("builtin:chorus", "builtin:flanger"));
        assert!(same_plugin("/usr/lib/vst3/A.vst3|ABCD", "/Library/Audio/Plug-Ins/VST3/A.vst3|abcd"));
        assert!(!same_plugin("/usr/lib/vst3/A.vst3|ABCD", "/usr/lib/vst3/A.vst3|ABCE"));
        assert!(!same_plugin("builtin:chorus", "/usr/lib/vst3/A.vst3|builtin:chorus"));
    }
}
