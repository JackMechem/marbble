//! The main-thread half: [`ClapController`] reads and sets the plug-in's parameters, saves and
//! loads its state, and answers what it asked of the host. Its calls into the plug-in are the
//! main thread's: reached from another (a render asking for the latency), they're answered from
//! what was read last, as for a plug-in that isn't responding.

use plugin_api::{Controller, ParamInfo, PluginState};

use super::*;
use crate::entry::buf_text;
use crate::params;
use crate::stream::{InStream, OutStream};

impl ClapController {
    pub(super) fn plugin(&self) -> &clap_plugin {
        unsafe { &*self.shared.plugin }
    }

    /// Whether the plug-in may be called now: it's responding, and this is the main thread.
    pub(super) fn callable(&self) -> bool {
        !self.unresponsive && crate::host::is_main()
    }

    fn params(&self) -> Option<&clap_plugin_params> {
        unsafe { self.shared.params.as_ref() }.filter(|_| self.callable())
    }

    fn infos(&self) -> Vec<clap_sys::ext::params::clap_param_info> {
        if !self.callable() {
            return Vec::new();
        }
        unsafe { params::infos(self.shared.plugin, self.shared.params) }
    }
}

impl Controller for ClapController {
    fn desc(&self) -> &PluginDescriptor {
        &self.desc
    }

    fn set_unresponsive(&mut self, on: bool) {
        self.unresponsive = on;
        self.shared.inst.host.set_quiet(on);
        if let Some(e) = &self.editor {
            e.set_quiet(on);
        }
    }

    fn instance(&self) -> u64 {
        self.shared.instance
    }

    fn responsive(&self) -> bool {
        !self.unresponsive
    }

    /// Whether its GUI can be embedded in a window here (floating-only ones can't).
    fn has_editor(&self) -> bool {
        !self.shared.gui.is_null()
    }

    fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    fn open_editor(&mut self, title: &str) -> Result<(), String> {
        self.open_window(title)
    }

    fn close_editor(&mut self) {
        self.close_window();
    }

    /// Answers the plug-in's requests: `on_main_thread` when it asked for a callback, what it asked
    /// of its editor window, a restart (`restart.rs`), and notes what it said changed.
    fn idle(&mut self) {
        if crate::host::is_main() {
            self.window_idle();
        }
        if !self.callable() {
            return;
        }
        let s = &self.shared;
        if s.take(|r| &r.callback) {
            if let Some(f) = self.plugin().on_main_thread {
                unsafe { f(s.plugin) };
            }
        }
        if s.take(|r| &r.changed) {
            self.params_restarted = true;
            self.ranges = params::ranges(&self.infos());
        }
        // Read each time: a plug-in may change it without saying so, as a VST3 may.
        self.latency_changed |= s.take(|r| &r.latency);
        let latency = self.read_latency();
        self.note_latency(latency);
        self.restart_idle();
    }

    fn settle(&mut self) {
        self.settle_restart();
    }

    fn set_offline(&mut self, offline: bool) {
        self.render_mode(offline);
    }

    /// The latency read when it loaded or at the last `idle`, which any thread may ask for.
    fn latency(&self) -> u32 {
        self.latency
    }

    fn take_latency_changed(&mut self) -> bool {
        std::mem::take(&mut self.latency_changed)
    }

    /// An input port past the main one (`ports::key_ports`).
    fn has_key_input(&self) -> bool {
        self.buses.iter().any(|b| b.input && b.aux && b.channels > 0)
    }

    fn key_active(&self) -> bool {
        self.key_active
    }

    fn parked(&self) -> bool {
        self.shared.parked.load(Ordering::Acquire)
    }

    /// CLAP plays every port, so the plug-in itself isn't called: the processor puts the key on
    /// its input, or silence. (Switching while it plays still goes through the gate's crossfade,
    /// as for a VST3.)
    fn set_key_active(&mut self, on: bool) -> Result<(), String> {
        if !self.has_key_input() || on == self.key_active {
            return Ok(());
        }
        if self.unresponsive {
            return Err(format!("{} isn't responding", self.desc.name));
        }
        self.key_active = on;
        self.shared.keyed.store(on, Ordering::Relaxed);
        Ok(())
    }

