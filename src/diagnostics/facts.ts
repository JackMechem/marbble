// What the Diagnostics window says about the session, as labelled values in sections, with the
// ones that are bad marked: `warn` for something to look at, `alert` for something going wrong now
// (a count that rose in the last few seconds, real time refused, memory swapped out). The Overview
// tab draws these and the copied report prints them, so both say the same.
import { risingAt, since, type Counter, type Counters, type Rises } from "./counters.ts";
import { asked, bytes, DASH, frames, kb, list, ms, num, pct, threadName } from "./format.ts";
import type { HeldPeaks } from "./peaks.ts";
import type { Diagnostics, TrackDiagnostics } from "./snapshot.ts";
import { memoryLockNote } from "../settings/panes/memoryLock.ts";

export type Level = "warn" | "alert";

export interface Fact {
  label: string;
  value: string;
  level?: Level;
  /** The tooltip: what the figure means. */
  hint?: string;
}

export interface Section {
  id: "audio" | "load" | "threads" | "memory" | "midi";
  title: string;
  facts: Fact[];
}

/** The counters as the window shows them, and which rose lately. */
export interface Counts {
  now: Counters;
  rising: Set<Counter>;
}

/** The counters since `base` (the last reset), with those that rose by `at` marked. */
export function countsAt(current: Counters, base: Counters | null, rises: Rises, at: number): Counts {
  return { now: since(current, base), rising: risingAt(rises, at) };
}

/** A count: warn while above 0, alert while it's rising. */
function counted(label: string, c: Counts, key: Counter, hint: string, value?: string): Fact {
  const n = c.now[key];
  return { label, value: value ?? String(n), level: c.rising.has(key) ? "alert" : n > 0 ? "warn" : undefined, hint };
}

/** A load against the buffer: warn past 70 %, alert past 100 % (a dropout). */
function load(label: string, v: number, hint: string): Fact {
  return { label, value: pct(v), level: v > 1 ? "alert" : v > 0.7 ? "warn" : undefined, hint };
}

const plural = (n: number, one: string, many = one + "s") => `${n} ${n === 1 ? one : many}`;

export function audioFacts(d: Diagnostics, c: Counts): Section {
  const a = d.audio;
  const rate = a.sample_rate || a.sample_rate_asked;
  const facts: Fact[] = [
    { label: "Status", value: a.running ? "Running" : a.error ? `Stopped: ${a.error}` : "Stopped", level: a.running ? undefined : "alert" },
    { label: "Driver", value: a.host || DASH },
    { label: "Output", value: a.output ? `${a.output} (${plural(a.output_channels, "channel")})` : DASH },
    { label: "Input", value: a.input ? `${a.input} (${plural(a.input_channels, "channel")})` : "None" },
    { label: "Sample rate", value: asked(a.sample_rate, a.sample_rate_asked), level: a.running && a.sample_rate !== a.sample_rate_asked ? "warn" : undefined },
    {
      label: "Buffer size",
      value: `${asked(a.buffer_size, a.buffer_size_asked)}${a.buffer_size && rate ? ` (${((a.buffer_size / rate) * 1000).toFixed(1)} ms)` : ""}`,
      level: a.running && a.buffer_size !== a.buffer_size_asked ? "warn" : undefined,
      hint: "Frames per buffer: what the device gave, and what Settings › Audio asked for when it differs.",
    },
    { label: "Latency in / out", value: `${ms(a.input_latency_ms)} / ${ms(a.output_latency_ms)}`, hint: "As the device last reported them." },
    { label: "Input ring", value: frames(a.ring_frames, rate), hint: "Input frames waiting between the input and output streams." },
    counted("Device gaps", c, "device_gaps", "Output callbacks that came after the previous buffer's audio had run out: the device played something Marbble never gave it."),
    counted("Device events", c, "device_events", "Reports from the device's streams that call for reopening it (unplugged, changed)."),
    counted("Stream errors", c, "stream_errors", "Other errors the device's streams reported.", a.last_stream_error ? `${c.now.stream_errors} (last: ${a.last_stream_error})` : undefined),
    counted("Restarts", c, "restarts", "Times Marbble reopened the device by itself.", a.gave_up_restarting ? `${c.now.restarts} (gave up)` : undefined),
  ];
  if (a.gave_up_restarting) facts[facts.length - 1].level = "alert";
  if (a.workgroup !== null) {
    facts.push({
      label: "IO workgroup",
      value: a.workgroup ? `Yes${a.workgroup_threads ? ` (suggests ${a.workgroup_threads} threads)` : ""}` : "No",
      hint: "Whether the output device has an audio workgroup for the processing threads to join (macOS).",
    });
  }
  return { id: "audio", title: "Audio device", facts };
}

