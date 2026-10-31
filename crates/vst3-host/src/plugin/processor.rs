//! The audio-thread half of a plug-in: [`PluginProcessor::process`], and queueing the parameter
//! changes for its next block. Parameter edits from the controller arrive through a ring.
//!
//! Runs on the audio thread or a pool worker, so it must not allocate, lock or block. Switching
//! output buses or a side-chain input stops this half without waiting: see `Gate` and `multi_out.rs`.

use super::*;

impl PluginProcessor {
    pub fn max_block(&self) -> usize {
        self.max_block
    }

    /// Queues a parameter change (normalized value) for the next `process` call, at a sample
    /// offset within that block. Points for one parameter must come in offset order; the
    /// plug-in ramps linearly between them. Never allocates: changes past the queue's
    /// capacity are dropped.
    pub fn queue_param(&mut self, id: u32, offset: i32, value: f64) {
        if self.queued.len() < self.queued.capacity() {
            self.queued.push((id, offset, value.clamp(0.0, 1.0)));
        }
    }

    /// Tells the plug-in processing has stopped, until the next `process` starts it again. VST3
    /// allows this on the audio thread ("UI-thread or processing-thread"), and plug-ins are told
    /// to use it to clear their delay lines and tails; the component stays active, so its state,
    /// parameters and editor remain valid.
    pub fn suspend(&mut self) {
        self.queued.clear();
        // Stopped already, if the controller is changing the active buses.
        if self._shared.out_gate.enter() {
            if self._shared.out_gate.processing.swap(false, Ordering::Relaxed) {
                unsafe {
                    vst3::ComRef::from_raw(self.processor).unwrap().setProcessing(0);
                }
            }
            self._shared.out_gate.leave();
        }
    }

    /// Starts fading to dry, after which `process` stops running the plug-in. A plug-in that
    /// isn't running now (its track is off) stops at once.
    pub fn close_gate(&mut self) {
        if matches!(self.gate, Gate::Open | Gate::Opening { .. }) {
            if self._shared.out_gate.processing.load(Ordering::Relaxed) {
                self.gate = Gate::Closing(self.gate_frames);
            } else {
                self.shut_gate();
            }
        }
    }

    /// Stops the plug-in now, fade or no fade: for one `process` isn't being called for
    /// (bypassed), or hasn't finished fading in time. The main thread may use the component
    /// after this.
    pub fn shut_gate(&mut self) {
        if self._shared.out_gate.processing.swap(false, Ordering::Relaxed) {
            unsafe {
                vst3::ComRef::from_raw(self.processor).unwrap().setProcessing(0);
            }
        }
        self.gate = Gate::Shut;
        self._shared.parked.store(true, Ordering::Release);
    }

    /// Runs the plug-in again, fading in after `settle` frames (its latency).
    pub fn open_gate(&mut self, settle: u32) {
        if matches!(self.gate, Gate::Shut | Gate::Closing(_)) {
            self._shared.parked.store(false, Ordering::Release);
            self.gate = Gate::Opening { settle, left: self.gate_frames };
        }
    }

    /// Mixes what a plug-in just produced with its dry input (in `dry_l`, `dry_r`) as a gate
    /// fades.
    fn fade_gate(&mut self, left: &mut [f32], right: &mut [f32]) {
        let len = self.gate_frames as f32;
        for i in 0..left.len() {
            let wet = match &mut self.gate {
                Gate::Closing(frames) => {
                    *frames = frames.saturating_sub(1);
                    *frames as f32 / len
                }
                Gate::Opening { settle, .. } if *settle > 0 => {
                    *settle -= 1;
                    0.0
                }
                Gate::Opening { left, .. } => {
                    *left = left.saturating_sub(1);
                    1.0 - *left as f32 / len
                }
                _ => 1.0,
            };
            left[i] = self.dry_l[i] + (left[i] - self.dry_l[i]) * wet;
            right[i] = self.dry_r[i] + (right[i] - self.dry_r[i]) * wet;
        }
        match self.gate {
            Gate::Closing(0) => self.shut_gate(),
            Gate::Opening { settle: 0, left: 0 } => self.gate = Gate::Open,
            _ => {}
        }
    }

    /// Parameter edits from the controller (its editor window) wait for the next `process`.
    pub fn has_edits(&self) -> bool {
        !self.edits_rx.is_empty()
    }

