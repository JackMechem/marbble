//! Controller assignments on the session side (the pure parts are in `crate::control::controllers`):
//! the global assignment list (here), learning an assignment for the last touched parameter
//! (`learning.rs`), reading and setting targets (`targets.rs`), and applying controller moves and
//! button presses (`incoming.rs`).
//!
//! A controller move goes through the same session paths as the mouse: it makes an undo step
//! (coalesced per parameter, like a fader drag), and while the transport plays in Touch, Latch
//! or Write it is recorded as automation. The controller holds the parameter (an automation
//! touch) while it keeps moving and lets go [`RELEASE_SECS`] after it stops, which is when a
//! Touch pass ends.
//!
//! The input threads take assigned controllers off the live MIDI path (`midi/input.rs`); `idle`
//! applies them here, on the main thread, where plug-in controllers may be called. A key
//! command assigned to a controller can't run here: its id goes out with the next tick
//! (`take_ctl_commands`), and the main window runs it as it would for the key.
//!
//! Assignments learnt when every strip had a fixed Channel EQ (`CtlTarget::Eq`, `EqOutput`)
//! drive the same setting of the strip's first Channel EQ insert, which is what opening an
//! older project turns that EQ into (`model/channel_eq_upgrade.rs`).

mod incoming;
mod learning;
mod targets;

use std::path::PathBuf;

use crate::control::controllers::{
    self as ctl, Assignment, AssignmentFile, ButtonAct, ButtonMode, Controller, CtlKind, CtlMsg, CtlTarget, Encoding, EqField, Follow, HiRes, Learner,
    SelectedParam, Takeover, TransportAction,
};
use crate::session::*;

/// How long a controller holds its parameter after its last move (s).
const RELEASE_SECS: f64 = 0.5;

/// The Channel EQ's bands, as the editor names them.
const BAND_NAMES: [&str; 8] = ["Low Cut", "Low Shelf", "Bell 1", "Bell 2", "Bell 3", "Bell 4", "High Shelf", "High Cut"];

#[derive(Default)]
pub(crate) struct Controls {
    list: Vec<Assignment>,
    /// Per assignment, in step with `list`.
    follow: Vec<Follow>,
    /// Where the list is saved; None keeps it in memory only.
    path: Option<PathBuf>,
    /// The target being learned and its name.
    learn: Option<(CtlTarget, String)>,
    /// What the last learn assigned, for the next tick.
    learned: Option<String>,
    /// The parameter the user last moved, for Learn Assignment.
    touched: Option<CtlTarget>,
    /// Automation touches held by controllers, and when each last moved.
    holding: Vec<(Id, AutoParam, Instant)>,
    /// A button just learnt and when: if it sends nothing (no release) within
    /// `ctl::LATCH_WAIT`, it latches by itself.
    latch_probe: Option<(Controller, Instant)>,
    /// Key commands controllers fired since the last tick, oldest first.
    commands: Vec<String>,
    /// 14-bit controllers' and NRPNs' state, per port and channel.
    hires: HiRes,
    /// What the learn in progress has heard.
    learner: Learner,
    /// The plug-in last opened or touched: the Selected Track's focused plug-in, when it's on
    /// that track.
    focused: Option<Id>,
    /// Plug-ins' parameter ids in list order, and when each list was read (`ctl_param_id`).
    param_ids: std::cell::RefCell<Vec<(Id, Instant, Vec<u32>)>>,
}

/// An assignment as the Controller Assignments window shows it.
#[derive(Debug, Clone, Serialize)]
pub struct AssignmentRow {
    #[serde(flatten)]
    pub assignment: Assignment,
    /// The target's name ("Bass › Volume"), or what it was when its track or plug-in isn't in
    /// this project.
    pub name: String,
    /// The target is in the open project.
    pub available: bool,
    /// The target is a switch or transport button (no mode or range).
    pub button: bool,
    /// The output (its key) feedback for the controller goes to, if one is paired.
    pub output: Option<String>,
}

/// The settings of an assignment the window changes; None leaves one alone.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AssignmentPatch {
    pub mode: Option<Takeover>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub button_mode: Option<ButtonMode>,
    pub velocity: Option<bool>,
    pub latching: Option<bool>,
    pub encoding: Option<Encoding>,
    pub sensitivity: Option<f32>,
}

/// A note's name as Logic writes it (C3 is middle C, 60).
fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
    format!("{}{}", NAMES[n as usize % 12], n as i32 / 12 - 2)
}

/// "CC 7, channel 1", "Pitch Bend, channel 1", "Note C1, channel 10" or "Program 5, channel 1".
pub fn controller_label(c: &Controller) -> String {
    let what = match c.kind {
        CtlKind::Cc { number } => format!("CC {number}"),
        CtlKind::PitchBend => "Pitch Bend".into(),
        CtlKind::Note { number } => format!("Note {}", note_name(number)),
        // Numbered from 1, as Settings › MIDI › Monitor shows them.
        CtlKind::Program { number } => format!("Program {}", number as u16 + 1),
        CtlKind::Cc14 { number } => format!("CC {number}/{} (14-bit)", number + 32),
        CtlKind::Nrpn { number } => format!("NRPN {number}"),
    };
    format!("{what}, channel {}", c.channel + 1)
}

