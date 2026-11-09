//! Controller assignments, as Logic's Control Surfaces › Learn Assignment and the Controller
//! Assignments window: a knob, fader, button or pad on a MIDI controller drives a mixer
//! control, a plug-in parameter, a transport button or any key command.
//!
//! This module is the pure part: reading controller messages, recognising a port again after
//! it was plugged back in, scaling a controller's travel onto a parameter's range (with the
//! fader's taper for levels), the Jump and Pickup takeover modes, and what a button press and
//! release do (Toggle, Momentary, Trigger). The session applies the
//! results (`session/controllers.rs`); `midi_io.rs` takes assigned controllers off the live
//! MIDI path.

use serde::{Deserialize, Serialize};

use crate::model::Id;

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
}

impl CtlKind {
    /// Notes and program changes are buttons whatever they drive: they press and release
    /// rather than move.
    pub fn is_button(&self) -> bool {
        matches!(self, CtlKind::Note { .. } | CtlKind::Program { .. })
    }
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

/// A transport button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportAction {
    /// Play / Stop.
    Play,
    Record,
    /// Cycle on or off.
    Cycle,
}

/// A band setting of the fixed Channel EQ (see `CtlTarget::Eq`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqField {
    On,
    Freq,
    Gain,
    Q,
}

/// What a controller drives. Tracks and plug-ins are referred to by their ids in the project,
/// so an assignment whose track or plug-in isn't in the open project does nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CtlTarget {
    Volume {
        track: Id,
    },
    Pan {
        track: Id,
    },
    Mute {
        track: Id,
    },
    Solo {
        track: Id,
    },
    /// Record enable.
    Arm {
        track: Id,
    },
    Send {
        track: Id,
        slot: u8,
    },
    /// A built-in effect's or a VST3 plug-in's parameter (`track` is MASTER_ID for Stereo
    /// Out). `plugin` is the slot's plug-in id: in another project the same instance id may
    /// hold a different plug-in, and then the assignment doesn't apply.
    Plugin {
        track: Id,
        uid: Id,
        plugin: String,
        param: u32,
    },
    /// A band setting of the fixed Channel EQ strips had before it became an insert (`track` is
    /// MASTER_ID for Stereo Out). Kept so assignments learnt then still work: it drives that
    /// setting of the strip's first Channel EQ insert. New assignments name the insert's
    /// parameter (`Plugin`).
    Eq {
        track: Id,
        band: u8,
        field: EqField,
    },
    /// The fixed Channel EQ's output gain (see `Eq`).
    EqOutput {
        track: Id,
    },
    /// Stereo Out's fader.
    MasterVolume,
    Transport {
        action: TransportAction,
    },
    /// A key command (an id of src/keymap.ts's KEY_COMMANDS, or of a menu item with a key):
    /// the session passes it to the main window, which runs it as if its key was pressed.
    Command {
        id: String,
    },
}

impl CtlTarget {
    /// Switches act on a press (a value above 63 after one at or below it) and ignore the
    /// release, so momentary buttons toggle them, as in Logic.
    pub fn is_button(&self) -> bool {
        matches!(
            self,
            CtlTarget::Mute { .. }
                | CtlTarget::Solo { .. }
                | CtlTarget::Arm { .. }
                | CtlTarget::Transport { .. }
                | CtlTarget::Eq { field: EqField::On, .. }
                | CtlTarget::Command { .. }
        )
    }

    /// The track (or MASTER_ID) the target belongs to, if any.
    pub fn track(&self) -> Option<Id> {
        match self {
            CtlTarget::Volume { track }
            | CtlTarget::Pan { track }
            | CtlTarget::Mute { track }
            | CtlTarget::Solo { track }
            | CtlTarget::Arm { track }
            | CtlTarget::Send { track, .. }
            | CtlTarget::Plugin { track, .. }
            | CtlTarget::Eq { track, .. }
            | CtlTarget::EqOutput { track } => Some(*track),
            CtlTarget::MasterVolume => Some(crate::model::MASTER_ID),
            CtlTarget::Transport { .. } | CtlTarget::Command { .. } => None,
        }
    }
}

/// How a controller takes over a parameter that is somewhere else than the controller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Takeover {
    /// The parameter jumps to the controller's value at once.
    #[default]
    Jump,
    /// Moves are ignored until the controller reaches (or passes) the parameter's value.
    Pickup,
}

