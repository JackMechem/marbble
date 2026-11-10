// Track header geometry, kept pure so it can be tested: which layout a header uses at a given
// height, and how the volume slider with its built-in level meter maps levels to pixels.
// (No runtime imports, so the tests can load it directly; the fader taper is passed in.)

/** A fader taper: dB to a position 0..1 (`dbToFader` in lib/decibels.ts). */
export type Taper = (db: number) => number;

/**
 * How a track header lays itself out at a given height:
 * - `full`: the icon tile, the name (on two lines when tall: `nameLines`) above a row of M S R I
 *   and freeze buttons and the volume slider;
 * - `compact`: everything on one line with a smaller icon; the buttons move into a "⋯" menu,
 *   and only the ones switched on stay in the header;
 * - `tiny`: as compact without the icon, with a smaller name and controls.
 */
export type HeaderTier = "full" | "compact" | "tiny";

/** Smallest height (px) with room for the name above the button row. */
export const FULL_TIER_H = 52;
/** Smallest height (px) that still shows the track icon. */
export const COMPACT_TIER_H = 32;

export function headerTier(height: number): HeaderTier {
  if (height >= FULL_TIER_H) return "full";
  if (height >= COMPACT_TIER_H) return "compact";
  return "tiny";
}

/** Smallest full header with room for the name on two lines above the button row. */
export const TWO_LINE_H = 72;
/** What the automation controls' own line adds to a full header (TrackHeader.tsx's AUTO_ROW_H). */
export const AUTO_LINE_H = 24;

/**
 * How many lines a header's name may take: two on a full header with room for the second line
 * (and for the automation controls' line, `autoRow`, when it shows), else one. A name takes the
 * second line only when it doesn't fit on one (`fitLines` in textFit.ts).
 */
export function nameLines(height: number, autoRow: boolean): 1 | 2 {
  return height >= TWO_LINE_H + (autoRow ? AUTO_LINE_H : 0) ? 2 : 1;
}

/** The header volume slider's size at a tier, in px: the slim line it runs on (which holds the
 *  meter), the round thumb's diameter, and the height that takes presses, taller than either so
 *  the slim slider stays easy to grab. Logic's header slider is a line with a small round thumb. */
export function levelSliderSize(tier: HeaderTier): { groove: number; thumb: number; height: number } {
  return tier === "tiny" ? { groove: 3, thumb: 10, height: 14 } : { groove: 4, thumb: 12, height: 18 };
}

/** Where the slider's thumb is centred, in px from the groove's left edge, for fader position
 *  `f` (0..1). The thumb never runs past either end. */
export function thumbCenter(f: number, width: number, thumbW: number): number {
  const c = Math.min(1, Math.max(0, f));
  return thumbW / 2 + c * Math.max(0, width - thumbW);
}

/** Level below which the meter shows nothing. */
export const SLIDER_METER_FLOOR = -60;
/** Below this the meter leaves the fader taper and runs straight down to the floor. */
const BLEND_DB = -40;

/**
 * A level in dBFS as a fraction (0..1) of the slider's travel. Above -40 dB this is the
 * fader's own `taper`, so the meter lines up with the scale the thumb moves on: the track meter
 * is post-fader, so a full-scale signal reaches just the thumb, and only a hotter one passes
 * it. Below that it runs linearly down to nothing at the floor, so silence draws no stub.
 */
export function meterTravel(db: number, taper: Taper): number {
  if (!(db > SLIDER_METER_FLOOR)) return 0;
  if (db < BLEND_DB) return ((db - SLIDER_METER_FLOOR) / (BLEND_DB - SLIDER_METER_FLOOR)) * taper(BLEND_DB);
  return taper(db);
}

/** The meter scale for a slider `width` px wide with a `thumbW` px thumb: a level as a fraction
 *  of the whole groove, measured to where the thumb's centre would be at the same position. */
export function meterScale(width: number, thumbW: number, taper: Taper): (db: number) => number {
  return (db) => {
    const t = meterTravel(db, taper);
    return t > 0 && width > 0 ? Math.min(1, thumbCenter(t, width, thumbW) / width) : 0;
  };
}

/** The parts of a track header that keep their own presses: its buttons and pop-ups, the name
 *  being edited, the volume slider, the automation controls and the resize edge. */
export const HEADER_CONTROLS = "button, input, select, textarea, a, [role='slider'], [role='button'], .th-auto, .th-resize";

/** Whether a press on `target` in a track header may drag the track: anywhere that isn't one of
 *  its controls (the name, the number, empty space), as in Logic. */
export function dragsTrack(target: { closest?(selectors: string): unknown } | null): boolean {
  return !!target && typeof target.closest === "function" && !target.closest(HEADER_CONTROLS);
}

/** How far (px) the pointer moves before a press on a header becomes a drag; less is a click. */
export const REORDER_SLOP = 4;

/** Where a dragged track goes, as an index into the tracks: before the first one whose middle
 *  is below the pointer at `y` (content px, as `tops`). */
export function reorderIndex(tops: number[], heights: number[], y: number): number {
  return tops.filter((t, i) => t + heights[i] / 2 < y).length;
}
