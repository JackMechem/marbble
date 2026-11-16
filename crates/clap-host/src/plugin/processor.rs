//! The audio-thread half: [`ClapProcessor`]'s `process`, and the parameter changes and notes it
//! passes in. Runs on the audio thread or a pool worker, so nothing here allocates, locks or
//! blocks; every call into the plug-in is made in its audio-thread role (`AudioRole`).

use clap_sys::events::*;
use clap_sys::fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR};
use clap_sys::process::clap_process;
use plugin_api::{KeyInput, MidiEvent, Processor, TransportInfo};

use super::*;
use crate::events::Event;
use crate::host::AudioRole;

impl ClapProcessor {
    fn plugin(&self) -> &clap_plugin {
        unsafe { &*self.shared.plugin }
    }

    /// Starts processing if it isn't yet. Whether it runs now.
    fn start(&mut self) -> bool {
        if !self.processing {
            let p = self.plugin();
            self.processing = p.start_processing.is_some_and(|f| unsafe { f(self.shared.plugin) });
        }
        self.processing
    }

    /// Stops processing, and with `reset` clears its buffers and voices too.
    fn stop(&mut self, reset: bool) {
        if !std::mem::take(&mut self.processing) {
            return;
        }
        let _role = AudioRole::enter();
        let p = self.plugin();
        unsafe {
            if let Some(f) = p.stop_processing {
                f(self.shared.plugin);
            }
            if let Some(f) = p.reset.filter(|_| reset) {
                f(self.shared.plugin);
            }
        }
    }

    /// The block's input events: edits from the controller, queued automation, then `midi`, in
    /// time order. Values go in the parameters' own units.
    fn gather(&mut self, n: usize, midi: &[MidiEvent]) {
        self.events_in.clear();
        let ranges = &self.events_out.ranges;
        while let Ok((id, v)) = self.edits_rx.pop() {
            if let Some(r) = ranges.get(id) {
                self.events_in.push(Event::param(0, id, r.to_plain(v)));
            }
        }
        for &(id, offset, v) in &self.queued {
            if let Some(r) = ranges.get(id) {
                self.events_in.push(Event::param(offset.clamp(0, n as i32 - 1) as u32, id, r.to_plain(v)));
            }
        }
        self.queued.clear();
        for m in midi {
            if let Some(e) = note_event(self.notes, m, n) {
                self.events_in.push(e);
            }
        }
        self.events_in.sort();
    }

    /// The main input gets the block, the others silence; the outputs are cleared; the buffer
    /// descriptions point at them all.
    fn fill_ports(&mut self, left: &[f32], right: &[f32], n: usize) {
        for (i, port) in self.inputs.iter_mut().enumerate() {
            if Some(i) == self.main_in {
                port.put_stereo(left, right, n);
            } else {
                port.silence(n);
            }
        }
        for port in self.outputs.iter_mut() {
            port.silence(n);
        }
        for (buf, port) in self.in_bufs.iter_mut().zip(self.inputs.iter_mut()).chain(self.out_bufs.iter_mut().zip(self.outputs.iter_mut())) {
            buf.data32 = port.ptrs.as_mut_ptr();
            buf.channel_count = port.channels as u32;
            buf.constant_mask = 0;
        }
    }

    fn read_main_output(&self, left: &mut [f32], right: &mut [f32], n: usize) {
        if let Some((l, r)) = self.main_out.and_then(|i| self.outputs[i].stereo(n)) {
            left[..n].copy_from_slice(l);
            right[..n].copy_from_slice(r);
        }
    }
}

/// A MIDI message as the plug-in takes it: notes as CLAP note events where it takes those,
/// everything as MIDI where it takes that. None for what it can't take.
fn note_event(notes: ports::Notes, m: &MidiEvent, n: usize) -> Option<Event> {
    use ports::Notes;
    let time = (m.offset as usize).min(n.saturating_sub(1)) as u32;
    let status = m.data[0] & 0xF0;
    let is_note = status == 0x80 || status == 0x90;
    match notes {
        Notes::Clap { .. } if is_note => {
            let (channel, key) = ((m.data[0] & 0x0F) as i16, (m.data[1] & 0x7F) as i16);
            let velocity = m.data[2] as f64 / 127.0;
            let on = status == 0x90 && m.data[2] > 0;
            Some(Event::note(time, if on { CLAP_EVENT_NOTE_ON } else { CLAP_EVENT_NOTE_OFF }, channel, key, velocity))
        }
        Notes::Clap { midi: true } | Notes::Midi => Some(Event::midi(time, m.data)),
        _ => None,
    }
}

