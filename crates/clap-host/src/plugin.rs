//! A CLAP plug-in split into an audio-thread half ([`ClapProcessor`]) and a main-thread half
//! ([`ClapController`]), as `vst3-host` splits a VST3.
//!
//! This file holds the types. `instantiate.rs` makes the two halves, `processor.rs` and
//! `controller.rs` are their code. CLAP has one plug-in object where VST3 has two: the halves
//! share it through `Shared`, whose drop deactivates and destroys it.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::clap_event_transport;
use clap_sys::ext::latency::clap_plugin_latency;
use clap_sys::ext::params::clap_plugin_params;
use clap_sys::ext::state::clap_plugin_state;
use clap_sys::plugin::clap_plugin;
use plugin_api::{BusLayout, Gesture, HostSet, OutputBus, PluginDescriptor};

use crate::entry::Library;
use crate::events::{InputEvents, OutputEvents};
use crate::host::Host;

mod controller;
mod instantiate;
mod ports;
mod processor;
pub use instantiate::instantiate;

/// Parameter changes the host can queue for one block (automation).
const MAX_QUEUED: usize = 512;
/// Input events one block can carry: edits, automation and MIDI together.
const MAX_EVENTS: usize = 2048;
/// Room in each ring between the halves: edits in, gestures out.
const RING: usize = 4096;

/// What both halves share. Torn down when both are gone, on the main thread.
struct Shared {
    instance: u64,
    plugin: *const clap_plugin,
    /// The `clap_host` the plug-in was made with, which must outlive it.
    host: Box<Host>,
    params: *const clap_plugin_params,
    state: *const clap_plugin_state,
    latency: *const clap_plugin_latency,
    /// The audio thread has stopped running the plug-in (`shut_gate`).
    parked: AtomicBool,
    /// Keeps the file loaded, though it's never unloaded anyway.
    _lib: Arc<Library>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        // The processor stopped processing when it was dropped.
        unsafe {
            let p = &*self.plugin;
            if let Some(f) = p.deactivate {
                f(self.plugin);
            }
            if let Some(f) = p.destroy {
                f(self.plugin);
            }
        }
    }
}

/// An audio port's buffers.
struct Port {
    channels: usize,
    buffers: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

/// The audio-thread half. Nothing in it allocates, locks or blocks.
pub struct ClapProcessor {
    shared: Arc<Shared>,
    inputs: Vec<Port>,
    outputs: Vec<Port>,
    /// Which of them are the main ports.
    main_in: Option<usize>,
    main_out: Option<usize>,
    in_bufs: Vec<clap_audio_buffer>,
    out_bufs: Vec<clap_audio_buffer>,
    events_in: InputEvents,
    events_out: OutputEvents,
    transport: clap_event_transport,
    /// Edits from the controller, normalized.
    edits_rx: rtrb::Consumer<(u32, f64)>,
    /// Host parameter changes (automation) for the next block: id, sample offset, normalized.
    queued: Vec<(u32, i32, f64)>,
    /// How the plug-in takes notes: CLAP note events, MIDI, or neither.
    notes: ports::Notes,
    /// `start_processing` was called and `stop_processing` not since.
    processing: bool,
    /// `shut_gate`: `process` leaves the block as it is.
    shut: bool,
    /// Frames processed so far, for `steady_time`.
    steady: i64,
    max_block: usize,
    ring_out: u32,
    name: String,
}

unsafe impl Send for ClapProcessor {}

/// The main-thread half: parameters, state, and what the plug-in asks of the host.
pub struct ClapController {
    desc: PluginDescriptor,
    shared: Arc<Shared>,
    gestures_rx: rtrb::Consumer<Gesture>,
    /// Gestures `drive_handler` acted out, for headless checks.
    driven: parking_lot::Mutex<Vec<Gesture>>,
    edits_tx: parking_lot::Mutex<rtrb::Producer<(u32, f64)>>,
    host_set: HostSet,
    /// The audio ports as the plug-in reported them when activated.
    buses: Vec<BusLayout>,
    outputs: Vec<OutputBus>,
    latency_changed: bool,
    params_restarted: bool,
    /// The host says a thread is stuck in the processor: nothing here calls into the plug-in.
    unresponsive: bool,
    last_latency: AtomicU32,
    /// It asked for a restart, which isn't done yet (#560): said once in the log.
    restart_noted: bool,
}

unsafe impl Send for ClapController {}

impl Shared {
    fn take(&self, pick: impl Fn(&crate::host::Requests) -> &AtomicBool) -> bool {
        pick(&self.host.requests).swap(false, Ordering::AcqRel)
    }
}
