// Open, Save and Import dialogs, the unsaved-changes question, and undo and redo with their toasts.
// project/ holds the project as a file: opening and saving, the Project Chooser, alternatives and
// templates, launch and crash recovery, MIDI files, dropped files, and bouncing (bounce/).
import { ask } from "@tauri-apps/plugin-dialog";
import { open, save } from "./fileDialog";
import { type Snapshot } from "../engine/snapshot";
import { act, call } from "../engine/commands";
import { getSettings } from "../settings/appSettings";
import { openProjectPath } from "./startup";
import { currentTick, getState, setState, showToast } from "../state/store";
import { AUDIO_EXTENSIONS } from "./fileDrop";

/** Asks before discarding unsaved changes. Resolves true when it is fine to continue. */
export async function confirmDiscard(action: string): Promise<boolean> {
  const snap = getState().snap;
  if (!snap?.dirty) return true;
  // With the question turned off in Settings › General, the autosave stays as a safety net.
  if (!getSettings().ui.confirm_unsaved) return true;
  const ok = await ask(`The project "${snap.project.name}" has unsaved changes. ${action} anyway and discard them?`, {
    title: "Unsaved Changes",
    kind: "warning",
    okLabel: "Don't Save",
    cancelLabel: "Cancel",
  });
  if (ok) await call("discard_autosave", { path: null });
  return ok;
}

/** Undoes (or redoes) one edit and says which. */
export async function undoRedo(which: "undo" | "redo") {
  const res = await call<{ snapshot: Snapshot; label: string | null }>(which);
  if (!res) return;
  const ids = new Set(res.snapshot.project.tracks.flatMap((t) => t.regions.map((r) => r.id)));
  setState({ snap: res.snapshot, selectedRegions: getState().selectedRegions.filter((id) => ids.has(id)) });
  showToast(res.label ? `${which === "undo" ? "Undo" : "Redo"} ${res.label}` : which === "undo" ? "Nothing to undo" : "Nothing to redo", 1800);
}

export async function openProjectDialog() {
  if (!(await confirmDiscard("Open another project"))) return;
  const dir = await open({ directory: true, title: "Open Project (select a .marbble folder)" });
  if (typeof dir === "string") await openProjectPath(dir);
}

export async function saveProject(as = false) {
  const snap = getState().snap;
  if (!snap) return;
  if (!as && snap.saved) {
    await act("save_project", { path: null });
    return;
  }
  const path = await save({ title: "Save Project", defaultPath: snap.path.replace(/\.(marbble|fermata)$/, "") + ".marbble" });
  if (path) await act("save_project", { path });
}

export async function importAudioDialog() {
  const snap = getState().snap;
  const track = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "audio") ?? snap?.project.tracks.find((t) => t.kind === "audio");
  if (!track) {
    showToast("Create or select an audio track first");
    return;
  }
  const file = await open({ title: "Import Audio File", filters: [{ name: "Audio files", extensions: AUDIO_EXTENSIONS }] });
  if (typeof file === "string") {
    await act("import_audio", { path: file, track: track.id, start: currentTick()?.pos ?? 0 });
  }
}
