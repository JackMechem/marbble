//! Hardware control on the session side: learning and running controller assignments, sending
//! feedback to controllers, and driving Mackie Control and HUI surfaces. The protocols and pure
//! parts are in `crate::control`, the ports in `crate::midi`.

pub mod controllers;
pub mod feedback;
pub mod surface;
