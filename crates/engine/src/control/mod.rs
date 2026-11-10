//! Hardware control of the DAW, as Logic's Control Surfaces menu has it: controller assignments
//! (a knob or button drives a mixer control, a plug-in parameter or a key command), the feedback
//! that sends each target's value back to the controller, and Mackie Control and HUI surfaces.
//! These are the pure parts; the session's side is in `session/control/`, the ports in `midi/`.

pub mod controllers;
pub mod feedback;
pub mod surface;
