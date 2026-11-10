//! Reading and setting what an assignment drives: finding its target in the project (the focused
//! plug-in, a legacy Channel EQ target), its name and value, and the edits a move or press makes.

use super::*;

impl Session {
    /// What a target stands for now: a Selected Track target, the selected track's control
    /// (None with no track selected, or no such control there); any other, itself.
    pub(crate) fn ctl_resolve(&self, t: &CtlTarget) -> Option<CtlTarget> {
        let CtlTarget::Selected { param } = t else { return Some(t.clone()) };
        let track = self.selected.filter(|id| self.project.track(*id).is_some())?;
        if let SelectedParam::PluginParam { index } = *param {
            let slot = self.ctl_focused_plugin(track)?;
            let p = self.ctl_param_id(slot.uid, index as usize)?;
            return Some(CtlTarget::Plugin { track, uid: slot.uid, plugin: slot.plugin_id.clone(), param: p });
        }
        param.on(track)
    }

    /// Whether a target is a switch, acting on a press rather than following the controller (see
    /// `CtlTarget::is_button`): a mute, a transport button and the like, or a plug-in parameter
    /// that is only on or off (a built-in's toggle, a VST3 parameter with one step). Pass a
    /// Selected Track target resolved (`ctl_resolve`) to ask about the control it stands for.
    pub(crate) fn ctl_is_switch(&self, t: &CtlTarget) -> bool {
        let CtlTarget::Plugin { track, uid, plugin, param } = t else { return t.is_button() };
        // A built-in's parameters are known from its id, even when it isn't in the project.
        if let Some((_, specs)) = crate::session::plugins::builtin::builtin_spec(plugin) {
            return specs.iter().any(|p| p.id == *param && p.kind == crate::fx::ParamKind::Toggle);
        }
        self.ctl_vst3_switch(*track, *uid, plugin, *param)
    }

    /// Whether an assignment's controller acts as a button: a note or program change, or any
    /// controller on a switch.
    pub(super) fn ctl_is_button(&self, a: &Assignment) -> bool {
        a.controller.kind.is_button() || self.ctl_is_switch(&a.target)
    }

    /// Whether a VST3 parameter has one step (off and on), as its controller describes it. Each
    /// answer is kept: it doesn't change, and feedback asks every idle tick while a plug-in
    /// describes every parameter to give one.
    fn ctl_vst3_switch(&self, track: Id, uid: Id, plugin: &str, param: u32) -> bool {
        if let Some((p, on_off)) = self.controls.vst3_switches.borrow().get(&(uid, param)) {
            if p == plugin {
                return *on_off;
            }
        }
        let Some(c) = self.controllers.get(&uid).filter(|_| self.ctl_plugin(track, uid, plugin).is_some()) else { return false };
        let params = c.parameters();
        // An unresponsive plug-in describes nothing: ask again later.
        if params.is_empty() {
            return false;
        }
        let on_off = params.iter().any(|p| p.id == param && p.steps == 1);
        self.controls.vst3_switches.borrow_mut().insert((uid, param), (plugin.to_string(), on_off));
        on_off
    }

    /// A plug-in's parameter id at `index` in its list. The lists are kept a moment, as
    /// feedback asks every idle tick and a VST3 plug-in describes every parameter to give one.
    pub(super) fn ctl_param_id(&self, uid: Id, index: usize) -> Option<u32> {
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
    pub(crate) fn ctl_focused_plugin(&self, track: Id) -> Option<&PluginSlot> {
        let t = self.project.track(track)?;
        let mut all = t.instrument.iter().chain(t.inserts.iter().flatten());
        let focused = self.controls.focused.and_then(|uid| all.find(|s| s.uid == uid));
        focused.or(t.instrument.as_ref()).or_else(|| t.inserts.iter().flatten().next())
    }

    /// A plug-in slot on a strip: its insert index (None for the instrument slot) and the slot.
    pub(super) fn ctl_slot(&self, track: Id, uid: Id) -> Option<(Option<usize>, &PluginSlot)> {
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
    pub(crate) fn ctl_drives_legacy_eq(&self, strip: Id) -> bool {
        self.controls.list.iter().any(|a| matches!(a.target, CtlTarget::Eq { track, .. } | CtlTarget::EqOutput { track } if track == strip))
    }

    /// Opens what an older project (or template) holds in an older form: each strip's fixed
    /// Channel EQ becomes an insert (`Project::upgrade_channel_eq`). Returns a warning naming
    /// the strips that had no Audio FX slot free for it.
    pub(crate) fn upgrade_document(&self, p: &mut Project) -> Option<String> {
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
    pub(crate) fn ctl_legacy_eq(&self, t: &CtlTarget) -> Option<CtlTarget> {
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
    pub(super) fn ctl_target_name(&self, t: &CtlTarget) -> Option<String> {
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
    pub(crate) fn ctl_value(&self, t: &CtlTarget) -> Option<f32> {
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
    pub(super) fn ctl_hold(&mut self, track: Id, param: AutoParam) {
        if !self.automation_writing(track) {
            return;
        }
        // A control surface's touch-sensitive fader holds it while touched instead.
        if self.surface_holds(track, &param) {
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
    pub(crate) fn ctl_set(&mut self, t: &CtlTarget, n: f32) {
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
        if let Some(spec) = crate::session::plugins::builtin::builtin_spec(plugin).and_then(|(_, specs)| specs.iter().find(|p| p.id == param).copied()) {
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
            // one comes only from the editor itself (`session/plugins/editor_undo/`), as a control surface's
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
    pub(crate) fn ctl_switch(&mut self, t: &CtlTarget, act: ButtonAct) {
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
            // A plug-in's on/off parameter (or, in an older assignment, a Channel EQ band's):
            // set as a move to the top or bottom of its travel would.
            CtlTarget::Plugin { .. } | CtlTarget::Eq { .. } => self.ctl_set(t, on as u8 as f32),
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
}
