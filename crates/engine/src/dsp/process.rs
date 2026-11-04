//! The block loop: [`Engine::process`] is what the audio callback calls. It applies commands,
//! cuts the device buffer into sub-blocks of at most [`MAX_BLOCK`] frames, and for each one
//! (`process_block`) moves the transport, plays every strip in order, serially or on the worker
//! pool, then mixes the master, the click and the output.
//!
//! One strip's work is in `strip.rs`, what commands do in `apply.rs`, offline renders in
//! `offline.rs`. Everything here runs on the audio thread and must not allocate.

use super::*;

impl Engine {
    /// `input` holds `frames * in_ch` interleaved samples; `output` holds `frames * out_ch`.
    pub fn process(&mut self, input: &[f32], in_ch: usize, output: &mut [f32], out_ch: usize) {
        let started = Instant::now();
        let _ftz = crate::denormals::NoDenormals::new();
        let frames = output.len().checked_div(out_ch).unwrap_or(0);

        // A block was given up on and a strip still isn't done: the strips and everything the
        // block reads belong to the threads playing it, commands wait, and the output is silent.
        if self.parallel.stalled {
            if !self.parallel.open_block_over() {
                output.fill(0.0);
                return;
            }
            self.parallel.stalled = false;
            self.flush_all_notes();
        }

        let mut applied = 0;
        while let Ok(cmd) = self.cmd_rx.pop() {
            self.apply(cmd);
            applied += 1;
        }
        if applied > 0 {
            // Publish the transport as the commands left it before counting them done: once
            // the session sees a locate done, it reads the position from `status.pos`, which
            // must not still hold the old one (see `Session::position`).
            self.status.pos.store(self.pos, Ordering::Relaxed);
            self.status.playing.store(self.playing, Ordering::Relaxed);
            self.status.recording.store(self.recording, Ordering::Relaxed);
            self.status.cmds_done.fetch_add(applied, Ordering::Release);
        }

        self.live.clear();
        while let Ok(m) = self.live_midi.pop() {
            if self.live.len() < self.live.capacity() {
                self.live.push(MidiEvent { offset: 0, data: m });
            }
            self.status.midi_activity.fetch_add(1, Ordering::Relaxed);
        }

        // After an overload: silence, nothing processed, until the thread has caught up. The
        // session stops the transport when it sees the overload count change.
        if self.shedding > 0 {
            self.shedding -= 1;
            output.fill(0.0);
            return;
        }

        let blocks_before = self.status.parallel_blocks.load(Ordering::Relaxed);
        let mut done = 0;
        while done < frames {
            let mut n = (frames - done).min(MAX_BLOCK);
            let (cyc_on, cs, ce) = self.cycle;
            // Cycle recording loops too: each pass becomes a take.
            let looping = self.playing && cyc_on;
            if looping && self.pos < ce && self.pos + n as i64 > ce {
                n = (ce - self.pos) as usize;
            }
            let in_slice = if in_ch > 0 && input.len() >= (done + n) * in_ch { &input[done * in_ch..(done + n) * in_ch] } else { &[][..] };
            match self.process_block(in_slice, in_ch, n) {
                BlockEnd::Played => {}
                // As when the callback's own panic is caught: silence, and the transport waits.
                BlockEnd::Silenced => {
                    self.bus_l[..n].fill(0.0);
                    self.bus_r[..n].fill(0.0);
                }
                // Counted and sent to silence like an overload. The session stops the transport.
                BlockEnd::GivenUp => {
                    self.parallel.stalled = true;
                    output.fill(0.0);
                    let budget = frames as f64 / self.sample_rate;
                    self.block_load.block((started.elapsed().as_secs_f64() / budget) as f32, budget);
                    self.status.cpu_peak.store(self.block_load.peak.to_bits(), Ordering::Relaxed);
                    self.status.late_blocks.store(self.block_load.late, Ordering::Relaxed);
                    self.behind_secs = 0.0;
                    self.shedding = (SHED_SECS * self.sample_rate / frames.max(1) as f64).ceil() as u32;
                    self.status.overload_secs.store((started.elapsed().as_secs_f32()).to_bits(), Ordering::Relaxed);
                    self.status.overload_track.store(self.parallel.stuck_track().unwrap_or(0), Ordering::Relaxed);
                    self.status.overload_stuck.store(true, Ordering::Relaxed);
                    self.status.overloads.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            // The last line of defence: the plug-ins are checked as they run.
            contain(&mut self.bus_l[..n], &mut self.bus_r[..n]);
            for i in 0..n {
                let o = (done + i) * out_ch;
                if out_ch >= 2 {
                    output[o] = self.bus_l[i];
                    output[o + 1] = self.bus_r[i];
                    for c in 2..out_ch {
                        output[o + c] = 0.0;
                    }
                } else {
                    output[o] = 0.5 * (self.bus_l[i] + self.bus_r[i]);
                }
            }
            // Live MIDI is only delivered with the first sub-block.
            self.live.clear();
            if self.playing {
                self.pos += n as i64;
                if looping && self.pos >= ce {
                    self.pos = cs;
                    self.flush_all_notes();
                    // MIDI cycle recording: where one pass ends (a take each, if so set).
                    if self.recording {
                        let _ = self.midi_rec.push((cs, crate::takes::PASS_MARK));
                    }
                }
            }
            done += n;
        }
        let pooled = self.status.parallel_blocks.load(Ordering::Relaxed) != blocks_before;
        if let Some(t) = self.parallel.threads_now(pooled) {
            self.status.threads.store(t, Ordering::Relaxed);
        }

        self.status.pos.store(self.pos, Ordering::Relaxed);
        self.status.playing.store(self.playing, Ordering::Relaxed);
        self.status.recording.store(self.recording, Ordering::Relaxed);

        if frames > 0 {
            let budget = frames as f64 / self.sample_rate;
            let spent = started.elapsed().as_secs_f64();
            let l = (spent / budget) as f32;
            self.load = self.load * 0.9 + l * 0.1;
            self.status.cpu_load.store(self.load.to_bits(), Ordering::Relaxed);
            self.parallel.weigh(l, self.load);
            // A render is meant to run flat out, so it isn't a late block.
            if !self.offline {
                self.block_load.block(l, budget);
                self.status.cpu_peak.store(self.block_load.peak.to_bits(), Ordering::Relaxed);
                self.status.late_blocks.store(self.block_load.late, Ordering::Relaxed);
            }
            // A callback that takes longer than the audio it makes leaves the thread no time
            // to sleep. Kept up, the kernel kills the whole app, so give up a moment of audio.
            // A check that plays flat out on a loaded machine (`ParallelTest::patient`) isn't
            // one either: shedding would make its output depend on the machine's load.
            let late = spent > budget && !self.offline && !self.parallel.test.patient;
            self.behind_secs = if late { self.behind_secs + spent } else { 0.0 };
            if self.behind_secs > OVERLOAD_SECS {
                self.behind_secs = 0.0;
                self.shedding = (SHED_SECS / budget).ceil() as u32;
                self.flush_all_notes();
                let slowest = self.tracks.iter().max_by(|a, b| a.spent.total_cmp(&b.spent)).map_or(0, |t| t.id);
                self.status.overload_secs.store((spent as f32).to_bits(), Ordering::Relaxed);
                self.status.overload_track.store(slowest, Ordering::Relaxed);
                self.status.overload_stuck.store(false, Ordering::Relaxed);
                self.status.overloads.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// What plug-ins are told about the transport for the block starting at the playhead.
    pub fn transport(&self) -> TransportInfo {
        let t = TransportInfo { playing: self.playing, recording: self.recording, sample_rate: self.sample_rate, cycle: None, ..Default::default() };
        let mut t = transport_at(&self.tempo, &t, self.pos);
        if self.cycle.0 {
            t.cycle = Some((self.tempo.beats_at(self.cycle.1 as f64), self.tempo.beats_at(self.cycle.2 as f64)));
        }
        t
    }

    /// For checks, after applying what's queued: whether the task graph holds for the routes.
    #[doc(hidden)]
    pub fn task_graph_fits(&mut self) -> bool {
        self.process(&[], 0, &mut [], 0);
        self.parallel.fits(&self.tracks)
    }

    /// Waits for a block given up on to end (never on the audio thread), so that what it was using
    /// can change or go away.
    pub(crate) fn settle(&mut self) {
        while self.parallel.stalled && !self.parallel.open_block_over() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        self.parallel.stalled = false;
    }

    pub(super) fn process_block(&mut self, input: &[f32], in_ch: usize, n: usize) -> BlockEnd {
        let in_ch = in_ch.min(MAX_INPUTS);
        let stride = input.len() / n.max(1);
        for c in 0..in_ch {
            let buf = &mut self.inputs[c];
            let mut peak = 0.0f32;
            for i in 0..n {
                let s = if stride > 0 { input[i * stride + c] } else { 0.0 };
                buf[i] = s;
                peak = peak.max(s.abs());
            }
            self.status.inputs[c].feed(peak, peak);
        }
        for c in in_ch..MAX_INPUTS {
            self.inputs[c][..n].fill(0.0);
        }
        // Only real input: a device without any shouldn't record silence at its own speed.
        if in_ch > 0 && stride > 0 {
            if let Some((sel, sink)) = &mut self.input_tap {
                for i in 0..n {
                    match *sel {
                        InputSel::Mono(c) => drop(sink.push(self.inputs[c as usize % MAX_INPUTS][i])),
                        InputSel::Stereo(c) => {
                            let c = c as usize % (MAX_INPUTS - 1);
                            let _ = sink.push(self.inputs[c][i]);
                            let _ = sink.push(self.inputs[c + 1][i]);
                        }
                        InputSel::None => {}
                    }
                }
            }
        }

        self.bus_l[..n].fill(0.0);
        self.bus_r[..n].fill(0.0);
        // Isolated renders work out audibility themselves (see `Isolate`).
        let any_solo = self.isolate.is_none() && self.tracks.iter().any(|t| t.params.solo);
        let iso = self.isolate.as_deref();
        let snap_gains = self.snap_gains;
        let transport = self.transport();
        let (pos, playing, recording, punch, sr) = (self.pos, self.playing, self.recording, self.punch, self.sample_rate);
        // Whether regions, notes and the click play (not while a render drains, see `draining`).
        let sources = playing && !self.draining;
        let punch_range = self.punch_range;
        let (capture_from, capture_end) = self.capture.unwrap_or((punch, i64::MAX));
        let auto_monitor = self.auto_monitor;
        let low_latency = self.pdc.low_latency;
        let block_end = pos + n as i64;
        let smooth = 1.0 - (-1.0 / (0.005 * sr as f32)).exp();

        let cx = BlockCtx {
            n,
            pos,
            playing,
            recording,
            sr,
            sources,
            punch_range,
            capture_from,
            capture_end,
            auto_monitor,
            low_latency,
            block_end,
            smooth,
            any_solo,
            snap_gains,
            iso,
            transport,
            tempo: &self.tempo,
            status: &self.status,
            inputs: &self.inputs,
            live: &self.live,
            midi_target: self.midi_target,
            midi_thru: self.midi_thru,
            chase: self.chase,
            analyzer_target: self.analyzer_target,
            wait: self.offline || self.parallel.test.patient,
        };
        let mut end = BlockEnd::Played;
        // Recorded MIDI comes from one track, but the ring has one producer, which `process` also
        // pushes to: this thread, before the strips start.
        if let Some(t) = self.midi_target.and_then(|id| self.tracks.iter().find(|t| t.id == id)).filter(|t| t.kind == TrackKind::Instrument) {
            if recording && t.params.armed && pos >= capture_from && pos < capture_end {
                for m in &self.live {
                    let _ = self.midi_rec.push((pos, m.data));
                }
            }
        }
        if self.parallel.wanted(self.offline, &self.tracks) {
            match self.parallel.run(&mut self.tracks, &cx) {
                parallel::Outcome::Done => {
                    self.status.parallel_blocks.fetch_add(1, Ordering::Relaxed);
                    // What the strips left for Stereo Out, in the order the serial loop adds it.
                    for t in &self.tracks {
                        if t.outs.dest() == Dest::Bus {
                            for i in 0..n {
                                self.bus_l[i] += t.l[i];
                                self.bus_r[i] += t.r[i];
                            }
                        }
                        if let Some(a) = t.outs.analysis(n) {
                            for &s in a {
                                let _ = self.analyzer_tx.push(s);
                            }
                        }
                    }
                }
                parallel::Outcome::Panicked(k) => {
                    self.status.engine_errors.fetch_add(k, Ordering::Relaxed);
                    end = BlockEnd::Silenced;
                }
                parallel::Outcome::Stalled => return BlockEnd::GivenUp,
            }
        } else {
            for ti in 0..self.tracks.len() {
                let (before, rest) = self.tracks.split_at_mut(ti);
                let (cur, tail) = rest.split_first_mut().unwrap();
                let mut routes = Direct { before, tail, ti, bus: (&mut self.bus_l[..n], &mut self.bus_r[..n]), analyzer: &mut self.analyzer_tx };
                let at = Instant::now();
                run_strip(&cx, ti, cur, &mut self.scratch, &mut routes);
                cur.spent = at.elapsed().as_secs_f32();
            }
        }

        // Isolated renders are captured before the Stereo Out strip.
        let master_fx = self.isolate.is_none();
        let (bl, br) = (&mut self.bus_l[..n], &mut self.bus_r[..n]);
        if master_fx {
            self.master_eq_state.process(&self.master_eq, bl, br);
        }
        if self.analyzer_target == Some(MASTER_ID) {
            for i in 0..n {
                let _ = self.analyzer_tx.push(0.5 * (bl[i] + br[i]));
            }
        }
        if master_fx {
            self.automate_master(n);
        }
        let (bl, br) = (&mut self.bus_l[..n], &mut self.bus_r[..n]);
        for ins in self.master_inserts.iter_mut().flatten().filter(|_| master_fx) {
            if !ins.bypass {
                let key = key_signal(ins.key.idx.and_then(|i| self.tracks.get(i)).map(|t| &**t), ins.key.pre, None, n);
                match &mut ins.plugin {
                    Plugin::Builtin(b) => b.process(bl, br, key, &fx_context(&transport)),
                    Plugin::Vst(p) => p.process_keyed(bl, br, key, &[], &transport),
                    p => p.process(bl, br, &[], &transport),
                }
                check_output(&self.status, ins, bl, br);
            } else {
                ins.plugin.pass_edits_aside(&transport);
            }
        }
        let master_target = if master_fx { self.master_auto.volume.unwrap_or(self.master_volume) } else { 1.0 };
        let mut g = if snap_gains { master_target } else { self.master_gain };
        let (mut pl, mut pr) = (0.0f32, 0.0f32);
        for i in 0..n {
            g += (master_target - g) * smooth;
            bl[i] *= g;
            br[i] *= g;
            pl = pl.max(bl[i].abs());
            pr = pr.max(br[i].abs());
        }
        self.master_gain = g;
        self.status.master.feed(pl, pr);
        self.snap_gains = false;

        // The click goes straight to the output, after the Stereo Out fader, as in Logic. The
        // count-in always clicks; otherwise the metronome follows its settings.
        let cp = self.click_params;
        // The music reaches the output `pdc.total` late; the click waits for it.
        let pos = pos - self.pdc.total as i64;
        let counting_in = recording && self.count_in && pos < punch;
        let metro = self.metronome && if recording { cp.while_recording && !cp.only_count_in } else { cp.while_playing };
        let clicking = sources && (metro || counting_in);
        let div = cp.division.clamp(1, 4);
        let map = &*self.tempo;
        // The clicks follow the tempo map's beats (and their subdivisions); the count-in
        // instead counts back from the punch at the tempo and signature in effect there.
        let count_in = CountIn::at(map, punch, div);
        let mut tick = if counting_in { None } else { Some(map.grid_at_or_after(map.beats_at(pos as f64), div)) };
        let mut k = if counting_in { count_in.first_at_or_after(pos) } else { 0 };
        let click_pos = |tick: &mut Option<GridPoint>, k: i64| -> i64 {
            if let Some(g) = tick {
                return g.pos.round() as i64;
            }
            let at = count_in.pos(k);
            if at < punch {
                return at;
            }
            // The count-in hands over to the map's grid at the punch, if the metronome plays on.
            let g = map.grid_at_or_after(map.beats_at(punch as f64), div);
            *tick = Some(g);
            if metro {
                g.pos.round() as i64
            } else {
                i64::MAX
            }
        };
        let mut next = if clicking { click_pos(&mut tick, k) } else { i64::MAX };
        let srf = sr as f32;
        for i in 0..n {
            if pos + i as i64 >= next {
                let (down, on_beat) = match &tick {
                    Some(g) => (cp.accent && g.downbeat(), g.on_beat()),
                    None => (cp.accent && count_in.downbeat(k), count_in.on_beat(k)),
                };
                let (freq, amp) = if down {
                    (cp.bar_freq, cp.level)
                } else if on_beat {
                    (cp.beat_freq, cp.level * 0.64)
                } else {
                    (cp.beat_freq, cp.level * 0.36)
                };
                self.click = Click::start(cp.sound, freq, amp, srf);
                match &mut tick {
                    Some(g) => *g = map.grid_after(g),
                    None => k += 1,
                }
                next = click_pos(&mut tick, k);
            }
            if self.click.remaining > 0 {
                let s = self.click.next(srf);
                self.bus_l[i] += s;
                self.bus_r[i] += s;
            }
        }
        end
    }
}
