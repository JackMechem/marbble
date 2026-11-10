//! Recording into take folders and editing them: turning a cycle or punch recording into takes
//! (`recording.rs`), Quick Swipe Comping (`comping.rs`), the take folder commands such as Flatten
//! and Pack (`folders.rs`), what the last recording made, and the MIDI a recording captures on
//! its way from the engine (`midi_capture.rs`). The comp logic itself is in `edit/takes/`.

pub mod comping;
pub mod folders;
pub mod last_recording;
pub mod midi_capture;
pub mod placement;
pub mod recording;
#[cfg(test)]
mod tests;
