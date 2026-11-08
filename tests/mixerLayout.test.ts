// Unit tests for src/mixerLayout.ts (the Mixer's stacks, strip widths and plate colours). Run with
// `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  bracketRows,
  clampStripWidth,
  DEFAULT_MIXER_VIEW,
  entryTracks,
  FOLDER_W,
  mixerEntries,
  parseMixerView,
  plateInk,
  stackKindLabel,
  STRIP_W,
  STRIP_W_MAX,
  STRIP_W_MIN,
  stripKey,
  stripWidth,
  tint,
  withStripWidth,
  type MixerEntry,
} from "../src/mixerLayout.ts";
import type { Track, TrackKind } from "../src/api.ts";

const track = (id: number, kind: TrackKind, parent: number | null = null, collapsed = false): Track =>
  ({ id, kind, name: `T${id}`, color: "#e0794a", parent, collapsed, hidden: false }) as Track;

/** Entries as nested arrays of ids: a stack is [id, open, count, ...members]. */
const shape = (es: MixerEntry[]): unknown[] => es.map((e) => (e.kind === "stack" ? [e.track.id, e.open, e.count, ...shape(e.members)] : e.track.id));

// Red Clay (folder 1) holds Drums (summing 2: OH 3, Kick 4) and Bass 5; Vox 6 is on its own.
const song = (collapse: number[] = []) =>
  [track(1, "folder"), track(2, "summing", 1), track(3, "audio", 2), track(4, "audio", 2), track(5, "audio", 1), track(6, "audio")].map((t) =>
    collapse.includes(t.id) ? { ...t, collapsed: true } : t,
  );

test("stacks hold their subtracks, nested stacks inside them, with every subtrack counted", () => {
  const tracks = song();
  const es = mixerEntries(tracks, tracks);
  assert.deepEqual(shape(es), [[1, true, 4, [2, true, 2, 3, 4], 5], 6]);
  assert.equal(bracketRows(es), 2, "a folder and a summing stack inside it");
  const drums = (es[0] as Extract<MixerEntry, { kind: "stack" }>).members[0];
  assert.equal(drums.depth, 1);
  assert.deepEqual(entryTracks(es).map((t) => t.id), [1, 2, 3, 4, 5, 6], "the strips in the Mixer's order");
});

test("a collapsed stack keeps its strip and count but shows none of its subtracks", () => {
  const tracks = song([1]);
  const es = mixerEntries(tracks, tracks);
  assert.deepEqual(shape(es), [[1, false, 4], 6]);
  assert.deepEqual(entryTracks(es).map((t) => t.id), [1, 6], "rows are shared by the strips shown");
  assert.equal(bracketRows(es), 1);
  const inner = song([2]);
  assert.deepEqual(shape(mixerEntries(inner, inner)), [[1, true, 4, [2, false, 2], 5], 6]);
});

test("a stack follows its own open state, inside a collapsed one too", () => {
  // Opening the inner stack shows nothing while the outer one is collapsed.
  const tracks = song([1]);
  const both = song([1, 2]);
  assert.deepEqual(shape(mixerEntries(tracks, tracks)), shape(mixerEntries(both, both)));
});

test("filtered strips leave their stack; a stack with nothing shown is a plain strip", () => {
  const tracks = song();
  // Audio strips hidden by the filter: Drums has nothing to bracket, the folder still holds it.
  const shown = tracks.filter((t) => t.kind !== "audio");
  const es = mixerEntries(tracks, shown);
  assert.deepEqual(shape(es), [[1, true, 1, 2]]);
  assert.equal(bracketRows(es), 1);
  assert.deepEqual(shape(mixerEntries(tracks, [tracks[0], tracks[5]])), [1, 6]);
  // A hidden stack: its subtracks go to the stack around it.
  const noDrums = tracks.filter((t) => t.id !== 2);
  assert.deepEqual(shape(mixerEntries(tracks, noDrums)), [[1, true, 3, 3, 4, 5], 6]);
});

test("the bracket names the stack's kind and size", () => {
  assert.equal(stackKindLabel(track(1, "folder"), 5), "Folder · 5 tracks");
  assert.equal(stackKindLabel(track(2, "summing"), 1), "Σ · 1 track");
});

test("strips take their size's width, a folder stays narrow, and a strip's own width wins", () => {
  const v = { ...DEFAULT_MIXER_VIEW, size: "wide" as const };
  assert.equal(stripWidth(v, stripKey.track(3)), STRIP_W.wide);
  assert.equal(stripWidth(v, stripKey.track(1), true), FOLDER_W);
  const sized = withStripWidth(v, stripKey.track(3), 180.4);
  assert.equal(stripWidth(sized, stripKey.track(3)), 180);
  assert.equal(stripWidth({ ...sized, size: "narrow" }, stripKey.track(3)), 180, "a size doesn't undo a strip's own width");
  assert.equal(stripWidth(sized, stripKey.track(4)), STRIP_W.wide);
  assert.equal(stripWidth(withStripWidth(sized, stripKey.track(3), null), stripKey.track(3)), STRIP_W.wide, "double-click resets it");
  assert.equal(stripWidth(withStripWidth(v, stripKey.master, 1), stripKey.master), STRIP_W_MIN);
  assert.equal(clampStripWidth(9999), STRIP_W_MAX);
  assert.deepEqual(v.widths, {}, "the view isn't changed in place");
  assert.notEqual(stripKey.track(7), stripKey.vca(7), "a VCA and a track with the same id are different strips");
});

test("a remembered view reads back, and anything malformed falls back", () => {
  const v = withStripWidth(withStripWidth({ ...DEFAULT_MIXER_VIEW, size: "narrow" }, stripKey.track(3), 150), stripKey.vca(2), 90);
  assert.deepEqual(parseMixerView(JSON.stringify(v)), v);
  assert.deepEqual(parseMixerView(null), DEFAULT_MIXER_VIEW);
  assert.deepEqual(parseMixerView("{not json"), DEFAULT_MIXER_VIEW);
  assert.deepEqual(parseMixerView("[1,2]"), DEFAULT_MIXER_VIEW);
  assert.deepEqual(parseMixerView(JSON.stringify({ size: "huge", widths: { t3: 5000, bogus: 100, t4: "wide", out: 70 } })), {
    size: "normal",
    widths: { t3: STRIP_W_MAX, out: 70 },
  });
});

test("name plates get the ink that reads best on the track's colour", () => {
  assert.equal(plateInk("#f2a63b"), "dark", "orange");
  assert.equal(plateInk("#c9cf3a"), "dark", "yellow");
  assert.equal(plateInk("#8d9099"), "dark", "Stereo Out's grey");
  assert.equal(plateInk("#6a5be0"), "light", "purple");
  assert.equal(plateInk("#1a3d7a"), "light", "dark blue");
  assert.equal(plateInk("#fff"), "dark");
  assert.equal(plateInk("not a colour"), "light");
  assert.equal(tint("#e0794a", 0.1), "rgba(224, 121, 74, 0.1)");
  assert.equal(tint("red", 0.1), "transparent");
});
