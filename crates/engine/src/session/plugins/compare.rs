//! Compare for a VST3, from its slot's right-click menu: as a built-in panel's Compare button
//! (src/plugins/panels/settings/compareModel.ts), it swaps between the settings as last saved and
//! the edited ones, each swap one undo step.
//!
//! The saved settings are the plug-in's state when Marbble first watches it in a project: when its
//! editor first opens, or when settings are first pasted into it. Marbble has no presets of its own
//! for VST3s, so nothing else is a saved setting.
//!
//! What counts as edited is what Marbble sees happen to the plug-in, not a comparison of state
//! blobs: plug-ins don't always write the same blob twice for the same settings (a timestamp,
//! floating-point noise, the order of a map), so two blobs that differ say nothing. An edit is an
//! undo step from its editor (`editor_undo/`) or Paste Settings; undo and redo put a state back.
//! A state put back is the saved one when it is that very state, or a byte-for-byte copy of it:
//! equal blobs do mean equal settings. So editing and undoing it turns Compare off again.
//!
//! Changes that aren't edits leave Compare alone: automation playing, a controller assignment,
//! the plug-in moving its own meters.

use crate::session::*;

/// What Compare knows of one VST3 instance.
#[derive(Debug, Clone)]
pub(crate) struct VstCompare {
    /// The settings as last saved.
    saved: Arc<PluginState>,
    /// The edited settings, kept while the saved ones play in their place.
    edited: Option<Arc<PluginState>>,
    /// The plug-in has the saved settings, as far as the edits Marbble saw tell.
    at_saved: bool,
}

/// What a press of Compare does.
#[derive(Debug, Clone, PartialEq)]
enum Swap {
    /// Keep the settings now as the edited ones, and play the saved ones.
    ToSaved(Arc<PluginState>),
    /// Go back to the edited settings.
    ToEdited(Arc<PluginState>),
}

/// Whether `a` is `b`'s settings: the same state, or the same bytes.
fn same(a: &Arc<PluginState>, b: &Arc<PluginState>) -> bool {
    Arc::ptr_eq(a, b) || a == b
}

impl VstCompare {
    fn new(saved: Arc<PluginState>) -> VstCompare {
        VstCompare { saved, edited: None, at_saved: true }
    }

    /// The settings differ from the saved ones.
    fn changed(&self) -> bool {
        !self.at_saved
    }

    /// The saved settings play in place of the edited ones.
    fn comparing(&self) -> bool {
        self.at_saved && self.edited.is_some()
    }

    /// The plug-in was edited. Editing while comparing makes those the edited settings, as in Logic.
    fn edited(&mut self) {
        self.at_saved = false;
        self.edited = None;
    }

    /// `st` was put into the plug-in.
    fn restored(&mut self, st: &Arc<PluginState>) {
        self.at_saved = same(st, &self.saved);
    }

    fn swap(&self) -> Option<Swap> {
        if self.comparing() {
            self.edited.clone().map(Swap::ToEdited)
        } else if self.changed() {
            Some(Swap::ToSaved(self.saved.clone()))
        } else {
            None
        }
    }
}

/// The VST3s whose Compare is lit or pressed, for the slot menus.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct CompareStates {
    /// The settings differ from the saved ones.
    pub edited: Vec<Id>,
    /// The saved settings play in place of the edited ones.
    pub comparing: Vec<Id>,
}

impl Session {
    /// Starts Compare's record of a VST3 with `st` as its saved settings, unless it has one.
    pub(crate) fn compare_watch(&mut self, uid: Id, st: &Arc<PluginState>) {
        self.vst_compare.entry(uid).or_insert_with(|| VstCompare::new(st.clone()));
    }

    /// A VST3 was edited (`editor_undo/`, Paste Settings).
    pub(crate) fn compare_edited(&mut self, uid: Id) {
        if let Some(c) = self.vst_compare.get_mut(&uid) {
            c.edited();
        }
    }

    /// A state was put into a VST3 (undo, redo, Compare, Paste Settings).
    pub(crate) fn compare_restored(&mut self, uid: Id, st: &Arc<PluginState>) {
        if let Some(c) = self.vst_compare.get_mut(&uid) {
            c.restored(st);
        }
    }

