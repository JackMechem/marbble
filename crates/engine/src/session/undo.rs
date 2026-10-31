//! Undo and redo: a stack of whole-project snapshots ([`UndoEntry`]), and `restore`, which moves
//! to one by sending the engine only what differs, so plug-ins that are still there keep running.
//!
//! Tauri commands take the checkpoint before an edit. Edits in a VST3's own editor come in through
//! `vst_undo.rs`. Anything sent to the engine must also be handled in `restore`.

use super::*;

impl Session {
    /// Records the project as it is before an edit. `key` merges a burst of edits (a fader drag)
    /// into one step; `plugins` also captures plug-in settings, for edits that remove plug-ins.
    /// Returns whether a new step was pushed (false when merged into the last one), for
    /// [`Session::abandon_checkpoint`].
    pub fn checkpoint(&mut self, label: &str, key: Option<String>, plugins: bool) -> bool {
        // An edit just made in a plug-in's editor comes first.
        self.flush_vst_edits();
        self.push_checkpoint(label, key, plugins)
    }

    pub(super) fn push_checkpoint(&mut self, label: &str, key: Option<String>, plugins: bool) -> bool {
        self.redo_stack.clear();
        if let (Some(k), Some(top)) = (&key, self.undo_stack.last_mut()) {
            if top.key.as_ref() == Some(k) && top.at.elapsed() < Duration::from_millis(1500) {
                top.at = Instant::now();
                return false;
            }
        }
        let project = if plugins { self.capture_plugin_states() } else { self.project.clone() };
        self.undo_stack.push(UndoEntry { project, label: label.to_string(), key, at: Instant::now(), plugin: None });
        let limit = self.prefs.general.undo_limit.max(1);
        if self.undo_stack.len() > limit {
            self.undo_stack.drain(..self.undo_stack.len() - limit);
        }
        true
    }

    /// Takes back the step [`Session::checkpoint`] just pushed, after its edit failed: anything
    /// the edit changed before failing is put back, and no undo entry is left for it.
    pub fn abandon_checkpoint(&mut self) {
        let Some(e) = self.undo_stack.pop() else { return };
        if !same(&e.project, &self.project) {
            self.restore(e.project);
        }
    }

    pub fn undo_label(&self) -> Option<String> {
        self.undo_stack.last().map(|e| e.label.clone())
    }

    pub fn redo_label(&self) -> Option<String> {
        self.redo_stack.last().map(|e| e.label.clone())
    }

    /// Steps back one edit. Returns its label.
    pub fn undo(&mut self) -> Option<String> {
        self.flush_vst_edits();
        let e = self.undo_stack.pop()?;
        let label = e.label.clone();
        let back = self.step_to(e);
        self.redo_stack.push(back);
        Some(label)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.flush_vst_edits();
        let e = self.redo_stack.pop()?;
        let label = e.label.clone();
        let back = self.step_to(e);
        self.undo_stack.push(back);
        Some(label)
    }

    /// Brings the project to an undo or redo step, and returns the step that comes back here.
    fn step_to(&mut self, e: UndoEntry) -> UndoEntry {
        let now = self.capture_plugin_states();
        let plugin = e.plugin.as_ref().map(|(uid, _)| (*uid, self.vst_state_now(*uid)));
        let back = UndoEntry { project: now, label: e.label.clone(), key: None, at: Instant::now(), plugin };
        self.restore(e.project);
        if let Some((uid, st)) = e.plugin {
            self.restore_vst_state(uid, st);
        }
        back
    }

