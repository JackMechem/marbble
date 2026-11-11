// Native drop positions: logical on WebKit (macOS, Linux), physical on WebView2 (Windows).
import { test } from "node:test";
import assert from "node:assert/strict";
import { dropToCss, dropUnits } from "../../src/project/dropPosition.ts";

const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const WINDOWS =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";

test("only WebView2 reports physical pixels", () => {
  assert.equal(dropUnits(MAC), "logical");
  assert.equal(dropUnits(LINUX), "logical");
  assert.equal(dropUnits(WINDOWS), "physical");
});

test("a Retina Mac's points are CSS pixels at 100%, whatever the pixel ratio", () => {
  assert.deepEqual(dropToCss({ x: 400, y: 300 }, dropUnits(MAC), 1, 2), [400, 300]);
  assert.deepEqual(dropToCss({ x: 400, y: 300 }, dropUnits(MAC), 1, 1), [400, 300]);
});

test("logical positions divide by the interface size", () => {
  assert.deepEqual(dropToCss({ x: 400, y: 300 }, dropUnits(MAC), 2, 2), [200, 150]);
  assert.deepEqual(dropToCss({ x: 500, y: 250 }, dropUnits(LINUX), 1.25, 1), [400, 200]);
});

test("physical positions divide by the pixel ratio, which includes the zoom", () => {
  // 150% display at interface size 100%, then at 200% (WebView2 reports a ratio of 3).
  assert.deepEqual(dropToCss({ x: 600, y: 300 }, dropUnits(WINDOWS), 1, 1.5), [400, 200]);
  assert.deepEqual(dropToCss({ x: 600, y: 300 }, dropUnits(WINDOWS), 2, 3), [200, 100]);
});

test("a missing factor leaves the position as it is", () => {
  assert.deepEqual(dropToCss({ x: 10, y: 20 }, "physical", 1, 0), [10, 20]);
});
