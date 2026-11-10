// A built-in plug-in's settings from its panel header: Copy and Paste Settings, and Compare. The
// settings clipboard and Compare's records are in the store, so every window shares them
// (detachModel.ts SYNCED_FIELDS), and outside the project, as the plug-in clipboard is. Settings
// travel as `copy_plugin` copies and go back in with `paste_plugin_settings`, one undo step each.
// The decisions are in compareModel.ts and settingsMenuModel.ts.
import type { Id, PluginSlot } from "../../../engine/project";
import { act, call } from "../../../engine/commands";
import { getState, setState } from "../../../state/store";
import { findFxSlot, fxInfo } from "../../fxHost";
import { compareStep, settingsKey, type CompareRecord } from "./compareModel";

/** Where an instance is now and its settings' key, from the latest snapshot. */
function instance(uid: Id): { track: Id; slot: number; plugin: PluginSlot; key: string } | null {
  const at = findFxSlot(getState().snap?.project, uid);
  const info = fxInfo(at?.plugin.plugin_id);
  return at && info ? { ...at, key: settingsKey(at.plugin, info.params) } : null;
}

const copyOf = (track: Id, slot: number) => call<PluginSlot>("copy_plugin", { track, slot });

const paste = (track: Id, slot: number, plugin: PluginSlot, compare: boolean) => act("paste_plugin_settings", { track, slot, plugin, compare });

function setRecord(uid: Id, rec: CompareRecord) {
  setState({ pluginCompare: { ...getState().pluginCompare, [uid]: rec } });
}

export async function copySettings(uid: Id) {
  const at = instance(uid);
  const copy = at && (await copyOf(at.track, at.slot));
  if (copy) setState({ settingsClip: copy });
}

export async function pasteSettings(uid: Id) {
  const at = instance(uid);
  const clip = getState().settingsClip;
  if (at && clip) await paste(at.track, at.slot, clip, false);
}

/** Takes the instance's settings now as Compare's saved ones: when its panel first shows them, and
 *  after a preset is loaded or saved. */
export async function rememberSaved(uid: Id) {
  const at = instance(uid);
  const saved = at && (await copyOf(at.track, at.slot));
  // The key of the settings as they are once the copy is back, which is what it holds.
  const now = instance(uid);
  if (!saved || !now) return;
  setRecord(uid, { pluginId: now.plugin.plugin_id, path: getState().snap?.path ?? "", saved, savedKey: now.key, edited: null });
}

/** The Compare button: plays the saved settings, keeping the edited ones, or goes back to them. */
export async function toggleCompare(uid: Id) {
  const at = instance(uid);
  const rec = getState().pluginCompare[uid];
  if (!at || !rec) return;
  const step = compareStep(rec, at.key);
  if (step.kind === "show-saved") {
    const edited = await copyOf(at.track, at.slot);
    if (!edited) return;
    setRecord(uid, { ...rec, edited });
    await paste(at.track, at.slot, step.saved, true);
  } else if (step.kind === "show-edited") {
    if (!(await paste(at.track, at.slot, step.edited, true))) return;
    const latest = getState().pluginCompare[uid];
    if (latest) setRecord(uid, { ...latest, edited: null });
  }
}
