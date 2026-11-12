//! The block loop: [`Engine::process_io`] is what the audio callback calls ([`Engine::process`]
//! over whole buffers). It applies commands, cuts the device buffer into sub-blocks of at most [`MAX_BLOCK`] frames, and for each one
//! (`process_block`) moves the transport, plays every strip in order, serially or on the worker
//! pool, then mixes the master, the click and the output.
//!
//! One strip's work is in `strip.rs`, what commands do in `apply.rs`, offline renders in
//! `offline.rs`. Everything here runs on the audio thread and must not allocate.

use super::*;

/// Where [`Engine::process_io`] takes a callback's input from and puts its output, one sub-block
/// at a time, so a device buffer of any length needs no buffer of that length: the audio
/// callback converts each sub-block straight into the device's format (`audio_io::callback`).
/// Everything here runs on the audio thread.
pub trait BlockIo {
    /// The input for the callback's next `n` frames (at most [`MAX_BLOCK`]): `n * in_ch`
    /// interleaved samples, or none.
    fn input(&mut self, n: usize) -> &[f32];
    /// Stereo Out for the callback's next `l.len()` frames.
    fn output(&mut self, l: &[f32], r: &[f32]);
    /// Silences the whole callback, from its first frame.
    fn silence(&mut self);
}

/// Interleaves Stereo Out into a device's `out_ch` channels, through `to`: extra channels get
/// silence, a mono device the sum of both sides at half level.
pub fn interleave<T: Copy>(l: &[f32], r: &[f32], output: &mut [T], out_ch: usize, to: impl Fn(f32) -> T) {
    for ((frame, &l), &r) in output.chunks_exact_mut(out_ch).zip(l).zip(r) {
        if out_ch >= 2 {
            frame[0] = to(l);
            frame[1] = to(r);
            frame[2..].fill(to(0.0));
        } else {
            frame[0] = to(0.5 * (l + r));
        }
    }
}

/// [`BlockIo`] over whole interleaved buffers, for [`Engine::process`].
struct Slices<'a> {
    input: &'a [f32],
    in_ch: usize,
    output: &'a mut [f32],
    out_ch: usize,
    /// Frames handed in and out so far.
    at_in: usize,
    at_out: usize,
}

impl BlockIo for Slices<'_> {
    fn input(&mut self, n: usize) -> &[f32] {
        let (from, to) = (self.at_in * self.in_ch, (self.at_in + n) * self.in_ch);
        self.at_in += n;
        if self.in_ch > 0 && self.input.len() >= to {
            &self.input[from..to]
        } else {
            &[]
        }
    }

    fn output(&mut self, l: &[f32], r: &[f32]) {
        let at = self.at_out * self.out_ch;
        interleave(l, r, &mut self.output[at..at + l.len() * self.out_ch], self.out_ch, |s| s);
        self.at_out += l.len();
    }

    fn silence(&mut self) {
        self.output.fill(0.0);
    }
}

impl Engine {
    /// `input` holds `frames * in_ch` interleaved samples; `output` holds `frames * out_ch`.
    pub fn process(&mut self, input: &[f32], in_ch: usize, output: &mut [f32], out_ch: usize) {
        let frames = output.len().checked_div(out_ch).unwrap_or(0);
        self.process_io(frames, in_ch, &mut Slices { input, in_ch, output, out_ch, at_in: 0, at_out: 0 });
    }