/// What pressing and releasing a button does: one on a controller (a note, a program change, or
/// a control change used as a switch) or one driving a switch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonMode {
    /// A press flips the switch; the release does nothing. On a continuous parameter a press
    /// goes to the press value and the next one back to the bottom of the range.
    #[default]
    Toggle,
    /// On while held: the press switches on, the release off (a continuous parameter goes to
    /// the press value and back to the bottom of the range).
    Momentary,
    /// A press fires once and the release does nothing: a switch is switched on, a continuous
    /// parameter goes to the press value.
    Trigger,
}

/// How long after a learnt press a button has to send its release to be taken for one that is
/// on while held; one that doesn't is latching (`Assignment::latching`).
pub const LATCH_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// What one button message does, from [`Follow::button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAct {
    /// Switch it to the other state.
    Flip,
    /// Switch it on (true) or off.
    Set(bool),
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
        Assignment { controller, target, mode: Takeover::Jump, min: 0.0, max: 1.0, button_mode, velocity: true, latching: false, feedback: true }
    }

    /// Whether the controller acts as a button here: a note or program change, or any
    /// controller on a switch.
    pub fn is_button(&self) -> bool {
        self.controller.kind.is_button() || self.target.is_button()
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

/// A controller message: its channel, which controller, the value scaled to 0..1, and whether
/// it counts as a button press (above 63, or above the centre for pitch bend; a note on; any
/// program change).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CtlMsg {
    pub channel: u8,
    pub kind: CtlKind,
    pub value: f32,
    pub pressed: bool,
}

