//! Controller assignments on the session side (the pure parts are in `crate::controllers`):
//! learning an assignment for the last touched parameter, the global assignment list, and
//! applying controller moves and button presses.
//!
//! A controller move goes through the same session paths as the mouse: it makes an undo step
//! (coalesced per parameter, like a fader drag), and while the transport plays in Touch, Latch
//! or Write it is recorded as automation. The controller holds the parameter (an automation
//! touch) while it keeps moving and lets go [`RELEASE_SECS`] after it stops, which is when a
//! Touch pass ends.
//!
//! The input threads take assigned controllers off the live MIDI path (`midi_io.rs`); `idle`
//! applies them here, on the main thread, where plug-in controllers may be called. A key
//! command assigned to a controller can't run here: its id goes out with the next tick
//! (`take_ctl_commands`), and the main window runs it as it would for the key.
//!
//! Assignments learnt when every strip had a fixed Channel EQ (`CtlTarget::Eq`, `EqOutput`)
//! drive the same setting of the strip's first Channel EQ insert, which is what opening an
//! older project turns that EQ into (`model/channel_eq_upgrade.rs`).

use std::path::PathBuf;

use super::*;
use crate::controllers::{
    self as ctl, Assignment, AssignmentFile, ButtonAct, ButtonMode, Controller, CtlKind, CtlMsg, CtlTarget, Encoding, EqField, Follow, HiRes, Learner,
    SelectedParam, Takeover, TransportAction,
};

/// How long a controller holds its parameter after its last move (s).
const RELEASE_SECS: f64 = 0.5;

/// The Channel EQ's bands, as the editor names them.
const BAND_NAMES: [&str; 8] = ["Low Cut", "Low Shelf", "Bell 1", "Bell 2", "Bell 3", "Bell 4", "High Shelf", "High Cut"];

