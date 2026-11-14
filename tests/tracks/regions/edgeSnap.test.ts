// Unit tests for src/tracks/regions/edgeSnap.ts: a moved selection's edges snap to other regions'.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { Track } from "../../../src/engine/project.ts";
import { edgeSnap, edgeTargets, movingEdges, regionEdges } from "../../../src/tracks/regions/edgeSnap.ts";

const region = (id: number, start: number, length: number, loop_end: number | null = null) => ({ id, start, length, loop_end });
const tracks = [{ regions: [region(1, 0, 1000), region(2, 2000, 500, 3000)] }, { regions: [region(3, 1500, 200)] }] as unknown as Track[];

test("a region's edges are its start and where it stops sounding", () => {
  assert.deepEqual(regionEdges(tracks, (id) => id !== 1), [2000, 3000, 1500, 1700]);
  assert.deepEqual(edgeTargets([3000, 0, 1500, 0]), [0, 1500, 3000]);
  // The selection's edges, from the grabbed region's start.
  assert.deepEqual(movingEdges(regionEdges(tracks, (id) => id === 1 || id === 3), 1500), [-1500, -500, 0, 200]);
});

test("the start or the end snaps to the nearest target in reach", () => {
  const targets = [1000, 5000];
  // A region 400 long: its start comes within 30 of 1000.
  assert.deepEqual(edgeSnap(1030, [0, 400], targets, 50), { to: 1000, at: 1000 });
  // Its end comes within 20 of 5000.
  assert.deepEqual(edgeSnap(4580, [0, 400], targets, 50), { to: 4600, at: 5000 });
  // Too far from both.
  assert.equal(edgeSnap(2000, [0, 400], targets, 50), null);
});

test("the nearest edge wins", () => {
  // Start 40 past 1000, end 10 short of 1450.
  assert.deepEqual(edgeSnap(1040, [0, 400], [1000, 1450], 50), { to: 1050, at: 1450 });
});

test("any region of the selection snaps, not just the grabbed one", () => {
  // Another selected region starts 3000 after the grabbed one.
  assert.deepEqual(edgeSnap(1990, [0, 100, 3000, 3100], [5000], 50), { to: 2000, at: 5000 });
});

test("never before the project's start", () => {
  // Dragged past the start, its end 200 on would land on 0 only with the region at -200.
  assert.equal(edgeSnap(-190, [0, 200], [0], 50), null);
  assert.equal(edgeSnap(0, [0], [], 50), null);
});
