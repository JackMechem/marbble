// A Diagnostics snapshot for the tests of src/diagnostics/: a running device, two tracks on two
// threads, a pool, and nothing gone wrong. Each test changes what it looks at.
import type { Diagnostics, TrackDiagnostics } from "../../src/diagnostics/snapshot.ts";

export function track(id: number, name: string, extra: Partial<TrackDiagnostics> = {}): TrackDiagnostics {
  return { id, name, kind: "audio", off: false, frozen: false, thread: 0, share: 0.1, peak: 0.2, latency: 0, delay: 0, held: false, plugins: [], asleep: 0, ...extra };
}

export function sample(): Diagnostics {
  return {
    audio: {
      running: true,
      host: "PipeWire",
      input: null,
      output: "Built-in Audio",
      input_channels: 0,
      output_channels: 2,
      sample_rate: 48000,
      sample_rate_asked: 48000,
      buffer_size: 256,
      buffer_size_asked: 256,
      input_latency_ms: 0,
      output_latency_ms: 5.3,
      ring_frames: 0,
      device_gaps: 0,
      device_events: 0,
      stream_errors: 0,
      last_stream_error: null,
      restarts: 0,
      gave_up_restarting: false,
      error: null,
      workgroup: null,
      workgroup_threads: null,
    },
    load: {
      cpu: 0.21,
      cpu_peak: 0.4,
      blocks: 1000,
      p50: 0.2,
      p99: 0.45,
      max: 0.6,
      late_blocks: 0,
      overloads: 0,
      overload_ms: null,
      overload_track: null,
      engine_errors: 0,
      bad_audio: 0,
      cmds_sent: 40,
      cmds_done: 40,
      cmds_dropped: 0,
      overruns: 0,
      midi_rec_lost: 0,
      callbacks: 1000,
      parallel_blocks: 900,
    },
    threads: {
      logical_cpus: 16,
      fast_cores: 8,
      fast_cpus: [0, 1, 2, 3],
      other_cores: 0,
      setting: 0,
      resolved: 4,
      playing_now: 2,
      pool: { threads: 4, playing: 4, starting: 0, refused: 0, stuck: 0, realtime: 3, joined: 0, refusal: null },
      left_out_tracks: 0,
      held_strips: 0,
    },
    tracks: [track(1, "Drums"), track(2, "Bass", { thread: 2, share: 0.3, peak: 0.35, plugins: [{ uid: 9, name: "Comp", instrument: false, bypass: false, builtin: true, latency: 64, unresponsive: false }] })],
    memory: { lock: { state: "locked" }, resident_kb: 204800, locked_kb: 3145728, swapped_kb: 0, mapped_kb: 3145728, clips: 3, clip_bytes: 11534336 },
    midi: { inputs: ["Keystation"], outputs: [], activity: 12, target: "Bass", recent: [{ seq: 1, ms: 10, port: "Keystation", data: [0x90, 60, 100] }] },
  };
}
