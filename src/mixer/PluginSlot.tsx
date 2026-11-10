// A channel strip's plug-in slot (an instrument or an Audio FX insert): its name, bypass and the
// right-click menu, dragging it to another slot, and the menu behind the Audio FX "+N more" row.
import type { Id, PluginSlot } from "../engine/project";
import { api } from "../engine/commands";
import { Icon } from "../ui/icons";
import { pluginMenu } from "../plugins/pluginMenu";
import { getState, useStore } from "../state/store";
import { openMenu, type MenuItem } from "../ui/Menu";
import { useSlotLatency } from "./Latency";
import { refuseFrozen } from "../tracks/freeze";
import { isBuiltinFx, loadPlugin, slotMenu, slotSideChainItems, toggleFxPanel } from "../plugins/fxHost";
import { keyName } from "../plugins/sideChain";
import { startSlotDrag } from "./slotDrag";
import { fxKey, usePanelShown } from "../plugins/panels/fxDockState";
import { slotName } from "./slotName";
import { slotProps } from "./slotKeys";
import { onPress } from "../keyboard/keyNav";

/** Opens a slot's plug-in: a built-in effect's panel, a VST3's editor. A frozen track's plug-ins
 *  are unloaded, and a missing VST3 or a built-in without a panel has nothing to open. */
function openSlot(trackId: Id, plugin: PluginSlot, title: string, missing: boolean) {
  if (isBuiltinFx(plugin.plugin_id)) {
    if (!refuseFrozen([trackId])) toggleFxPanel(plugin.uid);
    return;
  }
  if (missing || plugin.plugin_id.startsWith("builtin:") || refuseFrozen([trackId])) return;
  api.openEditor(plugin.uid, `${title} — ${plugin.name}`);
}

/** The menu behind an Audio FX "+N more": each plug-in in `slots` with what its slot offers,
 *  then a plug-in to add in the first free slot. */
export function moreFxMenu(trackId: Id, inserts: (PluginSlot | null)[], slots: number[], title: string): MenuItem[] {
  const missing = getState().snap?.missing ?? [];
  const items: MenuItem[] = slots.map((i) => {
    const p = inserts[i]!;
    const change = (f: () => void) => () => {
      if (!refuseFrozen([trackId])) f();
    };
    return {
      label: slotName(p.plugin_id, p.name),
      hint: p.bypass ? "Off" : undefined,
      children: [
        { label: "Open", onClick: () => openSlot(trackId, p, title, missing.includes(p.uid)) },
        { label: "Bypass", checked: p.bypass, onClick: change(() => api.setBypass(trackId, i, !p.bypass)) },
        { separator: true },
        { label: "Remove", onClick: change(() => api.removePlugin(trackId, i)) },
      ],
    };
  });
  const free = inserts.findIndex((p) => !p);
  if (free >= 0) {
    // The slot menu's list without its "No Plug-in" and the line under it.
    const add = pluginMenu(getState().plugins, { instrument: false, pick: (id) => loadPlugin(trackId, free, id), remove: () => {} }).slice(2);
    items.push({ separator: true }, { label: "Add Plug-in", children: add });
  }
  return items;
}