/// The transport as CLAP describes it, in its fixed-point beats and seconds.
fn transport(t: &TransportInfo) -> clap_event_transport {
    let beats = |q: f64| (q * CLAP_BEATTIME_FACTOR as f64).round() as i64;
    let secs = |s: f64| (s * CLAP_SECTIME_FACTOR as f64).round() as i64;
    let mut flags = CLAP_TRANSPORT_HAS_TEMPO | CLAP_TRANSPORT_HAS_BEATS_TIMELINE | CLAP_TRANSPORT_HAS_SECONDS_TIMELINE | CLAP_TRANSPORT_HAS_TIME_SIGNATURE;
    if t.playing {
        flags |= CLAP_TRANSPORT_IS_PLAYING;
    }
    if t.recording {
        flags |= CLAP_TRANSPORT_IS_RECORDING;
    }
    let seconds = if t.sample_rate > 0.0 { t.position_samples as f64 / t.sample_rate } else { 0.0 };
    let (mut loop_start, mut loop_end) = (0, 0);
    if let Some((a, b)) = t.cycle {
        flags |= CLAP_TRANSPORT_IS_LOOP_ACTIVE;
        (loop_start, loop_end) = (beats(a), beats(b));
    }
    // Seconds at the loop's ends would need the tempo map; they're left at zero, as a host may.
    let bar_len = t.sig_num.max(1) as f64 * 4.0 / t.sig_den.max(1) as f64;
    clap_event_transport {
        header: clap_event_header {
            size: std::mem::size_of::<clap_event_transport>() as u32,
            time: 0,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_TRANSPORT,
            flags: 0,
        },
        flags,
        song_pos_beats: beats(t.position_quarters),
        song_pos_seconds: secs(seconds),
        tempo: t.tempo,
        tempo_inc: 0.0,
        loop_start_beats: loop_start,
        loop_end_beats: loop_end,
        loop_start_seconds: 0,
        loop_end_seconds: 0,
        bar_start: beats(t.bar_start_quarters),
        bar_number: (t.bar_start_quarters / bar_len).round() as i32,
        tsig_num: t.sig_num.clamp(1, u16::MAX as i32) as u16,
        tsig_denom: t.sig_den.clamp(1, u16::MAX as i32) as u16,
    }
}

impl Processor for ClapProcessor {
    fn name(&self) -> &str {
        &self.name
    }

    fn max_block(&self) -> usize {
        self.max_block
    }

    /// The side-chain key isn't passed on yet (#560): `has_key_input` says there's none.
    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], _key: KeyInput, midi: &[MidiEvent], t: &TransportInfo) {
        let n = left.len().min(right.len()).min(self.max_block);
        if n == 0 || self.shut {
            return;
        }
        // Whatever it asked to be run or flushed for, this block does.
        self.shared.take(|r| &r.process);
        let _role = AudioRole::enter();
        if !self.start() {
            return;
        }
        self.gather(n, midi);
        self.fill_ports(left, right, n);
        self.transport = transport(t);
        let process = clap_process {
            steady_time: self.steady,
            frames_count: n as u32,
            transport: &self.transport,
            audio_inputs: if self.in_bufs.is_empty() { std::ptr::null() } else { self.in_bufs.as_ptr() },
            audio_outputs: if self.out_bufs.is_empty() { std::ptr::null_mut() } else { self.out_bufs.as_mut_ptr() },
            audio_inputs_count: self.in_bufs.len() as u32,
            audio_outputs_count: self.out_bufs.len() as u32,
            in_events: self.events_in.raw(),
            out_events: self.events_out.raw(),
        };
        if let Some(f) = self.plugin().process {
            unsafe { f(self.shared.plugin, &process) };
        }
        self.steady += n as i64;
        self.read_main_output(left, right, n);
    }

    fn queue_param(&mut self, id: u32, offset: i32, value: f64) {
        if self.queued.len() < self.queued.capacity() {
            self.queued.push((id, offset, value.clamp(0.0, 1.0)));
        }
    }

    /// Stops processing and resets the plug-in (its delay lines and voices); the next block
    /// starts it again.
    fn suspend(&mut self) {
        self.queued.clear();
        self.stop(true);
    }

    /// CLAP needn't stop a plug-in to switch its ports, so nothing asks this of one yet; it stops
    /// at once.
    fn close_gate(&mut self) {
        self.shut_gate();
    }

    fn shut_gate(&mut self) {
        self.stop(false);
        self.shut = true;
        self.shared.parked.store(true, Ordering::Release);
    }

    fn open_gate(&mut self, _settle: u32) {
        self.shut = false;
        self.shared.parked.store(false, Ordering::Release);
    }

    /// Edits wait, or the plug-in asked to flush its parameters.
    fn has_edits(&self) -> bool {
        !self.edits_rx.is_empty() || self.shared.requests().process.load(Ordering::Acquire)
    }

    fn wants_to_run(&self) -> bool {
        self.has_edits() || !self.queued.is_empty()
    }

    fn ring_out(&self) -> u32 {
        self.ring_out
    }

    /// Hands the waiting edits to the plug-in with `params.flush`, which CLAP has for a plug-in
    /// that isn't being processed.
    fn pass_edits(&mut self, _t: &TransportInfo) {
        if !self.has_edits() {
            return;
        }
        self.shared.take(|r| &r.process);
        let _role = AudioRole::enter();
        self.gather(1, &[]);
        let flush = unsafe { self.shared.params.as_ref() }.and_then(|p| p.flush);
        if let Some(f) = flush {
            let (input, output) = (self.events_in.raw(), self.events_out.raw());
            unsafe { f(self.shared.plugin, input, output) };
        }
        self.events_in.clear();
    }

    /// The main output only, as bus 0: extra output ports come with #560.
    fn output_bus(&self, bus: usize, n: usize) -> Option<(&[f32], &[f32])> {
        if bus != 0 {
            return None;
        }
        self.outputs.get(self.main_out?)?.stereo(n)
    }
}