    /// Forgets the records of instances that are gone.
    pub(crate) fn compare_idle(&mut self) {
        if !self.vst_compare.is_empty() {
            let controllers = &self.controllers;
            self.vst_compare.retain(|uid, _| controllers.contains_key(uid));
        }
    }

    /// The loaded VST3s whose Compare is lit or pressed.
    pub fn compare_states(&self) -> CompareStates {
        let mut out = CompareStates::default();
        for (&uid, c) in self.vst_compare.iter().filter(|(uid, _)| self.controllers.contains_key(uid)) {
            if c.comparing() {
                out.comparing.push(uid);
            } else if c.changed() {
                out.edited.push(uid);
            }
        }
        out.edited.sort_unstable();
        out.comparing.sort_unstable();
        out
    }

    /// Compare on a VST3: plays the saved settings in place of the edited ones, or goes back to
    /// them. Called after the command's checkpoint, which becomes the step that undoes it.
    pub fn toggle_vst_compare(&mut self, uid: Id) -> Result<()> {
        let swap = self.vst_compare.get(&uid).and_then(VstCompare::swap).ok_or_else(|| anyhow!("Nothing to compare: the settings are as last saved"))?;
        let before = self.load_vst_settings(
            uid,
            match &swap {
                Swap::ToSaved(st) | Swap::ToEdited(st) => st.clone(),
            },
        );
        if let Some(c) = self.vst_compare.get_mut(&uid) {
            c.edited = matches!(swap, Swap::ToSaved(_)).then_some(before);
        }
        Ok(())
    }

    /// Puts `st` into the running VST3 `uid`, the instance staying, as part of the undo step the
    /// command's checkpoint just took. Returns the state it had.
    pub(crate) fn load_vst_settings(&mut self, uid: Id, st: Arc<PluginState>) -> Arc<PluginState> {
        let before = self.vst_state_now(uid);
        self.compare_watch(uid, &before);
        self.restore_vst_state(uid, st);
        if let Some(top) = self.undo_stack.last_mut() {
            top.plugin = Some((uid, before.clone()));
        }
        self.dirty = true;
        before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(b: u8) -> Arc<PluginState> {
        Arc::new(PluginState { component: vec![b], controller: vec![] })
    }

    #[test]
    fn edits_light_it_and_a_swap_plays_the_saved_settings() {
        let saved = st(0);
        let mut c = VstCompare::new(saved.clone());
        assert_eq!((c.changed(), c.comparing(), c.swap()), (false, false, None));
        c.edited();
        assert!(c.changed() && !c.comparing());
        assert_eq!(c.swap(), Some(Swap::ToSaved(saved.clone())));
        // What `toggle_vst_compare` does: the saved state goes in, the edited one is kept.
        c.restored(&saved);
        c.edited = Some(st(1));
        assert!(c.comparing() && !c.changed());
        assert_eq!(c.swap(), Some(Swap::ToEdited(st(1))));
        c.restored(&st(1));
        c.edited = None;
        assert!(c.changed() && !c.comparing());
    }

    #[test]
    fn editing_while_comparing_makes_those_the_edited_settings() {
        let mut c = VstCompare::new(st(0));
        c.edited();
        c.restored(&st(0));
        c.edited = Some(st(1));
        c.edited();
        assert!(c.changed() && !c.comparing() && c.edited.is_none());
        assert_eq!(c.swap(), Some(Swap::ToSaved(st(0))));
    }

    #[test]
    fn undo_back_to_the_saved_state_turns_it_off() {
        let saved = st(0);
        let mut c = VstCompare::new(saved.clone());
        c.edited();
        // Undo puts back the very state the step kept...
        c.restored(&saved);
        assert!(!c.changed());
        // ...or a byte-for-byte copy; anything else counts as edited.
        c.restored(&st(2));
        assert!(c.changed());
        c.restored(&st(0));
        assert!(!c.changed());
    }

    #[test]
    fn undoing_a_swap_to_the_saved_settings_shows_the_edited_ones_as_edited() {
        let mut c = VstCompare::new(st(0));
        c.edited();
        c.restored(&st(0));
        c.edited = Some(st(1));
        // Undo of the swap puts the edited state back; the stash stays but isn't playing.
        c.restored(&st(1));
        assert!(c.changed() && !c.comparing());
        assert_eq!(c.swap(), Some(Swap::ToSaved(st(0))));
    }
}
