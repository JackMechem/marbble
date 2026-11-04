// How an instrument panel lays out its sections: in one row, or in a grid that fills the panel. The
// choice is kept apart for the bottom section and for a floating window, as they suit different ones.

export type SectionLayout = "row" | "grid";
export type PanelPlace = "docked" | "floating";
export type SectionLayouts = Partial<Record<PanelPlace, SectionLayout>>;

const KEY = "marbble.instSectionLayout";

/** What a place shows until it's been chosen: the bottom section is wide and short. */
export const defaultLayout = (place: PanelPlace): SectionLayout => (place === "docked" ? "row" : "grid");

export function layoutFor(saved: SectionLayouts, place: PanelPlace): SectionLayout {
  return saved[place] ?? defaultLayout(place);
}

/** Reads the remembered choices, dropping anything malformed. */
export function parseLayouts(text: string | null): SectionLayouts {
  const out: SectionLayouts = {};
  if (!text) return out;
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return out;
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) return out;
  for (const place of ["docked", "floating"] as const) {
    const l = (v as Record<string, unknown>)[place];
    if (l === "row" || l === "grid") out[place] = l;
  }
  return out;
}

export function savedLayout(place: PanelPlace): SectionLayout {
  try {
    return layoutFor(parseLayouts(localStorage.getItem(KEY)), place);
  } catch {
    return defaultLayout(place);
  }
}

export function saveLayout(place: PanelPlace, layout: SectionLayout) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...parseLayouts(localStorage.getItem(KEY)), [place]: layout }));
  } catch {
    // Not critical: the panel opens with the default next time.
  }
}

/** A scrolling box's position and sizes (an element's scroll* and client* properties). */
export interface ScrollBox {
  scrollTop: number;
  scrollLeft: number;
  clientWidth: number;
  clientHeight: number;
  scrollWidth: number;
  scrollHeight: number;
}

/**
 * Where the sections run on out of view, for the fade that says so: below the box in a grid
 * (it scrolls down), past its right edge in a row (it scrolls across). Null when the rest is in
 * view.
 */
export function moreBeyond(layout: SectionLayout, b: ScrollBox): "below" | "right" | null {
  if (layout === "grid") return b.scrollTop + b.clientHeight < b.scrollHeight - 1 ? "below" : null;
  return b.scrollLeft + b.clientWidth < b.scrollWidth - 1 ? "right" : null;
}
