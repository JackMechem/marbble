//! The main-thread half of a plug-in: its parameters, state, editor window, and what it reports
//! through the host (latency changes, restarts, gestures from its editor).
//!
//! VST3 wants the edit controller on the main thread, so everything here runs there. The two halves
//! share their COM objects through `Shared`, and their values pass through rings and atomics.

use super::*;

use std::cell::RefCell;

thread_local! {
    /// Editors closed while a thread was stuck in their plug-in (`set_unresponsive`), hidden but not
    /// told, each with the plug-in's COM objects: closed for real once both halves are gone
    /// (`close_stranded_editors`).
    static STRANDED: RefCell<Vec<(crate::editor::Editor, Arc<Shared>)>> = const { RefCell::new(Vec::new()) };
}

/// Closes the editors of plug-ins that stopped responding (`PluginController::set_unresponsive`)
/// whose instance is gone since: its processor came back from the thread stuck in it and was
/// dropped, and its controller was too. Call periodically on the thread that closed them (the UI
/// thread).
pub fn close_stranded_editors() {
    let done: Vec<_> = STRANDED.with(|s| {
        let mut s = s.borrow_mut();
        let (done, keep) = std::mem::take(&mut *s).into_iter().partition(|(_, shared)| Arc::strong_count(shared) == 1);
        *s = keep;
        done
    });
    for (e, shared) in done {
        e.close(shared);
    }
}

impl PluginController {
    /// Says whether a thread is stuck in the processor's `process` (the host left it there). While
    /// it is, nothing here calls into the plug-in, which may be holding a lock its controller and
    /// editor need: `idle` only handles the editor window's own events, parameter values read as
    /// unknown and edits go nowhere, the editor doesn't open, closing it only hides it until the
    /// instance is gone (`close_stranded_editors`), `latency` is the last one reported, `save_state`
    /// gives None, and output buses and the side-chain input don't switch.
    pub fn set_unresponsive(&mut self, on: bool) {
        self.unresponsive = on;
        if let Some(e) = &self.editor {
            e.set_quiet(on);
        }
    }

    /// The instance's number, from 1 up in the order instances were made in this process: one made
    /// after `instances_made` gave `n` has a higher number than `n`.
    pub fn instance(&self) -> u64 {
        self.shared.instance
    }

    /// Whether the plug-in may be called (see `set_unresponsive`).
    pub fn responsive(&self) -> bool {
        !self.unresponsive
    }

    pub fn has_editor(&self) -> bool {
        self.shared.controller.is_some()
    }

    pub fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    /// Call periodically on the UI thread.
    pub fn idle(&mut self) {
        if self.unresponsive {
            // The window's own events, which reach the plug-in no more (`set_quiet`), so that a
            // close is seen.
            if self.editor.as_mut().is_some_and(|e| e.pump()) {
                self.close_editor();
            }
            return;
        }
        if let Some(c) = &self.shared.controller {
            while let Ok((id, v)) = self.out_rx.pop() {
                unsafe {
                    c.setParamNormalized(id, v);
                }
            }
        }
        let closed = self.editor.as_mut().map(|e| e.pump()).unwrap_or(false);
        if closed {
            self.close_editor();
        }
        let flags = self.shared._handler.restart_flags.swap(0, Ordering::Relaxed);
        if flags != 0 {
            log::debug!("{}: restartComponent({flags:#x})", self.desc.name);
        }
        if flags & RestartFlags_::kLatencyChanged != 0 {
            self.latency_changed = true;
        }
        if flags & RestartFlags_::kParamValuesChanged != 0 {
            self.params_restarted = true;
        }
    }

    /// The processing latency the plug-in reports, in samples (`getLatencySamples`). Its
    /// output lags its input by this much; the host compensates the other paths.
    pub fn latency(&self) -> u32 {
        if self.unresponsive {
            return self.last_latency.load(Ordering::Relaxed);
        }
        let l = match &self.shared.processor {
            Some(p) => unsafe { p.getLatencySamples() },
            None => 0,
        };
        self.last_latency.store(l, Ordering::Relaxed);
        l
    }

    /// Whether the plug-in reported a new latency since the last call (checked in `idle`).
    pub fn take_latency_changed(&mut self) -> bool {
        std::mem::take(&mut self.latency_changed)
    }

    /// Whether the plug-in has an aux input a side-chain key can reach.
    pub fn has_key_input(&self) -> bool {
        self.key_bus.is_some()
    }

    /// Whether the aux input is active: the plug-in hears the key rather than its internal one.
    pub fn key_active(&self) -> bool {
        self.key_active
    }

    /// Whether the audio thread has stopped running the processor (see `Gate`).
    pub fn parked(&self) -> bool {
        self.shared.parked.load(Ordering::Acquire)
    }

