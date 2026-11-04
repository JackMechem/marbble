// Unit tests for src/settingsText.ts: the words Settings and the Recovery sheet show.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { PluginDesc } from "../src/api.ts";
import type { Recovery, RecordingRecovery } from "../src/prefs.ts";
import { describeMidi, oldFolderNote, pluginType, recordingSummary, recoveryHelp, recoveryIntro, when } from "../src/settingsText.ts";

test("describeMidi names messages, Logic-style with C3 as middle C", () => {
  assert.deepEqual(describeMidi([0x90, 60, 100]), ["Ch 1", "Note On  C3  vel 100"]);
  // A note on at velocity 0 is a note off.
  assert.deepEqual(describeMidi([0x90, 61, 0]), ["Ch 1", "Note Off C♯3"]);
  assert.deepEqual(describeMidi([0x83, 0, 0]), ["Ch 4", "Note Off C-2"]);
  assert.equal(describeMidi([0xb0, 64, 127])[1], "Control 64 = 127 (sustain)");
  assert.equal(describeMidi([0xb0, 1, 5])[1], "Control 1 = 5 (mod wheel)");
  assert.equal(describeMidi([0xb0, 7, 5])[1], "Control 7 = 5");
  assert.equal(describeMidi([0xe0, 0, 64])[1], "Pitch Bend 0");
  assert.equal(describeMidi([0xc0, 0, 0])[1], "Program 1");
  assert.equal(describeMidi([0xd0, 9, 0])[1], "Aftertouch 9");
  assert.equal(describeMidi([0xa0, 60, 9])[1], "Poly Pressure C3 9");
  assert.equal(describeMidi([0xf8, 0, 0])[1], "0xf8");
});

test("pluginType leaves out the categories everything has", () => {
  const p = (subcategories: string, is_instrument = false) => ({ subcategories, is_instrument }) as PluginDesc;
  assert.equal(pluginType(p("Fx|Dynamics|Stereo")), "Effect · Dynamics");
  assert.equal(pluginType(p("Instrument|Synth|Sampler", true)), "Instrument · Synth, Sampler");
  assert.equal(pluginType(p("Fx")), "Effect");
});

test("when says today for today", () => {
  assert.match(when(Date.now() / 1000), /^today at /);
  assert.doesNotMatch(when(86400), /^today/);
});

const rec = (over: Partial<RecordingRecovery>) => ({ takes: 2, seconds: 65, tracks: ["Vocal", "Guitar"], ...over }) as RecordingRecovery;

test("recordingSummary", () => {
  assert.equal(recordingSummary(rec({})), "2 takes (up to 1:05) on Vocal and Guitar");
  assert.equal(recordingSummary(rec({ takes: 1, tracks: ["Vocal"], seconds: 9 })), "1 take (0:09) on Vocal");
  assert.equal(recordingSummary(rec({ tracks: ["A", "B", "C"] })), "2 takes (up to 1:05) on A, B and C");
  assert.equal(recordingSummary(rec({ tracks: [] })), "2 takes (up to 1:05)");
});

test("recoveryHelp names every button the sheet shows, in its order", () => {
  const names = (single: boolean, saved: boolean) => recoveryHelp(single, saved).map(([n]) => n);
  assert.deepEqual(names(true, true), ["Discard", "Open Last Saved", "Recover", "Decide Later"]);
  assert.deepEqual(names(true, false), ["Discard", "Recover", "Decide Later"]);
  assert.deepEqual(names(false, true), ["Discard", "Recover", "Decide Later"]);
  assert.match(recoveryHelp(true, true)[3][1], /offered again/);
});

test("recoveryIntro says what was left", () => {
  const r = (has_autosave: boolean, recording: boolean) => ({ has_autosave, recording }) as unknown as Recovery;
  assert.match(recoveryIntro([r(false, true)]), /quit while this project was recording/);
  assert.match(recoveryIntro([r(true, true)]), /still being recorded/);
  assert.match(recoveryIntro([r(true, false)]), /autosaved but never saved/);
  assert.match(recoveryIntro([r(true, false), r(false, true)]), /These projects have unsaved work/);
  assert.match(recoveryIntro([r(true, false), r(true, false)]), /These projects have autosaved changes/);
});

test("the old projects folder's note says what renaming does, and what holds it", () => {
  const f = { old: "/h/Music/Fermata", new: "/h/Music/Marbble", open: null };
  const free = oldFolderNote(f);
  assert.match(free, /\/h\/Music\/Fermata/);
  assert.match(free, /\/h\/Music\/Marbble/);
  assert.doesNotMatch(free, /is open from it/);
  assert.match(oldFolderNote({ ...f, open: "Song" }), /“Song” is open from it/);
});
