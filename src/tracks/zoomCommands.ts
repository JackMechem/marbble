// View › Zoom In and Zoom Out (the menu bar's, the Tracks area's View menu and their key commands).
import { samplesPerBeat } from "../lib/time";
import { getState, setState } from "../state/store";
import { stepZoom } from "./zoom";

/** One Zoom In (`dir` 1) or Zoom Out (-1) of the Tracks area. */
export function zoomTracksStep(dir: 1 | -1) {
  const { pxPerBeat, snap } = getState();
  if (snap) setState({ pxPerBeat: stepZoom(pxPerBeat, dir, samplesPerBeat(snap.project)) });
}
