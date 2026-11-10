// Unit tests for an instrument panel's section layout (src/plugins/instruments/instLayout.ts).
import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultLayout, layoutFor, moreBeyond, parseLayouts } from "../../../src/plugins/instruments/instLayout.ts";

test("docked panels start in a row, floating ones in a grid", () => {
  assert.equal(defaultLayout("docked"), "row");
  assert.equal(defaultLayout("floating"), "grid");
});

test("each place keeps its own choice", () => {
  const saved = parseLayouts('{"docked":"grid"}');
  assert.equal(layoutFor(saved, "docked"), "grid");
  assert.equal(layoutFor(saved, "floating"), "grid");
  assert.equal(layoutFor(parseLayouts('{"floating":"row"}'), "docked"), "row");
  assert.equal(layoutFor(parseLayouts('{"floating":"row"}'), "floating"), "row");
});

test("anything malformed is dropped", () => {
  for (const bad of [null, "", "not json", "[]", "3", '{"docked":"spiral"}']) assert.deepEqual(parseLayouts(bad), {});
});

test("the fade goes on the edge the sections run on past", () => {
  const box = { scrollTop: 0, scrollLeft: 0, clientWidth: 1000, clientHeight: 300, scrollWidth: 1000, scrollHeight: 700 };
  assert.equal(moreBeyond("grid", box), "below");
  assert.equal(moreBeyond("grid", { ...box, scrollTop: 400 }), null);
  assert.equal(moreBeyond("grid", { ...box, scrollHeight: 300 }), null);
  assert.equal(moreBeyond("row", { ...box, scrollWidth: 3000, scrollHeight: 300 }), "right");
  assert.equal(moreBeyond("row", { ...box, scrollWidth: 3000, scrollLeft: 2000 }), null);
  // A row never fades at the bottom.
  assert.equal(moreBeyond("row", box), null);
});
