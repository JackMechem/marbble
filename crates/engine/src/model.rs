//! The project document: everything that gets saved and mirrored to the UI.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::fades::{DragMode, FadeShape};
use crate::prefs::{BitDepth, ClickSettings};
use crate::pattern::Pattern;
use crate::region_params::RegionParams;
use crate::tempo::{SigEvent, TempoEvent};

mod automation;
pub use automation::*;
mod project;
pub use project::*;
mod region;
pub use region::*;
mod take_folder;
pub use take_folder::*;
mod track;
pub use track::*;
#[cfg(test)]
mod routing_tests;
#[cfg(test)]
mod tests;

pub type Id = u64;

/// Track id reserved for the Stereo Out channel strip.
pub const MASTER_ID: Id = 0;

pub const INSERT_SLOTS: usize = 8;
/// Send slots per strip, as in Logic.
pub const SEND_SLOTS: usize = 8;

/// Logic's default track colours: blue for audio, green for software instruments.
pub const AUDIO_COLOR: &str = "#3b8fd9";
pub const INSTRUMENT_COLOR: &str = "#4fae4c";
pub const STACK_COLOR: &str = "#8f86c9";
pub const AUX_COLOR: &str = "#5fb3a6";
