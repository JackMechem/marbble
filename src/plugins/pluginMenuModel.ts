// The VST3 part of the plug-in slot menus (pluginMenu.ts): pure, so the tests can run it.
import type { PluginDesc } from "../engine/listings";
import type { MenuItem } from "../ui/Menu";

function category(p: PluginDesc): string {
  const parts = p.subcategories.split("|").filter((s) => s && s !== "Fx" && s !== "Instrument" && s !== "Mono" && s !== "Stereo");
  return parts[0] ?? (p.is_instrument ? "Synth" : "Other");
}

function byName(a: PluginDesc, b: PluginDesc) {
  return a.name.localeCompare(b.name, undefined, { numeric: true });
}

function vendorTree(list: PluginDesc[], pick: (id: string) => void, current?: string): MenuItem[] {
  const vendors = new Map<string, PluginDesc[]>();
  for (const p of list) {
    const v = p.vendor || "Unknown";
    if (!vendors.has(v)) vendors.set(v, []);
    vendors.get(v)!.push(p);
  }
  return [...vendors.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([vendor, ps]) => ({
      label: vendor,
      children: ps.sort(byName).map((p) => ({ label: p.name, checked: p.id === current, onClick: () => pick(p.id) })),
    }));
}

/**
 * The VST3s for a slot menu, under a "VST3" heading: one submenu per category (by manufacturer
 * within a category of more than 30), then "By Manufacturer". The heading keeps these categories
 * apart from the built-in effects' (Delay, EQ… in both). A note when there are none.
 */
export function vst3Items(pool: PluginDesc[], instrument: boolean, pick: (id: string) => void, current?: string): MenuItem[] {
  if (!pool.length) return [{ label: instrument ? "No VST3 instruments found" : "No VST3 effects found", disabled: true }];
  const cats = new Map<string, PluginDesc[]>();
  for (const p of pool) {
    const c = category(p);
    if (!cats.has(c)) cats.set(c, []);
    cats.get(c)!.push(p);
  }
  const items: MenuItem[] = [{ label: "VST3", header: true }];
  for (const [cat, ps] of [...cats.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    const children =
      ps.length > 30 ? vendorTree(ps, pick, current) : ps.sort(byName).map((p) => ({ label: p.name, hint: p.vendor, checked: p.id === current, onClick: () => pick(p.id) }));
    items.push({ label: cat, children });
  }
  items.push({ label: "By Manufacturer", children: vendorTree(pool, pick, current) });
  return items;
}
