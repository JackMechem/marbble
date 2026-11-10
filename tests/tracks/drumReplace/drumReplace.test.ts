// Unit tests for src/tracks/drumReplace/drumReplace.ts. Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as d from "../../../src/tracks/drumReplace/drumReplace.ts";

const hits: d.DrumHit[] = [
  { pos: 1000, peak: 1200, level: 0.8 },
  { pos: 9000, peak: 9300, level: 0.2 },
  { pos: 20000, peak: 20100, level: 0.01 },
];

test("the threshold keeps the hits at or above it, as the engine's kept does", () => {
  assert.equal(d.keptHits(hits, -30).length, 2);
  assert.equal(d.keptHits(hits, -60).length, 3);
  assert.equal(d.keptHits(hits, 0).length, 0);
  // Exactly at the threshold counts.
  assert.equal(d.keptHits([{ pos: 0, peak: 0, level: 0.1 }], -20).length, 1);
});

test("the summary says how many and how loud", () => {
  assert.equal(d.hitSummary(hits, d.keptHits(hits, -30)), "2 hits, the loudest at -1.9 dB; 1 quieter left out");
  assert.equal(d.hitSummary(hits, d.keptHits(hits, 0)), "No hits reach the threshold (3 below it)");
  assert.equal(d.hitSummary([], []), "No hits found on the track");
  assert.equal(d.hitSummary(hits.slice(0, 1), hits.slice(0, 1)), "1 hit, the loudest at -1.9 dB");
});

test("markers go where the notes will: attack or peak, plus the offset", () => {
  const s = { ...d.DRUM_DEFAULTS };
  assert.equal(d.notePos(hits[0], s, 48000), 1000);
  assert.equal(d.notePos(hits[0], { ...s, timing: "peak" }, 48000), 1200);
  assert.equal(d.notePos(hits[0], { ...s, offset_ms: -2 }, 48000), 904);
});

test("the instruments and their notes match the engine's", () => {
  const rs = readFileSync(fileURLToPath(new URL("../../../crates/engine/src/edit/drum_replace.rs", import.meta.url)), "utf8");
  for (const k of d.DRUM_KINDS) {
    const variant = k.id.replace(/(^|_)([a-z])/g, (_, __, c: string) => c.toUpperCase());
    assert.match(rs, new RegExp(`DrumKind::${variant} => ${k.note},`), k.id);
    assert.match(rs, new RegExp(`DrumKind::${variant} => "${k.name}",`), k.id);
  }
});

test("the markers' store tells its listeners", () => {
  let n = 0;
  const off = d.onDrumMarks(() => n++);
  d.setDrumMarks({ track: 3, at: [10, 20] });
  assert.deepEqual(d.drumMarks(), { track: 3, at: [10, 20] });
  d.setDrumMarks(null);
  off();
  d.setDrumMarks(null);
  assert.equal(n, 2);
});
