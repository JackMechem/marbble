// Which button columns the track headers share, so a button sits at the same place on every
// header: an instrument track leaves a gap where an audio track has its I button. Pure, so it can
// be unit tested.

/** A header switch, by its CSS class in TrackHeaders.tsx, in the order they appear. */
export type SwitchId = "power" | "mute" | "solo" | "rec" | "mon" | "freeze";

export const SWITCH_ORDER: readonly SwitchId[] = ["power", "mute", "solo", "rec", "mon", "freeze"];

/** What a header needs to know about its track. */
export interface HeaderTrack {
  kind: string;
  off?: boolean;
  mute: boolean;
  solo: boolean;
  armed: boolean;
  monitor: boolean;
  /** Truthy when frozen (the track's freeze info). */
  frozen?: unknown;
}

/** The switches a track of this kind has (canRecord and canFreeze in api.ts and freeze.ts). */
export function switchesOf(kind: string): SwitchId[] {
  const media = kind === "audio" || kind === "instrument";
  return SWITCH_ORDER.filter((s) => (s === "rec" || s === "freeze" ? media : s === "mon" ? kind === "audio" : true));
}

/** The switches that are lit on a track ("power" when the track is off). */
export function switchesOn(t: HeaderTrack): SwitchId[] {
  const on: Record<SwitchId, boolean> = { power: !!t.off, mute: t.mute, solo: t.solo, rec: t.armed, mon: t.monitor, freeze: !!t.frozen };
  return switchesOf(t.kind).filter((s) => on[s]);
}

/**
 * The columns of the button row, given the tracks in view:
 * - `full` headers show every switch their track has except power (it sits by the name), in
 *   columns for every switch any track in view has;
 * - `compact` headers (one line) show only the lit switches, in columns for every switch lit on
 *   any track in view, so a muted track's M lines up with another's.
 */
export function headerColumns(tracks: readonly HeaderTrack[]): { full: SwitchId[]; compact: SwitchId[] } {
  const any = new Set(tracks.flatMap((t) => switchesOf(t.kind)));
  const lit = new Set(tracks.flatMap((t) => switchesOn(t)));
  return {
    full: SWITCH_ORDER.filter((s) => s !== "power" && any.has(s)),
    compact: SWITCH_ORDER.filter((s) => lit.has(s)),
  };
}
