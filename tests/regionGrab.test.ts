// Unit tests for src/regionGrab.ts: what a press on a region in the Tracks area grabs.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { Region } from "../src/api.ts";
import { regionBar } from "../src/regionBar.ts";
import { grabKind, type RegionPress } from "../src/regionGrab.ts";
import type { Crossfade, RegionCrossfades } from "../src/fades.ts";

const pps = 0.1;
const laneH = 63;
// The geometry a 60 px region has: name bar, then the body with the fade band at its top.
const { bodyTop, bodyH } = regionBar(laneH - 3);
const inName = bodyTop - 2;
const inFadeBand = bodyTop + 2;
const lower = bodyTop + bodyH - 2;
assert.ok(lower - bodyTop > 14, "the lower body is outside the fade band");

const audio = (over: Partial<Region> = {}, fadeIn = 0, fadeOut = 0) =>
  ({ id: 1, start: 1000, length: 1000, loop_end: null, content: { type: "audio", fade_in: fadeIn, fade_out: fadeOut }, ...over }) as unknown as Region;
const midi = (over: Partial<Region> = {}) => ({ id: 1, start: 1000, length: 1000, loop_end: null, content: { type: "midi" }, ...over }) as unknown as Region;
const press = (region: Region, x: number, localY: number, inLoop = false): RegionPress => ({ region, x, localY, laneH, inLoop });
const none = () => undefined;

// The region is 100..200 px on screen.
test("the name bar moves, except at the right end where the loop tool is", () => {
  assert.equal(grabKind(press(midi(), 150, inName), pps, none), "move");
  assert.equal(grabKind(press(midi(), 105, inName), pps, none), "move");
  assert.equal(grabKind(press(midi(), 195, inName), pps, none), "loop");
});

test("the lower body trims at its edges and moves in between", () => {
  assert.equal(grabKind(press(midi(), 102, lower), pps, none), "left");
  assert.equal(grabKind(press(midi(), 198, lower), pps, none), "right");
  assert.equal(grabKind(press(midi(), 150, lower), pps, none), "move");
});

test("trim zones shrink on a narrow region so its middle still moves it", () => {
  const narrow = midi({ length: 200 }); // 20 px: zones of 5 px
  assert.equal(grabKind(press(narrow, 104, lower), pps, none), "left");
  assert.equal(grabKind(press(narrow, 110, lower), pps, none), "move");
  assert.equal(grabKind(press(narrow, 116, lower), pps, none), "right");
});

test("the band at the top of an audio body holds the fades", () => {
  assert.equal(grabKind(press(audio(), 102, inFadeBand), pps, none), "fadeIn");
  assert.equal(grabKind(press(audio(), 198, inFadeBand), pps, none), "fadeOut");
  // At a fade's current point, inside the region.
  assert.equal(grabKind(press(audio({}, 300, 0), 130, inFadeBand), pps, none), "fadeIn");
  assert.equal(grabKind(press(audio({}, 0, 300), 170, inFadeBand), pps, none), "fadeOut");
  // A MIDI region has no fades: the same place trims.
  assert.equal(grabKind(press(midi(), 102, inFadeBand), pps, none), "left");
});

test("a region crossfaded into has no fade-in handle, and a crossfade's handle is at its start", () => {
  const xfIn: RegionCrossfades = { in: {} as Crossfade };
  assert.equal(grabKind(press(audio(), 102, inFadeBand), pps, () => xfIn), "left");
  const xfOut: RegionCrossfades = { out: { fade_out: [1700, 300] } as unknown as Crossfade };
  assert.equal(grabKind(press(audio(), 170, inFadeBand), pps, () => xfOut), "fadeOut");
});

test("looping: the handle is at the loop's end, repetitions move, and the fade-out isn't grabbed", () => {
  const looped = audio({ loop_end: 3000 }); // repeats to 300 px
  assert.equal(grabKind(press(looped, 296, inName, true), pps, none), "loop");
  assert.equal(grabKind(press(looped, 250, inName, true), pps, none), "move");
  assert.equal(grabKind(press(looped, 250, lower, true), pps, none), "move");
  // At the body's right end the fade band trims rather than sets a fade.
  assert.equal(grabKind(press(looped, 198, inFadeBand), pps, none), "right");
});

test("a region drawn as its bar alone has no fade handles, but still trims and loops", () => {
  const short = 24; // a 21 px region: bar only
  assert.ok(regionBar(short - 3).barOnly);
  const g = regionBar(short - 3);
  const at = (region: Region, x: number, localY: number) => grabKind({ region, x, localY, laneH: short, inLoop: false }, pps, none);
  const below = g.bodyTop + 1;
  assert.equal(at(audio(), 102, below), "left");
  assert.equal(at(audio(), 198, below), "right");
  assert.equal(at(audio({}, 300, 300), 130, below), "move", "no fade-in handle at the fade point");
  assert.equal(at(audio(), 195, g.bodyTop - 1), "loop");
  assert.equal(at(audio(), 150, below), "move");
});