#[derive(Default)]
pub(super) struct Controls {
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
    pub(super) fn ctl_list(&self) -> &[Assignment] {
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

    // ---- learning -------------------------------------------------------------------------

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

    fn ctl_learned(&mut self, controller: Controller, target: CtlTarget, name: String) {
        let label = controller_label(&controller);
        // A controller drives one parameter.
        self.controls.list.retain(|a| a.controller != controller);
        let a = Assignment::new(controller.clone(), target);
        // A button that sends no release after the press it was learnt from latches by itself.
        self.controls.latch_probe = (a.is_button() && !matches!(controller.kind, CtlKind::Program { .. })).then(|| (controller, Instant::now()));
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
    pub(super) fn touched_plugin(&mut self, uid: Id, param: u32) {
        let p = &self.project;
        let tracks = p.tracks.iter().map(|t| (t.id, t.inserts.iter().flatten().chain(t.instrument.iter()).find(|s| s.uid == uid)));
        let master = std::iter::once((MASTER_ID, p.master.inserts.iter().flatten().find(|s| s.uid == uid)));
        if let Some((track, s)) = tracks.chain(master).find_map(|(id, s)| s.map(|s| (id, s))) {
            let t = CtlTarget::Plugin { track, uid, plugin: s.plugin_id.clone(), param };
            self.controls.touched = Some(t);
            self.controls.focused = Some(uid);
        }
    }

    // ---- targets --------------------------------------------------------------------------

    /// What a target stands for now: a Selected Track target, the selected track's control
    /// (None with no track selected, or no such control there); any other, itself.
    pub(super) fn ctl_resolve(&self, t: &CtlTarget) -> Option<CtlTarget> {
        let CtlTarget::Selected { param } = t else { return Some(t.clone()) };
        let track = self.selected.filter(|id| self.project.track(*id).is_some())?;
        if let SelectedParam::PluginParam { index } = *param {
            let slot = self.ctl_focused_plugin(track)?;
            let p = self.ctl_param_id(slot.uid, index as usize)?;
            return Some(CtlTarget::Plugin { track, uid: slot.uid, plugin: slot.plugin_id.clone(), param: p });
        }
        param.on(track)
    }

    /// A plug-in's parameter id at `index` in its list. The lists are kept a moment, as
    /// feedback asks every idle tick and a VST3 plug-in describes every parameter to give one.
    fn ctl_param_id(&self, uid: Id, index: usize) -> Option<u32> {
        let fresh = |at: &Instant| at.elapsed() < Duration::from_secs(2);
        if let Some((_, _, ids)) = self.controls.param_ids.borrow().iter().find(|(u, at, _)| *u == uid && fresh(at)) {
            return ids.get(index).copied();
        }
        let ids: Vec<u32> = self.plugin_params(uid).iter().map(|p| p.id).collect();
        let id = ids.get(index).copied();
        let mut cache = self.controls.param_ids.borrow_mut();
        cache.retain(|(u, at, _)| *u != uid && fresh(at));
        cache.push((uid, Instant::now(), ids));
        id
    }

    /// The track's focused plug-in: the one last opened or touched, if it's on the track, else
    /// its instrument, else its first Audio FX.
    fn ctl_focused_plugin(&self, track: Id) -> Option<&PluginSlot> {
        let t = self.project.track(track)?;
        let mut all = t.instrument.iter().chain(t.inserts.iter().flatten());
        let focused = self.controls.focused.and_then(|uid| all.find(|s| s.uid == uid));
        focused.or(t.instrument.as_ref()).or_else(|| t.inserts.iter().flatten().next())
    }

    /// A plug-in slot on a strip: its insert index (None for the instrument slot) and the slot.
    fn ctl_slot(&self, track: Id, uid: Id) -> Option<(Option<usize>, &PluginSlot)> {
        let (inserts, instrument) = if track == MASTER_ID {
            (&self.project.master.inserts, None)
        } else {
            let t = self.project.track(track)?;
            (&t.inserts, t.instrument.as_ref())
        };
        if let Some(i) = inserts.iter().position(|s| s.as_ref().is_some_and(|s| s.uid == uid)) {
            return inserts[i].as_ref().map(|s| (Some(i), s));
        }
        instrument.filter(|s| s.uid == uid).map(|s| (None, s))
    }

    /// Whether an assignment drives the old fixed Channel EQ of `strip` (in any project, as
    /// assignments are global), so opening an older project keeps that EQ even when it is flat.
    pub(super) fn ctl_drives_legacy_eq(&self, strip: Id) -> bool {
        self.controls.list.iter().any(|a| matches!(a.target, CtlTarget::Eq { track, .. } | CtlTarget::EqOutput { track } if track == strip))
    }

    /// Opens what an older project (or template) holds in an older form: each strip's fixed
    /// Channel EQ becomes an insert (`Project::upgrade_channel_eq`). Returns a warning naming
    /// the strips that had no Audio FX slot free for it.
    pub(super) fn upgrade_document(&self, p: &mut Project) -> Option<String> {
        let full = p.upgrade_channel_eq(|strip| self.ctl_drives_legacy_eq(strip));
        (!full.is_empty()).then(|| format!("No free Audio FX slot for the Channel EQ of {}, so it was left out.", full.join(", ")))
    }

    /// A strip's first Channel EQ insert (`track` MASTER_ID for Stereo Out): the one its EQ
    /// thumbnail shows, and the one assignments to the old fixed Channel EQ drive.
    pub fn first_channel_eq(&self, track: Id) -> Option<&PluginSlot> {
        let inserts = if track == MASTER_ID { &self.project.master.inserts } else { &self.project.track(track)?.inserts };
        inserts.iter().flatten().find(|s| s.plugin_id == crate::fx::channel_eq::ID)
    }

    /// What an assignment to the old fixed Channel EQ drives now: that parameter of the strip's
    /// first Channel EQ insert. None for other targets, and when the strip has none.
    pub(super) fn ctl_legacy_eq(&self, t: &CtlTarget) -> Option<CtlTarget> {
        use crate::fx::channel_eq as ceq;
        let (track, param) = match *t {
            CtlTarget::Eq { track, band, field } => {
                let b = band as usize;
                let param = match field {
                    EqField::On => ceq::on(b),
                    EqField::Freq => ceq::freq(b),
                    EqField::Gain if !ceq::is_cut(b) => ceq::gain(b),
                    EqField::Gain => return None,
                    EqField::Q => ceq::q(b),
                };
                (b < ceq::BANDS).then_some((track, param))?
            }
            CtlTarget::EqOutput { track } => (track, ceq::OUTPUT),
            _ => return None,
        };
        let s = self.first_channel_eq(track)?;
        Some(CtlTarget::Plugin { track, uid: s.uid, plugin: s.plugin_id.clone(), param })
    }

    /// The plug-in slot a target names, if it is there and still holds the same plug-in.
    fn ctl_plugin(&self, track: Id, uid: Id, plugin: &str) -> Option<(Option<usize>, &PluginSlot)> {
        self.ctl_slot(track, uid).filter(|(_, s)| s.plugin_id == plugin)
    }

    fn ctl_strip(&self, track: Id) -> Option<String> {
        if track == MASTER_ID {
            Some("Stereo Out".into())
        } else {
            self.project.track(track).map(|t| t.name.clone())
        }
    }

    /// A target's name, or None when it isn't in the open project. Calls plug-in controllers,
    /// so main thread only.
    fn ctl_target_name(&self, t: &CtlTarget) -> Option<String> {
        let track = |id: Id| self.project.track(id).map(|t| t.name.clone());
        Some(match t {
            CtlTarget::Volume { track: id } => format!("{} › Volume", track(*id)?),
            CtlTarget::Pan { track: id } => format!("{} › Pan", track(*id)?),
            CtlTarget::Mute { track: id } => format!("{} › Mute", track(*id)?),
            CtlTarget::Solo { track: id } => format!("{} › Solo", track(*id)?),
            CtlTarget::Arm { track: id } => format!("{} › Record Enable", track(*id)?),
            CtlTarget::Send { track: id, slot } => {
                self.project.track(*id)?.sends.get(*slot as usize)?.as_ref()?;
                format!("{} › Send {}", track(*id)?, slot + 1)
            }
            CtlTarget::Plugin { track: id, uid, plugin, param } => {
                let (_, slot) = self.ctl_plugin(*id, *uid, plugin)?;
                let p = self.plugin_params(*uid).into_iter().find(|p| p.id == *param)?;
                format!("{} › {} › {}", self.ctl_strip(*id)?, slot.name, p.name)
            }
            CtlTarget::Eq { .. } | CtlTarget::EqOutput { .. } => return self.ctl_target_name(&self.ctl_legacy_eq(t)?),
            CtlTarget::MasterVolume => "Stereo Out › Volume".into(),
            CtlTarget::Transport { action } => match action {
                TransportAction::Play => "Transport › Play / Stop".into(),
                TransportAction::Record => "Transport › Record".into(),
                TransportAction::Cycle => "Transport › Cycle".into(),
            },
            // The window names it from its own list of commands.
            CtlTarget::Command { id } => format!("Key Command › {id}"),
            CtlTarget::Monitor { track: id } => format!("{} › Input Monitoring", track(*id)?),
            CtlTarget::Bypass { track: id, slot } => format!("{} › Insert {} Bypass", self.ctl_strip(*id)?, slot + 1),
            CtlTarget::VcaLevel { vca } => format!("{} › Level", self.vca(*vca)?.name),
            CtlTarget::VcaMute { vca } => format!("{} › Mute", self.vca(*vca)?.name),
            CtlTarget::Selected { param } => format!("Selected Track › {}", param.name()),
        })
    }

    /// The target's value on its own 0..1 travel (the fader's taper for levels), or None
    /// when it isn't in the open project. Cheap enough for every controller message.
    pub(super) fn ctl_value(&self, t: &CtlTarget) -> Option<f32> {
        let tr = |id: Id| self.project.track(id);
        Some(match t {
            CtlTarget::Volume { track } => ctl::db_to_fader(tr(*track)?.volume_db),
            CtlTarget::Pan { track } => (tr(*track)?.pan + 1.0) / 2.0,
            CtlTarget::Mute { track } => tr(*track)?.mute as u8 as f32,
            CtlTarget::Solo { track } => tr(*track)?.solo as u8 as f32,
            CtlTarget::Arm { track } => tr(*track)?.armed as u8 as f32,
            CtlTarget::Send { track, slot } => ctl::db_to_fader(tr(*track)?.sends.get(*slot as usize)?.as_ref()?.level_db),
            CtlTarget::Plugin { track, uid, plugin, param } => {
                self.ctl_plugin(*track, *uid, plugin)?;
                match self.controllers.get(uid) {
                    Some(c) => c.param_value(*param)? as f32,
                    None => self.builtin_normalized(*uid, *param)?,
                }
            }
            CtlTarget::Eq { .. } | CtlTarget::EqOutput { .. } => return self.ctl_value(&self.ctl_legacy_eq(t)?),
            CtlTarget::MasterVolume => ctl::db_to_fader(self.project.master.volume_db),
            CtlTarget::Transport { action } => match action {
                TransportAction::Play => self.is_playing() as u8 as f32,
                TransportAction::Record => self.is_recording() as u8 as f32,
                TransportAction::Cycle => self.project.cycle.enabled as u8 as f32,
            },
            // Nothing to show: a command has no state.
            CtlTarget::Command { .. } => return None,
            CtlTarget::Monitor { track } => tr(*track)?.monitor as u8 as f32,
            CtlTarget::Bypass { track, slot } => {
                let inserts = if *track == MASTER_ID { &self.project.master.inserts } else { &tr(*track)?.inserts };
                inserts.get(*slot as usize)?.as_ref()?.bypass as u8 as f32
            }
            CtlTarget::VcaLevel { vca } => ctl::db_to_fader(self.vca(*vca)?.level_db),
            CtlTarget::VcaMute { vca } => self.vca(*vca)?.mute as u8 as f32,
            // The selected track's control, if any.
            CtlTarget::Selected { .. } => return self.ctl_value(&self.ctl_resolve(t)?),
        })
    }

    /// An undo step for a controller move, coalesced like a drag of the same control, unless
    /// the move is being written as automation (then it belongs to the pass).
    fn ctl_checkpoint(&mut self, track: Id, label: &str, key: String) {
        if !self.automation_writing(track) {
            self.checkpoint(label, Some(key), false);
        }
    }

    /// While automation is being written, the controller holds the parameter as a hand on the
    /// fader would; `controller_idle` lets go once it stops moving.
    fn ctl_hold(&mut self, track: Id, param: AutoParam) {
        if !self.automation_writing(track) {
            return;
        }
        let now = Instant::now();
        if let Some(h) = self.controls.holding.iter_mut().find(|h| h.0 == track && h.1.same(&param)) {
            h.2 = now;
            return;
        }
        self.automation_touch(track, param.clone(), true);
        self.controls.holding.push((track, param, now));
    }

    /// Moves a continuous target to `n` on its 0..1 travel, as the user would.
    fn ctl_set(&mut self, t: &CtlTarget, n: f32) {
        match t {
            CtlTarget::Volume { track } => {
                self.ctl_checkpoint(*track, "Volume", format!("track:{track}:volume"));
                self.ctl_hold(*track, AutoParam::Volume);
                let db = round_to(ctl::fader_to_db(n), 0.1);
                self.update_track_linked(*track, TrackPatch { volume_db: Some(db), ..Default::default() });
            }
            CtlTarget::Pan { track } => {
                self.ctl_checkpoint(*track, "Pan", format!("track:{track}:pan"));
                self.ctl_hold(*track, AutoParam::Pan);
                let pan = round_to(n * 2.0 - 1.0, 1.0 / 64.0);
                self.update_track_linked(*track, TrackPatch { pan: Some(pan), ..Default::default() });
            }
            CtlTarget::Send { track, slot } => {
                self.ctl_checkpoint(*track, "Send Level", format!("send:{track}:{slot}"));
                self.ctl_hold(*track, AutoParam::Send(*slot));
                self.set_send_level_linked(*track, *slot as usize, round_to(ctl::fader_to_db(n), 0.1));
            }
            CtlTarget::Plugin { track, uid, plugin, param } => self.ctl_set_plugin(*track, *uid, plugin, *param, n),
            CtlTarget::Eq { .. } | CtlTarget::EqOutput { .. } => {
                if let Some(p) = self.ctl_legacy_eq(t) {
                    self.ctl_set(&p, n);
                }
            }
            CtlTarget::MasterVolume => {
                self.ctl_checkpoint(MASTER_ID, "Stereo Out Volume", "master-volume".into());
                self.ctl_hold(MASTER_ID, AutoParam::Volume);
                self.set_master_volume_linked(round_to(ctl::fader_to_db(n), 0.1));
            }
            CtlTarget::VcaLevel { vca } => {
                // As a drag of the VCA fader: one undo step, not automated.
                self.checkpoint("VCA Level", Some(format!("vca:{vca}:level")), false);
                self.update_vca(*vca, VcaPatch { level_db: Some(round_to(ctl::fader_to_db(n), 0.1)), ..Default::default() });
            }
            CtlTarget::Mute { .. }
            | CtlTarget::Solo { .. }
            | CtlTarget::Arm { .. }
            | CtlTarget::Transport { .. }
            | CtlTarget::Command { .. }
            | CtlTarget::Monitor { .. }
            | CtlTarget::Bypass { .. }
            | CtlTarget::VcaMute { .. }
            | CtlTarget::Selected { .. } => {}
        }
    }

    fn ctl_set_plugin(&mut self, track: Id, uid: Id, plugin: &str, param: u32, n: f32) {
        let Some((slot, s)) = self.ctl_plugin(track, uid, plugin) else { return };
        let name = format!(
            "{}: {}",
            s.name,
            self.plugin_params(uid).into_iter().find(|p| p.id == param).map(|p| p.name).unwrap_or_else(|| format!("Parameter {param}"))
        );
        let auto = AutoParam::Plugin { uid, param, name };
        // Stereo Out's plug-ins aren't automated.
        let automated = track != MASTER_ID;
        if let Some(spec) = builtin::builtin_spec(plugin).and_then(|(_, specs)| specs.iter().find(|p| p.id == param).copied()) {
            // None: the instrument slot.
            let slot = slot.unwrap_or(usize::MAX);
            let pushed = !self.automation_writing(track) && self.checkpoint("Plug-in Parameter", Some(format!("fxparam:{uid}:{param}")), false);
            if automated {
                self.ctl_hold(track, auto);
            }
            if let Err(e) = self.set_builtin_param(track, slot, param, to_plain(&spec, n)) {
                log::warn!("controller assignment: {e}");
                if pushed {
                    self.abandon_checkpoint();
                }
            }
        } else if let Some(before) = self.controllers.get(&uid).and_then(|c| c.param_value(param)) {
            // A VST3 parameter: as if moved in the plug-in's editor, but without an undo step:
            // one comes only from the editor itself (`vst_undo.rs`), as a control surface's
            // moves aren't edits of the project a step should take back.
            if automated {
                self.ctl_hold(track, auto);
            }
            if let Some(c) = self.controllers.get(&uid) {
                c.edit_param(param, n as f64);
            }
            if automated {
                self.auto_plugin_moved(track, uid, param, before as f32, None);
            }
            self.dirty = true;
        }
    }

    /// Switches a switch or transport button: to the other state, or on or off (doing nothing
    /// when it already is).
    fn ctl_switch(&mut self, t: &CtlTarget, act: ButtonAct) {
        let Some(now) = self.ctl_value(t).map(|v| v >= 0.5) else { return };
        let on = match act {
            ButtonAct::Flip => !now,
            ButtonAct::Set(v) => v,
        };
        if on == now {
            return;
        }
        match t {
            CtlTarget::Mute { track } => self.update_track_linked(*track, TrackPatch { mute: Some(on), ..Default::default() }),
            CtlTarget::Solo { track } => self.update_track_linked(*track, TrackPatch { solo: Some(on), ..Default::default() }),
            CtlTarget::Arm { track } => self.update_track_linked(*track, TrackPatch { armed: Some(on), ..Default::default() }),
            CtlTarget::Monitor { track } => self.update_track_linked(*track, TrackPatch { monitor: Some(on), ..Default::default() }),
            CtlTarget::Bypass { track, slot } => {
                // As the slot's power button: an undo step.
                if !self.is_frozen(*track) {
                    self.checkpoint(if on { "Bypass Plug-in" } else { "Enable Plug-in" }, None, false);
                    self.set_bypass(*track, *slot as usize, on);
                }
            }
            // A performance switch, as on a track: no undo step.
            CtlTarget::VcaMute { vca } => self.update_vca(*vca, VcaPatch { mute: Some(on), ..Default::default() }),
            CtlTarget::Eq { field: EqField::On, .. } => {
                // A band's on switch: the insert's toggle.
                if let Some(p) = self.ctl_legacy_eq(t) {
                    self.ctl_set(&p, on as u8 as f32);
                }
            }
            CtlTarget::Transport { action: TransportAction::Play } => self.toggle_play(),
            CtlTarget::Transport { action: TransportAction::Record } => {
                if !on {
                    self.stop();
                } else if let Err(e) = self.record() {
                    self.warning = Some(e.to_string());
                }
            }
            CtlTarget::Transport { action: TransportAction::Cycle } => {
                let c = self.project.cycle.clone();
                if c.end > c.start || !on {
                    self.set_cycle(on, c.start, c.end);
                } else {
                    // No locators yet: four bars from the playhead's bar, as the Cycle button.
                    let map = self.tempo_map();
                    let bar = map.bar_at(self.position() as f64);
                    let (start, end) = (map.bar_start(bar).round() as i64, map.bar_start(bar + 4).round() as i64);
                    self.set_cycle(true, start, end);
                }
            }
            _ => {}
        }
    }

    // ---- incoming controller messages -----------------------------------------------------

    /// Plays a message as if it came from the hardware port `port` (for tests without MIDI
    /// hardware). `idle` handles controller assignments it triggers.
    pub fn inject_midi_from(&self, port: &str, msg: &[u8]) {
        self.midi.inject_from(port, msg);
    }

    /// Handles the controller messages the input threads took since the last call, and lets
    /// go of parameters controllers stopped moving. Returns whether the project changed (the
    /// interface needs a new snapshot).
    pub(super) fn controller_idle(&mut self) -> bool {
        let mut changed = false;
        // Continuous controllers: only the latest value of each matters in one batch.
        let mut moves: Vec<(usize, f32)> = Vec::new();
        // Relative encoders: their steps add up.
        let mut turns: Vec<(usize, i32)> = Vec::new();
        let now = Instant::now();
        for m in self.midi.take_controls() {
            let Some(raw) = ctl::parse(&m.data) else { continue };
            if self.controls.learn.is_some() {
                // What was learned doesn't act yet.
                if let Some(c) = self.controls.learner.feed(&m.port, raw, now) {
                    let (target, name) = self.controls.learn.take().unwrap();
                    self.ctl_learned(c, target, name);
                }
                continue;
            }
            let Controls { list, hires, .. } = &mut self.controls;
            let here = |a: &Assignment| a.controller.channel == raw.channel && *a.controller.port == *m.port;
            let pair = |n: u8| list.iter().any(|a| here(a) && a.controller.kind == CtlKind::Cc14 { number: n });
            let nrpn = list.iter().any(|a| here(a) && matches!(a.controller.kind, CtlKind::Nrpn { .. }));
            let Some(msg) = hires.read(&m.port, raw, pair, nrpn) else { continue };
            let Some(i) = ctl::find(list, &m.port, &msg) else { continue };
            self.feedback_heard(i, m.data, msg.pressed);
            let a = &self.controls.list[i];
            if self.controls.latch_probe.as_ref().is_some_and(|(c, _)| *c == a.controller) {
                // It sent its release: on while held.
                self.controls.latch_probe = None;
            }
            if a.is_button() {
                if let Some(act) = self.controls.follow[i].button(a.button_mode, &msg, a.latching) {
                    changed |= self.ctl_button(i, act, &msg);
                }
            } else if let Some(steps) = a.relative_steps(&msg) {
                match turns.iter_mut().find(|(j, _)| *j == i) {
                    Some(t) => t.1 += steps,
                    None => turns.push((i, steps)),
                }
            } else {
                match moves.iter_mut().find(|(j, _)| *j == i) {
                    Some(mv) => mv.1 = msg.value,
                    None => moves.push((i, msg.value)),
                }
            }
        }
        // A coarse part whose fine part didn't follow is learned as a 7-bit controller.
        if self.controls.learn.is_some() {
            if let Some(c) = self.controls.learner.poll(now) {
                let (target, name) = self.controls.learn.take().unwrap();
                self.ctl_learned(c, target, name);
            }
        }
        for (i, steps) in turns {
            changed |= self.ctl_turn(i, steps);
        }
        for (i, value) in moves {
            changed |= self.ctl_move(i, value);
        }
        self.ctl_latch_probe();
        self.ctl_release();
        changed
    }

    /// A button learnt a moment ago that hasn't sent its release latches by itself: from now
    /// on every message of it is a press.
    fn ctl_latch_probe(&mut self) {
        let Some((c, at)) = &self.controls.latch_probe else { return };
        if at.elapsed() < ctl::LATCH_WAIT {
            return;
        }
        let c = c.clone();
        self.controls.latch_probe = None;
        if let Some(a) = self.controls.list.iter_mut().find(|a| a.controller == c) {
            a.latching = true;
            self.ctl_list_changed(true);
        }
    }

    /// A button press or release on assignment `i`: a switch switches, a continuous parameter
    /// goes to the press value or the bottom of the range.
    fn ctl_button(&mut self, i: usize, act: ButtonAct, msg: &CtlMsg) -> bool {
        let a = self.controls.list[i].clone();
        if let CtlTarget::Command { id } = &a.target {
            // Fired on each press, and in Momentary on the release too (Show/Hide Mixer shows it
            // while held). The window runs it; the project hasn't changed yet.
            self.controls.commands.push(id.clone());
            return false;
        }
        let Some(target) = self.ctl_resolve(&a.target) else { return false };
        if self.ctl_value(&target).is_none() {
            return false;
        }
        if target.is_button() {
            self.ctl_switch(&target, act);
        } else {
            let want = self.controls.follow[i].button_value(act, a.press_value(msg), a.scale(0.0));
            self.ctl_set(&target, want);
            if let Some(v) = self.ctl_value(&target) {
                self.controls.follow[i].applied(v);
            }
        }
        true
    }

    fn ctl_move(&mut self, i: usize, value: f32) -> bool {
        let a = self.controls.list[i].clone();
        let Some(target) = self.ctl_resolve(&a.target) else { return false };
        let Some(current) = self.ctl_value(&target) else { return false };
        let want = a.scale(value);
        if !self.controls.follow[i].take(a.mode, want, current) {
            return false;
        }
        self.ctl_set(&target, want);
        if let Some(v) = self.ctl_value(&target) {
            self.controls.follow[i].applied(v);
        }
        true
    }

    /// A relative encoder on assignment `i` turned `steps`.
    fn ctl_turn(&mut self, i: usize, steps: i32) -> bool {
        let a = self.controls.list[i].clone();
        let Some(target) = self.ctl_resolve(&a.target) else { return false };
        let Some(current) = self.ctl_value(&target) else { return false };
        let Some(want) = self.controls.follow[i].relative(&a, steps, current) else { return false };
        self.ctl_set(&target, want);
        if let Some(v) = self.ctl_value(&target) {
            self.controls.follow[i].applied(v);
        }
        true
    }

    /// Lets go of parameters whose controllers have stopped (all of them once stopped).
    fn ctl_release(&mut self) {
        if self.controls.holding.is_empty() {
            return;
        }
        let playing = self.is_playing();
        let (done, keep): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.controls.holding).into_iter().partition(|h| !playing || h.2.elapsed().as_secs_f64() > RELEASE_SECS);
        self.controls.holding = keep;
        for (track, param, _) in done {
            self.automation_touch(track, param, false);
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
