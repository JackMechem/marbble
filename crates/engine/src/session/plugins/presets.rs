//! Copying and pasting plug-ins, and loading and saving presets (see `crate::library::presets` for where
//! they are stored).
//!
//! The clipboard lives in the UI, so it survives opening another project: copying hands the UI
//! the slot with its complete settings (a VST3's state captured from its controller), and
//! pasting makes a new instance from that. A Marbble Sampler's files are named in full on the
//! clipboard, and pasting into another project copies them into its Samples folder.

use crate::library::presets::{self, Preset};
use crate::session::*;

impl Session {
    /// A slot with everything needed to paste it elsewhere: its settings, and a VST3's state
    /// as it is right now.
    pub fn copy_plugin(&self, track: Id, slot: usize) -> Result<PluginSlot> {
        let s = self.slot_get(track, slot).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        let mut copy = s.clone();
        // A plug-in a processing thread is left in (`session/engine/left_out.rs`) keeps its last saved state.
        if let Some(st) = self.controllers.get(&s.uid).and_then(|c| c.save_state()) {
            copy.state = Some(encode_state(&st));
        }
        crate::session::plugins::sampler::full_sample_paths(&mut copy, &self.dir);
        Ok(copy)
    }

    pub fn slot_get(&self, track: Id, slot: usize) -> Option<&PluginSlot> {
        if track == MASTER_ID {
            return self.project.master.inserts.get(slot)?.as_ref();
        }
        let t = self.project.track(track)?;
        if slot == usize::MAX {
            t.instrument.as_ref()
        } else {
            t.inserts.get(slot)?.as_ref()
        }
    }

    /// Pastes a copied slot into `slot` (the instrument slot when `usize::MAX`) of each track,
    /// as new instances with the copy's settings. A side-chain only comes along where its
    /// source exists in this project and keys without a loop.
    pub fn paste_plugin(&mut self, tracks: &[Id], slot: usize, copy: &PluginSlot) -> Result<()> {
        self.check_unfrozen(tracks)?;
        let builtin_fx = crate::fx::find(&copy.plugin_id).is_some();
        let instrument = if crate::instrument::find(&copy.plugin_id).is_some() {
            true
        } else if builtin_fx {
            false
        } else {
            self.find_plugin(&copy.plugin_id).map(|d| d.is_instrument).ok_or_else(|| anyhow!("{} isn't installed", copy.name))?
        };
        if instrument != (slot == usize::MAX) {
            return Err(anyhow!(if instrument { "An instrument goes in the instrument slot" } else { "An audio effect goes in an Audio FX slot" }));
        }
        let mut copy = copy.clone();
        crate::session::plugins::sampler::adopt_samples(&mut copy, &self.dir, &self.dir)?;
        for &track in tracks {
            if track != MASTER_ID {
                let t = self.project.track(track).ok_or_else(|| anyhow!("no such track"))?;
                if t.kind == TrackKind::Folder {
                    return Err(anyhow!("Folder stacks have no plug-ins"));
                }
                if instrument && t.kind != TrackKind::Instrument {
                    return Err(anyhow!("{} has no instrument slot", t.name));
                }
            } else if instrument {
                return Err(anyhow!("Stereo Out has no instrument slot"));
            }
            let sidechain = copy.sidechain.filter(|&src| {
                src != track
                    && self.project.track(src).map(|t| t.kind != TrackKind::Folder).unwrap_or(false)
                    && (track == MASTER_ID || !self.project.depends(track, src))
            });
            let new_slot = PluginSlot { uid: self.project.alloc_id(), sidechain, ..copy.clone() };
            let ins = self.instantiate_slot(&new_slot)?;
            // A plug-in a processing thread is left in keeps its editor, hidden, untold.
            self.mark_unresponsive();
            let old = self.slot_ref(track, slot).ok_or_else(|| anyhow!("no such slot"))?.replace(new_slot);
            if let Some(o) = old {
                if let Some(mut c) = self.controllers.remove(&o.uid) {
                    c.close_editor();
                }
            }
            if slot == usize::MAX {
                self.send(Cmd::Instrument { track, insert: Some(ins) });
            } else {
                self.send(Cmd::Insert { track, slot, insert: Some(ins) });
            }
        }
        self.send(Cmd::Order(self.project.process_order()));
        self.update_pdc();
        self.dirty = true;
        Ok(())
    }

