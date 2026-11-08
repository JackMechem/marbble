// The Mixer's layout: track stacks as brackets over their strips (collapsible, following the
// stack's own open state, as Logic's Mixer does), strip widths (Narrow, Normal, Wide and a width
// per strip, remembered per project like track heights), and the colours of name plates and
// brackets. Pure, tested in tests/mixerLayout.test.ts.
import type { Id, Track } from "./api";

// ---- stacks ---------------------------------------------------------------------------------

/** A strip on its own, or inside the stacks around it. */
export interface StripEntry {
  kind: "strip";
  track: Track;
  /** How many stack brackets are above it. */
  depth: number;
}

/** A stack with members shown: its bracket, its own strip first, then its members (or, collapsed,
 *  a marker that opens it). */
export interface StackEntry {
  kind: "stack";
  track: Track;
  /** How many stack brackets are above this one's. */
  depth: number;
  /** Its subtracks in the Mixer, nested stacks among them; empty while collapsed. */
  members: MixerEntry[];
  /** How many of its subtracks (nested ones included) the Mixer would show open. */
  count: number;
  open: boolean;
}

export type MixerEntry = StripEntry | StackEntry;

/**
 * The Mixer's strips as a tree of stacks. `shown` are the strips the Mixer shows (filters and
 * hidden tracks already left out), in its order, with every stack's subtracks after it; `tracks`
 * are all of the project's, for the stacks a track is in. A track goes in the nearest stack around
 * it that is shown; a stack whose subtracks are all filtered out is a plain strip. A collapsed
 * stack keeps its count but shows none of its subtracks, nested stacks included.
 */
export function mixerEntries(tracks: readonly Track[], shown: readonly Track[]): MixerEntry[] {
  const byId = new Map(tracks.map((t) => [t.id, t]));
  const shownIds = new Set(shown.map((t) => t.id));
  // The shown stacks around a track, innermost first.
  const around = (t: Track): Track[] => {
    const out: Track[] = [];
    for (let p = t.parent, n = 0; p !== null && n < 64; n++) {
      const up = byId.get(p);
      if (!up) break;
      if (shownIds.has(up.id) && (up.kind === "folder" || up.kind === "summing")) out.push(up);
      p = up.parent;
    }
    return out;
  };
  const top: MixerEntry[] = [];
  const stacks = new Map<Id, StackEntry>();
  const counts = new Map<Id, number>();
  for (const t of shown) {
    const outer = around(t);
    for (const s of outer) counts.set(s.id, (counts.get(s.id) ?? 0) + 1);
    // Inside a collapsed stack (or one that isn't drawn): not shown.
    if (outer.some((s) => s.collapsed || !stacks.has(s.id))) continue;
    const entry: MixerEntry =
      t.kind === "folder" || t.kind === "summing"
        ? { kind: "stack", track: t, depth: outer.length, members: [], count: 0, open: !t.collapsed }
        : { kind: "strip", track: t, depth: outer.length };
    if (entry.kind === "stack") stacks.set(t.id, entry);
    if (outer.length) stacks.get(outer[0].id)!.members.push(entry);
    else top.push(entry);
  }
  // Counts are known once every track is seen; a stack with nothing to show is a plain strip.
  const settle = (list: MixerEntry[]): MixerEntry[] =>
    list.map((e) => {
      if (e.kind !== "stack") return e;
      const count = counts.get(e.track.id) ?? 0;
      return count ? { ...e, count, members: settle(e.members) } : { kind: "strip", track: e.track, depth: e.depth };
    });
  return settle(top);
}

/** The tracks whose strips are drawn, in order (not those in collapsed stacks). */
export function entryTracks(entries: readonly MixerEntry[]): Track[] {
  return entries.flatMap((e) => (e.kind === "stack" ? [e.track, ...entryTracks(e.members)] : [e.track]));
}

/** How many rows of brackets the Mixer needs above its strips: its deepest stack's. */
export function bracketRows(entries: readonly MixerEntry[]): number {
  let most = 0;
  for (const e of entries) if (e.kind === "stack") most = Math.max(most, e.depth + 1, bracketRows(e.members));
  return most;
}

/** "5 tracks", for a stack's bracket and its collapsed marker. */
export const tracksLabel = (n: number) => `${n} ${n === 1 ? "track" : "tracks"}`;

/** What a stack's bracket says after its name: its kind and how many tracks it holds. */
export const stackKindLabel = (t: Track, count: number) => `${t.kind === "summing" ? "Σ" : "Folder"} · ${tracksLabel(count)}`;

