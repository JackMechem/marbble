//! A CLAP plug-in split into an audio-thread half ([`ClapProcessor`]) and a main-thread half
//! ([`ClapController`]), as `vst3-host` splits a VST3.
//!
//! This file holds the types. `instantiate.rs` makes the two halves, `processor.rs` and
//! `controller.rs` are their code. CLAP has one plug-in object where VST3 has two: the halves
//! share it through `Shared`, and when both are gone it is deactivated and destroyed on the main
//! thread (`Instance`).

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::clap_event_transport;
use clap_sys::ext::latency::clap_plugin_latency;
use clap_sys::ext::params::clap_plugin_params;
use clap_sys::ext::state::clap_plugin_state;
use clap_sys::plugin::clap_plugin;
use plugin_api::{BusLayout, Gesture, HostSet, OutputBus, PluginDescriptor};

use crate::entry::Library;
use crate::events::{InputEvents, OutputEvents};
use crate::host::{Host, Requests};
use crate::params::ParamRanges;

mod controller;
mod instantiate;
mod ports;
mod processor;
#[cfg(test)]
mod tests;
pub use instantiate::instantiate;

/// Parameter changes the host can queue for one block (automation).
const MAX_QUEUED: usize = 512;
/// Input events one block can carry: edits, automation and MIDI together.
const MAX_EVENTS: usize = 2048;
/// Room in each ring between the halves: edits in, gestures out.
const RING: usize = 4096;

/// The plug-in with the `clap_host` it was made with, which must outlive it. Dropping it
/// deactivates and destroys the plug-in, which CLAP keeps to the main thread.
struct Instance {
    plugin: *const clap_plugin,
    host: Box<Host>,
    /// Keeps the file loaded, though it's never unloaded anyway (None for the tests' plug-in).
    _lib: Option<Arc<Library>>,
}

unsafe impl Send for Instance {}

impl Drop for Instance {
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

/// Instances whose last half went on another thread (a render's garbage, say), waiting for the
/// main thread to tear them down (`destroy_deferred`).
static DEFERRED: Mutex<Vec<Instance>> = Mutex::new(Vec::new());

/// Deactivates and destroys the plug-ins whose halves were both dropped off the main thread. Call
/// periodically on the main thread; elsewhere it does nothing.
pub fn destroy_deferred() {
    if crate::host::is_main() {
        let done = std::mem::take(&mut *DEFERRED.lock().unwrap_or_else(|e| e.into_inner()));
        drop(done);
    }
}

/// What both halves share. When both are gone, the plug-in goes with it on the main thread, or is
/// handed to it (`DEFERRED`).
struct Shared {
    instance: u64,
    /// The plug-in, as `inst` has it.
    plugin: *const clap_plugin,
    params: *const clap_plugin_params,
    state: *const clap_plugin_state,
    latency: *const clap_plugin_latency,
    /// The audio thread has stopped running the plug-in (`shut_gate`).
    parked: AtomicBool,
    inst: ManuallyDrop<Instance>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        // Taken once, here, and never used again.
        let inst = unsafe { ManuallyDrop::take(&mut self.inst) };
        if crate::host::is_main() {
            drop(inst);
        } else {
            DEFERRED.lock().unwrap_or_else(|e| e.into_inner()).push(inst);
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
    /// Every parameter's range, for normalizing values: built when it loads, and again when it
    /// says its parameters changed.
    ranges: ParamRanges,
    latency_changed: bool,
    params_restarted: bool,
    /// The host says a thread is stuck in the processor: nothing here calls into the plug-in.
    unresponsive: bool,
    /// Its latency, read when it loads and in `idle`: `latency.get` is the main thread's, and
    /// renders ask for the latency from theirs.
    latency: u32,
    /// It asked for a restart, which isn't done yet (#560): said once in the log.
    restart_noted: bool,
}

unsafe impl Send for ClapController {}

impl Shared {
    fn requests(&self) -> &Requests {
        &self.inst.host.requests
    }

    fn take(&self, pick: impl Fn(&Requests) -> &AtomicBool) -> bool {
        pick(self.requests()).swap(false, Ordering::AcqRel)
    }
}
