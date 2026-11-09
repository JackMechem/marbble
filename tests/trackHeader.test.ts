import { test } from "node:test";
import assert from "node:assert/strict";
import { AUTO_LINE_H, COMPACT_TIER_H, FULL_TIER_H, headerTier, levelSliderSize, meterScale, meterTravel, nameLines, SLIDER_METER_FLOOR, thumbCenter, TWO_LINE_H } from "../src/trackHeader.ts";
import { MIN_BODY_H, regionBar, fadeZone } from "../src/regionBar.ts";
import { dragsTrack, HEADER_CONTROLS, REORDER_SLOP, reorderIndex } from "../src/trackHeader.ts";

// The fader taper from time.ts (which can't be loaded here: it imports the tempo module).
function dbToFader(db: number): number {
  if (db <= -96) return 0;
  if (db >= 6) return 1;
  if (db >= -12) return 0.62 + ((db + 12) / 18) * 0.38;
  if (db >= -40) return 0.22 + ((db + 40) / 28) * 0.4;
  return ((db + 96) / 56) * 0.22;
}

// The smallest track height (layout.ts imports the store, so it's restated here).
const MIN_TRACK_H = 22;

test("header tiers by height", () => {
  assert.equal(headerTier(260), "full");
  assert.equal(headerTier(FULL_TIER_H), "full");
  assert.equal(headerTier(FULL_TIER_H - 1), "compact");
  assert.equal(headerTier(COMPACT_TIER_H), "compact");
  assert.equal(headerTier(COMPACT_TIER_H - 1), "tiny");
  assert.equal(headerTier(MIN_TRACK_H), "tiny");
  assert.equal(headerTier(64), "full", "the default height keeps the full layout");
});

test("a name takes two lines only on a full header with room for them", () => {
  assert.equal(nameLines(64, false), 1, "the default height keeps one line");
  assert.equal(nameLines(TWO_LINE_H - 1, false), 1);
  assert.equal(nameLines(TWO_LINE_H, false), 2);
  assert.equal(nameLines(260, false), 2);
  assert.equal(nameLines(COMPACT_TIER_H, false), 1, "one-line headers keep one line");
  assert.equal(headerTier(TWO_LINE_H), "full");
  // The automation controls' own line needs room as well.
  assert.equal(nameLines(TWO_LINE_H, true), 1);
  assert.equal(nameLines(TWO_LINE_H + AUTO_LINE_H - 1, true), 1);
  assert.equal(nameLines(TWO_LINE_H + AUTO_LINE_H, true), 2);
  // Two 16 px lines, the 6 px gap and the 21 px button row, with a few px above and below.
  assert.ok(TWO_LINE_H >= 2 * 16 + 6 + 21 + 8);
});

/** A stand-in element: `closest` finds the selector parts listed as its own or its ancestors'. */
const el = (...matches: string[]) => ({
  closest: (sel: string) => (sel.split(",").some((s) => matches.includes(s.trim())) ? {} : null),
});

test("a press on the name, the number or empty space drags the track; the controls keep theirs", () => {
  assert.equal(dragsTrack(el()), true, "the name, the number column, a gap");
  for (const part of ["button", "input", "select", "[role='slider']", ".th-auto", ".th-resize"]) {
    assert.ok(HEADER_CONTROLS.includes(part), part);
    assert.equal(dragsTrack(el(part)), false, part);
  }
  assert.equal(dragsTrack(null), false);
  assert.equal(dragsTrack({}), false, "not an element");
});

test("a dragged track goes before the first track whose middle is below the pointer", () => {
  const tops = [0, 64, 128, 128, 192];
  const heights = [64, 64, 0, 64, 64];
  assert.equal(reorderIndex(tops, heights, 10), 0);
  assert.equal(reorderIndex(tops, heights, 33), 1);
  assert.equal(reorderIndex(tops, heights, 100), 2, "above the second's middle");
  // A collapsed stack's hidden subtrack (height 0) sits at the next track's top.
  assert.equal(reorderIndex(tops, heights, 140), 3);
  assert.equal(reorderIndex(tops, heights, 170), 4);
  assert.equal(reorderIndex(tops, heights, 1000), 5, "past the last: at the end");
  assert.ok(REORDER_SLOP > 0 && REORDER_SLOP < 8, "a click with a little hand movement stays a click");
});

