//! An assignment: which controller (port, channel and message kind) drives which target, with its
//! range, takeover and button settings, as the Controller Assignments window edits it and the
//! assignments file saves it; and recognising a port again after it was plugged back in.

use serde::{Deserialize, Serialize};

use super::*;

/// The message a controller sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CtlKind {
    /// A 7-bit control change.
    Cc { number: u8 },
    /// Pitch bend (14 bits).
    PitchBend,
    /// A note, on any key: note on is the press (its velocity the value), note off (or note on
    /// at velocity 0) the release. Pads and button matrices send these.
    Note { number: u8 },
    /// A program change: a press with no release.
    Program { number: u8 },
    /// A 14-bit control change: CC `number` (0..=31) is the coarse part (MSB), CC `number` + 32
    /// the fine part (LSB).
    Cc14 { number: u8 },
    /// A non-registered parameter number (0..=16383), chosen with CC 99 and 98 and set with
    /// data entry, CC 6 (MSB) and 38 (LSB).
    Nrpn { number: u16 },
}

impl CtlKind {
    /// Notes and program changes are buttons whatever they drive: they press and release
    /// rather than move.
    pub fn is_button(&self) -> bool {
        matches!(self, CtlKind::Note { .. } | CtlKind::Program { .. })
    }

    /// Whether an assignment to this controller takes `raw`, a message as [`parse`] reads it,
    /// off the live MIDI path: its own messages, both control changes of a 14-bit pair, and on
    /// a channel with an NRPN assigned, the control changes NRPNs are made of.
    pub fn takes(&self, raw: CtlKind) -> bool {
        match (*self, raw) {
            (CtlKind::Cc14 { number }, CtlKind::Cc { number: n }) => n == number || n == number + 32,
            (CtlKind::Nrpn { .. }, CtlKind::Cc { number: n }) => is_nrpn_part(n),
            (own, raw) => own == raw,
        }
    }
}

/// The control changes NRPNs are sent with: data entry (6, 38), increment and decrement (96,
/// 97), and the NRPN and RPN numbers (98..=101).
pub(super) fn is_nrpn_part(n: u8) -> bool {
    matches!(n, 6 | 38 | 96..=101)
}

/// One physical control: where its messages come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Controller {
    /// The input port, as [`port_key`] names it.
    pub port: String,
    /// MIDI channel, 0..=15.
    pub channel: u8,
    pub kind: CtlKind,
}

impl Controller {
    pub fn matches(&self, port: &str, channel: u8, kind: CtlKind) -> bool {
        self.channel == channel && self.kind == kind && self.port == port
    }
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

/// A controller driving a target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub controller: Controller,
    pub target: CtlTarget,
    #[serde(default)]
    pub mode: Takeover,
    /// The stretch of the parameter's travel (0..1, on the control's own scale: the fader's
    /// taper for levels) the controller sweeps. `min` above `max` turns the controller round.
    #[serde(default)]
    pub min: f32,
    #[serde(default = "one")]
    pub max: f32,
    /// What a press and a release do, for a button (a note, a program change) or a switch.
    #[serde(default)]
    pub button_mode: ButtonMode,
    /// A note pressed on a continuous parameter goes to where its velocity falls in the range;
    /// false sends it to the top of the range (`max`) whatever the velocity.
    #[serde(default = "yes")]
    pub velocity: bool,
    /// The controller's button latches by itself: on (a value above 63, a note on) at one press
    /// and off at the next, rather than on while held. Every message is then a press.
    #[serde(default)]
    pub latching: bool,
    /// A control change read as a relative encoder's steps, or as a position.
    #[serde(default)]
    pub encoding: Encoding,
    /// A relative encoder's speed: at 1 a step moves the parameter 1/127 of the range.
    #[serde(default = "one")]
    pub sensitivity: f32,
    /// Send the target's value back to the controller (`feedback.rs`) when its input is paired
    /// with an output. On by default, and for assignments saved before there was feedback.
    #[serde(default = "yes")]
    pub feedback: bool,
}

impl Assignment {
    pub fn new(controller: Controller, target: CtlTarget) -> Assignment {
        // A pad on a fader sets a level, and a command runs: they fire on a press rather than
        // toggling.
        let fires = matches!(target, CtlTarget::Command { .. }) || controller.kind.is_button() && !target.is_button();
        let button_mode = if fires { ButtonMode::Trigger } else { ButtonMode::Toggle };
        Assignment {
            controller,
            target,
            mode: Takeover::Jump,
            min: 0.0,
            max: 1.0,
            button_mode,
            velocity: true,
            latching: false,
            encoding: Encoding::Absolute,
            sensitivity: 1.0,
            feedback: true,
        }
    }

    /// Whether the controller acts as a button here: a note or program change, or any
    /// controller on a switch.
    pub fn is_button(&self) -> bool {
        self.controller.kind.is_button() || self.target.is_button()
    }

    /// The steps a relative encoder turned, for an assignment that reads one; None otherwise.
    pub fn relative_steps(&self, msg: &CtlMsg) -> Option<i32> {
        if msg.step != 0 {
            return Some(msg.step as i32);
        }
        match msg.kind {
            CtlKind::Cc { .. } => self.encoding.steps((msg.value * 127.0).round() as u8),
            _ => None,
        }
    }

    /// Where a press puts a continuous parameter (0..1 on its travel): the velocity's place in
    /// the range, or the top of the range.
    pub fn press_value(&self, msg: &CtlMsg) -> f32 {
        let v = if self.velocity && matches!(msg.kind, CtlKind::Note { .. }) { msg.value } else { 1.0 };
        self.scale(v)
    }

    /// Where on the parameter's travel a controller value (0..1) lands.
    pub fn scale(&self, value: f32) -> f32 {
        let (lo, hi) = (self.min.clamp(0.0, 1.0), self.max.clamp(0.0, 1.0));
        lo + value.clamp(0.0, 1.0) * (hi - lo)
    }
}

/// `controller-assignments.json` in the config folder.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AssignmentFile {
    #[serde(default)]
    pub assignments: Vec<Assignment>,
}

/// A port's name without the client and port numbers ALSA appends ("nanoKONTROL2:nanoKONTROL2
/// MIDI 1 20:0"), which change when the device is plugged in again or in another order.
pub fn port_key(name: &str) -> &str {
    let trimmed = name.trim_end();
    if let Some((head, tail)) = trimmed.rsplit_once(' ') {
        if let Some((a, b)) = tail.split_once(':') {
            let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
            if digits(a) && digits(b) {
                return head.trim_end();
            }
        }
    }
    trimmed
}

/// Port names as keys ([`port_key`]), each once: Settings › MIDI's switched-off ports. Entries
/// saved before ports were switched off by key are full names, so they are read by their key too.
pub fn port_keys(names: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for n in names {
        let k = port_key(n);
        if !keys.iter().any(|x| x == k) {
            keys.push(k.to_string());
        }
    }
    keys
}

/// The assignment a message belongs to, if any.
pub fn find(list: &[Assignment], port: &str, msg: &CtlMsg) -> Option<usize> {
    list.iter().position(|a| a.controller.matches(port, msg.channel, msg.kind))
}
