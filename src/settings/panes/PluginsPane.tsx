// Settings › Plug-ins: Logic's Plug-in Manager (switching plug-ins off, the user's own categories
// and names), with scanning and the extra folders.
import { useEffect, useState } from "react";
import { open } from "../../project/fileDialog";
import { type PluginDesc } from "../../engine/listings";
import { call } from "../../engine/commands";
import { updateSettings, useSettings } from "../appSettings";
import { setState } from "../../state/store";
import { Foot, Row, Section } from "./parts";
import { pluginType } from "../settingsText";
import { matchesQuery } from "../../plugins/pluginSearch";
import { addCategory, categoryNameProblem, deleteCategory, renameCategory, setInCategory, setName, type PluginCustom } from "../../plugins/pluginCustom";
import { askName } from "../../ui/NameSheet";
import { openMenu, openMenuAt, type MenuItem } from "../../ui/Menu";
import { Icon } from "../../ui/icons";
import { onPress } from "../../keyboard/keyNav";

/** Changes the user's categories or names. */
const setCustom = (f: (c: PluginCustom) => Partial<PluginCustom>) => updateSettings((s) => ({ ...s, plugins: { ...s.plugins, ...f(s.plugins) } }));

/** Asks for a new category's name and makes it; resolves with the name, or null when cancelled. */
async function newCategory(categories: PluginCustom["categories"]): Promise<string | null> {
  const name = await askName({ title: "New Category", ok: "Create", check: (n) => categoryNameProblem(categories, n) });
  if (name) await setCustom((c) => ({ categories: addCategory(c.categories, name) }));
  return name;
}

interface ManagedPlugin extends PluginDesc {
  path: string;
  enabled: boolean;
}

interface ScanReport {
  at: number;
  bundles: number;
  plugins: number;
  failed: { path: string; reason: string }[];
  folders: string[];
}

interface PluginManagerData {
  plugins: ManagedPlugin[];
  report: ScanReport | null;
  folders: string[];
}

