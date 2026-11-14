// The colours the engine gives new groups (session/tracks/groups/mod.rs) are the track palette's,
// so Group Settings and the Groups sheet show which one a group has.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { TRACK_COLORS } from "../../src/tracks/trackColors.ts";

test("new groups take palette colours", () => {
  const src = readFileSync(fileURLToPath(new URL("../../crates/engine/src/session/tracks/groups/mod.rs", import.meta.url)), "utf8");
  const line = src.split("\n").find((l) => l.includes("let colors = ["));
  assert.ok(line, "create_group's colour list");
  const colors = [...line.matchAll(/"(#[0-9a-f]{6})"/g)].map((m) => m[1]);
  assert.ok(colors.length > 0);
  for (const c of colors) assert.ok(TRACK_COLORS.includes(c), `${c} is in TRACK_COLORS`);
});
