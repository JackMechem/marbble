// Unit tests for src/inspector/trackMidiModel.ts, the Track inspector's MIDI rows: which tracks
// have them, their defaults, the limit pairs and the note names. The last test reads the engine's
// sources, so the rows' fields and defaults stay those the engine saves and accepts.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { DEFAULT_TRACK_MIDI, formatKey, hasMidiSettings, limitWith, midiSettingsOf, parseKey, TRANSPOSE_MAX, VELOCITY_MAX } from "../../src/inspector/trackMidiModel.ts";
import type { Track, TrackKind } from "../../src/engine/project.ts";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));

const track = (kind: TrackKind, midi_settings?: Track["midi_settings"]): Track => ({
  id: 1,
  kind,
  name: "T",
  color: "#000",
  volume_db: 0,
  pan: 0,
  mute: false,
  solo: false,
  armed: false,
  monitor: false,
  input: { type: "none" },
  instrument: null,
  inserts: [],
  regions: [],
  parent: null,
  collapsed: false,
  group: null,
  hidden: false,
  sends: [],
  output: null,
  bus: 0,
  automation: [],
  auto_mode: "read",
  ...(midi_settings ? { midi_settings } : {}),
});

test("only software instrument tracks have MIDI rows", () => {
  assert.ok(hasMidiSettings(track("instrument")));
  for (const kind of ["audio", "aux", "folder", "summing"] as const) assert.ok(!hasMidiSettings(track(kind)));
});

test("a track saved without settings shows the defaults, which change nothing", () => {
  assert.deepEqual(midiSettingsOf(track("instrument")), DEFAULT_TRACK_MIDI);
  const set = { ...DEFAULT_TRACK_MIDI, transpose: -12, key_limit: [36, 60] as [number, number] };
  assert.deepEqual(midiSettingsOf(track("instrument", set)), set);
});

test("moving one end of a limit past the other takes the other along", () => {
  assert.deepEqual(limitWith([36, 60], "low", 40), [40, 60]);
  assert.deepEqual(limitWith([36, 60], "low", 72), [72, 72]);
  assert.deepEqual(limitWith([36, 60], "high", 50), [36, 50]);
  assert.deepEqual(limitWith([36, 60], "high", 20), [20, 20]);
  assert.deepEqual(limitWith([0, 127], "low", 0), [0, 127]);
});

test("key limits read and show as Logic's note names", () => {
  assert.equal(formatKey(0), "C-2");
  assert.equal(formatKey(60), "C3");
  assert.equal(formatKey(127), "G8");
  assert.equal(parseKey("C3"), 60);
  assert.equal(parseKey("f#2"), 54);
  assert.equal(parseKey("100"), 100);
  assert.equal(parseKey("loud"), null);
});

test("fields, defaults and ranges match the engine's", () => {
  const model = readFileSync(`${ROOT}crates/engine/src/edit/track_midi/mod.rs`, "utf8");
  const patch = readFileSync(`${ROOT}crates/engine/src/session/tracks/midi_settings.rs`, "utf8");
  const regionParams = readFileSync(`${ROOT}crates/engine/src/edit/region_params/mod.rs`, "utf8");
  const struct = /pub struct TrackMidiSettings \{([\s\S]*?)\n\}/.exec(model)![1];
  const fields = [...struct.matchAll(/pub (\w+):/g)].map((m) => m[1]);
  assert.deepEqual(fields.sort(), Object.keys(DEFAULT_TRACK_MIDI).sort());
  const patchStruct = /pub struct TrackMidiPatch \{([\s\S]*?)\n\}/.exec(patch)![1];
  assert.deepEqual([...patchStruct.matchAll(/pub (\w+):/g)].map((m) => m[1]).sort(), fields.sort());
  const d = DEFAULT_TRACK_MIDI;
  const defaults = `TrackMidiSettings { transpose: ${d.transpose}, velocity: ${d.velocity}, key_limit: (${d.key_limit.join(", ")}), velocity_limit: (${d.velocity_limit.join(", ")}), delay: 0.0, delay_ms: ${d.delay_ms} }`;
  assert.ok(model.includes(defaults), defaults);
  assert.ok(model.includes(`VELOCITY_RANGE: (i16, i16) = (-${VELOCITY_MAX}, ${VELOCITY_MAX})`));
  assert.ok(regionParams.includes(`TRANSPOSE_RANGE: (i16, i16) = (-${TRANSPOSE_MAX}, ${TRANSPOSE_MAX})`));
});
