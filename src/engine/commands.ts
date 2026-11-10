// The way to the engine through Tauri: `act` runs a command that returns a Snapshot and puts it in
// the store, `actMade` the same for one that reports what it made, `call` anything else. A failed
// command shows its error as a toast. `api` names the commands used everywhere.
import { invoke } from "@tauri-apps/api/core";
import { setState } from "../state/store";
import type { Id, Track } from "./project";
import type { Snapshot, Made } from "./snapshot";

function report(e: unknown) {
  const msg = typeof e === "string" ? e : (e as Error)?.message ?? String(e);
  setState({ toast: { text: msg, at: Date.now() } });
}

/** Invokes a command that returns a fresh snapshot and stores it. */
export async function act(cmd: string, args?: Record<string, unknown>): Promise<Snapshot | null> {
  try {
    const res = await invoke<Snapshot | Made>(cmd, args);
    // Commands that report what they made wrap the snapshot.
    const snap = res && "snapshot" in res ? res.snapshot : res;
    if (snap) setState({ snap });
    if (snap?.refused) report(snap.refused);
    if (snap?.warning) report(snap.warning);
    return snap;
  } catch (e) {
    report(e);
    return null;
  }
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    report(e);
    return null;
  }
}

/** Invokes a command returning a `Made` and stores its snapshot. */
export async function actMade(cmd: string, args?: Record<string, unknown>): Promise<Id[] | null> {
  const res = await call<Made>(cmd, args);
  if (!res) return null;
  setState({ snap: res.snapshot });
  return res.ids;
}

export const api = {
  updateTrack: (id: Id, patch: Partial<Omit<Track, "id" | "kind" | "regions" | "inserts" | "instrument" | "parent" | "group" | "hidden" | "sends" | "output" | "bus">>) =>
    act("update_track", { id, patch }),
  setTrackIcon: (ids: Id[], icon: string | null) => act("set_track_icon", { ids, icon }),
  selectTrack: (id: Id | null) => act("select_track", { id }),
  setPlugin: (track: Id, slot: number, pluginId: string, open = true) =>
    act("set_plugin", { track, slot, pluginId, open }),
  removePlugin: (track: Id, slot: number) => act("remove_plugin", { track, slot }),
  setBypass: (track: Id, slot: number, bypass: boolean) => act("set_bypass", { track, slot, bypass }),
  openEditor: (uid: Id, title: string) => act("open_editor", { uid, title }),
};