test("the slider is a slim line with a round thumb, easy to grab, and fits the smallest row", () => {
  for (const tier of ["full", "compact", "tiny"] as const) {
    const { groove, thumb, height } = levelSliderSize(tier);
    assert.ok(groove >= 3 && groove <= 4, "a slim line, still wide enough to show the meter");
    assert.ok(thumb > 2 * groove, "the thumb stands out from the line");
    assert.ok(height >= thumb && height >= 14, "presses land on more than the line");
  }
  // A row's content is its height less the 1 px bottom border; leave a pixel above and below.
  assert.ok(levelSliderSize("tiny").height <= MIN_TRACK_H - 1 - 2);
  assert.ok(levelSliderSize("compact").height <= COMPACT_TIER_H - 1 - 2);
});

test("the thumb stays inside the groove", () => {
  assert.equal(thumbCenter(0, 88, 8), 4);
  assert.equal(thumbCenter(1, 88, 8), 84);
  assert.equal(thumbCenter(0.5, 88, 8), 44);
  assert.equal(thumbCenter(-1, 88, 8), 4);
  assert.equal(thumbCenter(2, 88, 8), 84);
});

test("meter travel: silence draws nothing, and above -40 dB it follows the fader taper", () => {
  assert.equal(meterTravel(-Infinity, dbToFader), 0);
  assert.equal(meterTravel(-96, dbToFader), 0);
  assert.equal(meterTravel(SLIDER_METER_FLOOR, dbToFader), 0);
  for (const db of [-40, -18, -6, 0, 6]) assert.equal(meterTravel(db, dbToFader), dbToFader(db));
  // Continuous where the taper takes over, and rising all the way.
  assert.ok(Math.abs(meterTravel(-40.001, dbToFader) - dbToFader(-40)) < 1e-4);
  let last = -1;
  for (let db = -70; db <= 10; db += 0.5) {
    const t = meterTravel(db, dbToFader);
    assert.ok(t >= last, `rises at ${db} dB`);
    assert.ok(t >= 0 && t <= 1);
    last = t;
  }
});

test("a level reaches the thumb set to the same dB", () => {
  const width = 88;
  const thumbW = 8;
  const scale = meterScale(width, thumbW, dbToFader);
  for (const db of [-30, -12, -3, 0, 6]) {
    const meterPx = scale(db) * width;
    assert.ok(Math.abs(meterPx - thumbCenter(dbToFader(db), width, thumbW)) < 1e-9, `${db} dB`);
  }
  assert.equal(scale(-80), 0);
  assert.equal(scale(12), scale(6), "stops where the thumb's travel does (+6 dB)");
  assert.equal(meterScale(0, 8, dbToFader)(0), 0);
});

test("region name bar: unchanged on tall regions, slim on short ones, never past the region", () => {
  assert.deepEqual(regionBar(61), { bar: 14, font: "sm", bodyTop: 16, bodyH: 43 });
  for (let h = MIN_TRACK_H - 3; h <= 300; h++) {
    const g = regionBar(h);
    assert.ok(g.bodyTop >= g.bar);
    assert.ok(g.bodyTop + g.bodyH <= h, `body inside a ${h} px region`);
  }
  // The smallest track still shows a strip of waveform or notes.
  assert.ok(regionBar(MIN_TRACK_H - 3).bodyH >= MIN_BODY_H);
});

test("fade handles take at most the top half of a short body", () => {
  assert.equal(fadeZone(43, 14), 14);
  assert.equal(fadeZone(5, 14), 3);
  assert.equal(fadeZone(0, 14), 0);
});
