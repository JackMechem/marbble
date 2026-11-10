// Unit tests for src/tracks/headers/headerColumns.ts (track header button columns). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { headerColumns, switchesOf, switchesOn, type HeaderTrack } from "../../../src/tracks/headers/headerColumns.ts";

const track = (kind: string, on: Partial<HeaderTrack> = {}): HeaderTrack => ({ kind, mute: false, solo: false, armed: false, monitor: false, ...on });

test("each kind of track has its own switches", () => {
  assert.deepEqual(switchesOf("audio"), ["power", "mute", "solo", "rec", "mon", "freeze"]);
  assert.deepEqual(switchesOf("instrument"), ["power", "mute", "solo", "rec", "freeze"]);
  assert.deepEqual(switchesOf("aux"), ["power", "mute", "solo"]);
  assert.deepEqual(switchesOf("summing"), ["power", "mute", "solo"]);
});

test("full headers share a column for every switch a track in view has", () => {
  // An instrument track leaves a gap where the audio track has I.
  assert.deepEqual(headerColumns([track("audio"), track("instrument")]).full, ["mute", "solo", "rec", "mon", "freeze"]);
  // No audio track: no I column at all.
  assert.deepEqual(headerColumns([track("instrument"), track("aux")]).full, ["mute", "solo", "rec", "freeze"]);
  assert.deepEqual(headerColumns([track("aux")]).full, ["mute", "solo"]);
});

test("one-line headers share a column for every switch lit on a track in view", () => {
  const tracks = [track("audio", { mute: true }), track("instrument", { armed: true }), track("aux", { off: true }), track("aux", { monitor: true })];
  // The aux can't monitor, so its stray flag lights nothing.
  assert.deepEqual(headerColumns(tracks).compact, ["power", "mute", "rec"]);
  assert.deepEqual(switchesOn(track("instrument", { frozen: { file: "x" }, solo: true })), ["solo", "freeze"]);
  assert.deepEqual(headerColumns([track("audio")]).compact, []);
});