export function loadFacts(d: Diagnostics, c: Counts): Section {
  const l = d.load;
  const overload = l.overload_ms !== null ? ` (last: ${ms(l.overload_ms)}${l.overload_track ? `, ${l.overload_track}` : ""})` : "";
  return {
    id: "load",
    title: "Engine load",
    facts: [
      load("Average", l.cpu, "The CPU meter's figure: each buffer's time to finish against its audio's, smoothed."),
      load("Recent peak", l.cpu_peak, "The worst recent buffer, as the CPU meter's peak shows it."),
      { label: "Buffers measured", value: num(l.blocks), hint: "Since launch, or since the window's reset." },
      load("Median (p50)", l.p50, "Half the buffers took less than this share of their time."),
      load("p99", l.p99, "99 % of buffers took less than this share of their time."),
      load("Worst", l.max, "The slowest buffer. Over 100% is a dropout."),
      counted("Late buffers", c, "late_blocks", "Buffers that took longer than their own audio: each is a dropout."),
      counted("Overloads", c, "overloads", "Times callbacks ran late for 60 ms in a row and the engine played silence for a moment.", `${c.now.overloads}${overload}`),
      counted("Engine errors", c, "engine_errors", "Buffers silenced because processing panicked."),
      counted("Not ready ahead", c, "ahead_misses", "Blocks render ahead hadn't rendered in time: their track was taken back and played live."),
      counted("Bad plug-in output", c, "bad_audio", "Buffers in which a plug-in put out NaNs or a runaway level."),
      { label: "Commands sent / done", value: `${num(l.cmds_sent)} / ${num(l.cmds_done)}`, hint: "Edits sent to the audio thread, and those it has applied." },
      counted("Commands dropped", c, "cmds_dropped", "Commands given up on because the audio thread took none for a second."),
      counted("Recording overruns", c, "overruns", "Audio the recorder couldn't write in time."),
      counted("Recorded MIDI lost", c, "midi_rec_lost", "MIDI messages a recording found no room for."),
      { label: "Callbacks", value: num(l.callbacks), hint: "Device callbacks in which the tracks played." },
      { label: "Buffers on the pool", value: num(l.parallel_blocks), hint: "Buffers whose tracks played on several threads." },
    ],
  };
}

export function threadFacts(d: Diagnostics): Section {
  const t = d.threads;
  const p = t.pool;
  const facts: Fact[] = [
    { label: "Logical CPUs", value: num(t.logical_cpus) },
    { label: "Performance cores", value: `${num(t.fast_cores)}${t.other_cores ? ` (+${t.other_cores} efficiency)` : ""}`, hint: "Physical cores of the fastest kind." },
    { label: "Audio threads kept to", value: t.fast_cpus.length ? `CPUs ${list(t.fast_cpus)}` : "Any CPU", hint: "The logical CPUs of the performance cores, on a machine with two kinds (Linux)." },
    { label: "Processing Threads", value: t.setting === 0 ? `Automatic (${t.resolved})` : String(t.setting), hint: "Settings › Audio › Processing Threads, the audio thread included." },
    { label: "Playing now", value: plural(t.playing_now, "thread"), hint: "Threads that played the latest buffer's tracks." },
  ];
  if (p) {
    const workers = p.threads - 1;
    facts.push(
      { label: "Pool", value: `${p.playing} of ${p.threads} playing${p.starting ? `, ${p.starting} starting` : ""}` },
      { label: "Real time", value: `${p.realtime} of ${workers} workers`, level: p.refused ? "alert" : undefined, hint: "Workers granted real-time priority." },
      { label: "Refused real time", value: String(p.refused), level: p.refused ? "alert" : undefined, hint: p.refusal ?? "Workers the system refused real-time priority; they play nothing." },
      { label: "Stuck in a plug-in", value: String(p.stuck), level: p.stuck ? "alert" : undefined, hint: "Workers given up on in a plug-in that didn't return, demoted and left out." },
    );
    if (d.audio.workgroup !== null) facts.push({ label: "In the IO workgroup", value: `${p.joined} of ${workers}` });
  } else {
    facts.push({ label: "Pool", value: "None", hint: "Every track plays on the audio thread: one thread is set, or the project has nothing to gain." });
  }
  facts.push(
    { label: "Tracks left out", value: String(t.left_out_tracks), level: t.left_out_tracks ? "alert" : undefined, hint: "Tracks sitting out since a plug-in stopped responding." },
    { label: "Strips held", value: String(t.held_strips), level: t.held_strips ? "warn" : undefined, hint: "Strips threads are still holding in plug-ins that stopped responding." },
  );
  return { id: "threads", title: "Threads and cores", facts };
}

