//! Insert slots: loading, removing and bypassing a plug-in, its editor window, and the plug-in list.
//!
//! Touches VST3 controllers, so it runs on the main thread. Built-in effects and instruments have
//! their parameters in `builtin.rs`, presets are in `presets.rs`, moving inserts in `insert_order.rs`.

use crate::session::*;

impl Session {
    pub(crate) fn slot_ref(&mut self, track: Id, slot: usize) -> Option<&mut Option<PluginSlot>> {
        if track == MASTER_ID {
            return self.project.master.inserts.get_mut(slot);
        }
        let t = self.project.track_mut(track)?;
        if slot == usize::MAX {
            Some(&mut t.instrument)
        } else {
            t.inserts.get_mut(slot)
        }
    }

    /// Loads a plug-in into an insert slot, or the instrument slot when `slot == usize::MAX`.
    pub fn set_plugin(&mut self, track: Id, slot: usize, plugin_id: &str) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        let builtin = crate::fx::find(plugin_id);
        let instrument = crate::instrument::find(plugin_id);
        let name = if let Some(info) = &instrument {
            if slot != usize::MAX {
                return Err(anyhow!("{} is an instrument: it goes in the instrument slot", info.name));
            }
            info.name.to_string()
        } else if let Some(info) = &builtin {
            if slot == usize::MAX {
                return Err(anyhow!("{} is an audio effect: it goes in an Audio FX slot", info.name));
            }
            info.name.to_string()
        } else {
            self.find_plugin(plugin_id).map(|d| d.name.clone()).ok_or_else(|| anyhow!("unknown plug-in"))?
        };
        // A built-in plug-in's values are all written out, so the project file reads clearly.
        let specs = builtin.map(|i| i.params).or(instrument.map(|i| i.params)).unwrap_or_default();
        let params = specs.iter().map(|p| (p.id, p.default)).collect();
        let uid = self.project.alloc_id();
        let new_slot =
            PluginSlot { uid, plugin_id: plugin_id.to_string(), name, bypass: false, state: None, params, sidechain: None, sidechain_pre: false, preset: None };
        let ins = self.instantiate_slot(&new_slot)?;
        self.mark_unresponsive();
        let old = self.slot_ref(track, slot).ok_or_else(|| anyhow!("no such slot"))?.replace(new_slot);
        if let Some(o) = old {
            self.controllers.remove(&o.uid);
        }
        if slot == usize::MAX {
            self.send(Cmd::Instrument { track, insert: Some(ins) });
        } else {
            self.send(Cmd::Insert { track, slot, insert: Some(ins) });
        }
        self.update_pdc();
        self.dirty = true;
        Ok(uid)
    }

    /// Empties an insert slot, or with `slot` of `usize::MAX` the instrument slot. Refused on a frozen track.
    pub fn remove_plugin(&mut self, track: Id, slot: usize) {
        if self.is_frozen(track) {
            return;
        }
        // A plug-in a processing thread is left in keeps its editor, hidden, untold.
        self.mark_unresponsive();
        let old = self.slot_ref(track, slot).and_then(|s| s.take());
        if let Some(o) = old {
            if let Some(mut c) = self.controllers.remove(&o.uid) {
                c.close_editor();
            }
        }
        if slot == usize::MAX {
            self.send(Cmd::Instrument { track, insert: None });
        } else {
            self.send(Cmd::Insert { track, slot, insert: None });
        }
        self.update_pdc();
        self.dirty = true;
    }

    pub fn set_bypass(&mut self, track: Id, slot: usize, bypass: bool) {
        if self.is_frozen(track) {
            return;
        }
        let Some(Some(s)) = self.slot_ref(track, slot) else { return };
        s.bypass = bypass;
        let uid = s.uid;
        self.send(Cmd::Bypass { track, uid, bypass });
        self.update_pdc();
    }

    /// Opens a VST3's own editor, and starts watching it for edits to undo (see `editor_undo/`).
    pub fn open_editor(&mut self, uid: Id, title: &str) -> Result<()> {
        let c = self.controllers.get_mut(&uid).ok_or_else(|| anyhow!("this plug-in has no editor"))?;
        c.open_editor(title).map_err(|e| anyhow!(e))?;
        self.watch_vst_edits(uid, false);
        self.focus_plugin(uid);
        Ok(())
    }

    pub fn open_editors(&self) -> Vec<Id> {
        self.controllers.iter().filter(|(_, c)| c.editor_open()).map(|(&id, _)| id).collect()
    }

    pub fn set_plugins(&mut self, list: Vec<PluginDescriptor>) {
        self.plugins = list;
    }

    /// The installed plug-in a slot's id names. Ids start with the bundle's path, so when that
    /// path isn't installed (a project from another computer or operating system, or a moved
    /// bundle), the same class (`cid`) installed elsewhere stands in.
    pub fn find_plugin(&self, id: &str) -> Option<&PluginDescriptor> {
        self.plugins.iter().find(|d| d.id == id).or_else(|| {
            let (_, cid) = id.rsplit_once('|')?;
            self.plugins.iter().find(|d| d.cid.eq_ignore_ascii_case(cid))
        })
    }

    /// VST3 slots that aren't running: the plug-in isn't installed here (or failed to load).
    /// They stay in the project with their saved state, so a project opened where a plug-in is
    /// missing keeps its settings when saved, for the computer that has it.
    pub fn missing_plugins(&self) -> Vec<&PluginSlot> {
        let p = &self.project;
        let tracks = p.tracks.iter().filter(|t| t.frozen.is_none()).flat_map(|t| t.instrument.iter().chain(t.inserts.iter().flatten()));
        tracks
            .chain(p.master.inserts.iter().flatten())
            .filter(|s| !crate::session::plugins::builtin::is_builtin(&s.plugin_id) && !self.controllers.contains_key(&s.uid))
            .collect()
    }
}
