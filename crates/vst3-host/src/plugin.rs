//! Instantiating a VST3 plugin and splitting it into an audio-thread half (`PluginProcessor`) and
//! a UI-thread half (`PluginController`).
//!
//! This file holds the types. `instantiate.rs` makes the two halves, `processor.rs` and `controller.rs`
//! are their code, and `multi_out.rs` handles output buses past the main one. The engine moves the
//! processor to the audio thread, and the session keeps the controller on the main thread.

// C enums are i32 in the MSVC bindings and u32 elsewhere, so a cast that converts on one
// platform is a no-op on the other.
#![cfg_attr(target_env = "msvc", allow(clippy::unnecessary_cast))]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{ComPtr, ComWrapper};

use crate::host::{host_context, ComponentHandler, Gesture};
use crate::module::{load_module, PluginDescriptor};
use crate::process::{EventList, ParameterChanges};
use crate::stream::{as_stream, MemoryStream};
use crate::util::{unhex, utf16};

mod controller;
pub use controller::close_stranded_editors;
mod instantiate;
pub use instantiate::*;
mod multi_out;
mod processor;
use multi_out::OutGate;
pub use multi_out::OutputBus;

/// Parameter changes the host can queue for one `process` call (automation).
const MAX_QUEUED: usize = 512;
/// The longest the silent block of `pass_edits` needs to be.
const EDIT_BLOCK: usize = 64;
/// How long after the host set a parameter the same value from the plug-in counts as its answer.
const HOST_ECHO: Duration = Duration::from_millis(200);

/// How long a plug-in crossfades with its dry signal around a side-chain change.
const GATE_FADE_SECS: f64 = 0.005;

/// A side-chain signal for one block: left and right.
pub type KeyInput<'a> = Option<(&'a [f32], &'a [f32])>;

/// One of a plug-in's audio buses, as it reports it.
#[derive(Debug, Clone)]
pub struct BusLayout {
    pub name: String,
    pub input: bool,
    pub aux: bool,
    pub default_active: bool,
    pub channels: usize,
}

/// A three-byte MIDI message and the sample it lands on in the block (`offset`).
#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    pub offset: u32,
    pub data: [u8; 3],
}

/// The transport as a plug-in sees it for one block, filled into its `ProcessContext`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TransportInfo {
    pub playing: bool,
    pub recording: bool,
    pub sample_rate: f64,
    pub tempo: f64,
    pub sig_num: i32,
    pub sig_den: i32,
    pub position_samples: i64,
    pub position_quarters: f64,
    pub bar_start_quarters: f64,
    pub cycle: Option<(f64, f64)>,
}

/// A parameter as the plug-in's controller describes it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub units: String,
    /// Normalized 0..1.
    pub default: f64,
    /// 0 = continuous, 1 = on/off, n = n+1 discrete values.
    pub steps: i32,
}

/// A plug-in's saved state, from `getState`: the component's and the controller's own blobs.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PluginState {
    pub component: Vec<u8>,
    pub controller: Vec<u8>,
}

/// Instances made so far in this process, which numbers each (`PluginController::instance`).
static INSTANCES: AtomicU64 = AtomicU64::new(0);

/// How many plug-in instances have been made so far: every instance made from now on has a higher
/// number (`PluginController::instance`). Allocates nothing and takes no lock.
pub fn instances_made() -> u64 {
    INSTANCES.load(Ordering::Acquire)
}

/// The component's and a separate controller's connection points, connected to each other.
type Connections = (ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>);

/// COM objects shared by both halves. Torn down when both halves are gone.
struct Shared {
    /// The instance's number, from 1 up, in the order instances were made.
    instance: u64,
    component: ComPtr<IComponent>,
    processor: Option<ComPtr<IAudioProcessor>>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    connections: Option<Connections>,
    _handler: ComWrapper<ComponentHandler>,
    /// The audio thread has stopped calling the processor (see `Gate`).
    parked: AtomicBool,
    /// Lets the controller change which output buses are active (see `multi_out.rs`).
    out_gate: OutGate,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        unsafe {
            if let Some(p) = self.processor.take() {
                p.setProcessing(0);
            }
            self.component.setActive(0);
            terminate(&self.component, self.controller.take(), self.separate_controller, self.connections.take());
        }
    }
}

/// Undoes `initialize` and `connect`: disconnects the component and a separate controller,
/// takes the host's handler back from the controller and terminates both. The plug-in must be
/// inactive by then. The connections and the controller are released before the component is
/// terminated: some plug-ins (DPF's) check that nothing holds them any more. Used by `Shared`'s
/// Drop, and by `instantiate` when it fails before a `Shared` exists.
unsafe fn terminate(component: &ComPtr<IComponent>, controller: Option<ComPtr<IEditController>>, separate_controller: bool, connections: Option<Connections>) {
    if let Some((a, b)) = connections {
        a.disconnect(b.as_ptr());
        b.disconnect(a.as_ptr());
    }
    if let Some(c) = controller {
        c.setComponentHandler(std::ptr::null_mut());
        if separate_controller {
            c.terminate();
        }
    }
    component.terminate();
}

