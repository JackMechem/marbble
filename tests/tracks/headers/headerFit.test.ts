// Unit tests for src/tracks/headers/headerFit.ts (what fits in a track header across the column)
// and src/tracks/heightSlider.ts (the Height slider's scale).
import { test } from "node:test";
import assert from "node:assert/strict";
import { AUTO_SLIDER_MIN, DISCLOSURE_W, fitControls, headerBand, iconLeft, INDENT, leadWidth, ONE_LINE_SLIDER_MIN, oneLineSliderW, SLIDER_MAX, SLIDER_MIN } from "../../../src/tracks/headers/headerFit.ts";
import type { SwitchId } from "../../../src/tracks/headers/headerColumns.ts";
import { heightToSlider, sliderToHeight } from "../../../src/tracks/heightSlider.ts";

const AUDIO: SwitchId[] = ["mute", "solo", "rec", "mon", "freeze"];
// The column's default width (store.ts) less its border.
const W = 295;

test("the lead holds the stack indent and, while a stack is in view, the disclosure slot", () => {
  assert.equal(leadWidth(0, false), 0);
  assert.equal(leadWidth(0, true), DISCLOSURE_W);
  assert.equal(leadWidth(2, true), 2 * INDENT + DISCLOSURE_W);
  // A subtrack's icon sits one indent right of its stack's.
  assert.equal(iconLeft(leadWidth(1, true)) - iconLeft(leadWidth(0, true)), INDENT);
});

test("at the default width every button fits beside the slider, even on a subtrack", () => {
  const top = fitControls(W, leadWidth(0, false), AUDIO);
  assert.deepEqual(top.columns, AUDIO);
  assert.equal(top.more, false);
  assert.ok(top.sliderW >= 80 && top.sliderW <= SLIDER_MAX, `slider ${top.sliderW}`);
  const sub = fitControls(W, leadWidth(1, true), AUDIO);
  assert.deepEqual(sub.columns, AUDIO);
  assert.equal(sub.more, false);
  assert.ok(sub.sliderW >= SLIDER_MIN && sub.sliderW < top.sliderW, `slider ${sub.sliderW}`);
});

test("a wide column gives the slider no more than its longest", () => {
  assert.equal(fitControls(520, 0, AUDIO).sliderW, SLIDER_MAX);
});

test("short of room, buttons leave from the right for the ⋯ menu instead of shrinking", () => {
  const narrow = fitControls(219, leadWidth(1, true), AUDIO);
  assert.equal(narrow.more, true);
  assert.ok(narrow.columns.length < AUDIO.length);
  assert.deepEqual(narrow.columns, AUDIO.slice(0, narrow.columns.length), "the first columns stay");
  assert.ok(narrow.sliderW >= SLIDER_MIN);
  // The deeper the indent, the fewer buttons.
  const deeper = fitControls(219, leadWidth(3, true), AUDIO);
  assert.ok(deeper.columns.length <= narrow.columns.length);
});

test("the H button of hide mode and the automation controls take their room first", () => {
  const plain = fitControls(260, 0, AUDIO);
  const hide = fitControls(260, 0, AUDIO, 1);
  assert.ok(hide.columns.length < plain.columns.length || hide.sliderW < plain.sliderW);
  const auto = fitControls(260, leadWidth(1, true), AUDIO, 0, AUTO_SLIDER_MIN);
  assert.ok(auto.sliderW >= AUTO_SLIDER_MIN || auto.columns.length === 0);
});

test("with nothing but the slider left, it takes what there is", () => {
  const f = fitControls(150, leadWidth(4, true), AUDIO);
  assert.deepEqual(f.columns, []);
  assert.equal(f.more, true);
  assert.ok(f.sliderW >= 0);
  assert.deepEqual(fitControls(W, 0, []), { columns: [], more: false, sliderW: SLIDER_MAX });
});

test("a one-line header's slider gives way to its name, badge and buttons", () => {
  // A grouped subtrack with one switch lit (and "⋯"): room for the full headers' length at the
  // default width.
  const row = { buttons: 2, badge: true, alts: false, autoMode: false };
  assert.equal(oneLineSliderW(W, leadWidth(1, true), "compact", row, 60), 60);
  // Busier, it gets shorter, never longer than the full headers'.
  const busy = { buttons: 3, badge: true, alts: false, autoMode: false };
  const w = oneLineSliderW(W, leadWidth(1, true), "compact", busy, 60);
  assert.ok(w < 60 && w >= ONE_LINE_SLIDER_MIN, `slider ${w}`);
  // No room: no slider rather than one that runs into the buttons.
  assert.equal(oneLineSliderW(219, leadWidth(1, true), "compact", row, 53), 0);
  // Tiny headers have no icon, so there's more room.
  assert.ok(oneLineSliderW(W, 0, "tiny", busy, 96) > oneLineSliderW(W, 0, "compact", busy, 96));
});

test("a full header's contents stay in a band at its top on a tall track", () => {
  // The default height: centred, as before.
  assert.equal(headerBand(64, "full", 1, false), 64);
  // Taller tracks keep the contents where they were.
  assert.equal(headerBand(96, "full", 1, false), 64);
  assert.equal(headerBand(260, "full", 1, false), 64);
  // Two name lines and the automation line take more.
  assert.ok(headerBand(260, "full", 2, false) > 64);
  assert.ok(headerBand(260, "full", 2, true) > headerBand(260, "full", 2, false));
  // Shorter than the band: the whole header.
  assert.equal(headerBand(52, "full", 1, false), 52);
  // One-line headers fill their track.
  assert.equal(headerBand(40, "compact", 1, false), 40);
  assert.equal(headerBand(24, "tiny", 1, false), 24);
});

test("the Height slider runs on a log scale, with the default near the middle", () => {
  const [min, max] = [22, 260];
  assert.equal(heightToSlider(min, min, max), 0);
  assert.equal(heightToSlider(max, min, max), 1);
  const mid = heightToSlider(64, min, max);
  assert.ok(mid > 0.35 && mid < 0.55, `default at ${mid}`);
  for (const h of [22, 36, 52, 64, 96, 150, 260]) assert.equal(sliderToHeight(heightToSlider(h, min, max), min, max), h);
  assert.equal(sliderToHeight(-1, min, max), min);
  assert.equal(sliderToHeight(2, min, max), max);
  // Each step of the slider changes the height by the same factor.
  const a = sliderToHeight(0.2, min, max) / sliderToHeight(0.1, min, max);
  const b = sliderToHeight(0.9, min, max) / sliderToHeight(0.8, min, max);
  assert.ok(Math.abs(a - b) < 0.1);
});
