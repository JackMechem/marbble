// Snapping to the Tracks area's grid: the Snap menu's mode (the store's `snapMode`) at the zoom
// in use, so a drag or click lands where the grid lines are drawn.
import type { Project } from "../engine/project";
import { snapPos } from "../lib/time";
import { getState } from "../state/store";

/** Position `pos` (samples) snapped to the Tracks area's grid. */
export function snapToTracksGrid(project: Project, pos: number): number {
  const { snapMode, pxPerBeat } = getState();
  return snapPos(project, pos, snapMode, pxPerBeat);
}