    /// One device callback of `frames` frames, whatever its length: its input read from `io` and
    /// its output written there a sub-block at a time. Commands, live MIDI and the load measured
    /// are the callback's, as for [`Engine::process`].
    pub fn process_io(&mut self, frames: usize, in_ch: usize, io: &mut impl BlockIo) {
        let started = Instant::now();
        let _ftz = crate::dsp::denormals::NoDenormals::new();
        self.ahead.until = (frames > 0).then(|| started + std::time::Duration::from_secs_f64(frames as f64 / self.sample_rate * ahead::WAIT_SHARE));

        // A block was given up on and a strip still isn't done: the strips and everything the
        // block reads belong to the threads playing it, commands wait, and the output is silent.
        // A worker that has gone into a plug-in that doesn't return is left there meanwhile.
        if self.parallel.stalled {
            if !self.parallel.open_block_over() && !self.parallel.press_on() {
                io.silence();
                return;
            }
            self.parallel.stalled = false;
            self.leave_out();
            self.flush_all_notes();
        }
        self.release_held();

        self.take_commands();
        self.take_live_midi();

        // After an overload: silence, nothing processed, until the thread has caught up. The
        // session stops the transport when it sees the overload count change.
        if self.shedding > 0 {
            self.shedding -= 1;
            io.silence();
            return;
        }

        if frames > 0 {
            self.status.blocks.fetch_add(1, Ordering::Relaxed);
        }
        let blocks_before = self.status.parallel_blocks.load(Ordering::Relaxed);
        let mut done = 0;
        while done < frames {
            let mut n = (frames - done).min(MAX_BLOCK);
            let (cyc_on, _, cycle_end) = self.cycle;
            // Cycle recording loops too: each pass becomes a take.
            let looping = self.playing && cyc_on;
            if looping && self.pos < cycle_end && self.pos + n as i64 > cycle_end {
                n = (cycle_end - self.pos) as usize;
            }
            match self.process_block(io.input(n), in_ch, n) {
                BlockEnd::Played => {}
                // As when the callback's own panic is caught: silence, and the transport waits.
                BlockEnd::Silenced => {
                    self.bus_l[..n].fill(0.0);
                    self.bus_r[..n].fill(0.0);
                }
                end @ (BlockEnd::GivenUp | BlockEnd::LeftOut) => {
                    io.silence();
                    self.give_up_block(matches!(end, BlockEnd::LeftOut), started, frames);
                    return;
                }
            }
            // The last line of defence: the plug-ins are checked as they run.
            contain(&mut self.bus_l[..n], &mut self.bus_r[..n]);
            io.output(&self.bus_l[..n], &self.bus_r[..n]);
            // Live MIDI is only delivered with the first sub-block.
            self.live.clear();
            self.advance(n, looping);
            done += n;
        }
        let pooled = self.status.parallel_blocks.load(Ordering::Relaxed) != blocks_before;
        if let Some(t) = self.parallel.threads_now(pooled) {
            self.status.threads.store(t, Ordering::Relaxed);
        }

        self.publish_transport();
        if frames > 0 {
            self.measure_load(started, frames);
        }
        self.lend_ahead(frames, started);
    }

