//! How a controller's moves become a parameter's value: scaling its travel onto the range (with the
//! fader's taper for levels), the Jump and Pickup takeover modes, relative encoders, and what a
//! button press and release do (Toggle, Momentary, Trigger).

use serde::{Deserialize, Serialize};

use super::*;

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

/// How a controller's value is read: as a position, or as a relative encoder's steps (an endless
/// knob sends how far it turned, in one of these encodings).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// The value is the position.
    #[default]
    Absolute,
    /// 1..=63 up, 127 down to 64 down by 1..=64 (127 is one step down).
    TwosComplement,
    /// The 64 bit is the sign, the rest the steps: 1..=63 up, 65..=127 down by 1..=63.
    SignMagnitude,
    /// 64 is no move: above it up, below it down (65 is one step up, 63 one down).
    BinaryOffset,
}

impl Encoding {
    /// The steps a 7-bit value means, positive up; None for Absolute.
    pub fn steps(self, v: u8) -> Option<i32> {
        let v = (v & 0x7F) as i32;
        Some(match self {
            Encoding::Absolute => return None,
            Encoding::TwosComplement => {
                if v < 64 {
                    v
                } else {
                    v - 128
                }
            }
            Encoding::SignMagnitude => {
                if v & 0x40 != 0 {
                    -(v & 0x3F)
                } else {
                    v
                }
            }
            Encoding::BinaryOffset => v - 64,
        })
    }
}

/// What one button message does, from [`Follow::button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAct {
    /// Switch it to the other state.
    Flip,
    /// Switch it on (true) or off.
    Set(bool),
}

/// The fader's travel (0..1) for a level in dB, roughly Logic's taper: +6 dB at the top, -∞ at
/// the bottom. The same curve as `dbToFader` in src/lib/decibels.ts, so a controller moves the fader
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

/// The inverse of [`db_to_fader`] (`faderToDb` in src/lib/decibels.ts).
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

    /// Where a relative encoder's `steps` take a parameter now at `current`: they move the
    /// controller's own position in the range, which is kept between moves (so a stepped
    /// parameter that rounds each move still gets there) unless something else moved the
    /// parameter. None when the range is empty.
    pub fn relative(&mut self, a: &Assignment, steps: i32, current: f32) -> Option<f32> {
        let (lo, hi) = (a.min.clamp(0.0, 1.0), a.max.clamp(0.0, 1.0));
        if (hi - lo).abs() < 1e-6 {
            return None;
        }
        let pos = match (self.last, self.set) {
            (Some(p), Some(s)) if (s - current).abs() <= CATCH => p,
            _ => ((current - lo) / (hi - lo)).clamp(0.0, 1.0),
        };
        let speed = if a.sensitivity.is_finite() && a.sensitivity > 0.0 { a.sensitivity } else { 1.0 };
        let next = (pos + steps as f32 * speed / 127.0).clamp(0.0, 1.0);
        self.last = Some(next);
        Some(a.scale(next))
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