impl Drop for ClapProcessor {
    /// Dropped on the main thread (as the engine's garbage), but stopping is the audio thread's:
    /// the processor is out of the engine by now, so the main thread takes that role for it.
    fn drop(&mut self) {
        self.stop(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports::Notes;

    fn midi(offset: u32, data: [u8; 3]) -> MidiEvent {
        MidiEvent { offset, data }
    }

    fn kind(e: Option<Event>) -> Option<(u32, u16)> {
        e.map(|e| unsafe { (e.time(), (*(&e as *const Event as *const clap_event_header)).type_) })
    }

    #[test]
    fn notes_go_as_the_plug_in_takes_them() {
        let on = midi(5, [0x90, 60, 100]);
        let off = midi(6, [0x90, 60, 0]);
        let cc = midi(7, [0xB0, 1, 64]);
        let clap = Notes::Clap { midi: false };
        assert_eq!(kind(note_event(clap, &on, 64)), Some((5, CLAP_EVENT_NOTE_ON)));
        assert_eq!(kind(note_event(clap, &off, 64)), Some((6, CLAP_EVENT_NOTE_OFF)), "velocity 0 is a note off");
        assert_eq!(kind(note_event(clap, &cc, 64)), None);
        assert_eq!(kind(note_event(Notes::Clap { midi: true }, &cc, 64)), Some((7, CLAP_EVENT_MIDI)));
        assert_eq!(kind(note_event(Notes::Midi, &on, 64)), Some((5, CLAP_EVENT_MIDI)));
        assert_eq!(kind(note_event(Notes::None, &on, 64)), None);
        assert_eq!(kind(note_event(clap, &midi(99, [0x80, 60, 0]), 64)), Some((63, CLAP_EVENT_NOTE_OFF)), "kept inside the block");
    }

    #[test]
    fn the_transport_is_in_fixed_point() {
        let t = TransportInfo {
            playing: true,
            sample_rate: 48_000.0,
            tempo: 120.0,
            sig_num: 3,
            sig_den: 4,
            position_samples: 96_000,
            position_quarters: 4.0,
            bar_start_quarters: 3.0,
            cycle: Some((0.0, 8.0)),
            ..Default::default()
        };
        let c = transport(&t);
        assert_eq!(c.song_pos_beats, 4 * CLAP_BEATTIME_FACTOR);
        assert_eq!(c.song_pos_seconds, 2 * CLAP_SECTIME_FACTOR);
        assert_eq!(c.loop_end_beats, 8 * CLAP_BEATTIME_FACTOR);
        assert_eq!((c.tsig_num, c.tsig_denom, c.bar_number), (3, 4, 1));
        assert!(c.flags & CLAP_TRANSPORT_IS_PLAYING != 0 && c.flags & CLAP_TRANSPORT_IS_LOOP_ACTIVE != 0);
        assert!(c.flags & CLAP_TRANSPORT_IS_RECORDING == 0);
    }
}
