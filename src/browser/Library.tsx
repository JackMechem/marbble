// The Library side pane: Logic's lists patches for the selected track; this one lists the plug-ins
// to load onto it (built-in instruments or effects, then VST3s by vendor), with a search field.
import { useState } from "react";
import { BUILTIN_SYNTH, INSTRUMENT_SLOT, type Track } from "../engine/project";
import { call } from "../engine/commands";
import { setState, showToast, useStore } from "../state/store";
import { useSelectedTrack } from "../tracks/headers/TrackHeaders";
import { useListKeys } from "../keyboard/listKeys";
import { loadPlugin } from "../plugins/fxHost";

/** Logic's Library lists patches for the selected track; here it lists plug-ins to load. */
export function Library() {
  const track = useSelectedTrack();
  const plugins = useStore((s) => s.plugins);
  const [q, setQ] = useState("");
  const [rescanning, setRescanning] = useState(false);
  const isInst = track?.kind === "instrument";
  const fxList = useStore((s) => s.fxList);
  const instList = useStore((s) => s.instList);
  const instGroups = new Map<string, typeof instList>();
  for (const i of instList.filter((i) => !q || `${i.name} ${i.category} ${i.about}`.toLowerCase().includes(q.toLowerCase()))) {
    if (!instGroups.has(i.category)) instGroups.set(i.category, []);
    instGroups.get(i.category)!.push(i);
  }
  // Built-in effects first, by category, as in Logic's Library.
  const builtins = isInst ? [] : fxList.filter((f) => !q || `${f.name} ${f.category}`.toLowerCase().includes(q.toLowerCase()));
  const fxGroups = new Map<string, typeof builtins>();
  for (const f of builtins) {
    if (!fxGroups.has(f.category)) fxGroups.set(f.category, []);
    fxGroups.get(f.category)!.push(f);
  }
  const list = plugins
    .filter((p) => p.is_instrument === isInst)
    .filter((p) => !q || `${p.name} ${p.vendor} ${p.subcategories}`.toLowerCase().includes(q.toLowerCase()));
  const groups = new Map<string, typeof list>();
  for (const p of list) {
    const g = p.vendor || "Unknown";
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g)!.push(p);
  }
  const load = (id: string) => {
    if (!track) return;
    if (isInst) loadPlugin(track.id, INSTRUMENT_SLOT, id, true);
    else {
      const slot = track.inserts.findIndex((s) => !s);
      if (slot < 0) showToast("All Audio FX slots are in use");
      else loadPlugin(track.id, slot, id, true);
    }
  };
  const current = (t: Track | null) => (t?.kind === "instrument" ? t.instrument?.plugin_id : undefined);
  // The items in the order shown, under their headings: the built-in instruments (Marbble's
  // first) or effects (by category), then the plug-ins by vendor.
  const sections: { key: string; title?: string; items: { id: string; name: string; tip?: string }[] }[] = [];
  if (isInst && !instList.length) sections.push({ key: "builtin", items: [{ id: BUILTIN_SYNTH, name: "Marbble Synth" }] });
  if (isInst)
    for (const [group, is] of [...instGroups.entries()].sort((a, b) => (a[0] === "Marbble" ? -1 : b[0] === "Marbble" ? 1 : a[0].localeCompare(b[0]))))
      sections.push({ key: `inst:${group}`, title: group, items: is.map((i) => ({ id: i.id, name: i.name, tip: i.about })) });
  for (const [cat, fs] of [...fxGroups.entries()].sort((a, b) => a[0].localeCompare(b[0])))
    sections.push({ key: `fx:${cat}`, title: cat, items: fs.map((f) => ({ id: f.id, name: f.name, tip: `Marbble ${f.category}` })) });
  for (const [vendor, ps] of groups.entries()) sections.push({ key: `vendor:${vendor}`, title: vendor, items: ps.map((p) => ({ id: p.id, name: p.name, tip: p.subcategories })) });
  const items = sections.flatMap((g) => g.items);
  const on = current(track);
  // One Tab stop (listKeys.ts): Up and Down move, letters type ahead, Return loads, as a click does.
  const { listProps, rowProps } = useListKeys({ keys: items.map((i) => i.id), labels: items.map((i) => i.name), start: on, open: load });
  return (
    <div className="library">
      <div className="pane-title">Library</div>
      <input className="search" placeholder="Search" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
      {!track ? (
        <div className="insp-empty">Select a track</div>
      ) : track.kind === "folder" ? (
        // A folder stack has no channel strip of its own to put plug-ins on.
        <div className="insp-empty">Folder stacks have no plug-ins</div>
      ) : (
        <div className="lib-list" role="listbox" aria-label={isInst ? "Instruments" : "Audio FX"} {...listProps}>
          <div className="lib-group" aria-hidden>
            {isInst ? "Instruments" : "Audio FX"}
          </div>
          {sections.map((g) => (
            <div key={g.key} role="group" aria-label={g.title}>
              {g.title && (
                <div className="lib-vendor" aria-hidden>
                  {g.title}
                </div>
              )}
              {g.items.map((i) => (
                <div key={i.id} className={"lib-item" + (on === i.id ? " on" : "")} role="option" aria-selected={on === i.id} onClick={() => load(i.id)} title={i.tip} {...rowProps(i.id)}>
                  {i.name}
                </div>
              ))}
            </div>
          ))}
          {list.length === 0 && builtins.length === 0 && (!isInst || instGroups.size === 0) && <div className="insp-empty">No matching plug-ins</div>}
        </div>
      )}
      <div className="pane-foot">
        <span className="dim">{plugins.length} plug-ins</span>
        <button
          className="small-btn"
          disabled={rescanning}
          onClick={async () => {
            setRescanning(true);
            const l = await call<typeof plugins>("rescan_plugins");
            if (l) setState({ plugins: l });
            setRescanning(false);
          }}
        >
          {rescanning ? "Scanning…" : "Rescan"}
        </button>
      </div>
    </div>
  );
}
