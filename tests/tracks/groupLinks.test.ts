// Unit tests for src/tracks/groupLinks.ts: which tracks a group's links reach.
import { test } from "node:test";
import assert from "node:assert/strict";
import { activeGroups, linkedPeers, linkedRegionIds, linkingGroup, toggleRegion, toggleTrack, touchWrites, withSelectionPeers } from "../../src/tracks/groupLinks.ts";
import type { AutoMode, Group } from "../../src/engine/project.ts";

const group = (id: number, patch: Partial<Group> = {}): Group => ({ id, name: `Group ${id}`, color: "#fff", enabled: true, volume: true, pan: false, mute: true, solo: true, record: true, select: true, editing: true, track_color: false, monitor: false, auto_mode: false, hide: false, ...patch });
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

test("selecting a member selects its group; taking it out takes the group out", () => {
  const groups = [group(10), group(11, { select: false })];
  assert.deepEqual(withSelectionPeers(tracks, groups, [2]), [1, 2]);
  assert.deepEqual(withSelectionPeers(tracks, groups, [4, 3]), [3, 4], "in track order");
  assert.deepEqual(toggleTrack(tracks, groups, [1, 2, 3], 1), [3], "its partner goes too");
  assert.deepEqual(toggleTrack(tracks, [group(10, { select: false })], [1, 2, 3], 1), [2, 3], "unless selection isn't linked");
  assert.deepEqual(toggleTrack(tracks, groups, [3], 1), [3, 1]);
});

test("a region links the regions overlapping it on the members, and Shift-clicks add and take them out together", () => {
  const region = (id: number, start: number, length: number) => ({ id, start, length, loop_end: null });
  const lanes = [
    { id: 1, group: 10, regions: [region(100, 0, 10)] },
    { id: 2, group: 10, regions: [region(200, 5, 10), region(201, 10, 5)] },
    { id: 3, group: null, regions: [region(300, 0, 10)] },
  ];
  const groups = [group(10)];
  assert.deepEqual(linkedRegionIds(lanes, groups, 100), [100, 200], "201 starts where 100 ends");
  assert.deepEqual(linkedRegionIds(lanes, [group(10, { editing: false })], 100), [100]);
  assert.deepEqual(linkedRegionIds(lanes, groups, 300), [300]);
  const linked = linkedRegionIds(lanes, groups, 100);
  assert.deepEqual(toggleRegion([300], 100, linked), [300, 100, 200]);
  assert.deepEqual(toggleRegion([300, 100, 200], 100, linked), [300]);
  assert.deepEqual(toggleRegion([200], 100, linked), [200, 100], "no duplicates");
});

test("the group clutch leaves no group linking anything", () => {
  const project = { groups: [group(10)] };
  assert.equal(activeGroups({ project, groups_clutched: false }).length, 1);
  assert.deepEqual(activeGroups({ project, groups_clutched: true }), []);
  assert.deepEqual(activeGroups(null), []);
});
