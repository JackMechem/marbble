// Zooming the Tracks area in time: View › Zoom In and Zoom Out (the menu bar's, the Tracks area's
// View menu and their key commands), the zoom slider and the wheel, each around its anchor
// (zoomAnchor.ts) so the view neither jumps nor fights Catch.
import { samplesPerBeat } from "../lib/time";
import { currentTick, getState } from "../state/store";
import { framePos } from "../state/frameLoop";
import { viewport, zoomTo } from "./viewport";
import { stepZoom } from "./zoom";
import { zoomAnchorX } from "./zoomAnchor";

/** Zooms the Tracks area to `pxPerBeat` around its anchor: the pointer at `pointerX` (px into
 *  the view) for a wheel zoom, otherwise the playhead or the view's middle. */
export function zoomTime(pxPerBeat: number, pointerX?: number) {
  const { pxPerBeat: now, snap, catchPlayhead } = getState();
  if (!snap) return;
  const vp = viewport();
  const playheadX = (framePos() * now) / samplesPerBeat(snap.project) - vp.left;
  const playing = !!currentTick()?.playing;
  zoomTo(pxPerBeat, zoomAnchorX({ viewW: vp.width, playheadX, pointerX, catchOn: catchPlayhead, playing }));
}

/** One Zoom In (`dir` 1) or Zoom Out (-1) of the Tracks area. */
export function zoomTracksStep(dir: 1 | -1) {
  const { pxPerBeat, snap } = getState();
  if (snap) zoomTime(stepZoom(pxPerBeat, dir, samplesPerBeat(snap.project)));
}
