// The Copy Settings and Paste Settings items of a built-in plug-in's preset menu (Presets.tsx), as
// Logic's settings menu has them, and of a VST3 slot's right-click menu with Compare. Unlike the slot
// menu's Copy Plug-in, these move settings between instances of one plug-in and keep the instance.
// Pure, so it can be unit tested; the actions are in pluginSettings.ts.
import type { Id, PluginSlot } from "../../../engine/project";
import type { CompareStates } from "../../../engine/snapshot";
import type { MenuItem } from "../../../ui/Menu";

/** A VST3's class id, the end of its plug-in id (which starts with its bundle's path); null for a built-in. */
const vstClass = (pluginId: string) => (pluginId.includes("|") ? pluginId.slice(pluginId.lastIndexOf("|") + 1).toLowerCase() : null);

/** Whether the settings on the clipboard fit an instance of `pluginId`: only the same plug-in's do,
 *  a VST3's from another computer too (its bundle elsewhere, the same class). */
export function settingsFit(clip: PluginSlot | null, pluginId: string): boolean {
  if (!clip) return false;
  const cls = vstClass(clip.plugin_id);
  return clip.plugin_id === pluginId || (cls !== null && cls === vstClass(pluginId));
}

export function settingsMenuItems(pluginId: string, clip: PluginSlot | null, copy: () => void, paste: () => void): MenuItem[] {
  return [
    { label: "Copy Settings", onClick: copy },
    { label: "Paste Settings", disabled: !settingsFit(clip, pluginId), onClick: paste },
  ];
}

/**
 * A VST3 slot's Compare, Copy Settings and Paste Settings, for its right-click menu: its editor is a
 * window of its own, with no Marbble header. Compare is ticked while the saved settings play in
 * place of the edited ones, and greyed out while the settings are as last saved.
 */
export function vstSettingsItems(
  uid: Id,
  pluginId: string,
  clip: PluginSlot | null,
  states: CompareStates | undefined,
  act: { compare: () => void; copy: () => void; paste: () => void },
): MenuItem[] {
  const comparing = !!states?.comparing.includes(uid);
  const edited = !!states?.edited.includes(uid);
  return [{ label: "Compare", checked: comparing, disabled: !edited && !comparing, onClick: act.compare }, ...settingsMenuItems(pluginId, clip, act.copy, act.paste)];
}
