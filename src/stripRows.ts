// How many insert and send slots the channel strips show. Strips side by side (the Mixer, the
// Inspector's pair) show the same number of rows, so their faders, meters, pan knobs and
// automation buttons line up across the row however many plug-ins each one has. A strip with
// more than fits shows "+N more" in its last row, a menu of the rest. Pure, so it can be unit
// tested.

/** The most insert rows a strip shows; past them, "+N more". */
export const FX_ROWS_MAX = 6;
/** The most send rows a strip shows; past them, "+N more". */
export const SEND_ROWS_MAX = 4;

export interface SlotRows {
  inserts: number;
  sends: number;
}

/** The slots one strip wants: up to its last filled one plus an empty one to add to (at least
 *  one, at most `total`). */
export function slotsWanted(slots: readonly unknown[] | undefined, total: number): number {
  let last = -1;
  (slots ?? []).forEach((s, i) => {
    if (s) last = i;
  });
  return Math.min(total, Math.max(last + 2, 1));
}

/**
 * The rows every strip in a row shows: as many as the strip that wants the most, between one and
 * the maximum. Strips without sends (Stereo Out, folders) want 0 sends and don't count.
 */
export function stripRows(wants: readonly SlotRows[]): SlotRows {
  const most = (k: keyof SlotRows, max: number) => Math.min(max, Math.max(1, ...wants.map((w) => w[k])));
  return { inserts: most("inserts", FX_ROWS_MAX), sends: most("sends", SEND_ROWS_MAX) };
}

/**
 * How a strip fills its `rows` with its slots (`total` of them): `shown` slots as they are, then,
 * when filled slots sit past the rows, a "+N more" row in the last one standing for the filled
 * slots from there on (`more`, their slot numbers). When the filled slots fill every row there is
 * no empty one to add to; "+N more" and the Library still add.
 */
export function slotLayout(slots: readonly unknown[] | undefined, rows: number, total: number): { shown: number; more: number[] } {
  const filled = (slots ?? []).flatMap((s, i) => (s ? [i] : []));
  if (filled.every((i) => i < rows)) return { shown: Math.min(rows, total), more: [] };
  const shown = Math.max(rows - 1, 0);
  return { shown, more: filled.filter((i) => i >= shown) };
}

/** What a strip shows rows for: a track's inserts and sends, or Stereo Out's inserts (`kind`
 *  null). A folder has neither; Stereo Out has no sends. */
export function stripWants(kind: string | null, inserts: readonly unknown[] | undefined, sends: readonly unknown[] | undefined, sendSlots: number): SlotRows {
  if (kind === "folder") return { inserts: 0, sends: 0 };
  const fx = slotsWanted(inserts, inserts?.length ?? 0);
  return { inserts: fx, sends: kind === null ? 0 : slotsWanted(sends, sendSlots) };
}

/** One slot row: a 20 px slot and the 2 px gap under it (strip.css, .strip-slots). */
export const SLOT_ROW_H = 22;
/** The fader height the Mixer keeps before it shows every slot row: enough to set a level. */
export const FADER_WANTED = 120;

/**
 * The rows the Mixer's strips show in a view `viewH` tall. `fixedH` is a strip's height with no
 * slot rows and its fader at its least (`faderMin`). While the fader would get less than
 * `FADER_WANTED`, rows go, sends first (the larger section first), down to one in each; a strip
 * with more filled slots than rows shows "+N more" in its last one, as it does past the maximum. Still too short with one row each: the strips scroll (see Mixer).
 */
export function fitStripRows(rows: SlotRows, viewH: number, fixedH: number, faderMin: number): SlotRows {
  let { inserts, sends } = rows;
  const fader = () => viewH - fixedH - (inserts + sends) * SLOT_ROW_H + faderMin;
  while (fader() < FADER_WANTED && (inserts > 1 || sends > 1)) {
    if (sends > 1 && sends >= inserts) sends--;
    else if (inserts > 1) inserts--;
    else sends--;
  }
  return { inserts, sends };
}
