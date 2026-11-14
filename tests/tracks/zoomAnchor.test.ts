// Unit tests for where a Tracks area zoom holds still (src/tracks/zoomAnchor.ts), so it neither
// jumps nor fights Catch.
import { test } from "node:test";
import assert from "node:assert/strict";
import { anchoredZoomLeft, zoomAnchorX, type AnchorInput } from "../../src/tracks/zoomAnchor.ts";

const base: AnchorInput = { viewW: 1000, playheadX: 300, catchOn: false, playing: false };

test("a wheel zoom holds the time under the pointer", () => {
  assert.equal(zoomAnchorX({ ...base, pointerX: 700 }), 700);
  // Stopped, Catch on: still the pointer.
  assert.equal(zoomAnchorX({ ...base, pointerX: 700, catchOn: true }), 700);
  // Playing without Catch: the pointer.
  assert.equal(zoomAnchorX({ ...base, pointerX: 700, playing: true }), 700);
});

test("while Catch follows the playhead, every zoom holds the playhead", () => {
  const catching = { ...base, catchOn: true, playing: true };
  assert.equal(zoomAnchorX({ ...catching, pointerX: 700 }), 300);
  assert.equal(zoomAnchorX(catching), 300);
});

test("the menu, keys and slider hold the playhead with Catch on, else the middle", () => {
  assert.equal(zoomAnchorX({ ...base, catchOn: true }), 300);
  assert.equal(zoomAnchorX(base), 500, "Catch off");
  assert.equal(zoomAnchorX({ ...base, catchOn: true, playheadX: -40 }), 500, "playhead off screen");
  assert.equal(zoomAnchorX({ ...base, catchOn: true, playing: true, playheadX: 1200, pointerX: 80 }), 80, "playhead off screen, wheel");
});

test("the anchored time stays at the same x", () => {
  // Left 1000 px at 0.5 px a sample: x 200 shows sample 2400. At 1 px a sample it's at 2400 - 200.
  assert.equal(anchoredZoomLeft(1000, 200, 0.5, 1), 2200);
  // Zoomed out, it can ask for a left edge before the timeline's start; scrolling clamps that.
  assert.ok(Math.abs(anchoredZoomLeft(1000, 200, 0.5, 0.05) - -80) < 1e-9);
});
