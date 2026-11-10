// The Event List's preferences: the event types shown, what + adds, Catch Playhead and MIDI Out,
// kept in localStorage across sessions.
import { useSyncExternalStore } from "react";
import * as el from "./model";

export interface Prefs {
  /** Event types shown (the filter buttons). */
  show: Record<el.EventType, boolean>;
  /** What + adds. */
  addType: el.EventType;
  catchPlayhead: boolean;
  /** Play notes when they are clicked or their pitch or velocity is changed. */
  midiOut: boolean;
}

const PREFS_KEY = "marbble.eventList";
const DEFAULT_PREFS: Prefs = { show: { note: true, bend: true, control: true, pressure: true }, addType: "note", catchPlayhead: true, midiOut: true };

export let prefs: Prefs = (() => {
  try {
    const p = JSON.parse(localStorage.getItem(PREFS_KEY) ?? "{}");
    return { ...DEFAULT_PREFS, ...p, show: { ...DEFAULT_PREFS.show, ...p.show } };
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

export const usePrefs = () =>
  useSyncExternalStore(
    (f) => {
      prefSubs.add(f);
      return () => prefSubs.delete(f);
    },
    () => prefs,
  );
