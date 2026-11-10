// The Copy Settings and Paste Settings items of a built-in plug-in's preset menu (Presets.tsx), as
// Logic's settings menu has them. Unlike the slot menu's Copy Plug-in, these move settings between
// instances of one plug-in and keep the instance. Pure, so it can be unit tested; the actions are
// in pluginSettings.ts.
import type { PluginSlot } from "../../../engine/project";
import type { MenuItem } from "../../../ui/Menu";

/** Whether the settings on the clipboard fit an instance of `pluginId`: only the same plug-in's do. */
export function settingsFit(clip: PluginSlot | null, pluginId: string): boolean {
  return clip?.plugin_id === pluginId;
}

export function settingsMenuItems(pluginId: string, clip: PluginSlot | null, copy: () => void, paste: () => void): MenuItem[] {
  return [
    { label: "Copy Settings", onClick: copy },
    { label: "Paste Settings", disabled: !settingsFit(clip, pluginId), onClick: paste },
  ];
}
