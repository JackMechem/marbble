//! Incoming controller messages, applied on the main thread at `idle`: moves, turns and button
//! presses, Latch's probe, and letting go of the parameters a controller held.

use super::*;

impl Session {
    /// Plays a message as if it came from the hardware port `port` (for tests without MIDI
    /// hardware). `idle` handles controller assignments it triggers.
    pub fn inject_midi_from(&self, port: &str, msg: &[u8]) {
        self.midi.inject_from(port, msg);
    }

    /// Handles the controller messages the input threads took since the last call, and lets
    /// go of parameters controllers stopped moving. Returns whether the project changed (the
    /// interface needs a new snapshot).
    pub(crate) fn controller_idle(&mut self) -> bool {
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
            let button = self.ctl_is_button(a);
            if self.controls.latch_probe.as_ref().is_some_and(|(c, _)| *c == a.controller) {
                // It sent its release: on while held.
                self.controls.latch_probe = None;
            }
            if button {
                // An NRPN's increment and decrement step a continuous parameter; a switch has
                // no steps.
                if msg.step != 0 {
                    continue;
                }
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
        if self.ctl_is_switch(&target) {
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
