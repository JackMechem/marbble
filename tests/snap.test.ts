// Unit tests for the snap grid (src/time.ts snapStep and snapPos): Bar, Beat, Division, Ticks and
// Smart, in quarter notes and in samples. Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { DIVISIONS_PER_BEAT, snapPos, snapStep, TICKS_PER_DIVISION } from "../src/time.ts";
import { SNAP_LABELS } from "../src/rulerMenu.ts";
import type { Project } from "../src/api.ts";

test("each snap mode's step, following the signature", () => {
  // 4/4: a bar is 4 quarters, a beat 1, a division a sixteenth, a tick 1/960 of a beat.
  assert.equal(snapStep("bar", 4, 4, 30), 4);
  assert.equal(snapStep("beat", 4, 4, 30), 1);
  assert.equal(snapStep("division", 4, 4, 30), 0.25);
  assert.equal(snapStep("ticks", 4, 4, 30), 1 / (DIVISIONS_PER_BEAT * TICKS_PER_DIVISION));
  assert.equal(snapStep("off", 4, 4, 30), 0);
  // 6/8: the beat is an eighth, so a division is a thirty-second.
  assert.equal(snapStep("bar", 6, 8, 30), 3);
  assert.equal(snapStep("beat", 6, 8, 30), 0.5);
  assert.equal(snapStep("division", 6, 8, 30), 0.125);
  // Smart follows the zoom: divisions zoomed in, beats, then bars zoomed out.
  assert.equal(snapStep("smart", 4, 4, 100), 0.25);
  assert.equal(snapStep("smart", 4, 4, 30), 1);
  assert.equal(snapStep("smart", 4, 4, 5), 4);
});

test("positions snap to divisions and ticks", () => {
  // 120 BPM at 48 kHz: a beat is 24000 samples, a division 6000, a tick 25.
  const p = { sample_rate: 48000, tempo: 120, sig_num: 4, sig_den: 4, tempo_map: [], signatures: [] } as unknown as Project;
  assert.equal(snapPos(p, 8000, "division", 30), 6000);
  assert.equal(snapPos(p, 10000, "division", 30), 12000);
  assert.equal(snapPos(p, 8012, "ticks", 30), 8000);
  assert.equal(snapPos(p, 8013, "ticks", 30), 8025);
  assert.equal(snapPos(p, 8013.4, "off", 30), 8013);
});

test("every snap mode has a label in the menus", () => {
  assert.deepEqual(Object.keys(SNAP_LABELS), ["smart", "bar", "beat", "division", "ticks", "off"]);
});
