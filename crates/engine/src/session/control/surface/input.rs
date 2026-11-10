//! What the surfaces did: their faders, V-Pots, jog wheel and buttons, applied through the
//! controller assignments' edit paths (see `mod.rs`).

use super::*;

impl Session {
    /// One thing unit `ui` did. Returns whether the project changed.
    pub(super) fn surface_input(&mut self, ui: usize, ev: Input, now: Instant) -> bool {
        let u = &mut self.surfaces.units[ui];
        match ev {
            Input::Hello { serial, challenge } => {
                // It started (again): answer its challenge and send it everything.
                self.surfaces.out.push((u.cfg.output.clone(), mcu::connection_reply(u.device(), serial, challenge)));
                u.online = false;
                u.come_online();
                return false;
            }
            Input::Alive => {
                u.come_online();
                return false;
            }
            _ => u.come_online(),
        }
        let (offset, has_master) = (u.cfg.offset as usize, u.has_master());
        let tracks = self.surface_tracks();
        let bank = self.surface_bank(tracks.len());
        let track_at = |strip: u8| tracks.get(bank + offset + strip as usize).copied();
        match ev {
            Input::Fader { strip, value } => {
                if strip > MASTER {
                    return false;
                }
                let target =
                    if strip == MASTER { has_master.then_some(CtlTarget::MasterVolume) } else { track_at(strip).map(|track| CtlTarget::Volume { track }) };
                let u = &mut self.surfaces.units[ui];
                let shown = u.fader(strip, value);
                u.faders[strip as usize].moved(shown, now);
                let Some(target) = target else { return false };
                self.surfaces.fader_moved.insert(target.track().unwrap_or(MASTER_ID), now);
                self.ctl_set(&target, value);
                true
            }
            Input::Touch { strip, on } => {
                let track = if strip == MASTER { has_master.then_some(MASTER_ID) } else { track_at(strip) };
                self.surface_touch(ui, strip, on, track, now);
                false
            }
            Input::VPot { strip, steps } => {
                let place = offset + strip as usize;
                let Some(target) = self.surface_pot(place, track_at(strip)).and_then(|t| self.ctl_resolve(&t)) else { return false };
                let Some(current) = self.ctl_value(&target) else { return false };
                // A stepped plug-in parameter moves a step at a time.
                let step = match (&target, self.surfaces.pot) {
                    (CtlTarget::Plugin { param, .. }, PotMode::Plugin) => {
                        let steps = self.surface_params().and_then(|ps| ps.iter().find(|p| p.id == *param).map(|p| p.steps)).unwrap_or(0);
                        if steps > 0 {
                            1.0 / steps as f32
                        } else {
                            POT_STEP
                        }
                    }
                    _ => POT_STEP,
                };
                self.surfaces.pot_moved.insert(place, now);
                self.ctl_set(&target, (current + steps as f32 * step).clamp(0.0, 1.0));
                true
            }
            Input::Jog { steps } => {
                self.surface_jog(steps);
                true
            }
            Input::Button { button, down } => {
                let track = button.strip().and_then(track_at);
                self.surface_button(ui, button, down, track, offset)
            }
            Input::Hello { .. } | Input::Alive => false,
        }
    }

    /// A fader was touched or let go. While touched it holds its track's volume (an automation
    /// touch while automation is being written) and nothing moves it.
    fn surface_touch(&mut self, ui: usize, strip: u8, on: bool, track: Option<Id>, now: Instant) {
        let s = strip as usize;
        if s > STRIPS {
            return;
        }
        let u = &mut self.surfaces.units[ui];
        u.touched[s] = on;
        let held = std::mem::take(&mut u.holding[s]);
        if on {
            u.holding[s] = track;
        } else {
            // Let go: it is sent where its fader is, whatever it last sent.
            u.faders[s].button(true, now);
        }
        if let Some(id) = held {
            self.surface_release(id);
        }
        if let Some(id) = track {
            self.surfaces.fader_moved.insert(id, now);
            if on {
                self.surfaces.touches.push((id, false));
                self.surface_holds(id, &AutoParam::Volume);
            }
        }
    }

