// Unit tests for the Tracks area's horizontal zoom (src/tracks/zoom.ts): the steps of the menu,
// keys and wheel, held within the limits, which go in to single samples, and the zoom slider's
// scale, which stops short of that.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clampZoom, MAX_PX_PER_SAMPLE, sliderToZoom, SLIDER_MAX, stepZoom, wheelZoom, ZOOM_MIN, zoomMax, zoomToSlider } from "../../src/tracks/zoom.ts";

const close = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) < eps, `${a} ≈ ${b}`);
/** Samples a beat at 120 bpm and 48 kHz. */
const SPB = 24_000;

test("Zoom In and Out step by 1.4 within the limits", () => {
  close(stepZoom(100, 1, SPB), 140);
  close(stepZoom(140, -1, SPB), 100);
  assert.equal(stepZoom(zoomMax(SPB), 1, SPB), zoomMax(SPB));
  assert.equal(stepZoom(ZOOM_MIN, -1, SPB), ZOOM_MIN);
});

test("Zoom In goes on past the slider's end, in to single samples", () => {
  close(stepZoom(SLIDER_MAX, 1, SPB), SLIDER_MAX * 1.4);
  // Samples 32 px apart: 32 px a sample times 24 000 samples a beat.
  assert.equal(zoomMax(SPB), MAX_PX_PER_SAMPLE * SPB);
  assert.equal(stepZoom(zoomMax(SPB) / 1.2, 1, SPB), zoomMax(SPB));
  // The limit follows the project: twice the samples a beat, twice as far in.
  assert.equal(zoomMax(2 * SPB), 2 * zoomMax(SPB));
  // Never short of the slider's range.
  assert.equal(zoomMax(1), SLIDER_MAX);
});

test("a wheel notch steps by 1.15 within the limits, in when turned away", () => {
  close(wheelZoom(100, -1, SPB), 115);
  close(wheelZoom(115, 1, SPB), 100);
  close(wheelZoom(SLIDER_MAX, -1, SPB), SLIDER_MAX * 1.15);
  assert.equal(wheelZoom(zoomMax(SPB), -1, SPB), zoomMax(SPB));
  assert.equal(wheelZoom(ZOOM_MIN, 1, SPB), ZOOM_MIN);
});

test("clampZoom holds any zoom within the limits", () => {
  assert.equal(clampZoom(0.5, SPB), ZOOM_MIN);
  assert.equal(clampZoom(1e9, SPB), zoomMax(SPB));
  assert.equal(clampZoom(50, SPB), 50);
  assert.equal(clampZoom(5000, SPB), 5000);
});

test("the slider spans the widest zoom to 400 px a beat and stays at its end past that", () => {
  assert.equal(zoomToSlider(ZOOM_MIN), 0);
  close(zoomToSlider(SLIDER_MAX), 1);
  assert.equal(zoomToSlider(SLIDER_MAX * 100), 1);
  close(sliderToZoom(0), ZOOM_MIN);
  close(sliderToZoom(1), SLIDER_MAX);
  close(sliderToZoom(zoomToSlider(37)), 37);
});
