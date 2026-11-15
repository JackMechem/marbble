// The Plug-in Manager's own categories and names (`PluginPrefs` in src-tauri/src/settings.rs):
// applying them to the plug-in list, and the edits the manager makes. Both are keyed by the VST3
// class id, which a rescan, or a bundle moved to another folder, keeps. Pure, so the tests can run it.
import type { PluginDesc } from "../engine/listings";

/** A category made in the Plug-in Manager: its name and its plug-ins' class ids. */
export interface PluginCategory {
  name: string;
  plugins: string[];
}

/** What the Plug-in Manager keeps of the user's own. */
export interface PluginCustom {
  categories: PluginCategory[];
  /** Names given to plug-ins, by class id. */
  names: Record<string, string>;
}

/** A plug-in under the name the user gave it; `original` is its own name when it was renamed. */
export type Named<T extends PluginDesc = PluginDesc> = T & { original?: string };

/** The plug-ins of `list` under the names the user gave them. */
export function withNames<T extends PluginDesc>(list: readonly T[], names: Record<string, string> = {}): Named<T>[] {
  return list.map((p) => {
    const name = names[p.cid];
    return name ? { ...p, name, original: p.name } : p;
  });
}

/** The user's categories, in their order, with the plug-ins of `list` in each; empty ones are left out. */
export function customGroups<T extends PluginDesc>(list: readonly T[], categories: readonly PluginCategory[] = []): { name: string; plugins: T[] }[] {
  return categories.map((c) => ({ name: c.name, plugins: list.filter((p) => c.plugins.includes(p.cid)) })).filter((g) => g.plugins.length);
}

/** Why `name` can't be a category's (empty, or another category's already), or null. `except`
 *  is the category being renamed. */
export function categoryNameProblem(categories: readonly PluginCategory[], name: string, except?: string): string | null {
  const n = name.trim();
  if (!n) return "The name can't be empty.";
  if (categories.some((c) => c.name !== except && c.name.toLowerCase() === n.toLowerCase())) return `There is already a category called “${n}”.`;
  return null;
}

/** The categories with a new, empty one at the end. */
export const addCategory = (categories: readonly PluginCategory[], name: string): PluginCategory[] => [...categories, { name: name.trim(), plugins: [] }];

/** The categories with `from` called `to`. */
export const renameCategory = (categories: readonly PluginCategory[], from: string, to: string): PluginCategory[] =>
  categories.map((c) => (c.name === from ? { ...c, name: to.trim() } : c));

/** The categories without `name` (its plug-ins stay where they are otherwise). */
export const deleteCategory = (categories: readonly PluginCategory[], name: string): PluginCategory[] => categories.filter((c) => c.name !== name);

/** The categories with plug-in `cid` put in category `name`, or taken out (`into` false). */
export function setInCategory(categories: readonly PluginCategory[], name: string, cid: string, into: boolean): PluginCategory[] {
  return categories.map((c) => {
    if (c.name !== name || c.plugins.includes(cid) === into) return c;
    return { ...c, plugins: into ? [...c.plugins, cid] : c.plugins.filter((id) => id !== cid) };
  });
}

/** The names with plug-in `cid` called `name`; an empty name, or its own (`original`), takes the custom one away. */
export function setName(names: Record<string, string>, cid: string, name: string, original: string): Record<string, string> {
  const n = name.trim();
  const next = { ...names };
  if (n && n !== original) next[cid] = n;
  else delete next[cid];
  return next;
}
