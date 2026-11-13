// Unit tests for src/project/alternativesModel.ts (backup labels, names, track alternatives). Run with
// `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { activeTrackAlt, backupLabel, backupLabels, freeName, nameProblem, showsTrackAlts, trackAltNames } from "../../src/project/alternativesModel.ts";

const at = (y: number, mo: number, d: number, h: number, mi: number, s = 0, ms = 0) => new Date(y, mo - 1, d, h, mi, s, ms).getTime();

test("backup labels say today, yesterday or the date", () => {
  const now = at(2026, 10, 29, 16, 5);
  assert.equal(backupLabel(at(2026, 10, 29, 14, 32), now), "Today 14:32");
  assert.equal(backupLabel(at(2026, 10, 29, 0, 0), now), "Today 00:00");
  assert.equal(backupLabel(at(2026, 10, 28, 9, 10), now), "Yesterday 09:10");
  assert.equal(backupLabel(at(2026, 10, 28, 23, 59), at(2026, 10, 29, 0, 1)), "Yesterday 23:59");
  assert.equal(backupLabel(at(2026, 10, 27, 14, 32), now), "27 Oct 2026 14:32");
  assert.equal(backupLabel(at(2025, 1, 3, 8, 0), now), "3 Jan 2025 08:00");
});

test("backups in different minutes keep the plain labels", () => {
  const now = at(2026, 10, 29, 17, 0);
  const times = [at(2026, 10, 29, 16, 52, 40), at(2026, 10, 29, 16, 51, 10), at(2026, 10, 28, 9, 10, 5), at(2026, 10, 27, 14, 32, 59)];
  assert.deepEqual(backupLabels(times, now), ["Today 16:52", "Today 16:51", "Yesterday 09:10", "27 Oct 2026 14:32"]);
  assert.deepEqual(backupLabels([], now), []);
});

test("backups in the same minute get the seconds, and only they do", () => {
  const now = at(2026, 10, 29, 17, 0);
  const two = [at(2026, 10, 29, 16, 52, 41), at(2026, 10, 29, 16, 52, 7), at(2026, 10, 29, 16, 40, 3)];
  assert.deepEqual(backupLabels(two, now), ["Today 16:52:41", "Today 16:52:07", "Today 16:40"]);
  const three = [at(2026, 10, 29, 16, 52, 59), at(2026, 10, 29, 16, 52, 30), at(2026, 10, 29, 16, 52, 0)];
  assert.deepEqual(backupLabels(three, now), ["Today 16:52:59", "Today 16:52:30", "Today 16:52:00"]);
});

test("same-minute backups on other days get the seconds too", () => {
  const now = at(2026, 10, 29, 17, 0);
  const times = [at(2026, 10, 28, 9, 10, 50), at(2026, 10, 28, 9, 10, 5), at(2026, 10, 27, 14, 32, 30), at(2026, 10, 27, 14, 32, 1), at(2026, 10, 26, 14, 32, 1)];
  assert.deepEqual(backupLabels(times, now), ["Yesterday 09:10:50", "Yesterday 09:10:05", "27 Oct 2026 14:32:30", "27 Oct 2026 14:32:01", "26 Oct 2026 14:32"]);
});

test("backups in the same second are numbered, the newer higher", () => {
  const now = at(2026, 10, 29, 17, 0);
  const times = [at(2026, 10, 29, 16, 52, 7, 900), at(2026, 10, 29, 16, 52, 7, 500), at(2026, 10, 29, 16, 52, 7, 100), at(2026, 10, 29, 16, 52, 3)];
  assert.deepEqual(backupLabels(times, now), ["Today 16:52:07 (3)", "Today 16:52:07 (2)", "Today 16:52:07", "Today 16:52:03"]);
});

test("names are checked like the Rust side", () => {
  assert.equal(nameProblem("Rough Mix"), null);
  assert.equal(nameProblem("  "), "The name can't be empty.");
  for (const bad of [".hidden", "a/b", "a\\b", "a:b", "what?", 'say "hi"', "a|b", "tab\there"]) assert.notEqual(nameProblem(bad), null, bad);
  assert.notEqual(nameProblem("x".repeat(101)), null);
  assert.equal(freeName("Song", ["Other"]), "Song");
  assert.equal(freeName("Song", ["song", "Song 2"]), "Song 3");
});

test("track alternatives list the active one with the rest", () => {
  const one = { alternatives: undefined, alternative: undefined };
  assert.equal(activeTrackAlt(one), "A");
  assert.deepEqual(trackAltNames(one), ["A"]);
  assert.equal(showsTrackAlts(one), false);
  const three = { alternatives: [{ name: "C" }, { name: "a" }], alternative: "B" };
  assert.equal(activeTrackAlt(three), "B");
  assert.deepEqual(trackAltNames(three), ["a", "B", "C"]);
  assert.equal(showsTrackAlts(three), true);
});