    /// Brings the engine to `target`, touching only what differs, so plug-ins that didn't
    /// change keep running (and keep their state).
    pub(super) fn restore(&mut self, target: Project) {
        self.auto_abort();
        let current = std::mem::replace(&mut self.project, target);
        // The drag mode is a setting, not part of the edit history.
        self.project.drag_mode = current.drag_mode;
        // Neither is the project alternative's name, which is renamed on disk too.
        self.project.alternative = current.alternative.clone();
        let uid = |s: &Option<PluginSlot>| s.as_ref().map(|s| s.uid);

        // Tracks that no longer exist.
        let gone: Vec<&Track> = current.tracks.iter().filter(|t| self.project.track(t.id).is_none()).collect();
        for t in gone {
            for slot in t.inserts.iter().flatten().chain(t.instrument.iter()) {
                self.controllers.remove(&slot.uid);
            }
            self.meters.remove(&t.id);
            self.send(Cmd::RemoveTrack(t.id));
        }

        let tracks = self.project.tracks.clone();
        for (idx, t) in tracks.iter().enumerate() {
            let Some(cur) = current.track(t.id) else {
                // Came back: rebuild it, plug-ins restored from the snapshot's state.
                let dsp = self.build_track(t);
                self.send(Cmd::AddTrack(dsp, idx));
                continue;
            };
            if !same(&cur.regions, &t.regions) {
                self.push_regions(t.id);
            }
            if !same(&cur.eq, &t.eq) {
                let c = self.eq_coeffs(t);
                self.send(Cmd::Eq(t.id, c));
            }
            if !same(&cur.automation, &t.automation) {
                self.send(Cmd::Automation(t.id, lanes_dsp(&t.automation)));
                self.resync_builtins(t.id);
            }
            if cur.auto_mode != t.auto_mode {
                self.send(Cmd::AutoMode(t.id, t.auto_mode));
                self.resync_builtins(t.id);
            }
            // Freezing loads and unloads plug-ins itself.
            if self.restore_freeze(cur, t) {
                continue;
            }
            if uid(&cur.instrument) != uid(&t.instrument) {
                if let Some(old) = &cur.instrument {
                    self.controllers.remove(&old.uid);
                }
                let ins = t.instrument.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok());
                self.send(Cmd::Instrument { track: t.id, insert: ins });
            } else if let (Some(a), Some(b)) = (&cur.instrument, &t.instrument) {
                if a.bypass != b.bypass {
                    self.send(Cmd::Bypass { track: t.id, uid: b.uid, bypass: b.bypass });
                }
                self.restore_builtin(t.id, a, b);
            }
            let reordered = self.restore_insert_order(t.id, &cur.inserts, &t.inserts);
            let had_inserts = reordered.as_deref().unwrap_or(&cur.inserts);
            for (slot, want) in t.inserts.iter().enumerate() {
                let had = had_inserts.get(slot).cloned().flatten();
                if uid(&had) != uid(want) {
                    if let Some(old) = &had {
                        self.controllers.remove(&old.uid);
                    }
                    let ins = want.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok());
                    self.send(Cmd::Insert { track: t.id, slot, insert: ins });
                } else if let (Some(a), Some(b)) = (&had, want) {
                    if a.bypass != b.bypass {
                        self.send(Cmd::Bypass { track: t.id, uid: b.uid, bypass: b.bypass });
                    }
                    self.restore_builtin(t.id, a, b);
                }
            }
        }
        self.send(Cmd::Order(self.project.process_order()));
        self.push_all_params();

        // Stereo Out
        let master = self.project.master.clone();
        let reordered = self.restore_insert_order(MASTER_ID, &current.master.inserts, &master.inserts);
        let had_inserts = reordered.as_deref().unwrap_or(&current.master.inserts);
        for (slot, want) in master.inserts.iter().enumerate() {
            let had = had_inserts.get(slot).cloned().flatten();
            if uid(&had) != uid(want) {
                if let Some(old) = &had {
                    self.controllers.remove(&old.uid);
                }
                let ins = want.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok());
                self.send(Cmd::Insert { track: MASTER_ID, slot, insert: ins });
            } else if let (Some(a), Some(b)) = (&had, want) {
                if a.bypass != b.bypass {
                    self.send(Cmd::Bypass { track: MASTER_ID, uid: b.uid, bypass: b.bypass });
                }
                self.restore_builtin(MASTER_ID, a, b);
            }
        }
        if !same(&current.master.automation, &master.automation) || current.master.auto_mode != master.auto_mode {
            self.sync_master_automation();
            self.resync_builtins(MASTER_ID);
        }

        // Recordings and imports made after the snapshot stay loaded, so redo can bring them back.
        if self.selected.map(|id| self.project.track(id).is_none()).unwrap_or(false) {
            self.selected = self.project.tracks.first().map(|t| t.id);
        }
        self.sync_globals();
        self.dirty = true;
    }
}
