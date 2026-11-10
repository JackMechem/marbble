// Unit tests for src/ui/barFit.ts: a bar of controls giving way to fit one row, and the bars that
// use it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fitOf, nextFit, rowNeed } from "../../src/ui/barFit.ts";

test("a row that overflows takes one more step, up to the last", () => {
  assert.equal(nextFit(0, 900, 800, [], 5), 1);
  assert.equal(nextFit(3, 900, 800, [1000, 950, 920], 5), 4);
  assert.equal(nextFit(5, 900, 800, [], 5), 5, "past the last step there's nothing more to give");
  // Half a pixel is rounding, not overflow.
  assert.equal(nextFit(0, 800.4, 800, [], 5), 0);
});

test("a step is taken back once the room is there for the row without it", () => {
  // With 2 steps the row needed 950; at 940 it stays, at 950 it goes back.
  assert.equal(nextFit(3, 800, 940, [1000, 990, 950], 5), 3);
  assert.equal(nextFit(3, 800, 950, [1000, 990, 950], 5), 2);
  assert.equal(nextFit(0, 600, 800, [], 5), 0);
});

test("a step whose need is unknown (the content changed) is tried", () => {
  assert.equal(nextFit(2, 700, 800, [1000, undefined], 5), 1);
  // Tried and too wide: its need is now known and the bar goes back up and stays.
  assert.equal(nextFit(1, 990, 800, [1000, 990], 5), 2);
  assert.equal(nextFit(2, 700, 800, [1000, 990], 5), 2);
});

test("a bar that isn't laid out keeps its steps", () => {
  assert.equal(nextFit(2, 300, 0, [1000, 990], 5), 2);
  assert.equal(nextFit(0, 300, 0, [], 5), 0);
});

test("stepping settles: from any start, a fixed row and room end on the fewest steps that fit", () => {
  // What the row needs with each number of steps taken.
  const widths = [1340, 1140, 1050, 990, 930, 870, 700, 520];
  for (const room of [300, 520, 600, 869, 870, 1000, 1400]) {
    for (const start of [0, 3, widths.length - 1]) {
      let fit = start;
      const needs: (number | undefined)[] = [];
      for (let i = 0; i < 40; i++) {
        const need = widths[fit];
        if (need > room + 0.5) needs[fit] = need;
        const next = nextFit(fit, need, room, needs, widths.length - 1);
        if (next === fit) break;
        fit = next;
      }
      const want = widths.findIndex((w) => w <= room);
      assert.equal(fit, want < 0 ? widths.length - 1 : want, `room ${room} from ${start}`);
    }
  }
});

test("the steps in effect and their class names", () => {
  const steps = ["captions", "tight", "zoom"] as const;
  const none = fitOf(steps, 0);
  assert.equal(none.className, "");
  assert.equal(none.has("captions"), false);
  const two = fitOf(steps, 2);
  assert.equal(two.className, " fit-captions fit-tight");
  assert.equal(two.has("tight"), true);
  assert.equal(two.has("zoom"), false);
  assert.equal(fitOf(steps, 9).has("zoom"), true);
});

test("a row needs up to its rightmost box, its margin and the bar's end padding", () => {
  assert.equal(rowNeed(100, [], 8), 8);
  assert.equal(rowNeed(100, [{ right: 300, marginRight: 0 }, { right: 650, marginRight: 4 }, { right: 500, marginRight: 0 }], 8), 562);
  // A negative margin pulls the next box in; it doesn't shorten the row.
  assert.equal(rowNeed(0, [{ right: 50, marginRight: -4 }], 8), 58);
});

test("every bar that keeps to one row lists its steps and has the fit class", () => {
  for (const f of ["tracks/TracksArea.tsx", "editors/pianoRoll/PianoRollBar.tsx", "editors/eventList/EventList.tsx", "editors/stepSequencer/StepSequencer.tsx", "editors/audioEditor/AudioEditor.tsx", "mixer/Mixer.tsx"]) {
    const src = readFileSync(new URL(`../../src/${f}`, import.meta.url), "utf8");
    assert.match(src, /useBarFit\(/, `${f} measures its bar`);
    assert.match(src, /fit-bar/, `${f} marks its bar`);
  }
});
