// Plug-in search, shared by the Library and the slot menus' search field: which plug-ins a query
// matches, and the slot menu's list of them. Pure, so the tests can run it.
import type { MenuItem } from "../ui/Menu";

/** A plug-in as search sees it: its name, the hint shown beside it, and the other text it's found by. */
export interface Findable {
  id: string;
  name: string;
  /** Shown beside the name: a built-in's category, a VST3's manufacturer. */
  hint?: string;
  /** A tag after the name: a CLAP plug-in's format. */
  tag?: string;
  /** Searched as well as the name (manufacturer, categories, a built-in instrument's blurb). */
  keywords?: string;
}

const words = (q: string) => q.toLowerCase().split(/\s+/).filter(Boolean);

/** Whether `text` holds every word of the query `q` (any order, any case). An empty query matches. */
export function matchesQuery(q: string, ...text: (string | undefined)[]): boolean {
  const hay = text.join(" ").toLowerCase();
  return words(q).every((w) => hay.includes(w));
}

/** How well a name fits the query, best first: it starts with it, a word of it starts with it, it's elsewhere. */
function rank(name: string, q: string): number {
  const n = name.toLowerCase();
  const t = q.trim().toLowerCase();
  if (n.startsWith(t)) return 0;
  if (n.split(/[\s\-_]+/).some((w) => w.startsWith(t))) return 1;
  return 2;
}

/** The plug-ins of `list` that match `q`: those whose name fits best first, then by name. */
export function findPlugins<T extends Findable>(list: readonly T[], q: string): T[] {
  return list
    .filter((p) => matchesQuery(q, p.name, p.hint, p.keywords))
    .sort((a, b) => rank(a.name, q) - rank(b.name, q) || a.name.localeCompare(b.name, undefined, { numeric: true }));
}

/**
 * A slot menu's rows while its search field holds `q`: the matching built-in plug-ins under a
 * "Marbble" heading, then the matching VST3 and CLAP plug-ins under `heading` ("VST3" unless
 * given), each with its hint and tag; a note when nothing matches.
 */
export function searchMenuItems(builtins: readonly Findable[], hosted: readonly Findable[], q: string, pick: (id: string) => void, current?: string, heading = "VST3"): MenuItem[] {
  const row = (p: Findable): MenuItem => ({ label: p.name, tag: p.tag, hint: p.hint, checked: p.id === current, onClick: () => pick(p.id) });
  const items: MenuItem[] = [];
  for (const [title, list] of [
    ["Marbble", findPlugins(builtins, q)],
    [heading, findPlugins(hosted, q)],
  ] as const) {
    if (list.length) items.push({ label: title, header: true }, ...list.map(row));
  }
  return items.length ? items : [{ label: "No matching plug-ins", disabled: true }];
}
