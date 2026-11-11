// Unit tests for src/diagnostics/facts.ts: what the Diagnostics window says, and what it marks.
import { test } from "node:test";
import assert from "node:assert/strict";
import { countersOf } from "../../src/diagnostics/counters.ts";
import { countsAt, headline, heaviestFirst, overview, threadShares, trackRows, type Fact, type Section } from "../../src/diagnostics/facts.ts";
import { sample, track } from "./sample.ts";

const quiet = (d = sample()) => countsAt(countersOf(d), null, {}, 0);
const fact = (sections: Section[], id: Section["id"], label: string): Fact => {
  const f = sections.find((s) => s.id === id)?.facts.find((x) => x.label === label);
  assert.ok(f, `${id}: ${label}`);
  return f;
};

test("a healthy session marks nothing", () => {
  const d = sample();
  const marked = overview(d, quiet(d)).flatMap((s) => s.facts.filter((f) => f.level).map((f) => `${s.id}: ${f.label}`));
  assert.deepEqual(marked, []);
  assert.deepEqual(headline(d, quiet(d)).filter((f) => f.level), []);
  assert.deepEqual(overview(d, quiet(d)).map((s) => s.title), ["Audio device", "Engine load", "Threads and cores", "Memory", "MIDI"]);
});

test("late buffers and gaps are marked, and alert while rising", () => {
  const d = sample();
  d.load.late_blocks = 4;
  d.audio.device_gaps = 1;
  const now = countersOf(d);
  const counts = countsAt(now, null, { late_blocks: 900 }, 1000);
  const s = overview(d, counts);
  assert.equal(fact(s, "load", "Late buffers").level, "alert");
  assert.equal(fact(s, "audio", "Device gaps").level, "warn");
  // Since a reset after them: nothing to mark.
  const reset = overview(d, countsAt(now, now, {}, 10_000));
  assert.equal(fact(reset, "load", "Late buffers").value, "0");
  assert.equal(fact(reset, "load", "Late buffers").level, undefined);
});

test("refused real time, swapped memory and a stopped device are alerts", () => {
  const d = sample();
  d.threads.pool!.refused = 2;
  d.threads.pool!.refusal = "rtkit refused";
  d.memory.swapped_kb = 1024;
  d.audio.running = false;
  d.audio.error = "device busy";
  const s = overview(d, quiet(d));
  assert.equal(fact(s, "threads", "Refused real time").level, "alert");
  assert.equal(fact(s, "threads", "Refused real time").hint, "rtkit refused");
  assert.equal(fact(s, "memory", "Swapped out").level, "alert");
  assert.deepEqual([fact(s, "audio", "Status").value, fact(s, "audio", "Status").level], ["Stopped: device busy", "alert"]);
});

test("loads past 70% warn, past 100% alert", () => {
  const d = sample();
  d.load.p99 = 0.75;
  d.load.max = 1.4;
  const s = overview(d, quiet(d));
  assert.equal(fact(s, "load", "p99").level, "warn");
  assert.deepEqual([fact(s, "load", "Worst").value, fact(s, "load", "Worst").level], ["140%", "alert"]);
});

test("unknowns are dashes: memory off Linux, no pool, no workgroup line off macOS", () => {
  const d = sample();
  d.memory.resident_kb = null;
  d.memory.lock = { state: "unsupported" };
  d.threads.pool = null;
  const s = overview(d, quiet(d));
  assert.equal(fact(s, "memory", "Resident").value, "—");
  assert.equal(fact(s, "threads", "Pool").value, "None");
  assert.ok(!s[0].facts.some((f) => f.label === "IO workgroup"));
  d.audio.workgroup = true;
  d.audio.workgroup_threads = 4;
  assert.equal(fact(overview(d, quiet(d)), "audio", "IO workgroup").value, "Yes (suggests 4 threads)");
});

test("a buffer other than the one asked for is marked", () => {
  const d = sample();
  d.audio.buffer_size = 512;
  const f = fact(overview(d, quiet(d)), "audio", "Buffer size");
  assert.equal(f.value, "512 (asked 256) (10.7 ms)");
  assert.equal(f.level, "warn");
});

test("track rows say the thread, the share and what's wrong", () => {
  const d = sample();
  d.tracks.push(track(3, "Pad", { off: true }), track(4, "Synth", { thread: null }), track(5, "Stuck", { held: true, plugins: [{ uid: 1, name: "Slow", instrument: true, bypass: false, builtin: false, latency: null, unresponsive: true }] }));
  const rows = trackRows(d);
  assert.deepEqual(
    rows.map((r) => [r.name, r.thread, r.share]),
    [
      ["Drums", "Audio", "10.0%"],
      ["Bass", "DSP 2", "30.0%"],
      ["Pad", "—", "10.0%"],
      ["Synth", "—", "—"],
      ["Stuck", "Audio", "10.0%"],
    ],
  );
  assert.equal(rows[1].plugins, "Comp [64]");
  assert.equal(rows[2].state, "off");
  assert.equal(rows[4].state, "held in a plug-in; not responding: Slow");
  assert.equal(rows[4].level, "alert");
  assert.equal(rows[0].level, undefined);
});

test("held peaks replace the snapshot's, and heaviest first keeps ties in order", () => {
  const d = sample();
  const held = new Map([[1, { peak: 0.9, at: 0 }]]);
  const rows = trackRows(d, held);
  assert.equal(rows[0].peak, "90.0%");
  assert.equal(rows[0].level, "warn", "over half the buffer");
  assert.deepEqual(heaviestFirst(rows).map((r) => r.name), ["Drums", "Bass"]);
  assert.deepEqual(heaviestFirst(trackRows(d)).map((r) => r.name), ["Bass", "Drums"]);
  d.tracks = [track(1, "A", { peak: 0 }), track(2, "B", { peak: 0 })];
  assert.deepEqual(heaviestFirst(trackRows(d)).map((r) => r.name), ["A", "B"]);
});

test("threads' shares add up the tracks each played, off tracks aside", () => {
  const d = sample();
  d.tracks.push(track(3, "More", { thread: 2, share: 0.05 }), track(4, "Off", { thread: 0, off: true, share: 0.5 }));
  const shares = threadShares(d).map(([t, s]) => [t, Math.round(s * 100)]);
  assert.deepEqual(shares, [
    ["Audio", 10],
    ["DSP 2", 35],
  ]);
});
