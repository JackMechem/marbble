// Which global tracks show under the ruler (View › Show Global Tracks, G), as Logic's Global
// Tracks configuration: each of Arrangement, Signature and Tempo can be shown or hidden on its
// own, from the global tracks' header menu or View › Global Tracks. A project with no choice of
// its own shows the ones it uses, so one that uses only the Arrangement track gives the rest of
// the room to the tracks. (The marker lane is part of the ruler and always shows.) Pure, so it
// can be unit tested.

/** A global track that can be shown or hidden, top to bottom. */
export type GlobalTrackId = "arrangement" | "signature" | "tempo";

export const GLOBAL_TRACKS: readonly GlobalTrackId[] = ["arrangement", "signature", "tempo"];

export const GLOBAL_TRACK_LABELS: Record<GlobalTrackId, string> = { arrangement: "Arrangement", signature: "Signature", tempo: "Tempo" };

/** Height of the Arrangement lane. */
export const ARRANGEMENT_H = 22;
/** Height of the Signature lane. */
export const SIGNATURE_H = 20;
/** The Tempo lane's height range, and its height until it is resized. */
export const TEMPO_MIN_H = 32;
export const TEMPO_MAX_H = 240;
export const TEMPO_DEFAULT_H = 40;

/** What a project needs to say which global tracks it uses (fields of `Project`). */
export interface GlobalContent {
  arrangement?: readonly unknown[];
  tempo_map?: readonly unknown[];
  signatures?: readonly { bar: number }[];
}

/** The global tracks with something on them: arrangement sections, a tempo change (more than
 *  the project's one tempo), a signature change after bar 1. */
export function usedGlobalTracks(p: GlobalContent): GlobalTrackId[] {
  const used: Record<GlobalTrackId, boolean> = {
    arrangement: (p.arrangement?.length ?? 0) > 0,
    signature: (p.signatures ?? []).some((s) => s.bar > 1),
    tempo: (p.tempo_map?.length ?? 0) > 1,
  };
  return GLOBAL_TRACKS.filter((id) => used[id]);
}

/** The global tracks a project shows when it has no choice saved: the ones it uses, or all of
 *  them when it uses none (where there's nothing yet, G shows every track to start one on). */
export function defaultGlobalTracks(p: GlobalContent): GlobalTrackId[] {
  const used = usedGlobalTracks(p);
  return used.length ? used : [...GLOBAL_TRACKS];
}

/** The rows the shown global tracks take, in px. */
export function globalRowsHeight(shown: readonly GlobalTrackId[], tempoH: number): number {
  const h: Record<GlobalTrackId, number> = { arrangement: ARRANGEMENT_H, signature: SIGNATURE_H, tempo: tempoH };
  return GLOBAL_TRACKS.reduce((sum, id) => sum + (shown.includes(id) ? h[id] : 0), 0);
}

export const clampTempoH = (h: number) => Math.round(Math.min(TEMPO_MAX_H, Math.max(TEMPO_MIN_H, h)));

/** Whether the global tracks show (G) and which of them. */
export interface GlobalView {
  on: boolean;
  shown: GlobalTrackId[];
}

/**
 * A global track's item in the menus, clicked: a track in view is hidden, and one not in view
 * (hidden itself, or all the global tracks hidden) is shown along with the global tracks. Hiding
 * the last one hides the global tracks instead and keeps it chosen, so G brings it back.
 */
export function toggleGlobalTrack(v: GlobalView, id: GlobalTrackId): GlobalView {
  const inView = v.on && v.shown.includes(id);
  if (!inView) return { on: true, shown: GLOBAL_TRACKS.filter((x) => x === id || v.shown.includes(x)) };
  const shown = v.shown.filter((x) => x !== id);
  return shown.length ? { on: true, shown } : { on: false, shown: v.shown };
}

/** A project's saved choice: the tracks shown and the Tempo lane's height, each null when not
 *  saved (or not readable). */
export function parseGlobalView(text: string | null): { shown: GlobalTrackId[] | null; tempoH: number | null } {
  let v: unknown = null;
  try {
    v = text ? JSON.parse(text) : null;
  } catch {
    // Unreadable: the defaults.
  }
  const o = (v && typeof v === "object" ? v : {}) as { shown?: unknown; tempoH?: unknown };
  const shown = Array.isArray(o.shown) ? GLOBAL_TRACKS.filter((id) => (o.shown as unknown[]).includes(id)) : null;
  const tempoH = typeof o.tempoH === "number" && Number.isFinite(o.tempoH) ? clampTempoH(o.tempoH) : null;
  return { shown: shown && shown.length ? shown : null, tempoH };
}
