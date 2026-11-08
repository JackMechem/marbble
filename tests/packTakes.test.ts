// Unit tests for src/packTakes.ts: the tracks a Pack Take Folder across tracks takes from and
// the ones it leaves empty.
import { test } from "node:test";
import assert from "node:assert/strict";
import { emptiedQuestion, emptiedTracks, packTracks } from "../src/packTakes.ts";
import type { Track } from "../src/api.ts";

const track = (id: number, regions: number[], alternatives = false) =>
  ({ id, name: `T${id}`, regions: regions.map((r) => ({ id: r })), ...(alternatives ? { alternatives: [{ name: "B" }] } : {}) }) as unknown as Track;

test("the tracks holding the regions, in track order", () => {
  const tracks = [track(1, [10]), track(2, []), track(3, [30, 31]), track(4, [40])];
  assert.deepEqual(packTracks(tracks, [40, 10, 31]), [1, 3, 4]);
  assert.deepEqual(packTracks(tracks, [30, 31]), [3]);
  assert.deepEqual(packTracks(tracks, []), []);
});

test("the tracks a pack emptied, without the folder's own", () => {
  // After packing 10, 30, 40 and 50 into a folder on track 1: track 3 still has 31, track 5 has
  // another alternative.
  const after = [track(1, [99]), track(2, []), track(3, [31]), track(4, []), track(5, [], true)];
  assert.deepEqual(emptiedTracks(after, [1, 3, 4, 5]), [4]);
  // Track 2 was empty before and wasn't part of the pack.
  assert.deepEqual(emptiedTracks(after, [1]), []);
  assert.deepEqual(emptiedTracks([track(1, [])], [1, 7]), [], "a track no longer there");
});

test("the question names the tracks, up to three", () => {
  assert.equal(emptiedQuestion(["OH 2"]), "The takes are in the take folder now. Delete “OH 2”, left with no regions?");
  assert.match(emptiedQuestion(["OH 2", "OH 3", "OH 4"]), /Delete “OH 2”, “OH 3” and “OH 4”, left/);
  assert.match(emptiedQuestion(["a", "b", "c", "d"]), /Delete these 4 tracks, left/);
});
