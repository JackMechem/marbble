//! The session's link to the real-time engine: sending commands (`sync/`), turning the project
//! into what the engine plays (`playback.rs`), the audio device and its latencies (`device.rs`),
//! plug-in delay compensation, the processing threads, and the tracks left out after a plug-in
//! stopped responding on one of them.

pub mod device;
pub mod left_out;
pub mod pdc;
pub mod playback;
pub mod sync;
pub mod threads;
