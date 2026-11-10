//! Automation edited by hand: replacing a lane's points (the lane editor's drags share an undo
//! step), clearing lanes, and the plug-in parameters a lane can take.

use vst3_host::ParamInfo;

use crate::session::*;

impl Session {
    /// Replaces a lane's points (an empty list removes the lane). Values are clamped to the
    /// parameter's range.
    pub fn set_automation_lane(&mut self, track: Id, param: AutoParam, mut points: Vec<AutoPoint>) {
        let (lo, hi) = param.range();
        for p in &mut points {
            p.pos = p.pos.max(0);
            p.value = if p.value.is_finite() { p.value.clamp(lo, hi) } else { lo };
            if matches!(param, AutoParam::Mute) {
                p.value = p.value.round();
            }
        }
        points.sort_by_key(|p| p.pos);
        let Some(automation) = self.strip_lanes_mut(track) else { return };
        match automation.iter().position(|l| l.param.same(&param)) {
            Some(i) if points.is_empty() => {
                automation.remove(i);
            }
            Some(i) => automation[i] = AutoLane { param, points },
            None if points.is_empty() => {}
            None => automation.push(AutoLane { param, points }),
        }
        self.push_automation(track);
        self.dirty = true;
    }

    /// Deletes one lane, or all of the track's automation with `param` None.
    pub fn clear_automation(&mut self, track: Id, param: Option<AutoParam>) {
        let Some(automation) = self.strip_lanes_mut(track) else { return };
        match &param {
            Some(p) => automation.retain(|l| !l.param.same(p)),
            None => automation.clear(),
        }
        self.push_automation(track);
        self.dirty = true;
    }

    /// A plug-in instance's automatable parameters (none for the built-in synth).
    pub fn plugin_params(&self, uid: Id) -> Vec<ParamInfo> {
        if let Some(p) = self.builtin_param_infos(uid) {
            return p;
        }
        self.controllers.get(&uid).map(|c| c.parameters()).unwrap_or_default()
    }
}
