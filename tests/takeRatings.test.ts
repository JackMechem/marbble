import { test } from "node:test";
import assert from "node:assert/strict";
import type { Take, TakeFolder } from "../src/api.ts";
import { hasBadTakes, takeLabel, toggledRating } from "../src/takeRatings.ts";

const take = (id: number, rating?: Take["rating"]): Take => ({ id, name: `Take ${id}`, clip: 1, offset: 0, start: 0, end: 100, gain_db: 0, rating });
const folder = (takes: Take[]): TakeFolder => ({ takes, comp: [] });

test("rated takes carry their mark before the name", () => {
  assert.equal(takeLabel(take(1)), "Take 1");
  assert.equal(takeLabel(take(2, "good")), "★ Take 2");
  assert.equal(takeLabel(take(3, "bad")), "✗ Take 3");
  assert.equal(takeLabel(take(4, "none")), "Take 4");
});

test("choosing a take's rating again clears it", () => {
  assert.equal(toggledRating(undefined, "good"), "good");
  assert.equal(toggledRating("good", "good"), "none");
  assert.equal(toggledRating("good", "bad"), "bad");
  assert.equal(toggledRating("none", "none"), "none");
});

test("Delete Bad Takes needs a bad take and something to keep", () => {
  assert.ok(!hasBadTakes(folder([take(1), take(2, "good")])));
  assert.ok(hasBadTakes(folder([take(1, "bad"), take(2)])));
  assert.ok(!hasBadTakes(folder([take(1, "bad")])));
});