export function memoryFacts(d: Diagnostics): Section {
  const m = d.memory;
  const lock = memoryLockNote(m.lock);
  const lockName = { unsupported: "Not on this system", off: "Off", locked: "Locked", limit_too_small: "Not locked: limit too small", failed: "Not locked: failed" }[m.lock.state];
  return {
    id: "memory",
    title: "Memory",
    facts: [
      { label: "Memory lock", value: lockName, level: lock.warn ? "warn" : undefined, hint: lock.text },
      { label: "Resident", value: kb(m.resident_kb), hint: "What the process has in RAM." },
      { label: "Locked", value: kb(m.locked_kb), hint: "The size of the locked mappings." },
      { label: "Swapped out", value: kb(m.swapped_kb), level: (m.swapped_kb ?? 0) > 0 ? "alert" : undefined, hint: "What the system swapped out: audio that reads it back can drop out." },
      { label: "Mapped", value: kb(m.mapped_kb), hint: "Every mapping, touched or not." },
      { label: "Audio in memory", value: `${bytes(m.clip_bytes)} (${plural(m.clips, "file")})`, hint: "The decoded audio files the project can play." },
    ],
  };
}

/** A MIDI message as hex bytes: "90 3C 64". */
export const midiBytes = (data: number[]) => data.map((b) => b.toString(16).toUpperCase().padStart(2, "0")).join(" ");

export function midiFacts(d: Diagnostics): Section {
  const m = d.midi;
  const last = m.recent[m.recent.length - 1];
  return {
    id: "midi",
    title: "MIDI",
    facts: [
      { label: "Inputs", value: list(m.inputs) },
      { label: "Outputs", value: list(m.outputs) },
      { label: "Live MIDI plays", value: m.target ?? DASH },
      { label: "Messages played", value: num(m.activity), hint: "Live messages the engine has taken since launch." },
      { label: "Last message", value: last ? `${midiBytes(last.data)} from ${last.port}` : DASH },
    ],
  };
}

/** Every section of the Overview, in order. */
export function overview(d: Diagnostics, c: Counts): Section[] {
  return [audioFacts(d, c), loadFacts(d, c), threadFacts(d), memoryFacts(d), midiFacts(d)];
}

/** The figures always in view at the top of the window. */
export function headline(d: Diagnostics, c: Counts): Fact[] {
  const by = (s: Section, label: string) => s.facts.find((f) => f.label === label)!;
  const a = audioFacts(d, c);
  const l = loadFacts(d, c);
  return [
    { ...by(l, "Average"), label: "Load" },
    by(l, "p99"),
    { ...by(l, "Late buffers"), label: "Late" },
    { ...by(a, "Device gaps"), label: "Gaps" },
    { ...by(l, "Overloads"), value: String(c.now.overloads) },
    { label: "Threads", value: String(d.threads.playing_now), level: d.threads.pool?.refused || d.threads.pool?.stuck ? "alert" : undefined, hint: "Threads that played the latest buffer's tracks." },
    { label: "Buffer", value: d.audio.running ? `${d.audio.buffer_size} @ ${(d.audio.sample_rate / 1000).toFixed(1)} kHz` : "Stopped", level: d.audio.running ? undefined : "alert" },
  ];
}

/** A track's row in the Tracks table. */
export interface TrackRow {
  id: number;
  name: string;
  kind: string;
  thread: string;
  share: string;
  peak: string;
  latency: string;
  delay: string;
  plugins: string;
  state: string;
  level?: Level;
  /** The latest share and the peak as numbers, for bars and sorting, and the rest that sort. */
  shareOf: number;
  peakOf: number;
  threadOf: number;
  latencyOf: number;
  delayOf: number;
}

