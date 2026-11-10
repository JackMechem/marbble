//! The mixer's channel strips beyond their inserts: sends, aux buses and outputs, VCA faders, the
//! auxes of multi-output instruments, the clip and gain-reduction meters, and channel strip
//! settings. The inserts themselves are in `plugins/`.

pub mod meters;
pub mod multi_out;
pub mod routing;
pub mod strip_settings;
pub mod vca;
