//! Controller feedback on the session side (the pure parts are in `crate::feedback`, the output
//! ports in `midi_out.rs`).
//!
//! Every idle tick (30 ms, main thread) reads each assignment's target value where the session
//! keeps it, as the screen shows it (the automation being played in Read, Touch and Latch), and
//! sends it to the controller's paired output where it differs from what the controller shows.
//! Reading the value rather than following edits catches every source alike: the mouse, a
//! plug-in's editor, automation, undo, opening a project, an assignment edited. A controller
//! output connected anew gets everything again. Each controller's [`Throttle`] limits the rate
//! and keeps quiet while the controller itself is moving.

use super::*;
use crate::controllers::{self as ctl, Controller, CtlTarget};
use crate::feedback::{self as fb, Shown, Throttle};
use crate::midi_io::MidiPort;

#[derive(Default)]
pub(super) struct Feedback {
    /// Per controller that gets feedback (or was just moved): what it shows.
    states: Vec<(Controller, Throttle)>,
}

impl Feedback {
    fn state(&mut self, c: &Controller) -> &mut Throttle {
        let i = match self.states.iter().position(|(k, _)| k == c) {
            Some(i) => i,
            None => {
                self.states.push((c.clone(), Throttle::default()));
                self.states.len() - 1
            }
        };
        &mut self.states[i].1
    }
}

impl Session {
    /// The output (its key) feedback for controllers on the input `input` goes to now, if any.
    pub(super) fn feedback_output(&self, input: &str) -> Option<String> {
        let outputs = self.midi_out.keys();
        let chosen = self.prefs.midi.feedback_outputs.get(input).map(String::as_str);
        fb::paired_output(input, chosen, &outputs).map(str::to_string)
    }

    /// MIDI output ports and whether each is in use.
    pub fn midi_outputs(&self) -> Vec<MidiPort> {
        self.midi_out.ports()
    }

    /// Assignment `i`'s controller sent `data` (`pressed`: a press for a button). A fader or knob
    /// shows that now, and feedback to it waits while it moves. A button on a switch shows what
    /// was last sent to it, not its own press or release: its switch's state goes to it once it
    /// is let go, whatever the button sent.
    pub(super) fn feedback_heard(&mut self, i: usize, data: [u8; 3], pressed: bool) {
        let Some(a) = self.ctl_list().get(i) else { return };
        let (c, switch) = (a.controller.clone(), a.target.is_button());
        let state = self.feedback.state(&c);
        if switch {
            state.button(!pressed, Instant::now());
        } else {
            state.heard(data, Instant::now());
        }
    }

    /// Sends each assignment's target value to its controller where it changed (`idle`).
    /// `fresh`: the outputs connected since the last call.
    pub(super) fn feedback_idle(&mut self, fresh: &[String]) {
        if !self.midi_out.any() {
            self.feedback.states.clear();
            return;
        }
        let outputs = self.midi_out.keys();
        // (controller, output, the messages showing its target's value: None when the target
        // isn't in the project or there's nothing to show).
        let wanted: Vec<(Controller, String, Option<fb::Msgs>)> = self
            .ctl_list()
            .iter()
            .filter(|a| a.feedback)
            .filter_map(|a| {
                let chosen = self.prefs.midi.feedback_outputs.get(&a.controller.port).map(String::as_str);
                let out = fb::paired_output(&a.controller.port, chosen, &outputs)?;
                Some((a.controller.clone(), out.to_string(), self.ctl_shown(&a.target).and_then(|v| fb::message(a, v))))
            })
            .collect();
        // Controllers without feedback any more forget what they showed, so turning it back on
        // (or pairing them again) sends everything.
        self.feedback.states.retain(|(c, _)| wanted.iter().any(|w| w.0 == *c));
        let now = Instant::now();
        let mut batch = Vec::new();
        for (c, out, msg) in wanted {
            let state = self.feedback.state(&c);
            if fresh.contains(&out) {
                state.forget();
            }
            if let Some(ms) = msg.and_then(|m| state.due(m, now)) {
                batch.extend(ms.into_iter().map(|m| (out.clone(), m)));
            }
        }
        self.midi_out.send(batch);
    }

    /// What a target's controller should show: a switch's state, or the position on the target's
    /// travel as the screen shows it (automation playing over the control included). None when
    /// the target isn't in the open project.
    pub(super) fn ctl_shown(&self, t: &CtlTarget) -> Option<Shown> {
        // A Selected Track target shows the selected track's control.
        let t = &self.ctl_resolve(t)?;
        let own = self.ctl_value(t)?;
        let v = self.ctl_auto_param(t).and_then(|(track, p)| Some(auto_travel(&p, self.automation_shown(track, &p)?))).unwrap_or(own);
        Some(if t.is_button() { Shown::Switch(v >= 0.5) } else { Shown::Travel(v) })
    }

    /// The automation lane a target plays on: its strip and parameter, None for a target that
    /// has no lane. A new target that can be automated needs its arm here.
    fn ctl_auto_param(&self, t: &CtlTarget) -> Option<(Id, AutoParam)> {
        Some(match t {
            CtlTarget::Volume { track } => (*track, AutoParam::Volume),
            CtlTarget::Pan { track } => (*track, AutoParam::Pan),
            CtlTarget::Mute { track } => (*track, AutoParam::Mute),
            CtlTarget::Send { track, slot } => (*track, AutoParam::Send(*slot)),
            CtlTarget::Plugin { track, uid, param, .. } => (*track, AutoParam::Plugin { uid: *uid, param: *param, name: String::new() }),
            CtlTarget::Eq { .. } | CtlTarget::EqOutput { .. } => return self.ctl_auto_param(&self.ctl_legacy_eq(t)?),
            CtlTarget::MasterVolume => (MASTER_ID, AutoParam::Volume),
            CtlTarget::Selected { .. } => return self.ctl_auto_param(&self.ctl_resolve(t)?),
            // No lane: switches without automation, and VCAs.
            CtlTarget::Solo { .. }
            | CtlTarget::Arm { .. }
            | CtlTarget::Transport { .. }
            | CtlTarget::Command { .. }
            | CtlTarget::Monitor { .. }
            | CtlTarget::Bypass { .. }
            | CtlTarget::VcaLevel { .. }
            | CtlTarget::VcaMute { .. } => return None,
        })
    }
}

/// A lane's value (native units) on the control's 0..1 travel, as `ctl_value` has it.
fn auto_travel(p: &AutoParam, v: f32) -> f32 {
    match p {
        AutoParam::Volume | AutoParam::Send(_) => ctl::db_to_fader(v),
        AutoParam::Pan => (v + 1.0) / 2.0,
        AutoParam::Mute | AutoParam::Plugin { .. } => v,
    }
}
