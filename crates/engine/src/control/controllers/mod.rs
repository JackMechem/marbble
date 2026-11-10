//! Controller assignments, as Logic's Control Surfaces › Learn Assignment and the Controller
//! Assignments window: a knob, fader, button or pad on a MIDI controller drives a mixer
//! control, a plug-in parameter, a transport button or any key command.
//!
//! This module is the pure part: reading controller messages (14-bit pairs and NRPNs too,
//! [`HiRes`], and working out which kind a learn heard, [`Learner`]), recognising a port again
//! after it was plugged back in, scaling a controller's travel onto a parameter's range (with
//! the fader's taper for levels), the Jump and Pickup takeover modes, relative encoders, and
//! what a button press and release do (Toggle, Momentary, Trigger): `assignment.rs`, `targets.rs`,
//! `messages.rs` and `response.rs`. The session applies the
//! results (`session/control/controllers/`); `midi/input.rs` takes assigned controllers off the live
//! MIDI path.

mod assignment;
pub use assignment::*;
mod messages;
pub use messages::*;
mod response;
pub use response::*;
mod targets;
pub use targets::*;
#[cfg(test)]
mod tests;
