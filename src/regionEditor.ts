// Which editor a double-clicked region opens in the bottom section. Pure, so tests can import it.
import type { Region } from "./api";
import type { BottomPane } from "./store";

/**
 * The pane a double-click on `region` shows: the Step Sequencer for a pattern region, the
 * Piano Roll for a MIDI region and the Audio Track editor for an audio one (both live in the
 * "editor" pane). A MIDI region keeps the Event List if that is what the section shows, since
 * it edits the region too.
 */
export function editorPaneFor(region: Pick<Region, "content" | "pattern">, current: BottomPane): BottomPane {
  if (region.pattern) return "steps";
  if (region.content.type === "midi" && current === "list") return "list";
  return "editor";
}
