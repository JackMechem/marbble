// What an automation lane's parameter is: labels, which strip has which, the lane's scale through
// the fader taper, and Stereo Out as a track. Pure and tested in tests/automationParams.test.ts.
// autoLanes.ts has the curves and the lanes under a track.
import type { AutoLane, AutoMode, AutoParam, Id, Project, Track } from "../api";
import { OUTPUT_ID, sameParam, valueAt } from "../autoLanes.ts";
import { dbToFader, faderToDb } from "../time.ts";

export const VOLUME: AutoParam = { type: "volume" };

export const MODE_LABELS: Record<AutoMode, string> = { off: "Off", read: "Read", touch: "Touch", latch: "Latch", write: "Write" };
export const MODES: AutoMode[] = ["off", "read", "touch", "latch", "write"];

export { sameParam, valueAt };

export function paramLabel(p: AutoParam): string {
  switch (p.type) {
    case "volume":
      return "Volume";
    case "pan":
      return "Pan";
    case "mute":
      return "Mute";
    case "send":
      return `Send ${p.value + 1}`;
    case "plugin":
      return p.value.name;
  }
}

export function laneOf(t: Track, p: AutoParam): AutoLane | undefined {
  return t.automation?.find((l) => sameParam(l.param, p));
}

/** Every strip can be automated: a folder its volume and mute (which reach its subtracks),
 *  Stereo Out its volume and plug-ins. */
export function automatable(_t: Track): boolean {
  return true;
}

export function isOutput(t: Track): boolean {
  return t.id === OUTPUT_ID;
}

let outputCache: { project: Project; track: Track } | null = null;

/** Stereo Out as a track, for the Output track and the automation code: its volume, plug-ins
 *  and lanes. The same object for the same project snapshot. */
export function outputTrack(project: Project): Track {
  if (outputCache?.project === project) return outputCache.track;
  const m = project.master;
  const track: Track = {
    id: OUTPUT_ID,
    kind: "aux",
    name: "Stereo Out",
    color: "#8e8e93",
    volume_db: m.volume_db,
    pan: 0,
    mute: false,
    solo: false,
    armed: false,
    monitor: false,
    input: { type: "none" },
    instrument: null,
    inserts: m.inserts,
    regions: [],
    eq: m.eq,
    parent: null,
    collapsed: false,
    group: null,
    hidden: false,
    sends: [],
    output: null,
    bus: 0,
    automation: m.automation ?? [],
    auto_mode: m.auto_mode ?? "read",
    auto_lanes: m.auto_lanes ?? [],
  };
  outputCache = { project, track };
  return track;
}

/** A strip by id: a track, or the Output track for Stereo Out. */
export function stripOf(project: Project | undefined, id: Id): Track | undefined {
  if (!project) return undefined;
  return id === OUTPUT_ID ? outputTrack(project) : project.tracks.find((t) => t.id === id);
}

/** The parameters a strip has besides its plug-ins' (Stereo Out has no pan or mute, a folder
 *  no pan). */
export function baseParams(t: Track): AutoParam[] {
  if (isOutput(t)) return [VOLUME];
  if (t.kind === "folder") return [VOLUME, { type: "mute" }];
  return [VOLUME, { type: "pan" }, { type: "mute" }, ...t.sends.flatMap((s, k): AutoParam[] => (s ? [{ type: "send", value: k }] : []))];
}

/** Height in the lane (0..1) for a value. Volume follows the fader's taper, as in Logic. */
export function toUnit(p: AutoParam, v: number): number {
  switch (p.type) {
    case "volume":
    case "send":
      return dbToFader(v);
    case "pan":
      return (v + 1) / 2;
    default:
      return Math.min(1, Math.max(0, v));
  }
}

export function fromUnit(p: AutoParam, u: number): number {
  u = Math.min(1, Math.max(0, u));
  switch (p.type) {
    case "volume":
    case "send":
      return Math.round(faderToDb(u) * 10) / 10;
    case "pan":
      return Math.round((u * 2 - 1) * 64) / 64;
    case "mute":
      return u >= 0.5 ? 1 : 0;
    case "plugin":
      return Math.round(u * 1000) / 1000;
  }
}

/** `fromUnit` without the rounding, for following the pointer smoothly. */
export function fromUnitExact(p: AutoParam, u: number): number {
  u = Math.min(1, Math.max(0, u));
  switch (p.type) {
    case "volume":
    case "send":
      return faderToDb(u);
    case "pan":
      return u * 2 - 1;
    default:
      return u;
  }
}
