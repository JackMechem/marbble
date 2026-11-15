//! Minimal VST3 hosting: module loading, scanning, instantiation, processing and editor windows.
//! The two halves it makes are the engine's `plugin_api::Processor` and `Controller`
//! (`plugin/api.rs`), and the types they share with it are `plugin_api`'s, re-exported here.

pub mod host;
pub mod keys;
pub mod module;
pub mod plugin;
pub mod process;
pub mod scan;
pub mod stream;
mod util;

pub mod editor;
#[cfg(target_os = "macos")]
mod macos_bundle;

pub use host::Gesture;
pub use module::PluginDescriptor;
pub use plugin::{
    close_stranded_editors, instances_made, instantiate, instantiate_keyed, BusLayout, KeyInput, MidiEvent, OutputBus, ParamInfo, PluginController,
    PluginProcessor, PluginState, TransportInfo,
};
