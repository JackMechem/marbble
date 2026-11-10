//! Region edits in the Tracks area and the Audio Track editor: the basic edits (`edits.rs`), moves
//! and the drag modes, gain, fades and crossfades, playback parameters, range edits, the marquee,
//! zero crossings, the arranging commands across tracks, and Drum Replacement. The pure maths
//! behind most of them is in `crate::edit`.

pub mod arrange_cmds;
pub mod crossfades;
pub mod drag_modes;
pub mod drum_replace;
pub mod edits;
pub mod gain;
pub mod import;
pub mod marquee;
pub mod moves;
pub mod musical_moves;
pub mod new_track_drag;
pub mod params;
pub mod range_edit;
pub mod zero_crossings;
