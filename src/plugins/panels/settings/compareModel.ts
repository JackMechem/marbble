// Compare, in a built-in plug-in's panel header (CompareButton.tsx), as in Logic: it remembers the
// settings as last loaded or saved, lights once they're edited, and swaps between the edited and the
// saved ones. This is the pure part: what is remembered, what counts as a change, and what a press
// does. pluginSettings.ts keeps the records in the store and asks the engine.
import type { PluginSlot } from "../../../engine/project";

/** What Compare remembers of one plug-in instance. */
export interface CompareRecord {
  /** The plug-in and project it was taken in: instance ids start again in every project. */
  pluginId: string;
  path: string;
  /** The settings as last loaded or saved (a `copy_plugin` copy, so a Sampler's zones too), and
   *  their key (`settingsKey`). */
  saved: PluginSlot;
  savedKey: string;
  /** The edited settings while Compare plays the saved ones; null otherwise. */
  edited: PluginSlot | null;
}

/** A parameter as `settingsKey` needs it. */
interface ParamDefault {
  id: number;
  default: number;
}

/**
 * The settings of a slot in one comparable form: every parameter's value (its default when the
 * slot doesn't list it), and what isn't numbers. In a snapshot that is a Sampler's fingerprint of
 * its zones, so a change to a zone counts too.
 */
export function settingsKey(slot: Pick<PluginSlot, "params" | "state">, params: ParamDefault[]): string {
  const values = new Map(slot.params ?? []);
  return JSON.stringify([params.map((p) => values.get(p.id) ?? p.default), slot.state ?? null]);
}

/** Whether the saved settings must be taken (again) from the instance as it is now: the panel
 *  shows it for the first time, or it is another plug-in or project under the same id. */
export function needsCapture(rec: CompareRecord | undefined, pluginId: string, path: string): boolean {
  return !rec || rec.pluginId !== pluginId || rec.path !== path;
}

/** How the Compare button shows. */
export interface CompareView {
  /** The settings differ from the saved ones: the button lights. */
  changed: boolean;
  /** The saved settings play in place of the edited ones: the button is pressed. */
  comparing: boolean;
}

export function compareView(rec: CompareRecord | undefined, currentKey: string): CompareView {
  if (!rec) return { changed: false, comparing: false };
  const changed = currentKey !== rec.savedKey;
  // Editing while comparing makes those settings the edited ones, as in Logic: the stash is stale.
  return { changed, comparing: !changed && rec.edited !== null };
}

/** What a press of Compare does. */
export type CompareStep =
  /** Keep the settings now as the edited ones, and play the saved ones. */
  | { kind: "show-saved"; saved: PluginSlot }
  /** Go back to the edited settings. */
  | { kind: "show-edited"; edited: PluginSlot }
  | { kind: "none" };

export function compareStep(rec: CompareRecord | undefined, currentKey: string): CompareStep {
  if (!rec) return { kind: "none" };
  const view = compareView(rec, currentKey);
  if (view.comparing) return { kind: "show-edited", edited: rec.edited! };
  if (view.changed) return { kind: "show-saved", saved: rec.saved };
  return { kind: "none" };
}
