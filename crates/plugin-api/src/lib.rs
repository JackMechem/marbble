//! What the engine asks of a hosted plug-in, whatever its format: the audio-thread half
//! ([`Processor`]) and the main-thread half ([`Controller`]) that `vst3-host` and `clap-host` each
//! make, and the values that pass between them and the engine.

mod descriptor;
mod files;
mod host_set;
mod instances;
mod traits;
mod types;

pub use descriptor::{PluginDescriptor, PluginFormat};
pub use files::find_files;
pub use host_set::HostSet;
pub use instances::{instances_made, next_instance};
pub use traits::{Controller, Processor};
pub use types::{BusLayout, Gesture, KeyInput, MidiEvent, OutputBus, ParamInfo, PluginState, TransportInfo};