    /// A touched fader on `track` was let go: so is its automation parameter.
    pub(super) fn surface_release(&mut self, track: Id) {
        let Some(i) = self.surfaces.touches.iter().position(|t| t.0 == track) else { return };
        let (_, began) = self.surfaces.touches.remove(i);
        if began && !self.surfaces.touches.iter().any(|t| t.0 == track) {
            self.automation_touch(track, AutoParam::Volume, false);
        }
    }

    /// Whether a touched fader holds `param` of `track`; begins its automation touch when
    /// automation is being written (`ctl_hold` asks, and holds it for half a second otherwise).
    pub(crate) fn surface_holds(&mut self, track: Id, param: &AutoParam) -> bool {
        if !matches!(param, AutoParam::Volume) {
            return false;
        }
        let Some(i) = self.surfaces.touches.iter().position(|t| t.0 == track) else { return false };
        if !self.surfaces.touches[i].1 && self.automation_writing(track) {
            self.surfaces.touches[i].1 = true;
            self.automation_touch(track, AutoParam::Volume, true);
        }
        true
    }

    /// The jog wheel moves the playhead a beat a step (a sixteenth of one with Scrub on).
    fn surface_jog(&mut self, steps: i32) {
        let map = self.tempo_map();
        let pos = self.position() as f64;
        let beat = 4.0 / map.sig_at(pos).1.max(1) as f64;
        let step = if self.surfaces.scrub { beat / 4.0 } else { beat };
        let q = (map.beats_at(pos) + steps as f64 * step).max(0.0);
        self.locate(map.sample_at(q).round() as i64);
    }

    /// Moves the bank (or Plug-in mode's parameters) by `by` strips.
    fn surface_bank_by(&mut self, by: i64) {
        let strips = sf::strips_of(&self.prefs.midi.surfaces);
        if self.surfaces.pot == PotMode::Plugin {
            let n = self.surface_params().map(|p| p.len()).unwrap_or(0);
            self.surfaces.page = sf::clamp_bank(self.surfaces.page as i64 + by, n, strips);
        } else {
            let n = self.surface_tracks().len();
            let bank = self.surface_bank(n) as i64;
            self.surfaces.bank = sf::clamp_bank(bank + by, n, strips);
        }
    }

    /// The selected track's automation mode, set as the track header's menu does.
    fn surface_auto_mode(&mut self, mode: impl Fn(AutoMode) -> AutoMode) {
        let Some(track) = self.selected.filter(|id| self.project.track(*id).is_some()) else { return };
        let Some(now) = self.auto_mode_of(track) else { return };
        let to = mode(now);
        if to != now {
            self.checkpoint("Automation Mode", None, false);
            self.set_auto_mode(track, to);
        }
    }

