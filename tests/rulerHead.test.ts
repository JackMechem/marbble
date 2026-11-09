// Unit tests for src/rulerHead.ts: where a press grabs the playhead's head in the ruler.
import { test } from "node:test";
import assert from "node:assert/strict";
import { BARS_H, CYCLE_H, HEAD_H, HEAD_HIT_HALF_W, HEAD_W, onPlayheadHead, playheadStyle, playheadWidth, playheadX } from "../src/rulerHead.ts";

test("a press anywhere on the drawn head grabs it", () => {
  const half = (HEAD_W - 1) / 2;
  for (const dx of [-half, 0, half]) for (const y of [CYCLE_H, CYCLE_H + HEAD_H - 1]) assert.ok(onPlayheadHead(dx, y), `dx ${dx}, y ${y}`);
});

test("the head's hit area is clearly larger than what's drawn", () => {
  assert.ok(HEAD_HIT_HALF_W * 2 + 1 >= HEAD_W + 4);
  assert.ok(onPlayheadHead(HEAD_W / 2 + 2, CYCLE_H + 2));
  assert.ok(onPlayheadHead(-(HEAD_W / 2 + 2), CYCLE_H + 2));
  // Just above the head, in the cycle strip's lower edge, and below it down to the bar numbers' foot.
  assert.ok(onPlayheadHead(0, CYCLE_H - 2));
  assert.ok(onPlayheadHead(0, BARS_H - 1));
});

test("presses away from the head go to the strips as before", () => {
  assert.equal(onPlayheadHead(HEAD_HIT_HALF_W + 1, CYCLE_H + 2), false);
  assert.equal(onPlayheadHead(0, 2), false, "the cycle strip's top");
  assert.equal(onPlayheadHead(0, BARS_H), false, "the Autopunch strip");
  assert.equal(onPlayheadHead(0, BARS_H + 10), false, "the marker lane");
});

test("the playhead's line sits on whole device pixels, at every interface size", () => {
  for (const dpr of [1, 1.1, 1.25, 1.5, 2, 2.5]) {
    for (const x of [0, 0.3, 0.5, 117.37, 803.9]) {
      const at = playheadX(x, dpr) * dpr;
      // Within a thousandth of a device pixel: the style rounds to 4 decimals.
      assert.ok(Math.abs(at - Math.round(at)) < 1e-3, `x ${x} at ${dpr}`);
      assert.ok(Math.abs(playheadX(x, dpr) - x) <= 0.5 / dpr + 1e-3, "the nearest device pixel");
    }
    const w = playheadWidth(dpr) * dpr;
    assert.ok(Math.abs(w - Math.round(w)) < 1e-9 && w >= 1, `a whole number of device pixels at ${dpr}`);
  }
  assert.equal(playheadWidth(1), 1);
  assert.equal(playheadWidth(1.25), 0.8, "one device pixel, not 1.25 spread over two");
  assert.equal(playheadWidth(2), 1);
  // An element whose left edge is off the device grid (a track list 297 px wide at 125 %) still
  // gets its line onto a whole device pixel of the screen.
  for (const origin of [297, 296.4]) {
    const on = (playheadX(117.37, 1.25, origin) + origin) * 1.25;
    assert.ok(Math.abs(on - Math.round(on)) < 1e-3, `origin ${origin}`);
  }
  // Every part of the line gets the same column from the same position.
  assert.deepEqual(playheadStyle(117.37, 1.25), { transform: "translate3d(117.6px,0,0)", width: "0.8px" });
  assert.deepEqual(playheadStyle(117.37, 1), { transform: "translate3d(117px,0,0)", width: "1px" });
});
