//! The two halves of a hosted plug-in. A format's host crate makes both from a descriptor; the
//! engine moves the [`Processor`] to the audio thread and keeps the [`Controller`] in the session,
//! on the main thread. Parameter values are normalized 0..1 on both.

use crate::{BusLayout, Gesture, KeyInput, MidiEvent, OutputBus, ParamInfo, PluginDescriptor, PluginState, TransportInfo};

/// The audio-thread half. Nothing here may allocate, lock or block.
pub trait Processor: Send {
    /// The plug-in's name, for messages.
    fn name(&self) -> &str;

    /// The longest block `process` takes.
    fn max_block(&self) -> usize;

    /// Processes one stereo block in place, with a side-chain `key` (None: silence there).
    /// `left`/`right` must be at most `max_block` long, and the key at least as long as them.
    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: KeyInput, midi: &[MidiEvent], t: &TransportInfo);

    /// `process_keyed` without a key.
    fn process(&mut self, left: &mut [f32], right: &mut [f32], midi: &[MidiEvent], t: &TransportInfo) {
        self.process_keyed(left, right, None, midi, t);
    }

    /// Queues a parameter change for the next block, at a sample offset within it. Points for
    /// one parameter come in offset order; the plug-in ramps between them. Changes past the
    /// queue's room are dropped.
    fn queue_param(&mut self, id: u32, offset: i32, value: f64);

    /// Tells the plug-in processing has stopped, until the next `process` starts it again: it
    /// should clear its delay lines and voices. Its state, parameters and editor stay.
    fn suspend(&mut self);

    /// Starts fading to dry, after which `process` stops running the plug-in (see the session's
    /// `sidechain.rs`). One that isn't running stops at once.
    fn close_gate(&mut self);

    /// Stops the plug-in now, fade or no fade. The main thread may reconfigure it after this.
    fn shut_gate(&mut self);

    /// Runs the plug-in again, fading in after `settle` frames (its latency).
    fn open_gate(&mut self, settle: u32);

    /// Parameter edits from the controller wait for the next `process`.
    fn has_edits(&self) -> bool;

    /// Something waits for the next `process` (edits, automation, a fade): a host that skips
    /// blocks the plug-in can do without must run this one.
    fn wants_to_run(&self) -> bool;

    /// How long its output may go on after its input stops, in frames, or `u32::MAX` for a tail
    /// that never ends.
    fn ring_out(&self) -> u32;

    /// Gives a plug-in the engine isn't running (bypassed, say) the edits waiting for it, so its
    /// state has them.
    fn pass_edits(&mut self, t: &TransportInfo);

    /// The block just processed on output bus `bus` (mono on both sides), or None if the plug-in
    /// has no such bus.
    fn output_bus(&self, bus: usize, n: usize) -> Option<(&[f32], &[f32])>;
}

/// The main-thread half: parameters, state, the editor window, and what the plug-in reports.
pub trait Controller: Send {
    fn desc(&self) -> &PluginDescriptor;

    /// Says whether a thread is stuck in the processor (the host left it there). While it is,
    /// nothing calls into the plug-in: values read as unknown, edits go nowhere, the editor
    /// doesn't open and only hides on close, `latency` is the last one, `save_state` gives None,
    /// and buses don't switch.
    fn set_unresponsive(&mut self, on: bool);

    /// The instance's number, from 1 up in the order instances were made in this process, across
    /// formats (see `instances_made`).
    fn instance(&self) -> u64;

    /// Whether the plug-in may be called (see `set_unresponsive`).
    fn responsive(&self) -> bool;

    fn has_editor(&self) -> bool;
    fn editor_open(&self) -> bool;
    fn open_editor(&mut self, title: &str) -> Result<(), String>;
    fn close_editor(&mut self);

    /// Call periodically on the main thread.
    fn idle(&mut self);

    /// The processing latency the plug-in reports, in samples.
    fn latency(&self) -> u32;

    /// Whether the plug-in reported a new latency since the last call.
    fn take_latency_changed(&mut self) -> bool;

    /// Whether the plug-in has an input a side-chain key can reach.
    fn has_key_input(&self) -> bool;

    /// Whether that input is in use: the plug-in hears the key rather than its internal one.
    fn key_active(&self) -> bool;

    /// Whether the audio thread has stopped running the processor (`shut_gate`).
    fn parked(&self) -> bool;

    /// Switches the side-chain input. The processor's gate must be shut first (`parked`), or the
    /// instance not yet in use. Errors, with the side-chain gone, if the plug-in refuses.
    fn set_key_active(&mut self, on: bool) -> Result<(), String>;

    /// The plug-in's audio buses as it reports them now.
    fn audio_buses(&self) -> Vec<BusLayout>;

    /// The plug-in's audio output buses, main one first.
    fn output_buses(&self) -> &[OutputBus];

    /// Makes `want` the extra output buses in use (indexes; the main bus is always on).
    fn set_active_outputs(&mut self, want: &[u32]);

    /// The automatable parameters (hidden and read-only ones left out).
    fn parameters(&self) -> Vec<ParamInfo>;

    /// Every parameter's id, name and whether it's read-only (a meter), hidden ones included.
    fn param_names(&self) -> Vec<(u32, String, bool)>;

    /// A parameter's current value.
    fn param_value(&self, id: u32) -> Option<f64>;

    /// Shows a value the host is playing, such as automation, in the editor. Doesn't reach the
    /// processor: queue that with `Processor::queue_param`.
    fn set_param_value(&self, id: u32, value: f64);

    /// Sets a parameter as a control surface would: shows it and queues it for the processor,
    /// like an edit in the plug-in's own editor.
    fn edit_param(&self, id: u32, value: f64);

    /// How many times the host has set a parameter or loaded a state: when it changes, the
    /// plug-in's state has changed by something other than its editor.
    fn host_changes(&self) -> u64;

    /// Whether an edit from the plug-in only repeats what the host set a moment ago.
    fn host_echo(&self, id: u32, value: f64) -> bool;

    /// Whether edits are still on their way to the processor.
    fn edits_pending(&self) -> bool;

    /// Whether the plug-in said its parameters or state changed on their own since the last call.
    fn take_params_restarted(&mut self) -> bool;

    /// The editor's gestures since the last call, oldest first.
    fn take_gestures(&mut self) -> Vec<Gesture>;

    /// Acts out a gesture as the plug-in's editor would: for headless checks, where no editor can
    /// open.
    #[doc(hidden)]
    fn drive_handler(&self, g: Gesture);

    /// Acts out the plug-in saying its parameters all changed, for headless checks.
    #[doc(hidden)]
    fn drive_params_restarted(&self);

    /// The plug-in's state, or None while it mustn't be called (`set_unresponsive`).
    fn save_state(&self) -> Option<PluginState>;

    /// Does nothing while the plug-in mustn't be called.
    fn load_state(&self, st: &PluginState);
}
