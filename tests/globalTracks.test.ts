import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ARRANGEMENT_H,
  clampTempoH,
  defaultGlobalTracks,
  GLOBAL_TRACKS,
  globalRowsHeight,
  parseGlobalView,
  SIGNATURE_H,
  TEMPO_DEFAULT_H,
  TEMPO_MAX_H,
  TEMPO_MIN_H,
  toggleGlobalTrack,
  usedGlobalTracks,
} from "../src/globalTracks.ts";

const tempo = (n: number) => Array.from({ length: n }, (_, i) => ({ pos: i * 1000, bpm: 120 + i, ramp: false }));

test("the global tracks a project uses", () => {
  assert.deepEqual(usedGlobalTracks({}), []);
  assert.deepEqual(usedGlobalTracks({ arrangement: [], tempo_map: [], signatures: [] }), []);
  assert.deepEqual(usedGlobalTracks({ arrangement: [{ id: 1 }] }), ["arrangement"]);
  // The project's one tempo, and a signature at bar 1 (the project's own), aren't changes.
  assert.deepEqual(usedGlobalTracks({ tempo_map: tempo(1), signatures: [{ bar: 1 }] }), []);
  assert.deepEqual(usedGlobalTracks({ tempo_map: tempo(2) }), ["tempo"]);
  assert.deepEqual(usedGlobalTracks({ signatures: [{ bar: 1 }, { bar: 9 }] }), ["signature"]);
  // In the order they're drawn, top to bottom.
  assert.deepEqual(usedGlobalTracks({ tempo_map: tempo(3), signatures: [{ bar: 5 }], arrangement: [{}] }), ["arrangement", "signature", "tempo"]);
});

test("with no choice saved, a project shows what it uses, or everything when it uses nothing", () => {
  assert.deepEqual(defaultGlobalTracks({ arrangement: [{}] }), ["arrangement"]);
  assert.deepEqual(defaultGlobalTracks({ arrangement: [{}], tempo_map: tempo(2) }), ["arrangement", "tempo"]);
  assert.deepEqual(defaultGlobalTracks({}), [...GLOBAL_TRACKS]);
});

test("the rows they take", () => {
  assert.equal(globalRowsHeight(["arrangement"], 40), ARRANGEMENT_H);
  assert.equal(globalRowsHeight(["signature", "tempo"], 40), SIGNATURE_H + 40);
  assert.equal(globalRowsHeight([...GLOBAL_TRACKS], TEMPO_DEFAULT_H), ARRANGEMENT_H + SIGNATURE_H + TEMPO_DEFAULT_H);
  assert.equal(globalRowsHeight([], 40), 0);
  // The point of it: a project using only the Arrangement track gives the rest back.
  assert.ok(globalRowsHeight(defaultGlobalTracks({ arrangement: [{}] }), TEMPO_DEFAULT_H) <= 24);
  assert.ok(TEMPO_DEFAULT_H >= TEMPO_MIN_H && TEMPO_DEFAULT_H < 56, "a shorter Tempo lane than before");
});

test("a track's menu item hides it when in view, else shows it and the global tracks", () => {
  const all = [...GLOBAL_TRACKS];
  assert.deepEqual(toggleGlobalTrack({ on: true, shown: all }, "signature"), { on: true, shown: ["arrangement", "tempo"] });
  // Shown in its place in the order, whatever order it was chosen in.
  assert.deepEqual(toggleGlobalTrack({ on: true, shown: ["tempo"] }, "arrangement"), { on: true, shown: ["arrangement", "tempo"] });
  // With the global tracks hidden, nothing is in view: the item shows them, with that track.
  assert.deepEqual(toggleGlobalTrack({ on: false, shown: ["arrangement"] }, "tempo"), { on: true, shown: ["arrangement", "tempo"] });
  assert.deepEqual(toggleGlobalTrack({ on: false, shown: ["arrangement"] }, "arrangement"), { on: true, shown: ["arrangement"] });
  // Hiding the last one hides the global tracks and keeps it, so G brings it back.
  assert.deepEqual(toggleGlobalTrack({ on: true, shown: ["tempo"] }, "tempo"), { on: false, shown: ["tempo"] });
});

test("a saved choice is read back, and anything malformed falls back to the defaults", () => {
  assert.deepEqual(parseGlobalView(JSON.stringify({ shown: ["tempo", "arrangement"], tempoH: 72 })), { shown: ["arrangement", "tempo"], tempoH: 72 });
  assert.deepEqual(parseGlobalView(JSON.stringify({ tempoH: 72 })), { shown: null, tempoH: 72 });
  assert.deepEqual(parseGlobalView(JSON.stringify({ shown: ["tempo"] })), { shown: ["tempo"], tempoH: null });
  assert.deepEqual(parseGlobalView(null), { shown: null, tempoH: null });
  assert.deepEqual(parseGlobalView("{not json"), { shown: null, tempoH: null });
  assert.deepEqual(parseGlobalView("[1,2]"), { shown: null, tempoH: null });
  assert.deepEqual(parseGlobalView(JSON.stringify({ shown: ["video", 3], tempoH: "tall" })), { shown: null, tempoH: null }, "unknown names leave nothing chosen");
  assert.deepEqual(parseGlobalView(JSON.stringify({ shown: [], tempoH: 9999 })), { shown: null, tempoH: TEMPO_MAX_H });
  assert.equal(clampTempoH(1), TEMPO_MIN_H);
  assert.equal(clampTempoH(50.4), 50);
});