export function PluginsPane({ close }: { close: () => void }) {
  const pp = useSettings((s) => s.plugins);
  const [data, setData] = useState<PluginManagerData | null>(null);
  const [q, setQ] = useState("");
  const [scanning, setScanning] = useState(false);
  const [showFailed, setShowFailed] = useState(false);
  // The category shown, or null for all plug-ins.
  const [cat, setCat] = useState<string | null>(null);
  const load = () => call<PluginManagerData>("plugin_manager").then((d) => d && setData(d));
  useEffect(() => {
    load();
  }, []);
  const disabled = new Set(pp.disabled);
  const { categories, names } = pp;
  const all = data?.plugins ?? [];
  const members = categories.find((c) => c.name === cat)?.plugins;
  const list = all.filter((p) => (!members || members.includes(p.cid)) && matchesQuery(q, names[p.cid], p.name, p.vendor, p.subcategories));
  const inCategories = (p: PluginDesc) => categories.filter((c) => c.plugins.includes(p.cid)).map((c) => c.name);
  // A plug-in's categories, to tick: each of the user's, then one to make and put it in.
  const categoryItems = (p: PluginDesc): MenuItem[] => [
    ...categories.map((c) => {
      const on = c.plugins.includes(p.cid);
      return { label: c.name, checked: on, onClick: () => setCustom((x) => ({ categories: setInCategory(x.categories, c.name, p.cid, !on) })) };
    }),
    ...(categories.length ? [{ separator: true }] : []),
    {
      label: "New Category…",
      onClick: async () => {
        const name = await newCategory(categories);
        if (name) await setCustom((x) => ({ categories: setInCategory(x.categories, name, p.cid, true) }));
      },
    },
  ];
  const rename = async (p: PluginDesc) => {
    const name = await askName({ title: "Rename Plug-in", value: names[p.cid] ?? p.name, ok: "Rename", check: () => null });
    if (name !== null) await setCustom((x) => ({ names: setName(x.names, p.cid, name, p.name) }));
  };
  // Right-click on a plug-in: Rename…, Reset Name, and its categories.
  const rowMenu = (e: React.MouseEvent, p: PluginDesc) => {
    e.preventDefault();
    openMenu(e.clientX, e.clientY, [
      { label: "Rename…", onClick: () => rename(p) },
      { label: "Reset Name", disabled: !names[p.cid], onClick: () => setCustom((x) => ({ names: setName(x.names, p.cid, "", p.name) })) },
      { separator: true },
      { label: "Categories", header: true },
      ...categoryItems(p),
    ]);
  };
  const renameCat = async (from: string) => {
    const to = await askName({ title: "Rename Category", value: from, ok: "Rename", check: (n) => categoryNameProblem(categories, n, from) });
    if (!to) return;
    await setCustom((x) => ({ categories: renameCategory(x.categories, from, to) }));
    setCat(to);
  };
  const deleteCat = (name: string) => {
    setCustom((x) => ({ categories: deleteCategory(x.categories, name) }));
    setCat(null);
  };
  const setDisabled = (ids: string[], off: boolean) => {
    const next = new Set(pp.disabled);
    for (const id of ids) {
      if (off) next.add(id);
      else next.delete(id);
    }
    updateSettings((s) => ({ ...s, plugins: { ...s.plugins, disabled: [...next] } }));
  };
  const rescan = async () => {
    setScanning(true);
    const l = await call<PluginDesc[]>("rescan_plugins");
    if (l) setState({ plugins: l });
    await load();
    setScanning(false);
  };
  const setFolders = (folders: string[]) => updateSettings((s) => ({ ...s, plugins: { ...s.plugins, folders } }));
  const extra = new Set(pp.folders);
  const r = data?.report;
  return (
    <>
      <div className="pm-toolbar">
        <input className="search pm-search" placeholder="Search plug-ins" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
        <span className="dim">
          {data ? `${data.plugins.length} plug-ins${pp.disabled.length ? `, ${data.plugins.filter((p) => disabled.has(p.id)).length} off` : ""}` : ""}
        </span>
        <span className="spacer" />
        <button className="small-btn" disabled={!list.length} onClick={() => setDisabled(list.map((p) => p.id), false)}>
          Use All
        </button>
        <button className="small-btn" disabled={!list.length} onClick={() => setDisabled(list.map((p) => p.id), true)}>
          Use None
        </button>
      </div>
      <div className="pm-main">
        <div className="pm-cats">
          <div className="pm-head pm-cats-head">Categories</div>
          <div className="pm-cats-list" role="group" aria-label="Categories">
            <button className={"pm-cat-row" + (cat === null ? " on" : "")} aria-pressed={cat === null} onClick={() => setCat(null)}>
              <span>All Plug-ins</span>
              <span className="pm-count">{all.length}</span>
            </button>
            {categories.map((c) => (
              <button
                key={c.name}
                className={"pm-cat-row" + (cat === c.name ? " on" : "")}
                aria-pressed={cat === c.name}
                onClick={() => setCat(c.name)}
                onDoubleClick={() => renameCat(c.name)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setCat(c.name);
                  openMenu(e.clientX, e.clientY, [
                    { label: "Rename…", onClick: () => renameCat(c.name) },
                    { label: "Delete", onClick: () => deleteCat(c.name) },
                  ]);
                }}
              >
                <span>{c.name}</span>
                <span className="pm-count">{all.filter((p) => c.plugins.includes(p.cid)).length}</span>
              </button>
            ))}
          </div>
          <div className="pm-cats-foot">
            <button
              className="small-btn icon"
              title="New Category"
              aria-label="New Category"
              onClick={async () => {
                const name = await newCategory(categories);
                if (name) setCat(name.trim());
              }}
            >
              <Icon.plus />
            </button>
            <button className="small-btn icon" title="Rename Category" aria-label="Rename Category" disabled={cat === null} onClick={() => cat && renameCat(cat)}>
              <Icon.pencil />
            </button>
            <button className="small-btn icon" title="Delete Category" aria-label="Delete Category" disabled={cat === null} onClick={() => cat && deleteCat(cat)}>
              <Icon.minus />
            </button>
          </div>
        </div>
        <div className="pm-table">
          <div className="pm-row pm-head">
            <span>Use</span>
            <span>Name</span>
            <span>Manufacturer</span>
            <span>Category</span>
            <span>Type</span>
            <span>Version</span>
          </div>
          <div className="pm-body">
            {list.map((p) => {
              const own = names[p.cid];
              const mine = inCategories(p);
              return (
                <div key={p.id} className={"pm-row" + (disabled.has(p.id) ? " off" : "")} title={p.path} onContextMenu={(e) => rowMenu(e, p)}>
                  <span>
                    <input type="checkbox" checked={!disabled.has(p.id)} onChange={(e) => setDisabled([p.id], !e.target.checked)} />
                  </span>
                  <span className="pm-name-cell" onDoubleClick={() => rename(p)}>
                    <span className="pm-name">{own ?? p.name}</span>
                    {own && <span className="pm-orig">{p.name}</span>}
                    <button className="pm-rename" title="Rename" aria-label={`Rename ${own ?? p.name}`} onClick={() => rename(p)}>
                      <Icon.pencil />
                    </button>
                  </span>
                  <span>{p.vendor || "Unknown"}</span>
                  <span>
                    <button className="pm-cat" title={mine.length ? `Categories: ${mine.join(", ")}` : "Categories"} aria-haspopup="menu" {...onPress((e) => openMenuAt(e.currentTarget, categoryItems(p)))}>
                      <span>{mine.length ? mine.join(", ") : "—"}</span>
                      <Icon.chevron />
                    </button>
                  </span>
                  <span className="pm-type">{pluginType(p)}</span>
                  <span className="dim">{p.version}</span>
                </div>
              );
            })}
            {data && list.length === 0 && (
              <div className="pm-empty">
                {!data.plugins.length ? "No VST3 plug-ins found. Add a folder below and rescan." : members && !q ? "No plug-ins in this category yet: add them from the Category column." : "No matching plug-ins"}
              </div>
            )}
          </div>
        </div>
      </div>
      <div className="pref-note pm-note">
        Plug-ins switched off are hidden from the Library and the slot menus. Projects that already use them still load them. Your categories come first in the slot menus and the Library; right-click a plug-in to rename it.
      </div>

      <Section title="Folders">
        <Row label="VST3 Folders:" note="The standard locations are always searched. Rescan after adding a folder.">
          <div className="pm-folders">
            {(data?.folders ?? []).map((f) => (
              <div key={f} className={"pm-folder" + (extra.has(f) ? "" : " std")}>
                <span title={f}>{f}</span>
                {extra.has(f) && (
                  <button className="pm-remove" title="Remove folder" aria-label="Remove folder" onClick={() => setFolders(pp.folders.filter((x) => x !== f)).then(load)}>
                    ×
                  </button>
                )}
              </div>
            ))}
            <div className="row-inline">
              <button
                className="push"
                onClick={async () => {
                  const dir = await open({ directory: true, title: "Add a VST3 folder" });
                  if (typeof dir === "string" && !pp.folders.includes(dir)) await setFolders([...pp.folders, dir]).then(load);
                }}
              >
                Add Folder…
              </button>
            </div>
          </div>
        </Row>
        <Row label="Last Scan:" note="Bundles are scanned one by one in a separate process, so a crashing plug-in can't take Marbble down.">
          {r ? (
            <div className="pm-report">
              <span>
                {new Date(r.at * 1000).toLocaleString()} — {r.bundles} bundle{r.bundles === 1 ? "" : "s"}, {r.plugins} plug-in{r.plugins === 1 ? "" : "s"}
                {r.failed.length > 0 && (
                  <>
                    ,{" "}
                    <button className="link-btn warn-text" aria-expanded={showFailed} onClick={() => setShowFailed(!showFailed)}>
                      {r.failed.length} failed {showFailed ? "▾" : "▸"}
                    </button>
                  </>
                )}
              </span>
              {showFailed && (
                <div className="pm-failed">
                  {r.failed.map((f) => (
                    <div key={f.path}>
                      <b title={f.path}>{f.path.split("/").pop()}</b>
                      <span className="dim">{f.reason}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <span className="dim">No scan report yet — rescan to make one.</span>
          )}
        </Row>
      </Section>
      <Foot close={close}>
        <button className="push" disabled={scanning} onClick={rescan}>
          {scanning ? "Scanning…" : "Rescan Plug-ins"}
        </button>
      </Foot>
    </>
  );
}