/** What's notable about a track: off, frozen, held up in a plug-in, plug-ins not responding or
 *  asleep (fed silence). */
function trackState(t: TrackDiagnostics): string {
  const s: string[] = [];
  if (t.held) s.push("held in a plug-in");
  const dead = t.plugins.filter((p) => p.unresponsive).map((p) => p.name);
  if (dead.length) s.push(`not responding: ${dead.join(", ")}`);
  if (t.off) s.push("off");
  if (t.frozen) s.push("frozen");
  if (t.ahead && !t.off) s.push("rendered ahead");
  if (t.asleep && !t.off) s.push(t.asleep === 1 ? "1 plug-in asleep" : `${t.asleep} plug-ins asleep`);
  return s.join("; ");
}

const KIND_NAMES: Record<string, string> = { audio: "Audio", instrument: "Instrument", folder: "Folder", summing: "Summing", aux: "Aux" };

/** The Tracks table, in the project's order, with the peaks `held` holds (peaks.ts) if given. A
 *  strip that took over half its buffer is marked. */
export function trackRows(d: Diagnostics, held?: HeldPeaks): TrackRow[] {
  const rate = d.audio.sample_rate || d.audio.sample_rate_asked;
  return d.tracks.map((tr) => {
    const t = { ...tr, peak: held?.get(tr.id)?.peak ?? tr.peak };
    const plugins = t.plugins.map((p) => `${p.name}${p.bypass ? " (bypassed)" : ""}${p.latency ? ` [${p.latency}]` : ""}`);
    const level: Level | undefined = t.held || t.plugins.some((p) => p.unresponsive) || t.peak > 1 ? "alert" : t.peak > 0.5 ? "warn" : undefined;
    return {
      id: t.id,
      name: t.name,
      kind: KIND_NAMES[t.kind] ?? t.kind,
      thread: t.off ? DASH : threadName(t.thread),
      share: t.thread === null ? DASH : pct(t.share, 1),
      peak: t.thread === null ? DASH : pct(t.peak, 1),
      latency: t.latency ? frames(t.latency, rate) : "0",
      delay: t.delay ? frames(t.delay, rate) : "0",
      plugins: list(plugins),
      state: trackState(t),
      level,
      shareOf: t.thread === null || t.off ? 0 : t.share,
      peakOf: t.thread === null || t.off ? 0 : t.peak,
      threadOf: t.thread === null || t.off ? -1 : t.thread,
      latencyOf: t.latency,
      delayOf: t.delay,
    };
  });
}

/** What the Tracks table can sort by, as a task manager sorts its processes. */
export type SortKey = "order" | "name" | "kind" | "thread" | "share" | "peak" | "latency" | "delay";

/** The rows sorted by `key`, largest or last first when `desc`, keeping the project's order among
 *  equals. By peak, the latest share breaks ties. */
export function sortRows(rows: TrackRow[], key: SortKey, desc: boolean): TrackRow[] {
  const value = (r: TrackRow): number | string => {
    switch (key) {
      case "name":
        return r.name.toLowerCase();
      case "kind":
        return r.kind;
      case "thread":
        return r.threadOf;
      case "share":
        return r.shareOf;
      case "peak":
        return r.peakOf;
      case "latency":
        return r.latencyOf;
      case "delay":
        return r.delayOf;
      default:
        return 0;
    }
  };
  const cmp = (a: number | string, b: number | string) => (a < b ? -1 : a > b ? 1 : 0);
  const sign = desc ? -1 : 1;
  return rows
    .map((r, i) => ({ r, i }))
    .sort((a, b) => sign * cmp(value(a.r), value(b.r)) || (key === "peak" ? sign * cmp(a.r.shareOf, b.r.shareOf) : 0) || (key === "order" && desc ? b.i - a.i : a.i - b.i))
    .map(({ r }) => r);
}

/** Each thread's share of the latest buffer, summed over the tracks it played: [thread, share]. */
export function threadShares(d: Diagnostics): [string, number][] {
  const sums = new Map<number, number>();
  for (const t of d.tracks) if (t.thread !== null && !t.off) sums.set(t.thread, (sums.get(t.thread) ?? 0) + t.share);
  return [...sums.entries()].sort((a, b) => a[0] - b[0]).map(([t, s]) => [threadName(t), s]);
}
