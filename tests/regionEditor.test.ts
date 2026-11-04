// Unit tests for src/regionEditor.ts: which editor a double-clicked region opens.
import { test } from "node:test";
import assert from "node:assert/strict";
import { editorPaneFor } from "../src/regionEditor.ts";
import type { Region } from "../src/api.ts";

const midi: Pick<Region, "content" | "pattern"> = { content: { type: "midi", notes: [], cc: [] } };
const audio: Pick<Region, "content" | "pattern"> = { content: { type: "audio", clip: 1, offset: 0, gain_db: 0, fade_in: 0, fade_out: 0 } };
const pattern = { ...midi, pattern: { length: 16, rate: 16, rows: [] } } as unknown as Pick<Region, "content" | "pattern">;

test("each kind of region opens its own editor, from whatever the bottom section shows", () => {
  for (const current of ["none", "mixer", "fx", "editor", "steps"] as const) {
    assert.equal(editorPaneFor(midi, current), "editor", `MIDI from ${current}: the Piano Roll`);
    assert.equal(editorPaneFor(audio, current), "editor", `audio from ${current}: the Audio Track editor`);
    assert.equal(editorPaneFor(pattern, current), "steps", `pattern from ${current}: the Step Sequencer`);
  }
});

test("the Event List stays for MIDI, which it edits, but not for audio or patterns", () => {
  assert.equal(editorPaneFor(midi, "list"), "list");
  assert.equal(editorPaneFor(audio, "list"), "editor");
  assert.equal(editorPaneFor(pattern, "list"), "steps");
});
