// Unit tests for src/tracks/groupLinks.ts: which tracks a group's links reach.
import { test } from "node:test";
import assert from "node:assert/strict";
import { linkedPeers, linkingGroup, touchWrites } from "../../src/tracks/groupLinks.ts";
import type { AutoMode, Group } from "../../src/engine/project.ts";

const group = (id: number, patch: Partial<Group> = {}): Group => ({ id, name: `Group ${id}`, color: "#fff", enabled: true, volume: true, pan: false, mute: true, solo: true, record: true, select: true, editing: true, ...patch });
const track = (id: number, g: number | null, auto_mode: AutoMode = "read") => ({ id, group: g, auto_mode });

const tracks = [track(1, 10), track(2, 10, "touch"), track(3, null, "latch"), track(4, 11)];

test("a link reaches the other members of a group that is on and shares it", () => {
  const groups = [group(10), group(11)];
  assert.deepEqual(linkedPeers(tracks, groups, 1, "volume").map((t) => t.id), [2]);
  assert.deepEqual(linkedPeers(tracks, groups, 1, "pan"), [], "pan isn't linked");
  assert.deepEqual(linkedPeers(tracks, groups, 3, "volume"), [], "no group");
  assert.deepEqual(linkedPeers(tracks, [group(10, { enabled: false })], 1, "mute"), [], "group off");
  assert.equal(linkingGroup(groups, tracks[3], "select")?.id, 11);
  assert.equal(linkingGroup([], tracks[3], "select"), undefined, "a group that's gone");
});

test("holding a control touches when the track or a partner it moves writes", () => {
  const groups = [group(10)];
  assert.equal(touchWrites(tracks, groups, 1, { type: "volume" }), true, "the partner is in Touch");
  assert.equal(touchWrites(tracks, groups, 1, { type: "pan" }), false, "pan isn't linked");
  assert.equal(touchWrites(tracks, groups, 2, { type: "pan" }), true, "its own track writes");
  assert.equal(touchWrites(tracks, groups, 4, { type: "volume" }), false);
});
