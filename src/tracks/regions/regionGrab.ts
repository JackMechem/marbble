// What a press on a region in the Tracks area grabs. Pure and tested in tests/tracks/regions/regionGrab.test.ts;
// tracks/canvas/pointer.ts passes the point in.
import type { Id } from "../../engine/project";
import type { Region } from "../../engine/regions";
import { fadeHandles, type RegionCrossfades } from "./fades.ts";
import { isLooped, regionEnd } from "./loops.ts";
import { FADE_ZONE, fadeZone, regionBar } from "./regionBar.ts";

export type GrabKind = "move" | "left" | "right" | "fadeIn" | "fadeOut" | "loop";

/** A press on a region: content x, y within the lane, the lane's height, and whether it's on a loop
 *  repetition rather than the body. */
export interface RegionPress {
  region?: Region;
  x: number;
  localY: number;
  laneH: number;
  inLoop: boolean;
}

  /** What a press at `h` on a region grabs: the loop (the name bar at the region's or its loop's
   *  right end, Logic's loop tool), an edge to trim (lower part), a fade handle (the top of an
   *  audio region's body, at its edges or current fade points), or the region. A press on a
   *  loop repetition grabs the region. */
export function grabKind(h: RegionPress, pps: number, crossfades: (id: Id) => RegionCrossfades | undefined): GrabKind {
  const r = h.region!;
  const rx = r.start * pps;
  const rw = r.length * pps;
  // Trim zones are 8 px wide, or a quarter of a narrow region so its middle still moves it.
  const edge = Math.min(8, rw / 4);
  // The name bar (see regionBar; regions are drawn 1 px into the lane and 3 px shorter) and
  // the lane margin above it: the loop handle is up there, trims and fades below.
  // A region drawn as its bar alone shows no fades, so it has no fade handles either.
  const { barOnly, bodyTop, bodyH } = regionBar(h.laneH - 3);
  const zone = barOnly ? 0 : fadeZone(bodyH, FADE_ZONE);
  const looped = isLooped(r);
  const endX = regionEnd(r) * pps;
  if (h.localY <= bodyTop && h.x > endX - (looped ? Math.min(8, (endX - rx - rw) / 2) : edge)) return "loop";
  if (h.inLoop) return "move";
  if (r.content.type === "audio" && h.localY > bodyTop && h.localY < bodyTop + zone) {
    // A crossfade's handle is at its start; a region crossfaded into has no fade-in handle.
    const hd = fadeHandles(r.start, r.length, r.content.fade_in, r.content.fade_out, crossfades(r.id));
    const fiX = rx + r.content.fade_in * pps;
    const foX = rx + hd.fadeOut * pps;
    // How close to a fade point counts as grabbing it, again shrinking on narrow regions.
    const near = Math.min(7, rw / 4);
    if (hd.fadeIn !== null && (Math.abs(h.x - fiX) <= near || (r.content.fade_in === 0 && h.x < rx + edge))) return "fadeIn";
    // A looping region's fade-out is at the loop end; set it from the menu or inspector.
    if (!looped && (Math.abs(h.x - foX) <= near || (r.content.fade_out === 0 && h.x > rx + rw - edge))) return "fadeOut";
  }
  if (h.localY > bodyTop && h.x > rx + rw - edge) return "right";
  if (h.localY > bodyTop && h.x < rx + edge) return "left";
  return "move";
}
