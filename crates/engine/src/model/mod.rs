//! The project document: everything that gets saved and mirrored to the UI.
//!
//! The types are in `project.rs`, `track.rs`, `region.rs`, `take_folder.rs` and `automation.rs`, all
//! re-exported from here. Two modules of rules on the document keep their own paths: the tempo map
//! (`tempo.rs`, positions in samples against bars and beats) and how VCA faders reach their strips
//! (`vca.rs`). `src/engine/` mirrors them. Only `crate::session` edits a document; the engine
//! never sees one, only what the session makes from it. `channel_eq_upgrade.rs` converts the fixed
//! Channel EQ of older projects.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::edit::fades::{DragMode, FadeShape};
use crate::edit::pattern::Pattern;
use crate::edit::region_params::RegionParams;
use crate::model::tempo::{SigEvent, TempoEvent};
use crate::prefs::{BitDepth, ClickSettings};

mod automation;
pub use automation::*;
mod channel_eq_upgrade;
pub use channel_eq_upgrade::{channel_eq_slot, insert_first};
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
pub mod tempo;
#[cfg(test)]
mod tests;
pub mod vca;

/// Ids of tracks, regions, clips and plug-in instances, unique within a project (`Project::alloc_id`).
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
