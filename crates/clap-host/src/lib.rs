//! CLAP hosting on the `clap-sys` bindings: loading and scanning `.clap` files, the host's side of
//! the API, and the two halves the engine runs (`plugin_api::Processor` on the audio thread,
//! `plugin_api::Controller` on the main thread). Editor windows aren't here yet.
//!
//! CLAP says which thread may make each call. The main thread (the one that instantiates) makes
//! the `[main-thread]` calls: `init`, `activate`, parameter info and values, state,
//! `on_main_thread`. The processor makes the `[audio-thread]` ones from whichever thread plays its
//! strip, one at a time: `start_processing`, `process`, `flush`, `stop_processing`, `reset`. The
//! plug-in's requests to the host (`request_callback`, `request_restart`, `request_process`)
//! only set flags, which the controller's `idle` answers on the main thread.

mod entry;
mod events;
mod features;
mod host;
mod params;
mod plugin;
pub mod scan;
mod stream;

pub use plugin::{instantiate, ClapController, ClapProcessor};
