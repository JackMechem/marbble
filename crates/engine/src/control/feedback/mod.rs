//! Controller feedback, as Logic sends it to a control surface: each assignment's target value
//! goes back to the controller it was learnt from, so button lights and motor faders show the
//! DAW's state after a mouse move, an undo, automation or opening a project.
//!
//! This module is the pure part: the message that shows a value on a controller (the inverse of
//! how its moves are applied), which output port a controller's input is paired with, and the
//! per-controller [`Throttle`] that limits the rate and keeps quiet while the controller itself
//! is moving. The session reads the values and decides what to send on the main thread
//! (`session/control/feedback.rs`); `midi/output.rs` sends it from a thread of its own.

use std::time::{Duration, Instant};

use crate::control::controllers::{Assignment, CtlKind, Encoding};

/// The least time between two messages to one controller: at most 40 a second (the 30 ms idle
/// tick makes it about 33), always with the latest value.
pub const MIN_GAP: Duration = Duration::from_millis(25);

/// How long after a controller's last move feedback to it waits, so a motor fader isn't driven
/// against the hand moving it. The same half second after which a controller lets go of its
/// parameter (`session/control/controllers/`).
pub const QUIET: Duration = Duration::from_millis(500);

/// The messages that show one value on a controller (one, or two for a 14-bit controller, four
/// for an NRPN).
pub type Msgs = Vec<[u8; 3]>;

/// What a controller should show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown {
    /// A switch (Mute, Solo, Record, a transport button): full on or off, for a button's light.
    Switch(bool),
    /// A position on the target's own 0..1 travel (the fader's taper for levels).
    Travel(f32),
}