fn round_to(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

impl Session {
    // ---- the assignment list --------------------------------------------------------------

    /// Loads the global assignments (a missing or unreadable file is an empty list) and saves
    /// changes there from now on.
    pub fn load_controller_assignments(&mut self, path: PathBuf) {
        let file: AssignmentFile = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        self.controls.list = file.assignments;
        self.controls.path = Some(path);
        self.ctl_list_changed(false);
    }

    /// Every assignment, for the Controller Assignments window.
    pub fn controller_assignments(&self) -> Vec<AssignmentRow> {
        self.controls
            .list
            .iter()
            .map(|a| {
                let name = self.ctl_target_name(&a.target);
                AssignmentRow {
                    available: name.is_some(),
                    name: name.unwrap_or_else(|| missing_name(&a.target)),
                    button: a.target.is_button(),
                    output: self.feedback_output(&a.controller.port),
                    assignment: a.clone(),
                }
            })
            .collect()
    }

    /// Changes an assignment's takeover mode, range or button behaviour.
    pub fn update_controller_assignment(&mut self, index: usize, patch: AssignmentPatch) -> Result<()> {
        let a = self.controls.list.get_mut(index).ok_or_else(|| anyhow!("no such assignment"))?;
        if let Some(mode) = patch.mode {
            a.mode = mode;
        }
        if let Some(min) = patch.min {
            a.min = if min.is_finite() { min.clamp(0.0, 1.0) } else { 0.0 };
        }
        if let Some(max) = patch.max {
            a.max = if max.is_finite() { max.clamp(0.0, 1.0) } else { 1.0 };
        }
        if let Some(button_mode) = patch.button_mode {
            a.button_mode = button_mode;
        }
        if let Some(velocity) = patch.velocity {
            a.velocity = velocity;
        }
        if let Some(latching) = patch.latching {
            a.latching = latching;
        }
        if let Some(encoding) = patch.encoding {
            a.encoding = encoding;
        }
        if let Some(sensitivity) = patch.sensitivity.filter(|v| v.is_finite()) {
            a.sensitivity = sensitivity.clamp(0.05, 20.0);
        }
        self.ctl_list_changed(true);
        Ok(())
    }

    /// Turns sending the target's value back to the controller on or off.
    pub fn set_controller_feedback(&mut self, index: usize, on: bool) -> Result<()> {
        self.controls.list.get_mut(index).ok_or_else(|| anyhow!("no such assignment"))?.feedback = on;
        self.save_controller_assignments();
        Ok(())
    }

    /// The assignments, in the window's order (for feedback).
    pub(crate) fn ctl_list(&self) -> &[Assignment] {
        &self.controls.list
    }

    pub fn delete_controller_assignment(&mut self, index: usize) -> Result<()> {
        if index >= self.controls.list.len() {
            return Err(anyhow!("no such assignment"));
        }
        self.controls.list.remove(index);
        self.ctl_list_changed(true);
        Ok(())
    }

    /// The list changed: pickup states start over, the input threads learn which controllers
    /// to take, and the file is written.
    fn ctl_list_changed(&mut self, save: bool) {
        self.controls.follow = vec![Follow::default(); self.controls.list.len()];
        self.midi.set_assigned(self.controls.list.iter().map(|a| (a.controller.port.clone(), a.controller.channel, a.controller.kind)).collect());
        if save {
            self.save_controller_assignments();
        }
    }

    fn save_controller_assignments(&self) {
        let Some(path) = &self.controls.path else { return };
        let file = AssignmentFile { assignments: self.controls.list.clone() };
        let write = || -> Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_string_pretty(&file)?)?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        };
        if let Err(e) = write() {
            log::warn!("could not save {}: {e}", path.display());
        }
    }
}

/// How the window names a target whose track or plug-in isn't in the open project.
fn missing_name(t: &CtlTarget) -> String {
    let what = match t {
        CtlTarget::Volume { .. } => "Volume".to_string(),
        CtlTarget::Pan { .. } => "Pan".into(),
        CtlTarget::Mute { .. } => "Mute".into(),
        CtlTarget::Solo { .. } => "Solo".into(),
        CtlTarget::Arm { .. } => "Record Enable".into(),
        CtlTarget::Send { slot, .. } => format!("Send {}", slot + 1),
        CtlTarget::Plugin { plugin, param, .. } => format!("{} › Parameter {param}", plugin.strip_prefix("builtin:").unwrap_or(plugin)),
        CtlTarget::Eq { band, .. } => format!("Channel EQ › {}", BAND_NAMES.get(*band as usize).copied().unwrap_or("Band")),
        CtlTarget::EqOutput { .. } => "Channel EQ › Output".into(),
        CtlTarget::MasterVolume | CtlTarget::Transport { .. } => String::new(),
        CtlTarget::Command { id } => format!("Key Command › {id}"),
        CtlTarget::Monitor { .. } => "Input Monitoring".into(),
        CtlTarget::Bypass { slot, .. } => format!("Insert {} Bypass", slot + 1),
        CtlTarget::VcaLevel { .. } => "VCA › Level".into(),
        CtlTarget::VcaMute { .. } => "VCA › Mute".into(),
        CtlTarget::Selected { param } => format!("Selected Track › {}", param.name()),
    };
    match t.track() {
        Some(MASTER_ID) => format!("Stereo Out › {what}"),
        Some(id) => format!("Track {id} › {what}"),
        None => what,
    }
}
