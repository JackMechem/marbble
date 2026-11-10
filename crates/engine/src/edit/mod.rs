//! Editing logic on the project document, kept apart from the session so each piece can be tested
//! on its own: pure functions and types the session (`session/`) applies to the project, and some
//! of which the engine plays from (automation curves, fade shapes, region parameters).

pub mod arrange_cmds; // Section Between Locators, Trim, Remove Gaps: time maps over the timeline.
pub mod arrangement; // The Arrangement track's sections, and moving or copying them as time maps.
pub mod auto_move; // Move Automation with Regions, on one lane.
pub mod automation; // Evaluating automation lanes for the engine, and splicing and thinning curves.
pub mod drag_modes; // No Overlap, Shuffle L and Shuffle R on one track's lane.
pub mod drum_replace; // Drum Replacement: finding the hits on a drum recording.
pub mod fades; // Fade curves and crossfade types between audio regions.
pub mod midi_content; // Splitting, trimming and joining the notes and controllers of MIDI regions.
pub mod pattern; // Step Sequencer pattern regions and the notes they play.
pub mod region_params; // The Region inspector's playback transforms (quantize, transpose, velocity…).
pub mod strip_silence; // Strip Silence: the sounding parts of a region's audio.
pub mod takes; // Take folders and comping, and cutting a cycle or punch recording into takes.
pub mod transients; // Transient detection, for splits, sampler slices and Drum Replacement.
