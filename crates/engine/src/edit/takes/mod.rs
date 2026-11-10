//! Take folders and comping (Logic's Quick Swipe Comping), and how a cycle or autopunch
//! recording is cut into takes. The types are in `model/take_folder.rs`; this is the logic on them,
//! kept apart from the session (`session/takes/`) so it can be tested on its own.
//!
//! Everything inside a folder is in *take time* (see [`TakeFolder`]). Playback expands the
//! comp into [`CompPiece`]s, one per stretch of a take, with short crossfades (Settings ›
//! Editing › Crossfade Time and Curve) where one take hands over to the next.
//!
//! A folder can keep several comps and switch between them; MIDI take folders hold whole
//! takes only (see [`TakeFolder`]).

pub mod folder; // A folder's takes, comps and comp sections.
pub mod midi; // MIDI take folders: the content copied to and from the take it plays.
pub mod pieces; // What a folder plays: its comp as pieces of takes, with crossfades.
pub mod recording; // Cutting a recording into takes.
#[cfg(test)]
mod tests;
