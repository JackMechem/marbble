//! Minimal VST3 hosting: module loading, scanning, instantiation, processing, and editors (their
//! windows are `plugin_editor`'s).
//! The two halves it makes are the engine's `plugin_api::Processor` and `Controller`
//! (`plugin/api.rs`), and the types they share with it are `plugin_api`'s, re-exported here.

pub mod host;
pub mod module;
pub mod plugin;
pub mod process;
pub mod scan;
pub mod stream;
mod util;

mod editor;
#[cfg(target_os = "macos")]
mod macos_bundle;

pub use host::Gesture;
pub use module::PluginDescriptor;
pub use plugin::{
    instances_made, instantiate, instantiate_keyed, BusLayout, KeyInput, MidiEvent, OutputBus, ParamInfo, PluginController, PluginProcessor, PluginState,
    TransportInfo,
};
