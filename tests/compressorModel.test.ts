// Unit tests for src/compressorModel.ts: the steady-state curve the transfer graph draws, the copy
// of `static_reduction` in crates/engine/src/fx/compressor.rs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { Circuit, CIRCUITS, FET_CURVES, P, staticReduction } from "../src/compressorModel.ts";

/** The parameters as the panel reads them, with the threshold at -20 dB and a ratio of 4:1. */
const params = (over: Record<number, number>) => (id: number) => ({ [P.circuit]: Circuit.Platinum, [P.threshold]: -20, [P.ratio]: 4, [P.knee]: 0, [P.fetRatio]: 0, [P.optoMode]: 0, ...over })[id] ?? 0;
const near = (a: number, b: number, e = 1e-6) => assert.ok(Math.abs(a - b) < e, `${a} vs ${b}`);

test("each circuit has its name, by its index", () => {
  assert.equal(CIRCUITS[Circuit.Platinum].key, "platinum");
  assert.equal(CIRCUITS[Circuit.Fet].key, "fet");
  assert.equal(CIRCUITS.length, 5);
});

test("hard knee: nothing below the threshold, the ratio above it", () => {
  const v = params({});
  assert.equal(staticReduction(v, -30), 0);
  assert.equal(staticReduction(v, -20), 0);
  near(staticReduction(v, -10), 7.5);
  // A 10 dB knee around the threshold.
  const soft = params({ [P.knee]: 10 });
  assert.equal(staticReduction(soft, -26), 0);
  near(staticReduction(soft, -20), (0.75 * 25) / 20);
  near(staticReduction(soft, -14), 0.75 * 6);
});

test("ratio is floored at 1:1", () => {
  assert.equal(staticReduction(params({ [P.ratio]: 0.5 }), 0), 0);
});

test("Studio VCA shares Platinum's curve", () => {
  near(staticReduction(params({ [P.circuit]: Circuit.Vca }), -10), staticReduction(params({}), -10));
});

test("every circuit reduces more the louder the input", () => {
  for (const circuit of [Circuit.Platinum, Circuit.Vca, Circuit.Fet, Circuit.Opto, Circuit.VariMu]) {
    let last = -1;
    for (let x = -60; x <= 0; x += 5) {
      const r = staticReduction(params({ [P.circuit]: circuit }), x);
      assert.ok(r >= last - 1e-6, `circuit ${circuit} at ${x}`);
      assert.ok(r >= 0);
      last = r;
    }
  }
});

test("the FET's feedback settles: output above threshold rises at 1/(S+1)", () => {
  const [s] = FET_CURVES[0];
  const v = params({ [P.circuit]: Circuit.Fet, [P.fetRatio]: 0 });
  const y = (x: number) => x - staticReduction(v, x);
  // Well past the knee, 1 dB in gives 1/(S+1) dB out.
  near(y(0) - y(-1), 1 / (s + 1), 1e-3);
});

test("Opto's Limit mode squeezes harder than Compress", () => {
  const f = (mode: number) => staticReduction(params({ [P.circuit]: Circuit.Opto, [P.optoMode]: mode }), 0);
  assert.ok(f(1) > f(0));
});
