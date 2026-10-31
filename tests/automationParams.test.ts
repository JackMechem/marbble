// Unit tests for src/automation/params.ts: what a lane's parameter is and its scale.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { AutoParam, Project, Track } from "../src/api.ts";
import { OUTPUT_ID } from "../src/autoLanes.ts";
import { baseParams, fromUnit, fromUnitExact, isOutput, laneOf, outputTrack, paramLabel, stripOf, toUnit, VOLUME } from "../src/automation/params.ts";

const PAN: AutoParam = { type: "pan" };
const MUTE: AutoParam = { type: "mute" };
const send = (k: number): AutoParam => ({ type: "send", value: k });
const plugin: AutoParam = { type: "plugin", value: { uid: 3, param: 7, name: "EQ: Gain" } };

const track = (over: Partial<Track> = {}) => ({ id: 1, kind: "audio", sends: [], automation: [], ...over }) as unknown as Track;
const project = () => ({ tracks: [track()], master: { volume_db: -3, inserts: [], eq: null, automation: [{ param: VOLUME, points: [] }], auto_mode: "touch", auto_lanes: [] } }) as unknown as Project;

test("labels", () => {
  assert.equal(paramLabel(VOLUME), "Volume");
  assert.equal(paramLabel(send(1)), "Send 2");
  assert.equal(paramLabel(plugin), "EQ: Gain");
});

test("laneOf finds a lane by its parameter", () => {
  const lane = { param: PAN, points: [] };
  const t = track({ automation: [{ param: VOLUME, points: [] }, lane] } as unknown as Partial<Track>);
  assert.equal(laneOf(t, PAN), lane);
  assert.equal(laneOf(t, MUTE), undefined);
});

test("which parameters a strip has", () => {
  assert.deepEqual(baseParams(track({ kind: "folder" } as Partial<Track>)), [VOLUME, MUTE]);
  assert.deepEqual(baseParams(outputTrack(project())), [VOLUME]);
  // Empty send slots have no lane.
  const sends = [null, { bus: 2 }] as unknown as Track["sends"];
  assert.deepEqual(baseParams(track({ sends })), [VOLUME, PAN, MUTE, send(1)]);
});

test("Stereo Out as a track is the same object for the same snapshot", () => {
  const p = project();
  const o = outputTrack(p);
  assert.equal(outputTrack(p), o);
  assert.notEqual(outputTrack(project()), o);
  assert.ok(isOutput(o));
  assert.equal(o.id, OUTPUT_ID);
  assert.equal(o.volume_db, -3);
  assert.equal(o.auto_mode, "touch");
});

test("stripOf finds tracks and the Output track", () => {
  const p = project();
  assert.equal(stripOf(p, 1), p.tracks[0]);
  assert.equal(stripOf(p, OUTPUT_ID), outputTrack(p));
  assert.equal(stripOf(p, 99), undefined);
  assert.equal(stripOf(undefined, 1), undefined);
});

test("the lane's scale: pan and plug-ins are linear, volume follows the fader", () => {
  assert.equal(toUnit(PAN, -1), 0);
  assert.equal(toUnit(PAN, 1), 1);
  assert.equal(toUnit(plugin, 0.25), 0.25);
  assert.equal(toUnit(plugin, 2), 1);
  // Unity sits above the middle of the fader's travel.
  assert.ok(toUnit(VOLUME, 0) > 0.5);
  assert.equal(toUnit(VOLUME, -96), 0);
});

test("fromUnit rounds to what the control can hold and clamps", () => {
  assert.equal(fromUnit(MUTE, 0.4), 0);
  assert.equal(fromUnit(MUTE, 0.6), 1);
  assert.equal(fromUnit(PAN, 0.5), 0);
  assert.equal(fromUnit(PAN, 2), 1);
  assert.equal(fromUnit(plugin, 0.12345), 0.123);
  assert.equal(fromUnit(VOLUME, 0), -96);
  assert.equal(fromUnit(VOLUME, toUnit(VOLUME, -6)), -6);
});

test("fromUnitExact doesn't round, so a curve can follow the pointer", () => {
  assert.equal(fromUnitExact(plugin, 0.12345), 0.12345);
  assert.equal(fromUnitExact(PAN, 0.25), -0.5);
  // Unlike fromUnit, a mute lane passes through in between.
  assert.equal(fromUnitExact(MUTE, 0.4), 0.4);
  assert.equal(fromUnitExact(PAN, -1), -1);
});
