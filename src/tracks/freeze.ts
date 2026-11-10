// Track Freeze (Logic's snowflake button): renders a track's instrument and plug-ins to a file
// and plays that instead, with the plug-ins unloaded. The engine side is
// crates/engine/src/session/tracks/freeze.rs; these are the UI's entry points and guards.
import { listen } from "@tauri-apps/api/event";
import { type Id, type Track } from "../engine/project";
import { type Snapshot } from "../engine/snapshot";
import { act, call } from "../engine/commands";
import { isBouncing, setBouncing } from "../project/bounce/bounce";
import { getState, setState, showToast } from "../state/store";

const tracks = (): Track[] => getState().snap?.project.tracks ?? [];

/** Audio and software instrument tracks have a Freeze button. */
export const canFreeze = (t: Track) => t.kind === "audio" || t.kind === "instrument";

/**
 * Says why an edit can't happen when it would touch a frozen track. Returns true (and
 * shows a toast) when one of `ids` is frozen. The session refuses these edits too.
 */
export function refuseFrozen(ids: Id[]): boolean {
  const t = tracks().find((x) => ids.includes(x.id) && x.frozen);
  if (!t) return false;
  showToast(`${t.name} is frozen. Unfreeze it to edit its regions and plug-ins.`, 3000);
  return true;
}

/** Renders and freezes the tracks, showing progress, as one undo step. */
export async function freezeTracks(ids: Id[]) {
  const todo = tracks().filter((t) => ids.includes(t.id) && canFreeze(t) && !t.frozen);
  if (!todo.length || isBouncing()) return;
  const label = todo.length > 1 ? `${todo.length} tracks` : todo[0].name;
  setBouncing(true);
  showToast(`Freezing ${label}…`, 60000);
  const off = await listen<number>("bounce-progress", (e) => showToast(`Freezing ${label}… ${Math.round(e.payload * 100)}%`, 60000));
  try {
    const snap = await call<Snapshot>("freeze_tracks", { ids: todo.map((t) => t.id) });
    if (snap) {
      setState({ snap });
      showToast(`Froze ${label}`, 1500);
    }
  } finally {
    off();
    setBouncing(false);
  }
}

export function unfreezeTracks(ids: Id[]) {
  const todo = tracks().filter((t) => ids.includes(t.id) && t.frozen).map((t) => t.id);
  if (todo.length) act("unfreeze_tracks", { ids: todo });
}

/** The snowflake: freezes the tracks, or unfreezes them when they're all frozen already. */
export function toggleFreeze(ids: Id[]) {
  const sel = tracks().filter((t) => ids.includes(t.id) && canFreeze(t));
  if (sel.length && sel.every((t) => t.frozen)) unfreezeTracks(ids);
  else freezeTracks(ids);
}
