// What a preset box shows. The preset a slot or Channel EQ last loaded or saved is in the
// project (`PluginSlot.preset`, `eq_preset`), so it survives reopening the project and undo; the
// box compares the settings now with that preset's. Pure, so it can be unit tested.
import type { PresetRef } from "./api";

export interface PresetBoxLabel {
  label: string;
  /** The settings differ from the loaded preset's. */
  edited: boolean;
}

/**
 * - `loaded`: the preset the engine has as loaded, if any.
 * - `current`: the settings now, normalized; `presetNorm` the loaded preset's (null while
 *   unknown, or when the preset is no longer on this computer).
 * - `defaultNorm`: the default preset's ("Default", "Flat"), which names unnamed settings that
 *   match it.
 */
export function presetBoxLabel(loaded: PresetRef | null | undefined, current: string, presetNorm: string | null, defaultNorm: string | null, defaultName: string): PresetBoxLabel {
  if (loaded) return { label: loaded.name, edited: presetNorm !== null && presetNorm !== current };
  return { label: defaultNorm !== null && current === defaultNorm ? defaultName : "Manual", edited: false };
}
