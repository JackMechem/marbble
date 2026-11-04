// Settings › Plug-ins: Logic's Plug-in Manager, with scanning and the extra folders.
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { call, type PluginDesc } from "../../api";
import { updateSettings, useSettings } from "../../prefs";
import { setState } from "../../store";
import { Foot, Row, Section } from "./parts";
import { pluginType } from "../../settingsText";

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
  const load = () => call<PluginManagerData>("plugin_manager").then((d) => d && setData(d));
  useEffect(() => {
    load();
  }, []);
  const disabled = new Set(pp.disabled);
  const list = (data?.plugins ?? []).filter((p) => !q || `${p.name} ${p.vendor} ${p.subcategories}`.toLowerCase().includes(q.toLowerCase()));
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
      <div className="pm-table">
        <div className="pm-row pm-head">
          <span>Use</span>
          <span>Name</span>
          <span>Manufacturer</span>
          <span>Type</span>
          <span>Version</span>
        </div>
        <div className="pm-body">
          {list.map((p) => (
            <div key={p.id} className={"pm-row" + (disabled.has(p.id) ? " off" : "")} title={p.path}>
              <span>
                <input type="checkbox" checked={!disabled.has(p.id)} onChange={(e) => setDisabled([p.id], !e.target.checked)} />
              </span>
              <span className="pm-name">{p.name}</span>
              <span>{p.vendor || "Unknown"}</span>
              <span className="pm-type">{pluginType(p)}</span>
              <span className="dim">{p.version}</span>
            </div>
          ))}
          {data && list.length === 0 && <div className="pm-empty">{data.plugins.length ? "No matching plug-ins" : "No VST3 plug-ins found. Add a folder below and rescan."}</div>}
        </div>
      </div>
      <div className="pref-note pm-note">Plug-ins switched off are hidden from the Library and the slot menus. Projects that already use them still load them.</div>

      <Section title="Folders">
        <Row label="VST3 Folders:" note="The standard locations are always searched. Rescan after adding a folder.">
          <div className="pm-folders">
            {(data?.folders ?? []).map((f) => (
              <div key={f} className={"pm-folder" + (extra.has(f) ? "" : " std")}>
                <span title={f}>{f}</span>
                {extra.has(f) && (
                  <button className="pm-remove" title="Remove folder" onClick={() => setFolders(pp.folders.filter((x) => x !== f)).then(load)}>
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
        <Row label="Last Scan:">
          {r ? (
            <div className="pm-report">
              <span>
                {new Date(r.at * 1000).toLocaleString()} — {r.bundles} bundle{r.bundles === 1 ? "" : "s"}, {r.plugins} plug-in{r.plugins === 1 ? "" : "s"}
                {r.failed.length > 0 && (
                  <>
                    ,{" "}
                    <button className="link-btn warn-text" onClick={() => setShowFailed(!showFailed)}>
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
      <Foot close={close} note="Bundles are scanned one by one in a separate process, so a crashing plug-in can't take Marbble down.">
        <button className="push" disabled={scanning} onClick={rescan}>
          {scanning ? "Scanning…" : "Rescan Plug-ins"}
        </button>
      </Foot>
    </>
  );
}
