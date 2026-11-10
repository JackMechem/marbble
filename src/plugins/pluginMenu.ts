// Plug-in slot menus: built-in effects and instruments and VST3s, grouped by category.
import { BUILTIN_SYNTH } from "../engine/project";
import { type PluginDesc } from "../engine/listings";
import type { MenuItem } from "../ui/Menu";
import type { FxInfo, InstInfo } from "./builtinEffects";
import { getState } from "../state/store";
import { vst3Items } from "./pluginMenuModel";

/** The built-in effects as menu items, one submenu per category (Dynamics, Reverb, ...). */
function builtinFxItems(list: FxInfo[], pick: (id: string) => void, current?: string): MenuItem[] {
  const cats = new Map<string, FxInfo[]>();
  for (const f of list) {
    if (!cats.has(f.category)) cats.set(f.category, []);
    cats.get(f.category)!.push(f);
  }
  return [...cats.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([cat, fs]) => ({ label: cat, children: fs.sort((a, b) => a.name.localeCompare(b.name)).map((f) => ({ label: f.name, checked: f.id === current, onClick: () => pick(f.id) })) }));
}

/**
 * The built-in instruments as menu items: the house ones ("Marbble") in the menu itself, others in
 * a submenu per group (Vintage Synths, ...).
 */
function builtinInstrumentItems(list: InstInfo[], pick: (id: string) => void, current?: string): MenuItem[] {
  const item = (i: InstInfo): MenuItem => ({ label: i.name, checked: i.id === current, onClick: () => pick(i.id) });
  const groups = new Map<string, InstInfo[]>();
  for (const i of list) {
    if (!groups.has(i.category)) groups.set(i.category, []);
    groups.get(i.category)!.push(i);
  }
  const house = list.filter((i) => i.category === "Marbble").map(item);
  const others = [...groups.entries()].filter(([g]) => g !== "Marbble").sort((a, b) => a[0].localeCompare(b[0]));
  return [...house, ...others.map(([label, is]) => ({ label, children: is.map(item) }))];
}

/**
 * Builds the slot pop-up menu: "No Plug-in", the built-in plug-ins (effects under a "Marbble"
 * heading, by category), then the VST3s under a "VST3" heading: by category, and "By
 * Manufacturer". The headings keep the two sets of categories (Delay, EQ… in both) apart.
 */
export function pluginMenu(
  plugins: PluginDesc[],
  opts: { instrument: boolean; current?: string; pick: (id: string) => void; remove: () => void },
): MenuItem[] {
  const pool = plugins.filter((p) => p.is_instrument === opts.instrument);
  const items: MenuItem[] = [{ label: "No Plug-in", onClick: opts.remove, checked: !opts.current }, { separator: true }];
  if (opts.instrument) {
    // Before the list arrives, the synth every track can always have.
    const list = getState().instList;
    const fallback: MenuItem[] = [{ label: "Marbble Synth", checked: opts.current === BUILTIN_SYNTH, onClick: () => opts.pick(BUILTIN_SYNTH) }];
    items.push(...(list.length ? builtinInstrumentItems(list, opts.pick, opts.current) : fallback));
    items.push({ separator: true });
  } else if (getState().fxList.length) {
    items.push({ label: "Marbble", header: true }, ...builtinFxItems(getState().fxList, opts.pick, opts.current), { separator: true });
  }
  items.push(...vst3Items(pool, opts.instrument, opts.pick, opts.current));
  return items;
}