/// Reads a control change, pitch bend, note or program change message; anything else is None.
pub fn parse(data: &[u8]) -> Option<CtlMsg> {
    let (&status, rest) = data.split_first()?;
    let channel = status & 0x0F;
    match (status & 0xF0, rest) {
        (0x90, [number, velocity, ..]) if *velocity & 0x7F > 0 => {
            Some(CtlMsg { channel, kind: CtlKind::Note { number: number & 0x7F }, value: (velocity & 0x7F) as f32 / 127.0, pressed: true })
        }
        (0x80 | 0x90, [number, ..]) => Some(CtlMsg { channel, kind: CtlKind::Note { number: number & 0x7F }, value: 0.0, pressed: false }),
        (0xC0, [number, ..]) => Some(CtlMsg { channel, kind: CtlKind::Program { number: number & 0x7F }, value: 1.0, pressed: true }),
        (0xB0, [number, value, ..]) if *number < 120 => {
            let v = value & 0x7F;
            Some(CtlMsg { channel, kind: CtlKind::Cc { number: *number }, value: v as f32 / 127.0, pressed: v > 63 })
        }
        (0xE0, [lsb, msb, ..]) => {
            let v = ((msb & 0x7F) as u16) << 7 | (lsb & 0x7F) as u16;
            Some(CtlMsg { channel, kind: CtlKind::PitchBend, value: v as f32 / 16383.0, pressed: v > 8192 })
        }
        _ => None,
    }
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

/// The fader's travel (0..1) for a level in dB, roughly Logic's taper: +6 dB at the top, -∞ at
/// the bottom. The same curve as `dbToFader` in src/time.ts, so a controller moves the fader
/// evenly along its length.
pub fn db_to_fader(db: f32) -> f32 {
    if db <= -96.0 {
        0.0
    } else if db >= 6.0 {
        1.0
    } else if db >= -12.0 {
        0.62 + (db + 12.0) / 18.0 * 0.38
    } else if db >= -40.0 {
        0.22 + (db + 40.0) / 28.0 * 0.4
    } else {
        (db + 96.0) / 56.0 * 0.22
    }
}

/// The inverse of [`db_to_fader`] (`faderToDb` in src/time.ts).
pub fn fader_to_db(f: f32) -> f32 {
    if f <= 0.001 {
        -96.0
    } else if f >= 0.62 {
        (-12.0 + (f.min(1.0) - 0.62) / 0.38 * 18.0).min(6.0)
    } else if f >= 0.22 {
        -40.0 + (f - 0.22) / 0.4 * 28.0
    } else {
        -96.0 + f / 0.22 * 56.0
    }
}

/// How close (on the 0..1 travel) a Pickup controller must come to catch the parameter, and
/// how far the parameter may drift from where the controller left it before it has to be
/// caught again: a little over one 7-bit step.
const CATCH: f32 = 1.5 / 127.0;

/// What one assignment remembers between messages.
#[derive(Debug, Clone, Default)]
pub struct Follow {
    /// The position the controller last asked for.
    last: Option<f32>,
    /// Pickup: the controller has caught the parameter.
    engaged: bool,
    /// Where the parameter ended up after the controller last moved it.
    set: Option<f32>,
    /// A button is held down.
    down: bool,
    /// A button toggling a continuous parameter left it at its press value.
    lit: bool,
}

impl Follow {
    /// Whether a move to `want` should reach a parameter now at `current` (both 0..1 on the
    /// parameter's travel). Jump always moves it. Pickup waits until the controller comes
    /// within a step of the parameter or crosses it, and lets go again when something else
    /// (the mouse, automation, an undo) moves the parameter away from where it left it.
    pub fn take(&mut self, mode: Takeover, want: f32, current: f32) -> bool {
        if self.set.is_some_and(|s| (s - current).abs() > CATCH) {
            self.engaged = false;
        }
        let prev = self.last.replace(want);
        let ok = match mode {
            Takeover::Jump => true,
            Takeover::Pickup => self.engaged || (want - current).abs() <= CATCH || prev.is_some_and(|p| (p - current) * (want - current) <= 0.0),
        };
        self.engaged = ok;
        if !ok {
            self.set = None;
        }
        ok
    }

    /// Where the parameter ended up after a move [`Follow::take`] let through (a stepped
    /// parameter lands on its nearest step).
    pub fn applied(&mut self, value: f32) {
        self.set = Some(value);
    }

    /// What a button message does in `mode`, if anything: a press (the button going down) and,
    /// in Momentary, the release. A program change is a press every time and never released;
    /// so is every message of a `latching` button, whose own state Momentary follows.
    pub fn button(&mut self, mode: ButtonMode, msg: &CtlMsg, latching: bool) -> Option<ButtonAct> {
        let was = std::mem::replace(&mut self.down, msg.pressed);
        if latching || matches!(msg.kind, CtlKind::Program { .. }) {
            self.down = false;
            return Some(match mode {
                ButtonMode::Toggle => ButtonAct::Flip,
                ButtonMode::Momentary => ButtonAct::Set(msg.pressed),
                ButtonMode::Trigger => ButtonAct::Set(true),
            });
        }
        match (msg.pressed, was, mode) {
            (true, false, ButtonMode::Toggle) => Some(ButtonAct::Flip),
            (true, false, _) => Some(ButtonAct::Set(true)),
            (false, true, ButtonMode::Momentary) => Some(ButtonAct::Set(false)),
            _ => None,
        }
    }

    /// Where a button sends a continuous parameter: `on` is its press value, `off` the bottom
    /// of the range. Toggle remembers which of the two it went to last.
    pub fn button_value(&mut self, act: ButtonAct, on: f32, off: f32) -> f32 {
        self.lit = match act {
            ButtonAct::Flip => !self.lit,
            ButtonAct::Set(v) => v,
        };
        if self.lit {
            on
        } else {
            off
        }
    }
}

/// The assignment a message belongs to, if any.
pub fn find(list: &[Assignment], port: &str, msg: &CtlMsg) -> Option<usize> {
    list.iter().position(|a| a.controller.matches(port, msg.channel, msg.kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cc_and_pitch_bend() {
        let m = parse(&[0xB3, 7, 127]).unwrap();
        assert_eq!((m.channel, m.kind, m.value, m.pressed), (3, CtlKind::Cc { number: 7 }, 1.0, true));
        let m = parse(&[0xB0, 1, 63]).unwrap();
        assert!(!m.pressed);
        let m = parse(&[0xE1, 0x7F, 0x7F]).unwrap();
        assert_eq!((m.channel, m.kind, m.value), (1, CtlKind::PitchBend, 1.0));
        assert_eq!(parse(&[0xE0, 0, 0x40]).unwrap().value, 8192.0 / 16383.0);
        // Aftertouch and channel mode messages (120..127) aren't controllers.
        assert!(parse(&[0xA0, 60, 10]).is_none());
        assert!(parse(&[0xD0, 60, 0]).is_none());
        assert!(parse(&[0xB0, 123, 0]).is_none());
        assert!(parse(&[0xB0]).is_none());
    }

    #[test]
    fn parses_notes_and_program_changes() {
        let m = parse(&[0x92, 36, 127]).unwrap();
        assert_eq!((m.channel, m.kind, m.value, m.pressed), (2, CtlKind::Note { number: 36 }, 1.0, true));
        // Note off, and note on at velocity 0, release it.
        let m = parse(&[0x82, 36, 64]).unwrap();
        assert_eq!((m.kind, m.pressed), (CtlKind::Note { number: 36 }, false));
        assert!(!parse(&[0x92, 36, 0]).unwrap().pressed);
        let m = parse(&[0xC5, 9, 0]).unwrap();
        assert_eq!((m.channel, m.kind, m.pressed), (5, CtlKind::Program { number: 9 }, true));
        // A program change has two bytes.
        assert_eq!(parse(&[0xC0, 3]).unwrap().kind, CtlKind::Program { number: 3 });
        assert!(CtlKind::Note { number: 1 }.is_button() && CtlKind::Program { number: 1 }.is_button());
        assert!(!CtlKind::Cc { number: 1 }.is_button() && !CtlKind::PitchBend.is_button());
    }

    fn note(pressed: bool) -> CtlMsg {
        CtlMsg { channel: 0, kind: CtlKind::Note { number: 60 }, value: if pressed { 0.5 } else { 0.0 }, pressed }
    }

    #[test]
    fn toggle_flips_on_each_press() {
        let mut f = Follow::default();
        assert_eq!(f.button(ButtonMode::Toggle, &note(true), false), Some(ButtonAct::Flip));
        assert_eq!(f.button(ButtonMode::Toggle, &note(true), false), None, "held");
        assert_eq!(f.button(ButtonMode::Toggle, &note(false), false), None);
        assert_eq!(f.button(ButtonMode::Toggle, &note(true), false), Some(ButtonAct::Flip));
    }

    #[test]
    fn momentary_follows_the_button() {
        let mut f = Follow::default();
        assert_eq!(f.button(ButtonMode::Momentary, &note(true), false), Some(ButtonAct::Set(true)));
        assert_eq!(f.button(ButtonMode::Momentary, &note(false), false), Some(ButtonAct::Set(false)));
        assert_eq!(f.button(ButtonMode::Momentary, &note(false), false), None, "a second release");
    }

    #[test]
    fn trigger_fires_once_per_press() {
        let mut f = Follow::default();
        assert_eq!(f.button(ButtonMode::Trigger, &note(true), false), Some(ButtonAct::Set(true)));
        assert_eq!(f.button(ButtonMode::Trigger, &note(true), false), None);
        assert_eq!(f.button(ButtonMode::Trigger, &note(false), false), None);
        assert_eq!(f.button(ButtonMode::Trigger, &note(true), false), Some(ButtonAct::Set(true)));
    }

    #[test]
    fn program_changes_press_every_time() {
        let pc = CtlMsg { channel: 0, kind: CtlKind::Program { number: 4 }, value: 1.0, pressed: true };
        let mut f = Follow::default();
        assert_eq!(f.button(ButtonMode::Toggle, &pc, false), Some(ButtonAct::Flip));
        assert_eq!(f.button(ButtonMode::Toggle, &pc, false), Some(ButtonAct::Flip));
        assert_eq!(f.button(ButtonMode::Momentary, &pc, false), Some(ButtonAct::Set(true)));
    }

    #[test]
    fn a_latching_button_presses_with_every_message() {
        // It sends 127 at one press and 0 at the next, with nothing in between.
        let cc = |v: u8| CtlMsg { channel: 0, kind: CtlKind::Cc { number: 20 }, value: v as f32 / 127.0, pressed: v > 63 };
        let mut f = Follow::default();
        assert_eq!(f.button(ButtonMode::Toggle, &cc(127), true), Some(ButtonAct::Flip));
        assert_eq!(f.button(ButtonMode::Toggle, &cc(0), true), Some(ButtonAct::Flip));
        assert_eq!(f.button(ButtonMode::Trigger, &cc(127), true), Some(ButtonAct::Set(true)));
        assert_eq!(f.button(ButtonMode::Trigger, &cc(0), true), Some(ButtonAct::Set(true)));
        // Momentary follows the button's own state.
        assert_eq!(f.button(ButtonMode::Momentary, &cc(127), true), Some(ButtonAct::Set(true)));
        assert_eq!(f.button(ButtonMode::Momentary, &cc(0), true), Some(ButtonAct::Set(false)));
        // Not latching, the 0 is a release: Toggle acts every other message.
        let mut f = Follow::default();
        assert_eq!(f.button(ButtonMode::Toggle, &cc(127), false), Some(ButtonAct::Flip));
        assert_eq!(f.button(ButtonMode::Toggle, &cc(0), false), None);
    }

    #[test]
    fn buttons_on_a_continuous_parameter() {
        let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Note { number: 60 } };
        let mut a = Assignment::new(c, CtlTarget::MasterVolume);
        assert_eq!(a.button_mode, ButtonMode::Trigger, "a pad on a fader sets it");
        a.min = 0.2;
        assert!((a.press_value(&note(true)) - 0.6).abs() < 1e-6, "the velocity's place in the range");
        a.velocity = false;
        assert_eq!(a.press_value(&note(true)), 1.0);
        let mut f = Follow::default();
        assert_eq!(f.button_value(ButtonAct::Flip, 0.9, 0.2), 0.9);
        assert_eq!(f.button_value(ButtonAct::Flip, 0.9, 0.2), 0.2);
        assert_eq!(f.button_value(ButtonAct::Set(true), 0.9, 0.2), 0.9);
        assert_eq!(f.button_value(ButtonAct::Set(false), 0.9, 0.2), 0.2);
        // A command fires; a note on a switch toggles it, as a control change does.
        let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Cc { number: 20 } };
        assert_eq!(Assignment::new(c, CtlTarget::Command { id: "undo".into() }).button_mode, ButtonMode::Trigger);
        let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Note { number: 60 } };
        assert_eq!(Assignment::new(c, CtlTarget::Mute { track: 1 }).button_mode, ButtonMode::Toggle);
    }

    #[test]
    fn port_key_drops_alsa_numbers() {
        assert_eq!(port_key("nanoKONTROL2:nanoKONTROL2 MIDI 1 20:0"), "nanoKONTROL2:nanoKONTROL2 MIDI 1");
        assert_eq!(port_key("nanoKONTROL2:nanoKONTROL2 MIDI 1 24:0"), "nanoKONTROL2:nanoKONTROL2 MIDI 1");
        assert_eq!(port_key("Launch Control XL"), "Launch Control XL");
        assert_eq!(port_key("Device 3:a"), "Device 3:a");
        assert_eq!(port_key("12:0"), "12:0");
    }

    #[test]
    fn fader_taper_round_trips() {
        for db in [-90.0f32, -60.0, -40.0, -24.0, -12.0, -6.0, 0.0, 3.0, 6.0] {
            assert!((fader_to_db(db_to_fader(db)) - db).abs() < 1e-3, "{db}");
        }
        assert_eq!(db_to_fader(-200.0), 0.0);
        assert_eq!(fader_to_db(0.0), -96.0);
        assert_eq!(fader_to_db(1.0), 6.0);
        // 0 dB sits about three quarters of the way up, as on Logic's fader.
        let unity = db_to_fader(0.0);
        assert!(unity > 0.85 && unity < 0.9, "{unity}");
    }

    #[test]
    fn old_eq_assignments_keep_their_travel() {
        // The fixed Channel EQ's assignments mapped 0..1 to 20 Hz..20 kHz and Q 0.1..18 on log
        // scales and gain to ±24 dB; the Channel EQ insert's parameters they now drive do too.
        use crate::fx::channel_eq as ceq;
        let info = ceq::info();
        let plain = |id: u32, n: f32| crate::dsp::to_plain(info.param(id).unwrap(), n);
        for n in [0.0f32, 0.3, 0.5, 1.0] {
            assert!((plain(ceq::freq(2), n) - 20.0 * 1000f32.powf(n)).abs() < 0.01 * 20.0 * 1000f32.powf(n));
            assert!((plain(ceq::gain(2), n) - (-24.0 + 48.0 * n)).abs() < 1e-4);
            assert!((plain(ceq::q(2), n) - 0.1 * 180f32.powf(n)).abs() < 1e-3 * 180f32.powf(n));
            assert!((plain(ceq::OUTPUT, n) - (-24.0 + 48.0 * n)).abs() < 1e-4);
        }
        assert_eq!(plain(ceq::on(3), 0.7), 1.0);
    }

    #[test]
    fn range_scales_and_inverts() {
        let c = Controller { port: "P".into(), channel: 0, kind: CtlKind::Cc { number: 1 } };
        let mut a = Assignment::new(c, CtlTarget::MasterVolume);
        assert_eq!(a.scale(0.5), 0.5);
        a.min = 0.25;
        a.max = 0.75;
        assert_eq!((a.scale(0.0), a.scale(1.0)), (0.25, 0.75));
        a.min = 1.0;
        a.max = 0.0;
        assert_eq!((a.scale(0.0), a.scale(1.0)), (1.0, 0.0));
    }

    #[test]
    fn jump_always_moves() {
        let mut f = Follow::default();
        assert!(f.take(Takeover::Jump, 0.1, 0.8));
        assert!(f.take(Takeover::Jump, 0.9, 0.2));
    }

    #[test]
    fn pickup_waits_for_the_crossing() {
        let mut f = Follow::default();
        // Parameter at 0.6; the controller starts low and comes up.
        assert!(!f.take(Takeover::Pickup, 0.1, 0.6));
        assert!(!f.take(Takeover::Pickup, 0.3, 0.6));
        assert!(f.take(Takeover::Pickup, 0.7, 0.6), "passing the value catches it");
        f.applied(0.7);
        assert!(f.take(Takeover::Pickup, 0.2, 0.7), "caught, it follows anywhere");
        f.applied(0.2);
        // The mouse moves the parameter away: it has to be caught again.
        assert!(!f.take(Takeover::Pickup, 0.25, 0.9));
        assert!(f.take(Takeover::Pickup, 0.9, 0.9), "landing on it catches it");
    }

    #[test]
    fn pickup_catches_within_a_step() {
        let mut f = Follow::default();
        assert!(f.take(Takeover::Pickup, 0.5 + 1.0 / 127.0, 0.5));
    }

    #[test]
    fn pickup_follows_a_stepped_parameter() {
        let mut f = Follow::default();
        assert!(f.take(Takeover::Pickup, 0.48, 0.48));
        // A choice parameter lands on its nearest step; that isn't someone else's move.
        f.applied(0.5);
        assert!(f.take(Takeover::Pickup, 0.41, 0.5));
    }

    #[test]
    fn finds_by_port_channel_and_controller() {
        let c = |port: &str, channel, number| Controller { port: port.into(), channel, kind: CtlKind::Cc { number } };
        let list = vec![
            Assignment::new(c("A", 0, 7), CtlTarget::MasterVolume),
            Assignment::new(c("B", 0, 7), CtlTarget::MasterVolume),
            Assignment::new(c("A", 1, 7), CtlTarget::MasterVolume),
        ];
        let m = |ch, n| CtlMsg { channel: ch, kind: CtlKind::Cc { number: n }, value: 0.0, pressed: false };
        assert_eq!(find(&list, "A", &m(0, 7)), Some(0));
        assert_eq!(find(&list, "B", &m(0, 7)), Some(1));
        assert_eq!(find(&list, "A", &m(1, 7)), Some(2));
        assert_eq!(find(&list, "A", &m(2, 7)), None);
        assert_eq!(find(&list, "A", &m(0, 8)), None);
    }

    #[test]
    fn assignment_file_fills_defaults() {
        let json = r#"{"assignments":[{"controller":{"port":"P","channel":0,"kind":{"type":"cc","number":7}},"target":{"type":"volume","track":3}}]}"#;
        let f: AssignmentFile = serde_json::from_str(json).unwrap();
        let a = &f.assignments[0];
        assert_eq!(
            (a.mode, a.min, a.max, a.button_mode, a.velocity, a.latching, a.feedback),
            (Takeover::Jump, 0.0, 1.0, ButtonMode::Toggle, true, false, true)
        );
        assert_eq!(a.target, CtlTarget::Volume { track: 3 });
        let back: AssignmentFile = serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
        assert_eq!(back.assignments, f.assignments);
    }

    #[test]
    fn commands_are_saved_by_id() {
        let t = CtlTarget::Command { id: "next-marker".into() };
        assert_eq!(serde_json::to_string(&t).unwrap(), r#"{"type":"command","id":"next-marker"}"#);
        assert!(t.is_button() && t.track().is_none());
    }
}