    /// Activates or deactivates the aux input, which VST3 only allows on an inactive component:
    /// `setActive(false)`, the bus, `setActive(true)`. Its parameters and state stay; its
    /// delay lines and tails may not. The processor's gate must be shut first (`parked`), or
    /// the instance not yet in use. Errors, with the side-chain gone, if the plug-in refuses.
    pub fn set_key_active(&mut self, on: bool) -> Result<(), String> {
        let Some(bus) = self.key_bus.filter(|_| on != self.key_active) else { return Ok(()) };
        if self.unresponsive {
            return Err(format!("{} isn't responding", self.desc.name));
        }
        let refused = unsafe {
            self.shared.component.setActive(0);
            let r = self.shared.component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32, bus as i32, on as TBool);
            if self.shared.component.setActive(1) != kResultOk {
                log::warn!("{}: setActive failed", self.desc.name);
            }
            on && r != kResultOk
        };
        if refused {
            self.key_bus = None;
            return Err(format!("{} refused its side-chain input", self.desc.name));
        }
        self.key_active = on;
        Ok(())
    }

    /// The plug-in's audio buses as it reports them now (none while it mustn't be called, see
    /// `set_unresponsive`).
    pub fn audio_buses(&self) -> Vec<BusLayout> {
        // C enums are i32 in the MSVC bindings and u32 elsewhere; `as _` fits both.
        let default_active: u32 = BusInfo_::BusFlags_::kDefaultActive as _;
        let mut out = Vec::new();
        if self.unresponsive {
            return out;
        }
        for dir in [BusDirections_::kInput as i32, BusDirections_::kOutput as i32] {
            let count = unsafe { self.shared.component.getBusCount(MediaTypes_::kAudio as i32, dir) };
            for i in 0..count {
                unsafe {
                    let mut info: BusInfo = std::mem::zeroed();
                    if self.shared.component.getBusInfo(MediaTypes_::kAudio as i32, dir, i, &mut info) != kResultOk {
                        continue;
                    }
                    let mut arr: SpeakerArrangement = 0;
                    if let Some(p) = &self.shared.processor {
                        p.getBusArrangement(dir, i, &mut arr);
                    }
                    out.push(BusLayout {
                        name: utf16(&info.name),
                        input: dir == BusDirections_::kInput as i32,
                        aux: info.busType == BusTypes_::kAux as BusType,
                        default_active: info.flags & default_active != 0,
                        channels: channel_count(arr),
                    });
                }
            }
        }
        out
    }

    /// The controller's automatable parameters (hidden, read-only and program-change ones are
    /// left out).
    pub fn parameters(&self) -> Vec<ParamInfo> {
        use ParameterInfo_::ParameterFlags_::*;
        let Some(c) = self.shared.controller.as_ref().filter(|_| !self.unresponsive) else { return Vec::new() };
        let mut out = Vec::new();
        unsafe {
            for i in 0..c.getParameterCount() {
                let mut info: ParameterInfo = std::mem::zeroed();
                if c.getParameterInfo(i, &mut info) != kResultOk {
                    continue;
                }
                if info.flags & kCanAutomate == 0 || info.flags & (kIsHidden | kIsReadOnly | kIsProgramChange) != 0 {
                    continue;
                }
                out.push(ParamInfo {
                    id: info.id,
                    name: utf16(&info.title),
                    units: utf16(&info.units),
                    default: info.defaultNormalizedValue,
                    steps: info.stepCount,
                });
            }
        }
        out
    }

    /// Every parameter's id, name and whether it's read-only (a meter), hidden ones included:
    /// what the editor can send, unlike `parameters`.
    pub fn param_names(&self) -> Vec<(u32, String, bool)> {
        use ParameterInfo_::ParameterFlags_::*;
        let Some(c) = self.shared.controller.as_ref().filter(|_| !self.unresponsive) else { return Vec::new() };
        let mut out = Vec::new();
        unsafe {
            for i in 0..c.getParameterCount() {
                let mut info: ParameterInfo = std::mem::zeroed();
                if c.getParameterInfo(i, &mut info) == kResultOk {
                    out.push((info.id, utf16(&info.title), info.flags & kIsReadOnly != 0));
                }
            }
        }
        out
    }

    /// The controller's current normalized value of a parameter.
    pub fn param_value(&self, id: u32) -> Option<f64> {
        self.shared.controller.as_ref().filter(|_| !self.unresponsive).map(|c| unsafe { c.getParamNormalized(id) })
    }

    /// Moves the controller (and so the editor) to a value the host is playing, such as
    /// automation. Doesn't reach the processor: queue that with `PluginProcessor::queue_param`.
    pub fn set_param_value(&self, id: u32, value: f64) {
        if self.unresponsive {
            return;
        }
        self.host_changes.fetch_add(1, Ordering::Relaxed);
        self.host_set.lock().insert(id, (value.clamp(0.0, 1.0), Instant::now()));
        if let Some(c) = &self.shared.controller {
            unsafe {
                c.setParamNormalized(id, value.clamp(0.0, 1.0));
            }
        }
    }

    /// Sets a parameter as a control surface would: moves the controller (and so the editor)
    /// and queues the value for the processor, like an edit made in the plug-in's own editor.
    pub fn edit_param(&self, id: u32, value: f64) {
        if self.unresponsive {
            return;
        }
        let v = value.clamp(0.0, 1.0);
        self.set_param_value(id, v);
        let _ = self.shared._handler.edits.lock().push((id, v));
    }

    /// How many times the host has set a parameter or loaded a state: when it changes, the
    /// plug-in's state has changed by something other than its editor.
    pub fn host_changes(&self) -> u64 {
        self.host_changes.load(Ordering::Relaxed)
    }

    /// Whether an edit from the plug-in only repeats what the host set a moment ago: an editor
    /// answering `setParamNormalized` with `performEdit`, as some do.
    pub fn host_echo(&self, id: u32, value: f64) -> bool {
        let mut set = self.host_set.lock();
        let echo = set.get(&id).is_some_and(|(v, at)| (v - value).abs() < 1e-6 && at.elapsed() < HOST_ECHO);
        if !echo {
            set.remove(&id);
        }
        echo
    }

    /// Whether edits from the editor are still on their way to the processor.
    pub fn edits_pending(&self) -> bool {
        let edits = self.shared._handler.edits.lock();
        edits.slots() < edits.buffer().capacity()
    }

    /// Whether the plug-in reported that its parameters changed since the last call (checked in
    /// `idle`).
    pub fn take_params_restarted(&mut self) -> bool {
        std::mem::take(&mut self.params_restarted)
    }

    /// Calls the host's `IComponentHandler` as the plug-in's editor would, having moved the
    /// controller first as an editor does: for headless checks, where no editor can open.
    #[doc(hidden)]
    pub fn drive_handler(&self, g: Gesture) {
        let Some(h) = self.shared._handler.to_com_ptr::<IComponentHandler>().filter(|_| !self.unresponsive) else { return };
        unsafe {
            match g {
                Gesture::Begin(id) => h.beginEdit(id),
                Gesture::Perform(id, v) => {
                    if let Some(c) = &self.shared.controller {
                        c.setParamNormalized(id, v);
                    }
                    h.performEdit(id, v)
                }
                Gesture::End(id) => h.endEdit(id),
            };
        }
    }

    /// `restartComponent(kParamValuesChanged)` through the same handler, for headless checks.
    #[doc(hidden)]
    pub fn drive_params_restarted(&self) {
        if let Some(h) = self.shared._handler.to_com_ptr::<IComponentHandler>() {
            unsafe {
                h.restartComponent(RestartFlags_::kParamValuesChanged as _);
            }
        }
    }

    /// The editor's begin/perform/end edit calls since the last call, oldest first.
    pub fn take_gestures(&mut self) -> Vec<Gesture> {
        let mut out = Vec::new();
        while let Ok(g) = self.gestures_rx.pop() {
            out.push(g);
        }
        out
    }

    pub fn open_editor(&mut self, title: &str) -> Result<(), String> {
        if self.unresponsive {
            return Err(format!("{} isn't responding", self.desc.name));
        }
        if self.editor.is_some() {
            if let Some(e) = &self.editor {
                e.raise();
            }
            return Ok(());
        }
        let c = self.shared.controller.as_ref().ok_or("plugin has no editor")?;
        let view = unsafe {
            let v = c.createView(ViewType::kEditor);
            ComPtr::<IPlugView>::from_raw(v).ok_or("plugin did not create an editor view")?
        };
        self.editor = Some(crate::editor::Editor::open(view, title)?);
        Ok(())
    }

    pub fn close_editor(&mut self) {
        let Some(e) = self.editor.take() else { return };
        if self.unresponsive {
            // Telling the view it's removed would wait on the plug-in.
            e.hide();
            STRANDED.with(|s| s.borrow_mut().push((e, self.shared.clone())));
            return;
        }
        e.close(self.shared.clone());
    }

    /// The plug-in's state (`getState`), or None while it mustn't be called (`set_unresponsive`):
    /// keep the one saved last then.
    pub fn save_state(&self) -> Option<PluginState> {
        if self.unresponsive {
            return None;
        }
        let mut st = PluginState::default();
        unsafe {
            let s = MemoryStream::new(Vec::new());
            if self.shared.component.getState(as_stream(&s).as_ptr()) == kResultOk {
                st.component = s.data();
            }
            if let Some(c) = &self.shared.controller {
                let s = MemoryStream::new(Vec::new());
                if c.getState(as_stream(&s).as_ptr()) == kResultOk {
                    st.controller = s.data();
                }
            }
        }
        Some(st)
    }

    /// Does nothing while the plug-in mustn't be called (`set_unresponsive`).
    pub fn load_state(&self, st: &PluginState) {
        if self.unresponsive {
            return;
        }
        self.host_changes.fetch_add(1, Ordering::Relaxed);
        unsafe {
            if !st.component.is_empty() {
                let s = MemoryStream::new(st.component.clone());
                self.shared.component.setState(as_stream(&s).as_ptr());
                if let Some(c) = &self.shared.controller {
                    s.rewind();
                    c.setComponentState(as_stream(&s).as_ptr());
                }
            }
            if let (Some(c), false) = (&self.shared.controller, st.controller.is_empty()) {
                let s = MemoryStream::new(st.controller.clone());
                c.setState(as_stream(&s).as_ptr());
            }
        }
    }
}

impl Drop for PluginController {
    fn drop(&mut self) {
        self.close_editor();
    }
}
