// Unit tests for src/dynScale.ts: how the dynamics panels' values read and sit on a knob.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { FxParamSpec } from "../src/fx.ts";
import { arc, formatDyn, formatMs, fromNorm, polar, toNorm } from "../src/dynScale.ts";

const spec = (over: object) => ({ unit: "", kind: { type: "float" }, ...over }) as unknown as FxParamSpec;

test("formatMs changes unit as times grow", () => {
  assert.equal(formatMs(1000), "1.00 s");
  assert.equal(formatMs(12000), "12.0 s");
  assert.match(formatMs(250), /^250 ms$/);
});

test("formatDyn reads in the parameter's unit", () => {
  assert.equal(formatDyn(undefined, 3), "3");
  assert.equal(formatDyn(spec({ unit: "db" }), 2), "+2.0 dB");
  assert.equal(formatDyn(spec({ unit: "db" }), -2), "-2.0 dB");
  assert.equal(formatDyn(spec({ kind: { type: "toggle" } }), 1), "On");
  assert.equal(formatDyn(spec({ kind: { type: "choice", labels: ["A", "B"] } }), 1), "B");
});

test("toNorm and fromNorm undo each other, linearly and logarithmically", () => {
  assert.equal(toNorm(5, 0, 10, false), 0.5);
  assert.equal(fromNorm(0.5, 0, 10, false), 5);
  assert.ok(Math.abs(toNorm(100, 10, 1000, true) - 0.5) < 1e-9);
  assert.ok(Math.abs(fromNorm(0.5, 10, 1000, true) - 100) < 1e-9);
});

test("toNorm stays on the knob", () => {
  assert.equal(toNorm(-5, 0, 10, false), 0);
  assert.equal(toNorm(50, 0, 10, false), 1);
  assert.equal(toNorm(5, 10, 10, false), 0);
});

test("polar measures clockwise from the top", () => {
  const [x, y] = polar(10, 5, 0);
  assert.ok(Math.abs(x - 10) < 1e-9 && Math.abs(y - 5) < 1e-9);
  const [rx, ry] = polar(10, 5, 90);
  assert.ok(Math.abs(rx - 15) < 1e-9 && Math.abs(ry - 10) < 1e-9);
});

test("arc takes the large way round past half a turn", () => {
  assert.match(arc(10, 5, -135, 135), /A5 5 0 1 1 /);
  assert.match(arc(10, 5, -45, 45), /A5 5 0 0 1 /);
});
