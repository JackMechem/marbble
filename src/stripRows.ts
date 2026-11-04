// How many insert and send slots the channel strips show. Strips side by side (the Mixer, the
// Inspector's pair) show the same number of rows, so their faders, meters, pan knobs and
// automation buttons line up across the row however many plug-ins each one has. A strip with
// more than fits scrolls its slots in their own box. Pure, so it can be unit tested.

/** The most insert rows a strip shows; a strip with more scrolls them. */
export const FX_ROWS_MAX = 6;
/** The most send rows a strip shows; a strip with more scrolls them. */
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

/** How many filled slots sit past the shown rows, for the "+N" beside the section's label. */
export function filledBeyond(slots: readonly unknown[] | undefined, rows: number): number {
  return (slots ?? []).slice(rows).filter(Boolean).length;
}

/** What a strip shows rows for: a track's inserts and sends, or Stereo Out's inserts (`kind`
 *  null). A folder has neither; Stereo Out has no sends. */
export function stripWants(kind: string | null, inserts: readonly unknown[] | undefined, sends: readonly unknown[] | undefined, sendSlots: number): SlotRows {
  if (kind === "folder") return { inserts: 0, sends: 0 };
  const fx = slotsWanted(inserts, inserts?.length ?? 0);
  return { inserts: fx, sends: kind === null ? 0 : slotsWanted(sends, sendSlots) };
}
