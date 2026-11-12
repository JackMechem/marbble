//! Fills a [`Diagnostics`] snapshot from the session, one section at a time. Everything here reads
//! what the session keeps and the atomics the audio thread writes; nothing locks the engine, calls a
//! plug-in or lists the system's MIDI ports, so the app can take a snapshot a few times a second
//! under a brief session lock.

use super::*;
use crate::audio_io::memlock::ProcessMemory;
use crate::session::*;

impl Session {
    /// What the Diagnostics window shows, as of now. Reading it starts each strip's peak afresh.
    pub fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            audio: self.audio_diagnostics(),
            load: self.load_diagnostics(),
            threads: self.thread_diagnostics(),
            tracks: self.track_diagnostics(),
            memory: self.memory_diagnostics(),
            midi: self.midi_diagnostics(),
        }
    }

    /// Starts the block-load figures (count, percentiles, worst) afresh, for the Diagnostics
    /// window's reset. The audio thread may add a block as they clear, which only counts it.
    pub fn reset_load_statistics(&self) {
        self.status.block_loads.clear();
    }

    pub(super) fn audio_diagnostics(&self) -> AudioDiagnostics {
        let st = &self.audio_status;
        let (device_events, stream_errors) = self.device_events_seen;
        let io = self.io_thread();
        AudioDiagnostics {
            running: st.running,
            host: st.host.clone(),
            input: st.input_name.clone(),
            output: st.output_name.clone(),
            input_channels: st.input_channels,
            output_channels: st.output_channels,
            sample_rate: st.sample_rate,
            sample_rate_asked: self.audio_config.sample_rate,
            buffer_size: st.buffer_size,
            buffer_size_asked: self.audio_config.buffer_size,
            input_latency_ms: self.status.input_latency_us.load(Ordering::Relaxed) as f32 / 1000.0,
            output_latency_ms: self.status.output_latency_us.load(Ordering::Relaxed) as f32 / 1000.0,
            ring_frames: self.status.ring_frames.load(Ordering::Relaxed),
            device_gaps: self.status.device_gaps.load(Ordering::Relaxed),
            device_events,
            stream_errors,
            last_stream_error: self.audio.as_ref().filter(|_| stream_errors > 0).map(|a| format!("{:?}", a.events().last_other())),
            restarts: self.device_watch.restarts(),
            gave_up_restarting: self.device_watch.gave_up(),
            error: st.error.clone(),
            workgroup: io.and_then(|io| io.has_workgroup()),
            workgroup_threads: io.and_then(|io| io.suggested_threads()),
        }
    }

    pub(super) fn load_diagnostics(&self) -> LoadDiagnostics {
        let s = &self.status;
        let loads = s.block_loads.read();
        let overloads = s.overloads.load(Ordering::Relaxed);
        let track = s.overload_track.load(Ordering::Relaxed);
        LoadDiagnostics {
            cpu: f32::from_bits(s.cpu_load.load(Ordering::Relaxed)),
            cpu_peak: f32::from_bits(s.cpu_peak.load(Ordering::Relaxed)),
            blocks: loads.blocks(),
            p50: loads.percentile(0.5),
            p99: loads.percentile(0.99),
            max: loads.max,
            late_blocks: s.late_blocks.load(Ordering::Relaxed),
            overloads,
            overload_ms: (overloads > 0).then(|| f32::from_bits(s.overload_secs.load(Ordering::Relaxed)) * 1000.0),
            overload_track: (overloads > 0 && track != 0).then(|| self.track_name(track)),
            engine_errors: s.engine_errors.load(Ordering::Relaxed),
            ahead_misses: s.ahead_misses.load(Ordering::Relaxed),
            bad_audio: s.bad_audio.load(Ordering::Relaxed),
            cmds_sent: self.cmds_sent,
            cmds_done: s.cmds_done.load(Ordering::Relaxed),
            cmds_dropped: s.cmds_dropped.load(Ordering::Relaxed),
            overruns: s.overruns.load(Ordering::Relaxed),
            midi_rec_lost: s.midi_rec_lost.load(Ordering::Relaxed),
            callbacks: s.blocks.load(Ordering::Relaxed),
            parallel_blocks: s.parallel_blocks.load(Ordering::Relaxed),
        }
    }

    pub(super) fn thread_diagnostics(&self) -> ThreadDiagnostics {
        let topo = &self.threads.topology;
        let pool = self.threads.health.as_ref().map(|h| {
            let r = h.report();
            PoolDiagnostics {
                threads: r.threads,
                playing: r.playing,
                starting: r.starting,
                refused: r.refused,
                stuck: r.stuck,
                realtime: r.realtime,
                joined: r.joined,
                refusal: h.refusal().map(str::to_string),
            }
        });
        ThreadDiagnostics {
            logical_cpus: topo.logical,
            fast_cores: topo.fast_cores,
            fast_cpus: topo.fast.clone(),
            other_cores: topo.others,
            setting: self.audio_config.processing_threads,
            resolved: self.processing_threads(),
            playing_now: self.status.threads.load(Ordering::Relaxed),
            pool,
            left_out_tracks: self.status.left_out.load(Ordering::Relaxed),
            held_strips: self.held().len() as u32,
        }
    }

    fn track_diagnostics(&self) -> Vec<TrackDiagnostics> {
        let held = self.held();
        let latency = |id: Id| self.pdc_report.strips.iter().find(|s| s.track == id);
        self.project
            .tracks
            .iter()
            .map(|t| {
                let load = self.strip_loads.get(&t.id).map(|l| l.take()).unwrap_or_default();
                let strip = latency(t.id);
                let slot_latency = |uid: Id| strip.and_then(|s| s.slots.iter().find(|(u, _)| *u == uid)).map(|&(_, l)| l);
                let plugin = |slot: &PluginSlot, instrument: bool| PluginDiagnostics {
                    uid: slot.uid,
                    name: slot.name.clone(),
                    instrument,
                    bypass: slot.bypass,
                    builtin: slot.plugin_id.starts_with("builtin:"),
                    latency: slot_latency(slot.uid),
                    unresponsive: self.controllers.get(&slot.uid).is_some_and(|c| !c.responsive()),
                };
                let plugins = t.instrument.iter().map(|s| plugin(s, true)).chain(t.inserts.iter().flatten().map(|s| plugin(s, false))).collect();
                TrackDiagnostics {
                    id: t.id,
                    name: t.name.clone(),
                    kind: t.kind,
                    off: self.project.is_off(t.id),
                    frozen: t.frozen.is_some(),
                    thread: load.thread,
                    share: load.share,
                    peak: load.peak,
                    latency: strip.map_or(0, |s| s.latency),
                    delay: strip.map_or(0, |s| s.delay),
                    held: held.iter().any(|h| h.track == t.id),
                    plugins,
                    asleep: load.asleep,
                    ahead: load.ahead,
                }
            })
            .collect()
    }

    fn memory_diagnostics(&self) -> MemoryDiagnostics {
        let process = ProcessMemory::try_read();
        let field = |f: fn(&ProcessMemory) -> u64| process.as_ref().map(f);
        let clip_bytes = self.clips.values().map(|c| c.channels.iter().map(|ch| ch.len() as u64 * 4).sum::<u64>()).sum();
        MemoryDiagnostics {
            lock: self.memory_lock.clone(),
            resident_kb: field(|m| m.resident_kb),
            locked_kb: field(|m| m.locked_kb),
            swapped_kb: field(|m| m.swapped_kb),
            mapped_kb: field(|m| m.mapped_kb),
            clips: self.clips.len(),
            clip_bytes,
        }
    }

    fn midi_diagnostics(&self) -> MidiDiagnostics {
        /// The newest messages the window lists.
        const RECENT: usize = 12;
        let recent = self.midi.recent();
        MidiDiagnostics {
            inputs: self.midi.connected(),
            outputs: self.midi_out.keys(),
            activity: self.status.midi_activity.load(Ordering::Relaxed),
            target: self.midi_target_name(),
            recent: recent[recent.len().saturating_sub(RECENT)..].to_vec(),
        }
    }

    /// A track's name for the window, or its id if it's gone.
    fn track_name(&self, id: Id) -> String {
        self.project.track(id).map_or_else(|| format!("track {id}"), |t| t.name.clone())
    }
}