export function PluginSlotView({
  trackId,
  slot,
  plugin,
  instrument,
  title,
}: {
  trackId: number;
  slot: number;
  plugin: PluginSlot | null;
  instrument?: boolean;
  title: string;
}) {
  const plugins = useStore((s) => s.plugins);
  const openEditors = useStore((s) => s.snap?.open_editors) ?? [];
  const fxOpen = usePanelShown(plugin ? fxKey(plugin.uid) : null);
  useStore((s) => s.fxList);
  useStore((s) => s.instList);
  // A frozen track's plug-ins are unloaded and can't change until it is unfrozen.
  const frozen = useStore((s) => !!s.snap?.project.tracks.find((t) => t.id === trackId)?.frozen);
  const latency = useSlotLatency(plugin?.uid);
  // A VST3 that isn't installed here: the slot keeps its settings for where it is.
  const missing = useStore((s) => !!plugin && !!s.snap?.missing?.includes(plugin.uid));
  const rate = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  // A VST3 keyed by another track: built-in effects show theirs in the panel.
  const keyedBy = useStore((s) => (plugin && s.snap?.key_inputs?.includes(plugin.uid) ? keyName(s.snap.project.tracks, plugin) : null));
  const remove = () => api.removePlugin(trackId, slot);
  // The plug-in menu, beside the slot `el` is in.
  const menuBy = (el: Element) => {
    if (refuseFrozen([trackId])) return;
    const items = pluginMenu(plugins, {
      instrument: !!instrument,
      current: plugin?.plugin_id,
      pick: (id) => loadPlugin(trackId, slot, id),
      remove,
    });
    const r = el.closest(".slot")!.getBoundingClientRect();
    openMenu(r.right - 4, r.top, [...slotSideChainItems(trackId, slot, plugin), ...items]);
  };
  const menu = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.button === 0) menuBy(e.currentTarget);
  };
  // Right-click: Copy and Paste Plug-in.
  const contextAt = (x: number, y: number) => {
    if (!refuseFrozen([trackId])) openMenu(x, y, slotMenu(trackId, slot, plugin));
  };
  const context = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    contextAt(e.clientX, e.clientY);
  };
  // From the keyboard (slotKeys.ts): the menus open at the slot, Delete empties it as No Plug-in does.
  const keyed = (open: (el: HTMLElement) => void) =>
    slotProps(
      `${instrument ? "Instrument" : `Audio FX ${slot + 1}`}: ${plugin ? slotName(plugin.plugin_id, plugin.name) : "empty"}`,
      {
        open,
        menu: menuBy,
        context: (el) => {
          const r = el.getBoundingClientRect();
          contextAt(r.left, r.bottom + 2);
        },
        remove: plugin ? () => !refuseFrozen([trackId]) && remove() : undefined,
      },
      { group: !!plugin, popup: !plugin },
    );
  // Audio FX slots are drop targets for dragged plug-ins (see slotDrag.ts).
  const target = instrument ? {} : { "data-track": trackId, "data-slot": slot };
  if (!plugin) {
    return (
      <div className={"slot empty" + (instrument ? " instrument" : "")} {...target} {...keyed(menuBy)} onMouseDown={menu} onContextMenu={context} title={instrument ? "Choose an instrument" : "Add an Audio FX plug-in"}>
        {instrument ? <span className="slot-name dim">Instrument</span> : <span className="slot-add" />}
      </div>
    );
  }
  const open = openEditors.includes(plugin.uid) || fxOpen;
  return (
    <div
      className={"slot filled" + (plugin.bypass ? " bypassed" : "") + (open ? " editing" : "") + (instrument ? " instrument" : "") + (frozen ? " frozen" : "") + (missing ? " missing" : "") + (keyedBy ? " keyed" : "")}
      {...target}
      {...keyed(() => openSlot(trackId, plugin, title, missing))}
      onContextMenu={context}
    >
      <button
        className="slot-power"
        title={plugin.bypass ? "Turn on" : "Bypass"}
        aria-label="Plug-in On"
        aria-pressed={!plugin.bypass}
        {...onPress((e) => {
          e.stopPropagation();
          if (!refuseFrozen([trackId])) api.setBypass(trackId, slot, !plugin.bypass);
        })}
      >
        <Icon.power />
      </button>
      <span
        className="slot-name"
        title={missing ? `${plugin.name} — not installed on this computer; its settings are kept with the project` : `${plugin.name}${keyedBy ? ` — side chain: ${keyedBy}` : ""}${latency ? ` — latency ${latency} samples (${((latency * 1000) / rate).toFixed(1)} ms)` : ""}${frozen ? " — frozen" : instrument ? " — click to open" : " — click to open, drag to move (Alt to copy)"}`}
        onMouseDown={(e) => {
          e.stopPropagation();
          if (e.button !== 0) return;
          const open = () => openSlot(trackId, plugin, title, missing);
          if (instrument || frozen) open();
          else startSlotDrag(e, trackId, slot, slotName(plugin.plugin_id, plugin.name), open);
        }}
      >
        {slotName(plugin.plugin_id, plugin.name)}
      </span>
      <button className="slot-arrow" tabIndex={-1} onMouseDown={menu} aria-label="Plug-in menu">
        <Icon.chevron />
      </button>
    </div>
  );
}