    /// The MIDI played live since the last callback, for this one's first sub-block.
    fn take_live_midi(&mut self) {
        self.live.clear();
        while let Ok(m) = self.live_midi.pop() {
            if self.live.len() < self.live.capacity() {
                self.live.push(MidiEvent { offset: 0, data: m });
            }
            self.status.midi_activity.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Moves the playhead past a sub-block of `n` frames: back to the cycle's start at its end,
    /// and a Stop's drain (see `stopping`) nearer its end.
    fn advance(&mut self, n: usize, looping: bool) {
        let (_, cycle_start, cycle_end) = self.cycle;
        if self.playing {
            self.pos += n as i64;
            if looping && self.pos >= cycle_end {
                self.pos = cycle_start;
                self.flush_home_notes();
                // MIDI cycle recording: where one pass ends (a take each, if so set).
                if self.recording && self.midi_rec.push((cycle_start, crate::edit::takes::recording::PASS_MARK)).is_err() {
                    self.status.midi_rec_lost.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if let Some((_, left)) = &mut self.stopping {
            *left -= n as i64;
            if *left <= 0 {
                self.finish_stop();
            }
        }
    }

    /// A block the pool didn't finish, counted and sent to silence like an overload. The session
    /// stops the transport, unless the strips held up in plug-ins were left out (`left_out`) and
    /// the rest plays on.
    fn give_up_block(&mut self, left_out: bool, started: Instant, frames: usize) {
        let track = self.parallel.stuck_track().unwrap_or(0);
        if left_out {
            self.leave_out();
            self.flush_all_notes();
        } else {
            self.parallel.stalled = true;
        }
        let budget = frames as f64 / self.sample_rate;
        self.note_block_load((started.elapsed().as_secs_f64() / budget) as f32, budget);
        self.behind_secs = 0.0;
        self.shedding = (SHED_SECS * self.sample_rate / frames.max(1) as f64).ceil() as u32;
        self.report_overload(started.elapsed().as_secs_f32(), track, true, left_out);
    }

    /// Weighs the callback's time against the `frames` of audio it made: the CPU meters, the pool's
    /// thread count, and the overload that sheds a moment of audio when callbacks keep running late.
    fn measure_load(&mut self, started: Instant, frames: usize) {
        let budget = frames as f64 / self.sample_rate;
        let spent = started.elapsed().as_secs_f64();
        let l = (spent / budget) as f32;
        self.load = self.load * 0.9 + l * 0.1;
        self.status.cpu_load.store(self.load.to_bits(), Ordering::Relaxed);
        let standby = self.standby_load(frames);
        self.parallel.weigh(l, self.load, standby);
        // A render is meant to run flat out, so it isn't a late block.
        if !self.offline {
            self.note_block_load(l, budget);
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
            self.report_overload(spent as f32, slowest, false, false);
        }
    }

    /// The share of a block that could come back at once: what the strips whose plug-ins are asleep,
    /// or whose source is rendered ahead, took when they last played whole here. Each strip that
    /// played whole this callback updates its own.
    fn standby_load(&mut self, frames: usize) -> f32 {
        let budget = (frames as f64 / self.sample_rate) as f32;
        let mut standby = 0.0;
        for t in &mut self.tracks {
            let away = t.ahead.as_ref().is_some_and(|a| a.lent());
            if away || t.napping {
                standby += t.awake_share;
            } else if budget > 0.0 && !t.left_out && !t.dormant {
                let share = t.spent / budget;
                // Unmeasured, so taken whole rather than smoothed up from nothing.
                t.awake_share = if t.awake_share == 0.0 { share } else { t.awake_share * 0.9 + share * 0.1 };
            }
        }
        standby
    }

    /// Takes in a block that used `load` of its `budget` seconds, for the peak meter and the count
    /// of late blocks.
    fn note_block_load(&mut self, load: f32, budget: f64) {
        self.block_load.block(load, budget);
        self.status.block_loads.add(load);
        self.status.cpu_peak.store(self.block_load.peak.to_bits(), Ordering::Relaxed);
        self.status.late_blocks.store(self.block_load.late, Ordering::Relaxed);
    }

    /// Tells the session about an overload that took `secs`, blamed on `track`: `stuck` if a strip
    /// didn't finish, `left_out` if the strips held up were left out and the rest plays on.
    fn report_overload(&self, secs: f32, track: Id, stuck: bool, left_out: bool) {
        self.status.overload_secs.store(secs.to_bits(), Ordering::Relaxed);
        self.status.overload_track.store(track, Ordering::Relaxed);
        self.status.overload_stuck.store(stuck, Ordering::Relaxed);
        self.status.overload_left_out.store(left_out, Ordering::Relaxed);
        self.status.overloads.fetch_add(1, Ordering::Relaxed);
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

    /// The Stereo Out gain the last `Cmd::MasterVolume` set.
    #[cfg(test)]
    pub(crate) fn master_volume_for_tests(&self) -> f32 {
        self.master_volume
    }

    /// The ids of the strips the engine plays, in order.
    #[cfg(test)]
    pub(crate) fn track_ids_for_tests(&self) -> Vec<Id> {
        self.tracks.iter().map(|t| t.id).collect()
    }

    /// Whether a block given up on is still open.
    #[cfg(test)]
    pub(crate) fn stalled_for_tests(&self) -> bool {
        self.parallel.stalled
    }

    /// Whether blocks on the pool play on a graph the engine built itself.
    #[cfg(test)]
    pub(crate) fn parallel_own_for_tests(&self) -> bool {
        self.parallel.own
    }

    /// Applies every command queued, in order, and counts them done.
    fn take_commands(&mut self) {
        let mut applied = 0;
        while let Ok(cmd) = self.cmd_rx.pop() {
            self.apply(cmd);
            applied += 1;
        }
        if applied > 0 {
            // Publish the transport as the commands left it before counting them done: once
            // the session sees a locate done, it reads the position from `status.pos`, which
            // must not still hold the old one (see `Session::position`).
            self.publish_transport();
            self.status.cmds_done.fetch_add(applied, Ordering::Release);
        }
    }

    /// Applies what the audio thread has yet to take, from the session's thread with the engine
    /// locked and no device calling back (or before a graph is swapped in), once a block given up
    /// on is over: commands are for the graph they were sent to, never one replacing it.
    pub(crate) fn take_queued(&mut self) {
        self.settle();
        self.take_commands();
    }

    /// Waits for a block given up on to end (never on the audio thread), so that what it was using
    /// can change or go away.
    pub(crate) fn settle(&mut self) {
        while self.parallel.stalled && !self.parallel.open_block_over() && !self.parallel.press_on() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if std::mem::take(&mut self.parallel.stalled) {
            self.leave_out();
        }
    }

    /// Once a block in which workers were left in plug-ins is over: their strips give way to
    /// stand-ins, which sit out from now on with everything they feed (`Parallel::leave_out`).
    fn leave_out(&mut self) {
        if self.parallel.leave_out(&mut self.tracks, &self.status) {
            // A stand-in has no routes or keys of its own.
            self.resolve_routes();
        }
    }

    /// Strips whose threads have come back out of the plug-in they were left in go back as
    /// garbage, and the session brings their tracks back (`Cmd::Back`); so do workers replaced
    /// since, once they hold no strip.
    pub(super) fn release_held(&mut self) {
        while let Some(g) = self.parallel.take_released(&self.status) {
            self.discard(g);
        }
    }

    /// One sub-block of at most [`MAX_BLOCK`] frames: the inputs read, every strip played (serially
    /// or on the pool), then Stereo Out's plug-ins and fader, and the click.
    pub(super) fn process_block(&mut self, input: &[f32], in_ch: usize, n: usize) -> BlockEnd {
        if self.read_inputs(input, in_ch, n) {
            self.feed_input_tap(n);
        }
        self.bus_l[..n].fill(0.0);
        self.bus_r[..n].fill(0.0);

        let (pos, playing, recording, punch, sr) = (self.pos, self.playing, self.recording, self.punch, self.sample_rate);
        // Whether regions, notes and the click play (not while a render drains, see `draining`).
        let sources = playing && !self.draining;
        let (capture_from, capture_end) = self.capture.unwrap_or((punch, i64::MAX));
        self.record_live_midi(capture_from, capture_end);

        let snap_gains = self.snap_gains;
        let transport = self.transport();
        let smooth = 1.0 - (-1.0 / (0.005 * sr as f32)).exp();
        let cx = BlockCtx {
            n,
            pos,
            playing,
            recording,
            sr,
            sources,
            punch_range: self.punch_range,
            capture_from,
            capture_end,
            auto_monitor: self.auto_monitor,
            low_latency: self.pdc.low_latency,
            block_end: pos + n as i64,
            smooth,
            // Isolated renders work out audibility themselves (see `Isolate`).
            any_solo: self.isolate.is_none() && self.tracks.iter().any(|t| t.params.solo),
            snap_gains,
            iso: self.isolate.as_deref(),
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
            ahead: self.ahead.shared.as_deref(),
            ahead_until: self.ahead.until,
        };
        let mut end = BlockEnd::Played;
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
                parallel::Outcome::LeftOut => return BlockEnd::LeftOut,
            }
        } else {
            let budget = (n as f64 / self.sample_rate) as f32;
            for ti in 0..self.tracks.len() {
                let (before, rest) = self.tracks.split_at_mut(ti);
                let (cur, tail) = rest.split_first_mut().unwrap();
                let mut routes = Direct { before, tail, ti, bus: (&mut self.bus_l[..n], &mut self.bus_r[..n]), analyzer: &mut self.analyzer_tx };
                let at = Instant::now();
                run_strip(&cx, ti, cur, &mut self.scratch, &mut routes);
                cur.spent = at.elapsed().as_secs_f32();
                cur.load.played(0, cur.spent, budget);
            }
        }

        self.play_master(n, &transport, smooth);
        self.snap_gains = false;
        self.play_click(n, sources);
        end
    }

    /// Copies the device's interleaved input into one buffer per channel, metering each, and
    /// silences the channels it doesn't have. Whether the device gave any input.
    fn read_inputs(&mut self, input: &[f32], in_ch: usize, n: usize) -> bool {
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
        in_ch > 0 && stride > 0
    }

    /// The input tap's ring gets the input it listens to. Only real input: a device without any
    /// shouldn't record silence at its own speed.
    fn feed_input_tap(&mut self, n: usize) {
        let Some((sel, sink)) = &mut self.input_tap else { return };
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

    /// Live MIDI into the recording, while the instrument track it plays is armed and the
    /// playhead is inside the capture range. Recorded MIDI comes from one track, but the ring has
    /// one producer, which `process` also pushes to: this thread, before the strips start. What
    /// a full ring has no room for is counted, for the session to report.
    fn record_live_midi(&mut self, capture_from: i64, capture_end: i64) {
        let pos = self.pos;
        if let Some(t) = self.midi_target.and_then(|id| self.tracks.iter().find(|t| t.id == id)).filter(|t| t.kind == TrackKind::Instrument) {
            if self.recording && t.params.armed && pos >= capture_from && pos < capture_end {
                for m in &self.live {
                    if self.midi_rec.push((pos, m.data)).is_err() {
                        self.status.midi_rec_lost.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    /// Stereo Out: its automation, plug-ins and fader over what the strips left on the bus, and
    /// its meter. Isolated renders are captured before the Stereo Out strip, so they skip all but
    /// the meter.
    fn play_master(&mut self, n: usize, transport: &TransportInfo, smooth: f32) {
        let master_fx = self.isolate.is_none();
        if master_fx {
            self.automate_master(n);
        }
        let (bl, br) = (&mut self.bus_l[..n], &mut self.bus_r[..n]);
        for ins in self.master_inserts.iter_mut().flatten().filter(|_| master_fx) {
            if !ins.bypass {
                let key = key_signal(ins.key.idx.and_then(|i| self.tracks.get(i)).map(|t| &**t), ins.key.pre, None, n);
                let silent = idle::silent(bl, br, key);
                if ins.sleeps(silent) {
                    bl.fill(0.0);
                    br.fill(0.0);
                } else {
                    ins.plugin.process_insert(bl, br, key, transport);
                    check_output(&self.status, ins, bl, br);
                    ins.count_quiet(silent, bl, br, self.sample_rate);
                }
            } else {
                ins.plugin.pass_edits_aside(transport);
            }
            if self.analyzer_target == Some((MASTER_ID, ins.uid)) {
                for i in 0..n {
                    let _ = self.analyzer_tx.push(0.5 * (bl[i] + br[i]));
                }
            }
        }
        let master_target = if master_fx { self.master_auto.volume.unwrap_or(self.master_volume) } else { 1.0 };
        let mut g = if self.snap_gains { master_target } else { self.master_gain };
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
    }

    /// The click goes straight to the output, after the Stereo Out fader, as in Logic. The
    /// count-in always clicks; otherwise the metronome follows its settings.
    fn play_click(&mut self, n: usize, sources: bool) {
        let (recording, punch) = (self.recording, self.punch);
        let cp = self.click_params;
        // The music reaches the output `pdc.total` late; the click waits for it.
        let pos = self.pos - self.pdc.total as i64;
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
        let srf = self.sample_rate as f32;
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
    }
}
