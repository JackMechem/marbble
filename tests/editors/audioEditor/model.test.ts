// Unit tests for src/editors/audioEditor/model.ts: the Audio Track Editor's layout and what a press grabs.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { Project } from "../../../src/engine/project.ts";
import type { Region } from "../../../src/engine/regions.ts";
import type { Crossfade, RegionCrossfades } from "../../../src/tracks/regions/fades.ts";
import { dbGain, geometry, grabKind, HEADER, lowerBound, MAX_CONTENT_PX, MAX_PPS, RULER, timelineFrames, zoomBounds, type AudioPress } from "../../../src/editors/audioEditor/model.ts";

test("geometry: the lane below the ruler, the body below the name bar", () => {
  const g = geometry(200);
  assert.ok(g.top > RULER);
  assert.ok(g.bodyTop >= g.top + HEADER);
  assert.ok(g.bodyTop + g.bodyH <= g.top + g.h);
  // However short the pane, the region keeps its name bar and a strip of body.
  assert.ok(geometry(10).bodyH > 0);
});

test("the timeline is at least six minutes, and a minute past the last region", () => {
  const project = (ends: number[]) => ({ sample_rate: 100, tracks: [{ regions: ends.map((e) => ({ start: 0, length: e })) }] }) as unknown as Project;
  assert.equal(timelineFrames(project([])), 100 * 60 * 6);
  assert.equal(timelineFrames(project([100 * 60 * 10])), 100 * 60 * 11);
});

test("dbGain: unity at 0 dB, silence at the bottom of the range", () => {
  assert.equal(dbGain(0), 1);
  assert.ok(Math.abs(dbGain(-6) - 0.501) < 0.001);
  assert.equal(dbGain(-96), 0);
});

test("lowerBound", () => {
  assert.equal(lowerBound([], 3), 0);
  assert.equal(lowerBound([1, 3, 3, 5], 3), 1);
  assert.equal(lowerBound([1, 3, 3, 5], 4), 3);
  assert.equal(lowerBound([1, 3, 3, 5], 9), 4);
});

test("zoomBounds: from the whole timeline fitting to the closest allowed", () => {
  assert.deepEqual(zoomBounds(600, 6000), [0.1, MAX_PPS]);
  // On a very long timeline the width the browser can lay out sets the limit.
  const long = MAX_CONTENT_PX * 10;
  assert.equal(zoomBounds(600, long)[1], 0.1);
});

const g = geometry(200);
const audio = (fadeIn = 0, fadeOut = 0) => ({ id: 1, start: 1000, length: 1000, content: { type: "audio", fade_in: fadeIn, fade_out: fadeOut } }) as unknown as Region;
const press = (region: Region | undefined, x: number, y: number): AudioPress => ({ region, x, y, g });
const pps = 0.1;
const none = () => undefined;
const inBand = g.bodyTop + 2;
const lower = g.bodyTop + g.bodyH - 2;

// The region is 100..200 px on screen.
test("the rest of the lane selects a range", () => {
  assert.equal(grabKind(press(undefined, 50, lower), pps, none), "select");
  assert.equal(grabKind(press(audio(), 150, lower), pps, none), "select");
});

test("the name bar moves, the edges trim", () => {
  assert.equal(grabKind(press(audio(), 150, g.top + 2), pps, none), "move");
  assert.equal(grabKind(press(audio(), 102, lower), pps, none), "left");
  assert.equal(grabKind(press(audio(), 198, lower), pps, none), "right");
});

test("the band at the top of the waveform holds the fades", () => {
  assert.equal(grabKind(press(audio(), 102, inBand), pps, none), "fadeIn");
  assert.equal(grabKind(press(audio(), 198, inBand), pps, none), "fadeOut");
  assert.equal(grabKind(press(audio(300, 0), 130, inBand), pps, none), "fadeIn");
  assert.equal(grabKind(press(audio(0, 300), 170, inBand), pps, none), "fadeOut");
});

test("a crossfade's handle is at its start and replaces the fade-in", () => {
  const xfIn: RegionCrossfades = { in: {} as Crossfade };
  assert.equal(grabKind(press(audio(), 102, inBand), pps, () => xfIn), "left");
  const xfOut: RegionCrossfades = { out: { fade_out: [1700, 300] } as unknown as Crossfade };
  assert.equal(grabKind(press(audio(), 170, inBand), pps, () => xfOut), "fadeOut");
});
