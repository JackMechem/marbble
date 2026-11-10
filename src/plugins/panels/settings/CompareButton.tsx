// The Compare button in a built-in plug-in's panel header (PanelHeader.tsx, after the preset box):
// dim while the settings are as last loaded or saved, lit once they're edited, pressed while it
// plays the saved ones in their place. A press swaps; see compareModel.ts.
import { useEffect } from "react";
import type { Id, PluginSlot } from "../../../engine/project";
import { useStore } from "../../../state/store";
import type { FxInfo } from "../../builtinEffects";
import { compareView, needsCapture, settingsKey } from "./compareModel";
import { rememberSaved, toggleCompare } from "./pluginSettings";

export function CompareButton({ uid, plugin, info }: { uid: Id; plugin: PluginSlot; info: FxInfo }) {
  const rec = useStore((s) => s.pluginCompare[uid]);
  const path = useStore((s) => s.snap?.path ?? "");
  useEffect(() => {
    if (needsCapture(rec, plugin.plugin_id, path)) rememberSaved(uid);
  }, [uid, plugin.plugin_id, path, rec]);

  const { changed, comparing } = compareView(rec, settingsKey(plugin, info.params));
  const label = comparing ? "Compare: back to the edited settings" : changed ? "Compare: hear the settings as last loaded or saved" : "Compare (the settings are as last loaded or saved)";
  return (
    <button
      className={"fx-compare" + (changed ? " edited" : "") + (comparing ? " on" : "")}
      title={label}
      aria-label="Compare"
      aria-pressed={comparing}
      disabled={!changed && !comparing}
      onClick={() => toggleCompare(uid)}
    >
      <CompareGlyph />
    </button>
  );
}

/** Two settings side by side: a circle, half of it filled. */
function CompareGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
      <circle cx="7" cy="7" r="5.5" />
      <path d="M7 1.5a5.5 5.5 0 0 0 0 11z" fill="currentColor" stroke="none" />
    </svg>
  );
}
