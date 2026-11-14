// Snapping to the Tracks area's grid: the Snap menu's mode (the store's `snapMode`) at the zoom
// in use, so a drag or click lands where the grid lines are drawn. A drag passes the keys held,
// which can make it finer or turn it off (lib/snapKeys.ts).
import type { Project } from "../engine/project";
import { snapPos } from "../lib/time";
import { snapModeWith, type SnapKeys } from "../lib/snapKeys";
import { getState, type Snap } from "../state/store";

/** The snap mode in use with `keys` held. */
export function tracksSnapMode(keys?: SnapKeys): Snap {
  return snapModeWith(getState().snapMode, keys);
}

/** Position `pos` (samples) snapped to the Tracks area's grid. */
export function snapToTracksGrid(project: Project, pos: number, keys?: SnapKeys): number {
  return snapPos(project, pos, tracksSnapMode(keys), getState().pxPerBeat);
}
