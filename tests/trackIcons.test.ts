import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { defaultIcon, iconFromName, iconLabel, NAME_ICONS, pickerMove, shownIcon, TRACK_ICON_GROUPS, trackIconInfo } from "../src/trackIcons.ts";

const ids = TRACK_ICON_GROUPS.flatMap((g) => g.icons.map((i) => i.id));

test("the set has each icon once, the kinds' own first, and a drawing for each", () => {
  assert.equal(new Set(ids).size, ids.length);
  assert.ok(ids.length >= 30 && ids.length <= 40, `${ids.length} icons`);
  assert.deepEqual(TRACK_ICON_GROUPS[0].icons.map((i) => i.id), ["audio", "instrument", "aux", "folder", "summing"]);
  const art = readFileSync(new URL("../src/trackIconArt.tsx", import.meta.url), "utf8");
  for (const id of ids) assert.match(art, new RegExp(`^  ("${id}"|${id}): `, "m"), `no drawing for ${id}`);
});

test("every icon the name table suggests is in the set", () => {
  for (const [, id] of NAME_ICONS) assert.ok(trackIconInfo(id), id);
});

test("names suggest icons the way engineers label tracks", () => {
  const cases: [string, string | undefined][] = [
    ["Kick", "kick"],
    ["Kick In", "kick"],
    ["Kick Out", "kick"],
    ["kick2", "kick"],
    ["KickIn", "kick"],
    ["Bass Drum", "kick"],
    ["Snare Top", "snare"],
    ["Snare Bottom", "snare"],
    ["Hi-Hat", "hihat"],
    ["HH", "hihat"],
    ["Toms", "toms"],
    ["Floor Tom", "toms"],
    ["OH", "overheads"],
    ["OH L", "overheads"],
    ["Overheads", "overheads"],
    ["Ride", "overheads"],
    ["Room", "room"],
    ["Drum Room", "room"],
    ["Drums", "kit"],
    ["808", "drum-machine"],
    ["Perc", "congas"],
    ["Tamb", "tambourine"],
    ["Bass", "bass"],
    ["Bass DI", "bass"],
    ["Synth Bass", "bass"],
    ["Bass Amp", "amp"],
    ["Upright", "upright-bass"],
    ["Gtr", "electric-guitar"],
    ["Guitar 2", "electric-guitar"],
    ["Gtrs", "electric-guitar"],
    ["Acoustic Gtr", "acoustic-guitar"],
    ["Keys", "piano"],
    ["Piano", "piano"],
    ["B3", "organ"],
    ["Synth Pad", "synth"],
    ["Strings", "violin"],
    ["Cello", "violin"],
    ["Brass", "trumpet"],
    ["Sax", "sax"],
    ["Vox", "mic"],
    ["Lead Vocal", "mic"],
    ["Vocals", "mic"],
    ["BVs", "choir"],
    ["Backing Vocals", "choir"],
    ["Choir", "choir"],
    ["Reverb", "reverb"],
    ["Delay", "delay"],
    ["FX", "fx"],
    ["Cue", "headphones"],
    // Not words of their own.
    ["Audio 1", undefined],
    ["Inst 3", undefined],
    ["Ohm", undefined],
    ["Hattie", undefined],
    ["Bassoonist", undefined],
    ["Tomorrow", undefined],
  ];
  for (const [name, icon] of cases) assert.equal(iconFromName(name), icon, name);
});

test("a track shows its chosen icon, else its name's, else its kind's", () => {
  assert.equal(shownIcon({ kind: "audio", name: "Audio 1" }), "audio");
  assert.equal(shownIcon({ kind: "instrument", name: "Inst 1", icon: null }), "instrument");
  assert.equal(shownIcon({ kind: "summing", name: "Sum 1" }), "summing");
  assert.equal(shownIcon({ kind: "audio", name: "Kick In" }), "kick");
  assert.equal(shownIcon({ kind: "folder", name: "Drums" }), "kit");
  assert.equal(shownIcon({ kind: "aux", name: "Verb" }), "reverb");
  // A choice wins over the name, and renaming doesn't change it.
  assert.equal(shownIcon({ kind: "audio", name: "Kick In", icon: "mic" }), "mic");
  assert.equal(shownIcon({ kind: "audio", name: "Snare", icon: "mic" }), "mic");
  // An id this version doesn't know (a newer project's) falls back to the default.
  assert.equal(shownIcon({ kind: "audio", name: "Snare", icon: "theremin" }), "snare");
  assert.equal(defaultIcon({ kind: "audio", name: "Snare" }), "snare");
});

test("the inspector names the chosen icon, or Default", () => {
  assert.equal(iconLabel({ icon: "kick" }), "Kick");
  assert.equal(iconLabel({ icon: null }), "Default");
  assert.equal(iconLabel({}), "Default");
  assert.equal(iconLabel({ icon: "theremin" }), "Default");
});

test("the picker's arrow keys move through Default and the group rows", () => {
  const rows = [1, 5, 8, 4];
  assert.deepEqual(pickerMove(rows, 1, 2, "ArrowRight"), [1, 3]);
  assert.deepEqual(pickerMove(rows, 1, 4, "ArrowRight"), [2, 0], "on to the next row");
  assert.deepEqual(pickerMove(rows, 2, 0, "ArrowLeft"), [1, 4], "back to the row before");
  assert.deepEqual(pickerMove(rows, 0, 0, "ArrowLeft"), [0, 0], "not past the start");
  assert.deepEqual(pickerMove(rows, 3, 3, "ArrowRight"), [3, 3], "nor the end");
  assert.deepEqual(pickerMove(rows, 2, 7, "ArrowUp"), [1, 4], "a shorter row takes its last");
  assert.deepEqual(pickerMove(rows, 1, 3, "ArrowDown"), [2, 3]);
  assert.deepEqual(pickerMove(rows, 0, 0, "ArrowDown"), [1, 0]);
  assert.deepEqual(pickerMove(rows, 1, 3, "ArrowUp"), [0, 0], "up to Default");
  assert.deepEqual(pickerMove(rows, 3, 2, "ArrowDown"), [3, 2]);
  assert.deepEqual(pickerMove(rows, 2, 5, "Home"), [0, 0]);
  assert.deepEqual(pickerMove(rows, 2, 5, "End"), [3, 3]);
  assert.deepEqual(pickerMove(rows, 2, 5, "a"), [2, 5]);
});