// ---- widths ---------------------------------------------------------------------------------

export type StripSize = "narrow" | "normal" | "wide";

/** The Mixer's sizes: id, label. */
export const STRIP_SIZES: [StripSize, string][] = [
  ["narrow", "Narrow"],
  ["normal", "Normal"],
  ["wide", "Wide"],
];

/** A strip's width at each size, in CSS pixels. Narrow still fits the two button columns, the
 *  fader with its meters and the automation mode. */
export const STRIP_W: Record<StripSize, number> = { narrow: 72, normal: 104, wide: 140 };
/** What a strip can be dragged to. */
export const STRIP_W_MIN = 68;
export const STRIP_W_MAX = 280;
/** A folder stack's strip has only a fader, its buttons and a name, so it stays narrow at every
 *  size (until it is dragged wider). */
export const FOLDER_W = STRIP_W.narrow;
/** Below this the strip's name plate leaves out the icon, so the name keeps the room. */
export const SLIM_W = 90;

/** A channel strip's level meter beside the fader: two lanes of 7 px (a VCA strip keeps the room,
 *  so its fader sits where theirs do). */
export const METER_W = 15;

/** The Mixer's size, and the strips given a width of their own (by `stripKey`). */
export interface MixerView {
  size: StripSize;
  widths: Record<string, number>;
}

export const DEFAULT_MIXER_VIEW: MixerView = { size: "normal", widths: {} };

/** A strip's key in `MixerView.widths`: a track, a VCA or Stereo Out (ids don't overlap). */
export const stripKey = {
  track: (id: Id) => `t${id}`,
  vca: (id: Id) => `v${id}`,
  master: "out",
};

const KEY_RE = /^(t\d+|v\d+|out)$/;

export const clampStripWidth = (w: number) => Math.round(Math.min(STRIP_W_MAX, Math.max(STRIP_W_MIN, w)));

/** A strip's width: its own, else its size's (a folder's, `folder`). */
export function stripWidth(view: MixerView, key: string, folder = false): number {
  return view.widths[key] ?? (folder ? FOLDER_W : STRIP_W[view.size]);
}

/** `view` with one strip's width set, or reset to its size's (null). */
export function withStripWidth(view: MixerView, key: string, w: number | null): MixerView {
  const widths = { ...view.widths };
  if (w === null) delete widths[key];
  else widths[key] = clampStripWidth(w);
  return { ...view, widths };
}

/** A remembered view (JSON from localStorage): an unknown size is Normal, and widths that aren't a
 *  strip's key or a number are dropped, the rest kept within limits. */
export function parseMixerView(text: string | null): MixerView {
  let v: unknown = null;
  try {
    v = text ? JSON.parse(text) : null;
  } catch {
    return DEFAULT_MIXER_VIEW;
  }
  if (!v || typeof v !== "object") return DEFAULT_MIXER_VIEW;
  const o = v as { size?: unknown; widths?: unknown };
  const size = STRIP_SIZES.some(([s]) => s === o.size) ? (o.size as StripSize) : "normal";
  const widths: Record<string, number> = {};
  if (o.widths && typeof o.widths === "object") {
    for (const [k, w] of Object.entries(o.widths)) if (KEY_RE.test(k) && typeof w === "number" && Number.isFinite(w)) widths[k] = clampStripWidth(w);
  }
  return { size, widths };
}

// ---- colours --------------------------------------------------------------------------------

/** "#rgb" or "#rrggbb" as 0-255 channels, or null. */
function rgb(hex: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as [number, number, number];
}

/** WCAG relative luminance of 0-255 channels. */
function luminance([r, g, b]: [number, number, number]): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** The text that reads best on a plate filled with `color`: dark ink (`--swatch-ink`) or light
 *  (`--swatch-ink-light`), whichever has the higher contrast. A colour that isn't hex gets light. */
export function plateInk(color: string): "dark" | "light" {
  const c = rgb(color);
  if (!c) return "light";
  const l = luminance(c);
  // Against near-black (#141416, luminance ~0.007) and white.
  return (l + 0.05) / 0.057 >= 1.05 / (l + 0.05) ? "dark" : "light";
}

/** `color` at `alpha`, for the faint tint of a stack's strips (transparent if it isn't hex). */
export function tint(color: string, alpha: number): string {
  const c = rgb(color);
  return c ? `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${alpha})` : "transparent";
}
