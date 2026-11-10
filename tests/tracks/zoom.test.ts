// Unit tests for the Tracks area's horizontal zoom (src/tracks/zoom.ts): the steps of the menu,
// keys and wheel, held within the limits.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ZOOM_MAX, ZOOM_MIN, clampZoom, stepZoom, wheelZoom } from "../../src/tracks/zoom.ts";

const close = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) < eps, `${a} ≈ ${b}`);

test("Zoom In and Out step by 1.4 within the limits", () => {
  close(stepZoom(100, 1), 140);
  close(stepZoom(140, -1), 100);
  assert.equal(stepZoom(ZOOM_MAX, 1), ZOOM_MAX);
  assert.equal(stepZoom(ZOOM_MIN, -1), ZOOM_MIN);
  assert.equal(stepZoom(350, 1), ZOOM_MAX);
});

test("a wheel notch steps by 1.15 within the limits, in when turned away", () => {
  close(wheelZoom(100, -1), 115);
  close(wheelZoom(115, 1), 100);
  assert.equal(wheelZoom(ZOOM_MAX, -1), ZOOM_MAX);
  assert.equal(wheelZoom(ZOOM_MIN, 1), ZOOM_MIN);
});

test("clampZoom holds any zoom within the limits", () => {
  assert.equal(clampZoom(0.5), ZOOM_MIN);
  assert.equal(clampZoom(1e6), ZOOM_MAX);
  assert.equal(clampZoom(50), 50);
});
