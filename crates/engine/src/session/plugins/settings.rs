//! A plug-in's settings put into another instance of the same plug-in: the panel header's Paste
//! Settings, and its Compare, which swaps between the settings last loaded or saved and the edited
//! ones. Unlike pasting a plug-in (`presets.rs`), the instance stays: only its parameters, a
//! Sampler's zones and the preset it shows change.
//!
//! The settings come as a slot from [`Session::copy_plugin`], so they carry a Sampler's files named
//! in full; pasting copies any from another project into this one's Samples folder. Built-in
//! plug-ins only: a VST3's editor is its own window, with no Marbble header to paste from.

use crate::instrument::sampler::{state::SamplerState, ID as SAMPLER};
use crate::library::presets::Preset;
use crate::session::plugins::builtin::is_builtin;
use crate::session::plugins::sampler::adopt_samples;
use crate::session::*;

impl Session {
    /// Gives the plug-in in `slot` (the instrument slot when `usize::MAX`) of `track` the settings
    /// of `copy`, which must be the same plug-in: its parameters (unlisted ones at their defaults),
    /// a Sampler's zones, and the preset its box shows.
    pub fn paste_plugin_settings(&mut self, track: Id, slot: usize, copy: &PluginSlot) -> Result<()> {
        let target = self.slot_get(track, slot).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        if !is_builtin(&target.plugin_id) {
            return Err(anyhow!("{}'s settings are in its own window", target.name));
        }
        if target.plugin_id != copy.plugin_id {
            return Err(anyhow!("Settings of {} can't go into {}", copy.name, target.name));
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
