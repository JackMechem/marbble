// Unit tests for the window the Tracks area's scroller shows of a timeline too wide for a browser
// to scroll (src/tracks/canvas/scrollWindow.ts).
import { test } from "node:test";
import assert from "node:assert/strict";
import { clampLeft, MAX_SCROLL_W, nearWindowEnd, windowAt } from "../../src/tracks/canvas/scrollWindow.ts";

test("a timeline that fits is scrolled as it is", () => {
  assert.deepEqual(windowAt(5000, 800, 100_000), { origin: 0, width: 100_000 });
  assert.equal(nearWindowEnd({ origin: 0, width: 100_000 }, 99_000, 800, 100_000), false);
});

test("a wider one shows a window with the view in its middle", () => {
  const total = 500_000_000;
  const w = windowAt(200_000_000, 800, total);
  assert.equal(w.width, MAX_SCROLL_W);
  assert.equal(200_000_000 + 400 - w.origin, MAX_SCROLL_W / 2);
  // Held at the ends of the timeline.
  assert.deepEqual(windowAt(100, 800, total), { origin: 0, width: MAX_SCROLL_W });
  assert.equal(windowAt(total - 800, 800, total).origin, total - MAX_SCROLL_W);
});

test("the window moves on when a scroll nears an end the timeline goes on past", () => {
  const total = 500_000_000;
  const w = { origin: 100_000_000, width: MAX_SCROLL_W };
  assert.equal(nearWindowEnd(w, w.origin + MAX_SCROLL_W / 2, 800, total), false);
  assert.equal(nearWindowEnd(w, w.origin + 1000, 800, total), true);
  assert.equal(nearWindowEnd(w, w.origin + MAX_SCROLL_W - 2000, 800, total), true);
  // At the timeline's own start there's nothing further to show.
  assert.equal(nearWindowEnd({ origin: 0, width: MAX_SCROLL_W }, 0, 800, total), false);
});

test("the left edge stays on the timeline", () => {
  assert.equal(clampLeft(-50, 800, 10_000), 0);
  assert.equal(clampLeft(9_900, 800, 10_000), 9_200);
  assert.equal(clampLeft(100, 800, 500), 0);
});