    /// Gives a plug-in the engine isn't running (bypassed, say) the edits made in its editor, in
    /// a short block of silence that goes nowhere: its state must have them, for saving and
    /// for undo, and they mustn't wait in the ring until it runs again.
    pub fn pass_edits(&mut self, t: &TransportInfo) {
        if self.edits_rx.is_empty() {
            return;
        }
        let (mut l, mut r) = (std::mem::take(&mut self.edit_l), std::mem::take(&mut self.edit_r));
        l.fill(0.0);
        r.fill(0.0);
        self.process(&mut l, &mut r, &[], t);
        self.edit_l = l;
        self.edit_r = r;
        self.suspend();
    }

    /// Processes one stereo block in place. `left`/`right` must be at most `max_block` long.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], midi: &[MidiEvent], t: &TransportInfo) {
        self.process_keyed(left, right, None, midi, t);
    }

    /// `process`, with a side-chain key for the plug-in's aux input (None: silence there). The
    /// key must be at least as long as the block.
    pub fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: KeyInput, midi: &[MidiEvent], t: &TransportInfo) {
        let n = left.len().min(right.len()).min(self.max_block);
        if n == 0 || matches!(self.gate, Gate::Shut) {
            return;
        }
        if !self._shared.out_gate.enter() {
            self.parked_block(left, right, n);
            return;
        }
        let fading = !matches!(self.gate, Gate::Open);
        if fading {
            self.dry_l[..n].copy_from_slice(&left[..n]);
            self.dry_r[..n].copy_from_slice(&right[..n]);
        }
        unsafe {
            if !self._shared.out_gate.processing.swap(true, Ordering::Relaxed) {
                vst3::ComRef::from_raw(self.processor).unwrap().setProcessing(1);
            }

            self.param_in.clear();
            self.param_out.clear();
            self.events_in.clear();
            self.events_out.clear();
            while let Ok((id, v)) = self.edits_rx.pop() {
                self.param_in.push(id, 0, v);
            }
            for &(id, offset, v) in &self.queued {
                self.param_in.push(id, offset.clamp(0, n as i32 - 1), v);
            }
            self.queued.clear();
            for m in midi {
                self.push_midi(m);
            }

            if let Some(bus) = self.inputs.first_mut() {
                match bus.channels {
                    0 => {}
                    1 => {
                        for i in 0..n {
                            bus.buffers[0][i] = 0.5 * (left[i] + right[i]);
                        }
                    }
                    _ => {
                        bus.buffers[0][..n].copy_from_slice(&left[..n]);
                        bus.buffers[1][..n].copy_from_slice(&right[..n]);
                        for b in bus.buffers.iter_mut().skip(2) {
                            b[..n].fill(0.0);
                        }
                    }
                }
            }
            for bus in self.inputs.iter_mut().skip(1) {
                for b in bus.buffers.iter_mut() {
                    b[..n].fill(0.0);
                }
            }
            let mut key_silence = true;
            if let (Some(bus), Some((kl, kr))) = (self.key_bus.and_then(|k| self.inputs.get_mut(k)), key) {
                key_silence = false;
                match bus.channels {
                    0 => {}
                    1 => {
                        for i in 0..n {
                            bus.buffers[0][i] = 0.5 * (kl[i] + kr[i]);
                        }
                    }
                    _ => {
                        bus.buffers[0][..n].copy_from_slice(&kl[..n]);
                        bus.buffers[1][..n].copy_from_slice(&kr[..n]);
                    }
                }
            }
            for bus in self.outputs.iter_mut() {
                for b in bus.buffers.iter_mut() {
                    b[..n].fill(0.0);
                }
            }
            for (abb, bus) in self.in_abb.iter_mut().zip(self.inputs.iter_mut()) {
                abb.numChannels = bus.channels as i32;
                abb.silenceFlags = 0;
                abb.__field0.channelBuffers32 = bus.ptrs.as_mut_ptr();
            }
            if let (Some(abb), true) = (self.key_bus.and_then(|k| self.in_abb.get_mut(k)), key_silence) {
                abb.silenceFlags = (1u64 << abb.numChannels.clamp(0, 63)) - 1;
            }
            for (abb, bus) in self.out_abb.iter_mut().zip(self.outputs.iter_mut()) {
                abb.numChannels = bus.channels as i32;
                abb.silenceFlags = 0;
                abb.__field0.channelBuffers32 = bus.ptrs.as_mut_ptr();
            }

            let mut ctx: ProcessContext = std::mem::zeroed();
            use ProcessContext_::StatesAndFlags_::*;
            // C enums are i32 in the MSVC bindings and u32 elsewhere; `as _` fits both.
            let mut state = kTempoValid | kTimeSigValid | kProjectTimeMusicValid | kBarPositionValid;
            if t.playing {
                state |= kPlaying;
            }
            if t.recording {
                state |= kRecording;
            }
            ctx.sampleRate = t.sample_rate;
            ctx.projectTimeSamples = t.position_samples;
            ctx.projectTimeMusic = t.position_quarters;
            ctx.barPositionMusic = t.bar_start_quarters;
            ctx.tempo = t.tempo;
            ctx.timeSigNumerator = t.sig_num;
            ctx.timeSigDenominator = t.sig_den;
            if let Some((a, b)) = t.cycle {
                state |= kCycleValid | kCycleActive;
                ctx.cycleStartMusic = a;
                ctx.cycleEndMusic = b;
            }
            ctx.state = state as _;

            let mut data = ProcessData {
                processMode: ProcessModes_::kRealtime as i32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
                numSamples: n as i32,
                numInputs: self.in_abb.len() as i32,
                numOutputs: self.out_abb.len() as i32,
                inputs: if self.in_abb.is_empty() { std::ptr::null_mut() } else { self.in_abb.as_mut_ptr() },
                outputs: if self.out_abb.is_empty() { std::ptr::null_mut() } else { self.out_abb.as_mut_ptr() },
                inputParameterChanges: self.param_in.as_com_ref::<IParameterChanges>().unwrap().as_ptr(),
                outputParameterChanges: self.param_out.as_com_ref::<IParameterChanges>().unwrap().as_ptr(),
                inputEvents: self.events_in.as_com_ref::<IEventList>().unwrap().as_ptr(),
                outputEvents: self.events_out.as_com_ref::<IEventList>().unwrap().as_ptr(),
                processContext: &mut ctx,
            };
            vst3::ComRef::from_raw(self.processor).unwrap().process(&mut data);

            match self.outputs.first() {
                Some(bus) if bus.channels >= 2 => {
                    left[..n].copy_from_slice(&bus.buffers[0][..n]);
                    right[..n].copy_from_slice(&bus.buffers[1][..n]);
                }
                Some(bus) if bus.channels == 1 => {
                    left[..n].copy_from_slice(&bus.buffers[0][..n]);
                    right[..n].copy_from_slice(&bus.buffers[0][..n]);
                }
                _ => {}
            }
            if fading {
                self.fade_gate(&mut left[..n], &mut right[..n]);
            }

            let tx = &mut self.out_tx;
            self.param_out.for_each_last(|id, v| {
                let _ = tx.push((id, v));
            });
        }
        self.after_block(left, right, n);
    }

    unsafe fn push_midi(&mut self, m: &MidiEvent) {
        let status = m.data[0] & 0xF0;
        let channel = (m.data[0] & 0x0F) as i16;
        let offset = m.offset as i32;
        let mut e: Event = std::mem::zeroed();
        e.busIndex = 0;
        e.sampleOffset = offset;
        e.flags = Event_::EventFlags_::kIsLive as u16;
        match status {
            0x90 if m.data[2] > 0 => {
                e.r#type = Event_::EventTypes_::kNoteOnEvent as u16;
                e.__field0.noteOn = NoteOnEvent {
                    channel,
                    pitch: m.data[1] as i16,
                    tuning: 0.0,
                    velocity: m.data[2] as f32 / 127.0,
                    length: 0,
                    noteId: -1,
                };
                self.events_in.push(e);
            }
            0x80 | 0x90 => {
                e.r#type = Event_::EventTypes_::kNoteOffEvent as u16;
                e.__field0.noteOff = NoteOffEvent {
                    channel,
                    pitch: m.data[1] as i16,
                    velocity: m.data[2] as f32 / 127.0,
                    noteId: -1,
                    tuning: 0.0,
                };
                self.events_in.push(e);
            }
            0xB0 => {
                if let Some(id) = self.midi_map[m.data[1] as usize & 0x7F] {
                    self.param_in.push(id, offset, m.data[2] as f64 / 127.0);
                }
            }
            0xD0 => {
                if let Some(id) = self.midi_map[ControllerNumbers_::kAfterTouch as usize] {
                    self.param_in.push(id, offset, m.data[1] as f64 / 127.0);
                }
            }
            0xE0 => {
                if let Some(id) = self.midi_map[ControllerNumbers_::kPitchBend as usize] {
                    let v = ((m.data[2] as u16) << 7 | m.data[1] as u16) as f64 / 16383.0;
                    self.param_in.push(id, offset, v);
                }
            }
            _ => {}
        }
    }
}