    fn audio_buses(&self) -> Vec<BusLayout> {
        if !self.callable() {
            return Vec::new();
        }
        self.buses.clone()
    }

    fn output_buses(&self) -> &[OutputBus] {
        &self.outputs
    }

    /// Notes which extra outputs auxes use. A CLAP plug-in plays every port unless the host
    /// switches some off with `audio-ports-activation`, which Marbble doesn't use: nothing is
    /// switched in the plug-in, so unused outputs still cost it their work.
    fn set_active_outputs(&mut self, want: &[u32]) {
        for b in self.outputs.iter_mut().skip(1) {
            b.active = want.contains(&b.index);
        }
    }

    fn parameters(&self) -> Vec<ParamInfo> {
        params::automatable(&self.infos())
    }

    fn param_names(&self) -> Vec<(u32, String, bool)> {
        use clap_sys::ext::params::CLAP_PARAM_IS_READONLY;
        self.infos().iter().map(|i| (i.id, buf_text(&i.name), i.flags & CLAP_PARAM_IS_READONLY != 0)).collect()
    }

    fn param_value(&self, id: u32) -> Option<f64> {
        let get = self.params()?.get_value?;
        let r = self.ranges.get(id)?;
        let mut v = 0.0;
        unsafe { get(self.shared.plugin, id, &mut v) }.then(|| r.to_normalized(v))
    }

    /// CLAP has no separate controller to move: the plug-in shows what it processes, so this only
    /// notes what the host set.
    fn set_param_value(&self, id: u32, value: f64) {
        if !self.unresponsive {
            self.host_set.param(id, value.clamp(0.0, 1.0));
        }
    }

    fn edit_param(&self, id: u32, value: f64) {
        if self.unresponsive {
            return;
        }
        let v = value.clamp(0.0, 1.0);
        self.set_param_value(id, v);
        let _ = self.edits_tx.lock().push((id, v));
    }

    fn host_changes(&self) -> u64 {
        self.host_set.changes()
    }

    fn host_echo(&self, id: u32, value: f64) -> bool {
        self.host_set.echo(id, value)
    }

    /// Edits wait on the ring, or the block that took them isn't over.
    fn edits_pending(&self) -> bool {
        let tx = self.edits_tx.lock();
        tx.slots() < tx.buffer().capacity() || self.shared.edits_in_block.load(Ordering::Acquire)
    }

    fn take_params_restarted(&mut self) -> bool {
        std::mem::take(&mut self.params_restarted)
    }

    fn take_gestures(&mut self) -> Vec<Gesture> {
        let mut out = std::mem::take(&mut *self.driven.lock());
        while let Ok(g) = self.gestures_rx.pop() {
            out.push(g);
        }
        out
    }

    /// As the plug-in's editor would: the gesture reaches the host, and a value reaches the
    /// plug-in (its editor would have set it there).
    fn drive_handler(&self, g: Gesture) {
        if self.unresponsive {
            return;
        }
        if let Gesture::Perform(id, v) = g {
            let _ = self.edits_tx.lock().push((id, v));
        }
        self.driven.lock().push(g);
    }

    fn drive_params_restarted(&self) {
        self.shared.requests().changed.store(true, Ordering::Release);
    }

    /// The plug-in's one state blob, in `component`.
    fn save_state(&self) -> Option<PluginState> {
        if !self.callable() {
            return None;
        }
        let save = unsafe { self.shared.state.as_ref() }.and_then(|s| s.save);
        let component = save.and_then(|f| OutStream::collect(|s| unsafe { f(self.shared.plugin, s) })).unwrap_or_default();
        Some(PluginState { component, controller: Vec::new() })
    }

    fn load_state(&self, st: &PluginState) {
        if !self.callable() || st.component.is_empty() {
            return;
        }
        self.host_set.other();
        if let Some(f) = unsafe { self.shared.state.as_ref() }.and_then(|s| s.load) {
            if !InStream::over(&st.component, |s| unsafe { f(self.shared.plugin, s) }) {
                log::warn!("{} couldn't load its saved state", self.desc.name);
            }
        }
    }
}