    /// A button pressed (`down`) or let go on unit `ui`; `track` is a strip button's track.
    /// Returns whether the project changed.
    fn surface_button(&mut self, ui: usize, b: Button, down: bool, track: Option<Id>, offset: usize) -> bool {
        let sfs = &mut self.surfaces;
        if !down {
            sfs.held.retain(|h| *h != b);
            // Marker on its own makes a marker; with Rewind or Forward it went to one.
            if b == Button::Marker && !std::mem::take(&mut sfs.marker_used) {
                self.ctl_command("create-marker");
            }
            return false;
        }
        if !sfs.held.contains(&b) {
            sfs.held.push(b);
        }
        let shift = sfs.held.contains(&Button::Shift);
        let marker = sfs.held.contains(&Button::Marker);
        let (zoom, nudge) = (sfs.zoom, sfs.nudge);
        let strips = sf::strips_of(&self.prefs.midi.surfaces) as i64;
        let switch = |t: Option<CtlTarget>| t.map(|t| (t, ButtonAct::Flip));
        let flip = match b {
            Button::Rec(_) => switch(track.map(|track| CtlTarget::Arm { track })),
            Button::Solo(_) => switch(track.map(|track| CtlTarget::Solo { track })),
            Button::Mute(_) => switch(track.map(|track| CtlTarget::Mute { track })),
            Button::Cycle => switch(Some(CtlTarget::Transport { action: TransportAction::Cycle })),
            Button::Record => switch(Some(CtlTarget::Transport { action: TransportAction::Record })),
            _ => None,
        };
        if let Some((t, act)) = flip {
            self.ctl_switch(&t, act);
            return true;
        }
        let command = match b {
            Button::F(n) => {
                let id = self.surfaces.units[ui].cfg.functions.get(n as usize).filter(|id| !id.is_empty()).cloned();
                if let Some(id) = id {
                    self.ctl_command(&id);
                }
                return false;
            }
            Button::Save => "save",
            Button::Undo if shift => "redo",
            Button::Undo => "undo",
            Button::Drop => "autopunch",
            Button::Replace => "replace-mode",
            Button::ToStart => "to-start",
            Button::Rewind | Button::Forward if marker => {
                self.surfaces.marker_used = true;
                if b == Button::Rewind {
                    "previous-marker"
                } else {
                    "next-marker"
                }
            }
            Button::Rewind | Button::Left if nudge => "nudge-left",
            Button::Forward | Button::Right if nudge => "nudge-right",
            Button::Left if zoom => "zoom-out",
            Button::Right if zoom => "zoom-in",
            Button::Rewind | Button::Left => "rewind",
            Button::Forward | Button::Right => "forward",
            Button::Up if zoom => "shorter-tracks",
            Button::Down if zoom => "taller-tracks",
            Button::Up => "select-previous",
            Button::Down => "select-next",
            _ => "",
        };
        if !command.is_empty() {
            self.ctl_command(command);
            return false;
        }
        let sfs = &mut self.surfaces;
        match b {
            Button::Select(_) => match track {
                Some(t) => self.select_track(Some(t)),
                None => return false,
            },
            Button::VPot(s) => {
                // Back to its default: pan centre, a send at 0 dB, a plug-in's own default.
                let place = offset + s as usize;
                let Some(target) = self.surface_pot(place, track).and_then(|t| self.ctl_resolve(&t)) else { return false };
                let default = match &target {
                    CtlTarget::Pan { .. } => 0.5,
                    CtlTarget::Send { .. } => ctl::db_to_fader(0.0),
                    CtlTarget::Plugin { param, .. } => {
                        let Some(d) = self.surface_params().and_then(|ps| ps.iter().find(|p| p.id == *param).map(|p| p.default as f32)) else { return false };
                        d
                    }
                    _ => return false,
                };
                self.surfaces.pot_moved.insert(place, Instant::now());
                self.ctl_set(&target, default);
            }
            Button::AssignPan => sfs.pot = PotMode::Pan,
            Button::AssignSend => {
                sfs.pot = match sfs.pot {
                    PotMode::Send(s) => PotMode::Send((s + 1) % SEND_SLOTS as u8),
                    _ => PotMode::Send(0),
                }
            }
            Button::AssignPlugin => {
                if sfs.pot != PotMode::Plugin {
                    sfs.pot = PotMode::Plugin;
                    sfs.page = 0;
                }
            }
            Button::BankLeft => self.surface_bank_by(-strips),
            Button::BankRight => self.surface_bank_by(strips),
            Button::ChannelLeft => self.surface_bank_by(-1),
            Button::ChannelRight => self.surface_bank_by(1),
            Button::NameValue => sfs.names = !sfs.names,
            Button::SmpteBeats => sfs.time = if sfs.time == TimeMode::Bars { TimeMode::Time } else { TimeMode::Bars },
            Button::Marker => sfs.marker_used = false,
            Button::Nudge => {
                sfs.nudge = !sfs.nudge;
                sfs.zoom &= !sfs.nudge;
            }
            Button::Zoom => {
                sfs.zoom = !sfs.zoom;
                sfs.nudge &= !sfs.zoom;
            }
            Button::Scrub => sfs.scrub = !sfs.scrub,
            Button::Read => self.surface_auto_mode(|m| if m == AutoMode::Read { AutoMode::Off } else { AutoMode::Read }),
            Button::Write => self.surface_auto_mode(|_| AutoMode::Write),
            Button::Touch => self.surface_auto_mode(|_| AutoMode::Touch),
            Button::Latch => self.surface_auto_mode(|_| AutoMode::Latch),
            Button::Click => {
                let on = !self.project.metronome;
                self.set_metronome(on);
            }
            Button::SoloClear => {
                let soloed: Vec<Id> = self.project.tracks.iter().filter(|t| t.solo).map(|t| t.id).collect();
                for id in soloed {
                    self.update_track(id, TrackPatch { solo: Some(false), ..Default::default() });
                }
            }
            Button::Stop => self.stop(),
            Button::Play => self.play(),
            _ => return false,
        }
        true
    }
}
