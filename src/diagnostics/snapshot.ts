// What the Diagnostics window gets each time it asks (`diagnostics`, src-tauri/src/diagnostics/):
// the session's figures (crates/engine/src/session/diagnostics/mod.rs, mirrored here field for
// field) and the log lines it hasn't seen yet.
import type { Id, TrackKind } from "../engine/project.ts";
import type { MemoryLock } from "../settings/panes/memoryLock.ts";

export interface DiagnosticsSnapshot {
  version: string;
  os: string;
  arch: string;
  /** Null when the session was busy: the window keeps what it showed. */
  engine: Diagnostics | null;
  /** The log lines after the last one the window had, oldest first. */
  log: LogLine[];
}

export interface LogLine {
  /** Counts up from 1 with every line. */
  seq: number;
  /** Milliseconds since Marbble started. */
  ms: number;
  level: LogLevel;
  target: string;
  message: string;
}

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

export interface Diagnostics {
  audio: AudioDiagnostics;
  load: LoadDiagnostics;
  threads: ThreadDiagnostics;
  tracks: TrackDiagnostics[];
  memory: MemoryDiagnostics;
  midi: MidiDiagnostics;
}

export interface AudioDiagnostics {
  running: boolean;
  host: string;
  input: string | null;
  output: string | null;
  input_channels: number;
  output_channels: number;
  sample_rate: number;
  sample_rate_asked: number;
  buffer_size: number;
  buffer_size_asked: number;
  input_latency_ms: number;
  output_latency_ms: number;
  ring_frames: number;
  device_gaps: number;
  device_events: number;
  stream_errors: number;
  last_stream_error: string | null;
  restarts: number;
  gave_up_restarting: boolean;
  error: string | null;
  /** macOS only: whether the device has an IO workgroup, and the threads it suggests. */
  workgroup: boolean | null;
  workgroup_threads: number | null;
}

export interface LoadDiagnostics {
  /** 1 = a whole buffer's time. */
  cpu: number;
  cpu_peak: number;
  blocks: number;
  p50: number;
  p99: number;
  max: number;
  late_blocks: number;
  overloads: number;
  overload_ms: number | null;
  overload_track: string | null;
  engine_errors: number;
  bad_audio: number;
  cmds_sent: number;
  cmds_done: number;
  cmds_dropped: number;
  overruns: number;
  midi_rec_lost: number;
  callbacks: number;
  parallel_blocks: number;
}

export interface ThreadDiagnostics {
  logical_cpus: number;
  fast_cores: number;
  fast_cpus: number[];
  other_cores: number;
  /** Processing Threads (0: Automatic), and what it means on this machine. */
  setting: number;
  resolved: number;
  playing_now: number;
  pool: PoolDiagnostics | null;
  left_out_tracks: number;
  held_strips: number;
}

export interface PoolDiagnostics {
  threads: number;
  playing: number;
  starting: number;
  refused: number;
  stuck: number;
  realtime: number;
  joined: number;
  refusal: string | null;
}

export interface TrackDiagnostics {
  id: Id;
  name: string;
  kind: TrackKind;
  off: boolean;
  frozen: boolean;
  /** 0: the audio callback; n: marbble-dsp-n; null before it played. */
  thread: number | null;
  /** The latest block's time over its audio's, and the worst since the last snapshot. */
  share: number;
  peak: number;
  /** Frames. */
  latency: number;
  delay: number;
  held: boolean;
  plugins: PluginDiagnostics[];
  /** Its inserts that slept through its latest block, fed silence. */
  asleep: number;
}

export interface PluginDiagnostics {
  uid: Id;
  name: string;
  instrument: boolean;
  bypass: boolean;
  builtin: boolean;
  latency: number | null;
  unresponsive: boolean;
}

export interface MemoryDiagnostics {
  lock: MemoryLock;
  /** kB; null where the system doesn't say. */
  resident_kb: number | null;
  locked_kb: number | null;
  swapped_kb: number | null;
  mapped_kb: number | null;
  clips: number;
  clip_bytes: number;
}

export interface MidiDiagnostics {
  inputs: string[];
  outputs: string[];
  activity: number;
  target: string | null;
  recent: { seq: number; ms: number; port: string; data: [number, number, number] }[];
}
