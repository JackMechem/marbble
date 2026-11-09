// A region's name bar and body at a given height. Tracks can be made very short, so the bar
// shrinks on short regions to leave the waveform or notes a strip of their own, and the shortest
// regions are drawn as their name bar alone. Pure, for the tests.
import type { TypeSize } from "./canvasText.ts";

/** Regions shorter than this (px) get the slim name bar. */
export const SLIM_REGION_H = 30;
/** Regions shorter than this (px) are drawn as a bar of their colour with their name and
 *  nothing else: the body under a slim name bar would be under 15 px, too thin to read a
 *  waveform or notes in. A region is 3 px shorter than its lane, so tracks get there below
 *  trackHeader.ts' COMPACT_TIER_H, as their headers go to one line. */
export const BAR_ONLY_H = 29;

export interface RegionBar {
  /** Drawn as the name bar alone, filling the region, with the name centred and no waveform,
   *  notes or fades. `bar`, `bodyTop` and `bodyH` still divide it for presses: the loop
   *  handle at the top, the trims below. */
  barOnly: boolean;
  /** Height of the name bar at the region's top. */
  bar: number;
  /** Type-scale size of the name in the bar: the slim bar is 12 px, too short for "sm" (11 px)
   *  with room above and below, so it takes the scale's smallest; a bar-only region has room. */
  font: TypeSize;
  /** Offset of the body (waveform, notes, fades) from the region's top, and its height. */
  bodyTop: number;
  bodyH: number;
}

/** The name bar and body of a region drawn `h` px tall. */
export function regionBar(h: number): RegionBar {
  const slim = h < SLIM_REGION_H;
  const barOnly = h < BAR_ONLY_H;
  const bar = slim ? 12 : 14;
  const gap = slim ? 1 : 2;
  return { barOnly, bar, font: slim && !barOnly ? "xs" : "sm", bodyTop: bar + gap, bodyH: Math.max(0, h - bar - 2 * gap) };
}

/** Height of the band at the top of an audio region's body where its fade handles are. */
export const FADE_ZONE = 14;

/** Fade-handle band at the top of a region body (`zone` is its full size, used on tall
 *  regions): at most half the body, so the lower half still trims the region's edges. */
export function fadeZone(bodyH: number, zone: number): number {
  return Math.min(zone, Math.ceil(bodyH / 2));
}

/** Height of the name bar as drawn: the whole region when it's drawn as its bar alone. */
export const drawnBar = (g: RegionBar, h: number): number => (g.barOnly ? h : g.bar);
