// What fits in a track header, so every header in view lays out alike: the same button columns
// and the same volume slider length on every row, whatever its indent, group or buttons, and its
// contents kept at the top of a tall track. Pure, so it can be tested; the sizes restate the CSS
// (tracks-area.css, track-stacks.css) and change with it.

import type { HeaderTier } from "./headerGeometry";
import type { SwitchId } from "./headerColumns";

/** How far each stack level indents a subtrack (px). */
export const INDENT = 12;
/** The stack disclosure triangle's slot. Every header keeps one while a stack is in view, so the
 *  icons of tracks and stacks line up. */
export const DISCLOSURE_W = 12;
/** The gap between a header's parts (number, lead, icon, name, slider). */
const GAP = 6;
/** A full header's parts besides its lead and its name column: the padding either side (9 with
 *  the colour bar, and 10), the number (20) and the icon (34), with the gaps after them. */
const FIXED_W = 9 + 20 + GAP + 34 + GAP + 10;
/** A switch button's width, and the space between buttons. */
export const BUTTON_W = 20;
const BUTTON_GAP = 3;
/** Between the button row and the volume slider. */
const CONTROLS_GAP = 8;
/** The volume slider's length limits. */
export const SLIDER_MIN = 48;
export const SLIDER_MAX = 96;
/** What the automation controls need where they stand in for the slider (the mode pop-up and +). */
export const AUTO_SLIDER_MIN = 64;

/** The space before a header's icon: its stack indent, then the disclosure triangle's slot. */
export function leadWidth(depth: number, stacks: boolean): number {
  return depth * INDENT + (stacks ? DISCLOSURE_W : 0);
}

/** Where a header's icon starts, past the colour bar, the number and a `leadWidth` lead: an
 *  automation sub-lane's controls start there too. */
export function iconLeft(lead: number): number {
  return 9 + 20 + GAP + (lead > 0 ? lead + GAP : 0);
}

/** The headers' shared controls: the switch columns shown, whether the rest are in a "⋯" menu,
 *  and the volume slider's length. */
export interface ControlsFit {
  columns: SwitchId[];
  more: boolean;
  sliderW: number;
}

/**
 * The controls every full header in view shows, for a header column `width` px wide whose widest
 * lead (`leadWidth`) is `lead`. The row of `columns` switch buttons plus `extra` buttons always
 * shown (H in hide mode) comes first, at full size; the slider gets what is left, from
 * `minSlider` up to SLIDER_MAX. When that isn't enough, switches leave the row from the right
 * (Freeze, then I, R, S, M) for a "⋯" button holding all of them, rather than squeezing.
 */
export function fitControls(width: number, lead: number, columns: readonly SwitchId[], extra = 0, minSlider = SLIDER_MIN): ControlsFit {
  const main = width - FIXED_W - (lead > 0 ? lead + GAP : 0);
  const row = (buttons: number) => (buttons > 0 ? buttons * BUTTON_W + (buttons - 1) * BUTTON_GAP + CONTROLS_GAP : 0);
  const fit = (k: number): ControlsFit => {
    const more = k < columns.length;
    const room = main - row(k + extra + (more ? 1 : 0));
    return { columns: columns.slice(0, k), more, sliderW: Math.max(0, Math.min(SLIDER_MAX, Math.floor(room))) };
  };
  for (let k = columns.length; k > 0; k--) {
    const f = fit(k);
    if (f.sliderW >= minSlider) return f;
  }
  // Not even M fits: the slider takes what is left.
  return fit(0);
}

/** What a one-line header shows beside its name, for `oneLineSliderW`. */
export interface OneLineRow {
  /** Its buttons: the switches that are on (with the gaps of the shared columns), H and "⋯". */
  buttons: number;
  /** The group badge, the Track Alternatives pop-up and the automation mode button. */
  badge: boolean;
  alts: boolean;
  autoMode: boolean;
}

/** One-line headers' sizes (track-headers-compact.css): the compact tier's icon, the tiny tier's
 *  buttons, the least of the name, and the badge's, pop-up's and mode button's usual widths. */
const COMPACT_ICON = 24;
const TINY_BUTTON_W = 18;
const NAME_MIN = 44;
const BADGE_W = 26;
const ALTS_W = 40;
const AUTO_MODE_W = 44;
/** Below this a one-line header leaves its slider out (the Inspector and Mixer still have it). */
export const ONE_LINE_SLIDER_MIN = 32;

/**
 * The volume slider's length on a one-line header: the full headers' `sliderW` where the row has
 * room, shorter where its name (at the least a few letters), badge and lit buttons need it, and 0
 * (no slider) below ONE_LINE_SLIDER_MIN, so the row's parts never run into each other.
 */
export function oneLineSliderW(width: number, lead: number, tier: HeaderTier, row: OneLineRow, sliderW: number): number {
  const icon = tier === "tiny" ? 0 : COMPACT_ICON + GAP;
  const fixed = FIXED_W - (34 + GAP) + icon + (lead > 0 ? lead + GAP : 0);
  const bw = tier === "tiny" ? TINY_BUTTON_W : BUTTON_W;
  const extras = (row.badge ? BADGE_W + 4 : 0) + (row.alts ? ALTS_W + 4 : 0);
  const items = row.buttons * bw + (row.autoMode ? AUTO_MODE_W : 0);
  const gaps = Math.max(0, row.buttons + (row.autoMode ? 1 : 0) - 1) * BUTTON_GAP;
  // The name's line and the buttons, the gap between them, and the gap before the slider.
  const used = NAME_MIN + extras + (items > 0 ? GAP + items + gaps : 0) + GAP;
  const room = Math.floor(width - fixed - used);
  const w = Math.min(sliderW, room);
  return w >= ONE_LINE_SLIDER_MIN ? w : 0;
}

/** A full header's lines (px, tracks-area.css): a name line, the button row, the automation line,
 *  and the gap between lines. */
const NAME_LINE = 16;
const BUTTON_LINE = 20;
const AUTO_LINE = 20;
const LINE_GAP = 6;
/** The space above and below a full header's contents at their natural size. */
const BAND_PAD = 11;

/**
 * The height of the band at the top of a header that its contents are centred in: on a full
 * header, the height its lines need with a margin above and below (64 px for one name line, as
 * the default track height), so on a taller track they stay at the top beside the lane's
 * regions, as in Logic, rather than drift to the middle. One-line headers fill their track.
 * `own` is the track's height without its take rows.
 */
export function headerBand(own: number, tier: HeaderTier, nameLines: 1 | 2, autoRow: boolean): number {
  if (tier !== "full") return own;
  const content = nameLines * NAME_LINE + LINE_GAP + BUTTON_LINE + (autoRow ? LINE_GAP + AUTO_LINE : 0);
  return Math.min(own, content + 2 * BAND_PAD);
}
