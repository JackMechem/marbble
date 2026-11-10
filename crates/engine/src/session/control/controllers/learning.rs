//! Learning an assignment (Learn Assignment, the Learn buttons), and the last touched parameter that
//! a learn with no target takes.

use super::*;

impl Session {
    /// Waits for a controller to assign to `target`, or to the last touched parameter. Returns
    /// the parameter's name.
    pub fn start_learn(&mut self, target: Option<CtlTarget>) -> Result<String> {
        self.start_learn_named(target, None)
    }

    /// `start_learn`, with the name to show for the target: a key command's, which only the
    /// interface knows.
    pub fn start_learn_named(&mut self, target: Option<CtlTarget>, name: Option<String>) -> Result<String> {
        let target = target
            .or_else(|| self.controls.touched.clone())
            .ok_or_else(|| anyhow!("Move a fader, knob or plug-in parameter first, then learn an assignment for it"))?;
        let target = self.ctl_complete(target);
        let known = self.ctl_target_name(&target).ok_or_else(|| anyhow!("That parameter isn't in this project any more"))?;
        let name = name.filter(|n| !n.is_empty()).unwrap_or(known);
        self.controls.learn = Some((target, name.clone()));
        self.controls.learner = Learner::default();
        self.midi.set_learning(true);
        Ok(name)
    }

    /// The key commands controllers fired since the last call, oldest first (for the tick).
    pub fn take_ctl_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.controls.commands)
    }

    /// Fires a key command, as a controller assigned to it does: the main window runs it.
    pub(crate) fn ctl_command(&mut self, id: &str) {
        self.controls.commands.push(id.to_string());
    }

    pub fn cancel_learn(&mut self) {
        self.controls.learn = None;
        self.midi.set_learning(false);
    }

    /// The name of the parameter being learned, if any (for the tick).
    pub fn learning(&self) -> Option<String> {
        self.controls.learn.as_ref().map(|l| l.1.clone())
    }

    /// What the last learn assigned, once (for the tick).
    pub fn take_learned(&mut self) -> Option<String> {
        self.controls.learned.take()
    }

    pub(super) fn ctl_learned(&mut self, controller: Controller, target: CtlTarget, name: String) {
        let label = controller_label(&controller);
        // A controller drives one parameter.
        self.controls.list.retain(|a| a.controller != controller);
        let switch = self.ctl_is_switch(&target);
        let a = Assignment::new(controller.clone(), target, switch);
        // A button that sends no release after the press it was learnt from latches by itself.
        self.controls.latch_probe = (self.ctl_is_button(&a) && !matches!(controller.kind, CtlKind::Program { .. })).then(|| (controller, Instant::now()));
        self.controls.list.push(a);
        self.midi.set_learning(false);
        self.ctl_list_changed(true);
        self.controls.learned = Some(format!("{label} assigned to {name}"));
    }

    /// Fills in a plug-in target's plug-in id (the UI may leave it empty).
    fn ctl_complete(&self, mut target: CtlTarget) -> CtlTarget {
        if let CtlTarget::Plugin { track, uid, plugin, .. } = &mut target {
            if let Some((_, s)) = self.ctl_slot(*track, *uid) {
                *plugin = s.plugin_id.clone();
            }
        }
        target
    }

    // ---- the last touched parameter -------------------------------------------------------

    /// The user moved a parameter; Learn Assignment without a target learns this one.
    pub fn touched(&mut self, target: CtlTarget) {
        if let CtlTarget::Plugin { uid, .. } = target {
            self.controls.focused = Some(uid);
        }
        self.controls.touched = Some(self.ctl_complete(target));
    }

    /// A plug-in's window or panel was opened or brought forward: on the selected track, the
    /// Selected Track targets' plug-in parameters are its.
    pub fn focus_plugin(&mut self, uid: Id) {
        self.controls.focused = Some(uid);
    }

    /// A track's volume, pan or a switch was changed from the interface.
    pub fn touched_patch(&mut self, track: Id, patch: &TrackPatch) {
        let t = if patch.volume_db.is_some() {
            CtlTarget::Volume { track }
        } else if patch.pan.is_some() {
            CtlTarget::Pan { track }
        } else if patch.mute.is_some() {
            CtlTarget::Mute { track }
        } else if patch.solo.is_some() {
            CtlTarget::Solo { track }
        } else if patch.armed.is_some() {
            CtlTarget::Arm { track }
        } else if patch.monitor.is_some() {
            CtlTarget::Monitor { track }
        } else {
            return;
        };
        self.touched(t);
    }

    /// A built-in effect's or instrument's parameter was changed from its panel.
    pub fn touched_slot(&mut self, track: Id, slot: usize, param: u32) {
        if let Some(s) = self.slot_get(track, slot) {
            let t = CtlTarget::Plugin { track, uid: s.uid, plugin: s.plugin_id.clone(), param };
            self.touched(t);
        }
    }

    /// An automation parameter was grabbed (the controls that report touches).
    pub fn touched_auto(&mut self, track: Id, param: &AutoParam) {
        let t = match param {
            AutoParam::Volume => CtlTarget::Volume { track },
            AutoParam::Pan => CtlTarget::Pan { track },
            AutoParam::Mute => CtlTarget::Mute { track },
            AutoParam::Send(slot) => CtlTarget::Send { track, slot: *slot },
            AutoParam::Plugin { uid, param, .. } => CtlTarget::Plugin { track, uid: *uid, plugin: String::new(), param: *param },
        };
        self.touched(t);
    }

    /// A parameter moved in a plug-in's own editor.
    pub(crate) fn touched_plugin(&mut self, uid: Id, param: u32) {
        let p = &self.project;
        let tracks = p.tracks.iter().map(|t| (t.id, t.inserts.iter().flatten().chain(t.instrument.iter()).find(|s| s.uid == uid)));
        let master = std::iter::once((MASTER_ID, p.master.inserts.iter().flatten().find(|s| s.uid == uid)));
        if let Some((track, s)) = tracks.chain(master).find_map(|(id, s)| s.map(|s| (id, s))) {
            let t = CtlTarget::Plugin { track, uid, plugin: s.plugin_id.clone(), param };
            self.controls.touched = Some(t);
            self.controls.focused = Some(uid);
        }
    }
}