    /// Applies a preset to a built-in effect's slot. A built-in effect's parameters the preset
    /// doesn't list go to their defaults. The slot remembers the preset's name for its preset box.
    pub fn load_preset(&mut self, track: Id, slot: usize, effect: &str, name: &str, factory: bool) -> Result<()> {
        let p = presets::get(&presets::user_root(), effect, name, factory)?;
        self.apply_preset(track, slot, &p)?;
        self.set_preset_ref(track, slot, Some(PresetRef { name: name.into(), factory }));
        Ok(())
    }

    /// The preset a slot last loaded or saved.
    pub fn preset_ref(&self, track: Id, slot: usize) -> Option<&PresetRef> {
        self.slot_get(track, slot)?.preset.as_ref()
    }

    fn set_preset_ref(&mut self, track: Id, slot: usize, preset: Option<PresetRef>) {
        let at = self.slot_ref(track, slot).and_then(|s| s.as_mut()).map(|s| &mut s.preset);
        if let Some(at) = at.filter(|at| **at != preset) {
            *at = preset;
            self.dirty = true;
        }
    }

    /// Renames one of the user's presets, and what in the project refers to it. Returns the new name.
    pub fn rename_user_preset(&mut self, effect: &str, old: &str, new: &str) -> Result<String> {
        let renamed = presets::rename_user(&presets::user_root(), effect, old, new)?;
        if self.project.retarget_user_preset(effect, old, Some(&renamed)) {
            self.dirty = true;
        }
        Ok(renamed)
    }

    /// Deletes one of the user's presets; what in the project refers to it forgets it.
    pub fn delete_user_preset(&mut self, effect: &str, name: &str) -> Result<()> {
        presets::delete_user(&presets::user_root(), effect, name)?;
        if self.project.retarget_user_preset(effect, name, None) {
            self.dirty = true;
        }
        Ok(())
    }

    pub fn apply_preset(&mut self, track: Id, slot: usize, p: &Preset) -> Result<()> {
        self.check_unfrozen(&[track])?;
        let s = self.slot_ref(track, slot).and_then(|s| s.as_mut()).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        if s.plugin_id != p.effect {
            return Err(anyhow!("\"{}\" is a preset for another plug-in", p.name));
        }
        let (_, specs) = crate::session::plugins::builtin::builtin_spec(&s.plugin_id).ok_or_else(|| anyhow!("{} has no presets here", s.name))?;
        s.params = specs
            .iter()
            .map(|spec| (spec.id, p.values.iter().find(|v| v.0 == spec.id).map(|v| crate::dsp::builtin_clamp(spec, v.1)).unwrap_or(spec.default)))
            .collect();
        let (uid, values) = (s.uid, s.params.clone());
        for (id, value) in values {
            self.send(Cmd::FxParam { track, uid, id, value });
        }
        // What isn't numbers comes in too, with any files it brought.
        if let Some(state) = &p.state {
            self.sampler_apply_preset(track, state, p.base.as_deref())?;
        }
        if self.latency_changed() {
            self.update_pdc();
        }
        self.dirty = true;
        Ok(())
    }

    /// Saves the settings of a built-in effect's or instrument's slot as a user preset, which the
    /// slot then shows as loaded. Returns the name it was saved under.
    pub fn save_preset(&mut self, track: Id, slot: usize, name: &str, overwrite: bool) -> Result<String> {
        let saved = self.write_preset(track, slot, name, overwrite)?;
        self.set_preset_ref(track, slot, Some(PresetRef { name: saved.clone(), factory: false }));
        Ok(saved)
    }

    fn write_preset(&self, track: Id, slot: usize, name: &str, overwrite: bool) -> Result<String> {
        let s = self.slot_get(track, slot).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        if !crate::session::plugins::builtin::is_builtin(&s.plugin_id) {
            return Err(anyhow!("Presets are saved in {}'s own window", s.name));
        }
        if s.plugin_id == crate::instrument::sampler::ID {
            return self.sampler_save_instrument(track, name, overwrite);
        }
        let p =
            Preset { name: name.into(), effect: s.plugin_id.clone(), version: 0, values: self.builtin_params(s.uid), legacy_eq: None, state: None, base: None };
        presets::write_user(&presets::user_root(), &p, overwrite)
    }
}
