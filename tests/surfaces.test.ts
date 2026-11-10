// Unit tests for src/surfaces.ts, Settings › MIDI › Control Surfaces.
import { test } from "node:test";
import assert from "node:assert/strict";
import { guessPort, hasFunctionKeys, kindName, newSurface, nextOffset, portOptions, setFunction, statusText, stripsOf, type SurfaceConfig } from "../src/surfaces.ts";

const unit = (patch: Partial<SurfaceConfig>): SurfaceConfig => ({ kind: "mcu", input: "", output: "", offset: 0, functions: [], ...patch });

test("units sit side by side", () => {
  assert.equal(stripsOf([]), 8);
  assert.equal(nextOffset([]), 0);
  assert.equal(nextOffset([unit({})]), 8);
  assert.equal(nextOffset([unit({}), unit({ kind: "mcu_extender", offset: 8 })]), 16);
  // An extender on the left: the next one goes right of the rightmost.
  assert.equal(nextOffset([unit({ kind: "mcu_extender" }), unit({ offset: 8 })]), 16);
});

test("a new unit is a Mackie Control, then extenders, with ports guessed from their names", () => {
  const inputs = ["Keystation 49:Keystation 49 MIDI 1", "X-Touch:X-Touch MIDI 1", "X-Touch-Ext:X-Touch-Ext MIDI 1"];
  const first = newSurface([], inputs, inputs);
  assert.deepEqual(first, { kind: "mcu", input: "X-Touch:X-Touch MIDI 1", output: "X-Touch:X-Touch MIDI 1", offset: 0, functions: [] });
  const second = newSurface([first], inputs, inputs);
  assert.equal(second.kind, "mcu_extender");
  assert.equal(second.input, "X-Touch-Ext:X-Touch-Ext MIDI 1");
  assert.equal(second.offset, 8);
  // Nothing that looks like one: no port.
  assert.equal(newSurface([], ["Keystation 49"], []).input, "");
  // A port another unit has isn't offered again.
  assert.equal(guessPort("mcu", ["MCU Pro"], ["MCU Pro"]), "");
  assert.equal(guessPort("hui", ["HUI Port 1", "MCU"], []), "HUI Port 1");
  assert.equal(newSurface([unit({ kind: "hui" })], [], []).kind, "hui");
});

test("port pop-ups keep a chosen port that's gone", () => {
  const ports = [{ key: "A" }, { key: "B" }, { key: "A" }];
  assert.deepEqual(portOptions(ports, "B"), [
    ["", "None"],
    ["A", "A"],
    ["B", "B"],
  ]);
  assert.deepEqual(portOptions(ports, "Gone")[3], ["Gone", "Gone (not found)"]);
});

test("the status line says what's missing", () => {
  const u = unit({ input: "X", output: "X" });
  const s = { input: true, output: true, online: true, showing: "Tracks 1–8" };
  assert.equal(statusText(s, u), "Online · Tracks 1–8");
  assert.equal(statusText({ ...s, online: false }, u), "Waiting for the surface to answer");
  assert.equal(statusText({ ...s, online: false }, { ...u, kind: "hui" }), "Waiting for the HUI to answer");
  assert.equal(statusText({ ...s, input: false }, u), "Input not connected");
  assert.equal(statusText({ ...s, output: false }, u), "Output not connected");
  assert.equal(statusText({ ...s, input: false, output: false }, u), "Not connected");
  assert.equal(statusText(s, unit({ input: "X" })), "Choose its input and output");
});

test("function keys", () => {
  assert.deepEqual(setFunction([], 2, "save"), ["", "", "save"]);
  assert.deepEqual(setFunction(["undo", "", "save"], 2, ""), ["undo"]);
  assert.deepEqual(setFunction(["undo"], 0, "redo"), ["redo"]);
  assert.equal(hasFunctionKeys("mcu"), true);
  assert.equal(hasFunctionKeys("hui"), true);
  assert.equal(hasFunctionKeys("mcu_extender"), false);
  assert.equal(kindName("mcu_extender"), "Mackie Control Extender");
});