struct Bus {
    channels: usize,
    buffers: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl Bus {
    fn new(channels: usize, max_block: usize) -> Bus {
        let mut buffers: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; max_block]).collect();
        let ptrs = buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        Bus { channels, buffers, ptrs }
    }

    /// Puts `n` frames of a stereo pair on the first two channels: mixed to one on a mono bus,
    /// nowhere on a bus without channels.
    fn put_stereo(&mut self, l: &[f32], r: &[f32], n: usize) {
        match self.channels {
            0 => {}
            1 => {
                for i in 0..n {
                    self.buffers[0][i] = 0.5 * (l[i] + r[i]);
                }
            }
            _ => {
                self.buffers[0][..n].copy_from_slice(&l[..n]);
                self.buffers[1][..n].copy_from_slice(&r[..n]);
            }
        }
    }
}

/// Whether the processor runs. Activating a bus means `setActive(false)` and back on the main
/// thread, which `process` must stay out of. The audio thread can't wait for that, so the signal
/// crossfades to dry, the plug-in stops, its dry signal passes while the main thread works, and
/// it crossfades back. See `Session::sync_vst_keys`.
#[derive(Debug, Clone, Copy)]
enum Gate {
    Open,
    /// Frames left of the fade to dry.
    Closing(u32),
    Shut,
    /// Frames to wait before the fade back, so a latent plug-in's first output has arrived,
    /// and frames left of the fade.
    Opening {
        settle: u32,
        left: u32,
    },
}

/// The audio-thread half. `process` never allocates.
pub struct PluginProcessor {
    _shared: Arc<Shared>,
    // Borrowed from `shared`, which keeps the reference alive.
    processor: *mut IAudioProcessor,
    inputs: Vec<Bus>,
    outputs: Vec<Bus>,
    in_abb: Vec<AudioBusBuffers>,
    out_abb: Vec<AudioBusBuffers>,
    param_in: ComWrapper<ParameterChanges>,
    param_out: ComWrapper<ParameterChanges>,
    events_in: ComWrapper<EventList>,
    events_out: ComWrapper<EventList>,
    edits_rx: rtrb::Consumer<(u32, f64)>,
    out_tx: rtrb::Producer<(u32, f64)>,
    midi_map: [Option<ParamID>; 130],
    max_block: usize,
    pub is_instrument: bool,
    /// Host parameter changes (automation) for the next block: id, sample offset, value.
    queued: Vec<(ParamID, i32, f64)>,
    /// The input bus that takes a side-chain key (see `KeyBus`).
    key_bus: Option<usize>,
    gate: Gate,
    gate_frames: u32,
    /// The input while a fade needs it.
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
    /// Fades around a change of the active output buses (see `multi_out.rs`).
    out_fade: multi_out::OutFade,
    /// The silent block `pass_edits` runs.
    edit_l: Vec<f32>,
    edit_r: Vec<f32>,
    /// See `ring_out`.
    ring_out: u32,
}

unsafe impl Send for PluginProcessor {}

/// The UI-thread half: parameters, state and the editor window.
pub struct PluginController {
    pub desc: PluginDescriptor,
    shared: Arc<Shared>,
    out_rx: rtrb::Consumer<(u32, f64)>,
    editor: Option<crate::editor::Editor>,
    gestures_rx: rtrb::Consumer<Gesture>,
    /// The plug-in said its latency changed (`restartComponent(kLatencyChanged)`) since the
    /// last `take_latency_changed`.
    latency_changed: bool,
    /// The input bus that takes a side-chain key, and whether it's active. None when the plug-in
    /// has no aux input, or refused to activate it.
    key_bus: Option<usize>,
    key_active: bool,
    /// Its audio output buses, and which of the extra ones are active (see `multi_out.rs`).
    outputs: Vec<OutputBus>,
    /// The plug-in said its parameters all changed (`restartComponent(kParamValuesChanged)`)
    /// since the last `take_params_restarted`.
    params_restarted: bool,
    /// Counts what the host itself did to the parameters, and the values it set a moment ago,
    /// so that undo can tell those from edits in the editor (see `host_echo`).
    host_changes: AtomicU64,
    host_set: parking_lot::Mutex<HashMap<u32, (f64, Instant)>>,
    /// The host says a thread is stuck in the processor (`set_unresponsive`): nothing here calls
    /// into the plug-in meanwhile. The latency it reported last, for `latency` to give then.
    unresponsive: bool,
    last_latency: AtomicU32,
}

unsafe impl Send for PluginController {}
