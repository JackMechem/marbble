// The Piano Roll's preferences: zoom, tool, grid, Quantize settings and the rest, kept in localStorage
// across editor switches and sessions, and shared by every part of the editor.
import { useSyncExternalStore } from "react";
import * as me from "../../midiEdit";
import { type CurveShape, type Density } from "../../ccCurves";
import { LANE_SHARE, type Tool } from "../../pianoRollModel";

export interface Prefs {
  /** Horizontal zoom, pixels per beat. */
  pxPerBeat: number;
  noteH: number;
  /** The lane's share of the height under the ruler (pianoRollModel's LANE_SHARE). */
  laneShare: number;
  showLane: boolean;
  /** "velocity", "cc:<n>", "pb" or "at". */
  lane: string;
  tool: Tool;
  grid: me.GridValue;
  snap: boolean;
  qValue: me.GridValue;
  /** Percentages. */
  qStrength: number;
  qSwing: number;
  qEnds: boolean;
  color: "velocity" | "region";
  scale: boolean;
  midiOut: boolean;
  catchPlayhead: boolean;
  /** The controller lane's Line Curve shape and Ramp Density (ccCurves.ts). */
  curve: CurveShape;
  density: Density;
}

const PREFS_KEY = "marbble.pianoRoll";
const DEFAULT_PREFS: Prefs = {
  pxPerBeat: 80,
  noteH: 12,
  laneShare: LANE_SHARE.initial,
  showLane: true,
  lane: "velocity",
  tool: "pointer",
  grid: "1/16",
  snap: true,
  qValue: "1/16",
  qStrength: 100,
  qSwing: 0,
  qEnds: false,
  color: "velocity",
  scale: false,
  midiOut: true,
  catchPlayhead: true,
  curve: "linear",
  density: "1/32",
};

let prefs: Prefs = (() => {
  try {
    return { ...DEFAULT_PREFS, ...JSON.parse(localStorage.getItem(PREFS_KEY) ?? "{}") };
  } catch {
    return DEFAULT_PREFS;
  }
})();
const prefSubs = new Set<() => void>();

export function setPrefs(p: Partial<Prefs>) {
  prefs = { ...prefs, ...p };
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Private mode: settings just don't persist.
  }
  prefSubs.forEach((f) => f());
}

export function usePrefs(): Prefs {
  return useSyncExternalStore(
    (f) => {
      prefSubs.add(f);
      return () => prefSubs.delete(f);
    },
    () => prefs,
  );
}
