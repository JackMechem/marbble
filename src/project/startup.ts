// What happens at launch, as Settings › General says (the Project Chooser, the last project or an
// empty one), and crash recovery: unsaved work found in autosaves and interrupted recordings,
// offered in the Recovery sheet (RecoverySheet.tsx).
import { useSyncExternalStore } from "react";
import { type Snapshot } from "../engine/snapshot";
import type { RecentProject } from "../engine/listings";
import { act, call } from "../engine/commands";
import { getState, setState } from "../state/store";
import { getSettings } from "../settings/appSettings";

/** Audio a recording left behind when Marbble quit mid-take. */
export interface RecordingRecovery {
  started: number;
  takes: number;
  /** The longest take, in seconds. */
  seconds: number;
  tracks: string[];
}

/** A project folder with unsaved work: an autosave newer than its saved document, or audio
 * from a recording that never finished. */
export interface Recovery {
  path: string;
  name: string;
  /** The autosave's time, or the interrupted recording's start when there's only that. */
  autosaved: number;
  saved: number | null;
  has_autosave: boolean;
  recording: RecordingRecovery | null;
}

let recoveries: Recovery[] = [];
let recoveryAtLaunch = false;
/** Projects whose recovery was put off ("Later") during this launch: not offered again. */
let putOff: string[] = [];
const recoveryListeners = new Set<() => void>();

export function useRecoveries(): Recovery[] {
  return useSyncExternalStore(
    (l) => {
      recoveryListeners.add(l);
      return () => recoveryListeners.delete(l);
    },
    () => recoveries,
  );
}

function showRecovery(list: Recovery[], atLaunch: boolean) {
  recoveries = list;
  recoveryAtLaunch = atLaunch;
  recoveryListeners.forEach((l) => l());
  setState({ dialog: "recovery" });
}

/**
 * Called when the recovery sheet closes. If nothing was opened from it at launch, carries on
 * with the usual startup.
 */
export function recoveryDone(opened: boolean) {
  const atLaunch = recoveryAtLaunch;
  if (atLaunch && !opened) putOff = [...putOff, ...recoveries.map((r) => r.path)];
  recoveries = [];
  recoveryAtLaunch = false;
  recoveryListeners.forEach((l) => l());
  setState({ dialog: null });
  if (atLaunch && !opened) runStartup(false);
}

/**
 * What happens at launch without a project on the command line: offer to recover unsaved
 * work first, then open the last project or show the Project Chooser, as set in
 * Settings › General.
 */
export async function runStartup(checkRecovery = true) {
  if (checkRecovery) {
    const lost = await call<Recovery[]>("recoverable_projects");
    if (lost?.length) return showRecovery(lost, true);
  }
  const startup = getSettings().ui.startup;
  if (startup === "empty_project") return;
  const recent = await call<RecentProject[]>("recent_projects");
  const last = recent?.find((p) => p.exists);
  if (startup === "last_project" && last && !putOff.includes(last.path)) {
    if (await openProjectPath(last.path)) return;
    // It has newer autosaved changes, so the recovery sheet is up instead. Leave it there; if
    // it's closed without opening anything, startup carries on with the Chooser.
    if (getState().dialog === "recovery") {
      recoveryAtLaunch = true;
      return;
    }
  }
  setState({ dialog: "chooser", chooserTab: last ? "recent" : "new" });
}

/** Offers the autosave of a project that is already open (one given on the command line). */
export async function offerRecoveryFor(path: string) {
  const lost = await call<Recovery | null>("recovery_info", { path });
  if (lost) showRecovery([lost], false);
}

/**
 * Opens a project, offering its autosave first when one is newer than the saved file.
 * Resolves to the snapshot, or null if it didn't open (or the recovery sheet took over).
 */
export async function openProjectPath(path: string): Promise<Snapshot | null> {
  const lost = await call<Recovery | null>("recovery_info", { path });
  if (lost) {
    showRecovery([lost], false);
    return null;
  }
  const snap = await act("open_project", { path });
  if (snap) setState({ selectedRegions: [] });
  return snap;
}