/// The controller value (0..1) that puts the parameter at `travel`: the inverse of
/// [`Assignment::scale`], so a controller whose range is a stretch of the travel shows where the
/// parameter is inside it (pinned to an end outside it).
pub fn unscale(a: &Assignment, travel: f32) -> f32 {
    let (lo, hi) = (a.min.clamp(0.0, 1.0), a.max.clamp(0.0, 1.0));
    if (hi - lo).abs() < 1e-6 {
        return 0.0;
    }
    ((travel - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// The messages that show `value` on the assignment's controller, as the same kind of message
/// the controller sends (two for a 14-bit controller, four for an NRPN). None for a controller
/// that has nothing to show: a program change, or a relative encoder, whose value isn't a
/// position.
pub fn message(a: &Assignment, value: Shown) -> Option<Msgs> {
    if a.encoding != Encoding::Absolute {
        return None;
    }
    let v = match value {
        Shown::Switch(on) => on as u8 as f32,
        Shown::Travel(t) => unscale(a, t),
    };
    encode(a.controller.kind, a.controller.channel, v)
}

/// A controller value (0..1) as that controller's messages. A new kind of controller needs its
/// arm here.
pub fn encode(kind: CtlKind, channel: u8, v: f32) -> Option<Msgs> {
    let ch = channel & 0x0F;
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let fine = (v * 16383.0).round() as u16;
    let (msb, lsb) = ((fine >> 7) as u8, (fine & 0x7F) as u8);
    let one = |m: [u8; 3]| Some(vec![m]);
    match kind {
        CtlKind::Cc { number } => one([0xB0 | ch, number & 0x7F, (v * 127.0).round() as u8]),
        CtlKind::PitchBend => one([0xE0 | ch, lsb, msb]),
        // The coarse part, then the fine part, as the controller sends them.
        CtlKind::Cc14 { number } => Some(vec![[0xB0 | ch, number & 0x1F, msb], [0xB0 | ch, (number & 0x1F) + 32, lsb]]),
        // The number (CC 99, 98), then the value by data entry (CC 6, 38).
        CtlKind::Nrpn { number } => {
            Some(vec![[0xB0 | ch, 99, (number >> 7) as u8 & 0x7F], [0xB0 | ch, 98, (number & 0x7F) as u8], [0xB0 | ch, 6, msb], [0xB0 | ch, 38, lsb]])
        }
        // A pad's light: a note on, velocity 127 for on and 0 for off (a level in between for a
        // pad on a fader, which pads with colours or brightness show).
        CtlKind::Note { number } => one([0x90 | ch, number & 0x7F, (v * 127.0).round() as u8]),
        // A program change has no state to show.
        CtlKind::Program { .. } => None,
    }
}

/// A message as a controller shows it: a note off is a note on with velocity 0, which is how
/// feedback switches a note's light off.
fn canonical(d: [u8; 3]) -> [u8; 3] {
    if d[0] & 0xF0 == 0x80 {
        [0x90 | (d[0] & 0x0F), d[1], 0]
    } else {
        d
    }
}

/// What one controller shows and when it was last sent to or moved.
#[derive(Debug, Clone, Default)]
pub struct Throttle {
    /// The last message sent to the controller or received from it: what it shows now. None
    /// when that isn't known (a new assignment, an output that was just connected).
    shown: Option<Msgs>,
    sent_at: Option<Instant>,
    /// Feedback waits until then: the controller is being moved.
    quiet_until: Option<Instant>,
}

impl Throttle {
    /// The controller (a fader or knob) sent `data` at `now`: it shows that, and is being moved
    /// for [`QUIET`]. Buttons on switches go through [`Throttle::button`] instead.
    pub fn heard(&mut self, data: [u8; 3], now: Instant) {
        self.moved(vec![canonical(data)], now);
    }

    /// The controller moved to where `shown` puts it (a control surface's fader, whose position
    /// may take two messages), at `now`: [`Throttle::heard`].
    pub fn moved(&mut self, shown: Msgs, now: Instant) {
        self.shown = Some(shown);
        self.quiet_until = Some(now + QUIET);
    }

    /// The controller's button on a switch was pressed (`released`: let go) at `now`. A button's
    /// light shows what the host last sent it, not its own presses, so what it shows is
    /// unknown until then: once the button is let go, the switch's state is sent even when it
    /// is what the button itself sent (a pad's note off after the press that switched Mute off).
    pub fn button(&mut self, released: bool, now: Instant) {
        self.shown = None;
        self.quiet_until = Some(if released { now } else { now + QUIET });
    }

    /// The message to send at `now` so the controller shows `want`, if any: none when it shows
    /// that already, while it is being moved, or within [`MIN_GAP`] of the last message (a
    /// later call sends the value then current, so the last one always goes).
    pub fn due(&mut self, want: Msgs, now: Instant) -> Option<Msgs> {
        if self.shown.as_ref() == Some(&want)
            || self.quiet_until.is_some_and(|t| now < t)
            || self.sent_at.is_some_and(|t| now.saturating_duration_since(t) < MIN_GAP)
        {
            return None;
        }
        self.shown = Some(want.clone());
        self.sent_at = Some(now);
        Some(want)
    }

    /// What the controller shows is unknown again (its output was connected anew): the next
    /// [`Throttle::due`] sends the value whatever it is.
    pub fn forget(&mut self) {
        self.shown = None;
    }
}

/// The device a port belongs to: ALSA's client name ("nanoKONTROL2" of "nanoKONTROL2:nanoKONTROL2
/// MIDI 1"), or the name inside Windows' "MIDIIN2 (…)" and "MIDIOUT2 (…)". `key` is a port name as
/// `controllers::port_key` gives it.
pub fn device_name(key: &str) -> &str {
    if let Some((client, _)) = key.split_once(':') {
        return client;
    }
    for prefix in ["MIDIIN", "MIDIOUT"] {
        if let Some(rest) = key.strip_prefix(prefix) {
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            if let Some(inner) = rest.strip_prefix(" (").and_then(|r| r.strip_suffix(')')) {
                return inner;
            }
        }
    }
    key
}

/// The output paired with the input `input` when none is chosen in Settings › MIDI: the output
/// port of the same name (on Linux a device's input and output are one ALSA port), or else the
/// only output of the same device. `outputs` are the connected outputs' keys.
pub fn auto_output<'a>(input: &str, outputs: &'a [String]) -> Option<&'a str> {
    if let Some(o) = outputs.iter().find(|o| *o == input) {
        return Some(o);
    }
    let dev = device_name(input);
    let mut same = outputs.iter().filter(|o| device_name(o) == dev);
    match (same.next(), same.next()) {
        (Some(o), None) => Some(o),
        _ => None,
    }
}

/// The output feedback for controllers on `input` goes to: `chosen` in Settings › MIDI (an
/// empty name for none), else [`auto_output`]. None when that output isn't connected.
pub fn paired_output<'a>(input: &str, chosen: Option<&str>, outputs: &'a [String]) -> Option<&'a str> {
    match chosen {
        Some(c) => outputs.iter().find(|o| !c.is_empty() && *o == c).map(|o| o.as_str()),
        None => auto_output(input, outputs),
    }
}

#[cfg(test)]
mod tests;
