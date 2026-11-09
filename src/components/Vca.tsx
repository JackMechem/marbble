// VCA faders in the Mixer: the VCA strip, and the VCA slot on channel strips (and on VCA
// strips, for nesting). The pure rules are in src/vca.ts; the engine side in
// crates/engine/src/vca.rs and session/vca.rs.
import { useLayoutEffect, useRef, useState } from "react";
import { act, actMade, type Id, type Track, type Vca } from "../api";
import { useStore } from "../store";
import { formatDb } from "../time";
import { colorMenu, selectedTracks } from "../ops";
import { canNest, vcaSlotLabel } from "../vca";
import { Fader } from "./Controls";
import { openMenu, openMenuAt, type MenuItem } from "./Menu";
import { slotProps } from "../slotKeys";
import type { SlotRows } from "../stripRows";
import { BlankEq, BlankRow, BlankSlots, PlateName } from "./StripSlots";
import { METER_W, plateInk } from "../mixerLayout";
import { onPress } from "../keyNav";

const NO_VCAS: Vca[] = [];

/** The project's VCAs (a stable empty list when there are none). */
export function useVcas(): Vca[] {
  return useStore((s) => s.snap?.project.vcas) ?? NO_VCAS;
}

/** Mixer › Options › Create New VCA, or New VCA… in a VCA slot: assigns `tracks` to it. */
export function createVca(tracks: Id[] = []) {
  return actMade("create_vca", { tracks });
}

/** The pop-up behind a VCA slot: No VCA, the VCAs to pick from, and New VCA…. */
function vcaMenu(vcas: Vca[], current: Id | null, pick: (vca: Id | null) => void, create?: () => void, allowed: (v: Vca) => boolean = () => true): MenuItem[] {
  const items: MenuItem[] = [{ label: "No VCA", checked: current === null, onClick: () => pick(null) }];
  if (vcas.length) items.push({ separator: true });
  for (const v of vcas) items.push({ label: v.name, checked: v.id === current, disabled: !allowed(v), onClick: () => pick(v.id) });
  if (create) items.push({ separator: true }, { label: "New VCA…", onClick: create });
  return items;
}

function Slot({ label, assigned, title, onOpen }: { label: string; assigned: boolean; title: string; onOpen: (el: HTMLElement) => void }) {
  return (
    <div
      className={"slot static vca-slot" + (assigned ? " assigned" : "")}
      title={title}
      {...slotProps(`VCA: ${label || "none"}`, { open: onOpen }, { popup: true })}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (e.button === 0) onOpen(e.currentTarget);
      }}
    >
      {assigned ? <span className="slot-text">{label}</span> : <span className="slot-name dim">VCA</span>}
    </div>
  );
}

/** A channel strip's VCA slot. Acts on the selected strips when this one is among them. */
export function VcaSlot({ track }: { track: Track }) {
  const vcas = useVcas();
  const label = vcaSlotLabel(vcas, track.vca);
  const open = (el: HTMLElement) => {
    const sel = selectedTracks();
    const ids = sel.includes(track.id) ? sel : [track.id];
    openMenuAt(el, vcaMenu(vcas, track.vca ?? null, (vca) => act("assign_vca", { tracks: ids, vca }), () => createVca(ids)));
  };
  return <Slot label={label} assigned={!!label} title={label ? `Assigned to ${label}: its fader offsets this strip's` : "VCA: assign this strip to a VCA fader"} onOpen={open} />;
}

/**
 * A VCA strip: fader, level, mute and name. It passes no audio. Its count sits in the channel
 * strips' Input row and its own VCA slot in their VCA row; blank rows fill the rest, so its fader
 * lines up with theirs (`rows` as for ChannelStrip).
 */
export function VcaStrip({ vca, rows }: { vca: Vca; rows: SlotRows }) {
  const faderBox = useRef<HTMLDivElement>(null);
  const [faderHeight, setFaderHeight] = useState(160);
  useLayoutEffect(() => {
    const el = faderBox.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setFaderHeight(Math.max(60, Math.floor(e.contentRect.height))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const vcas = useVcas();
  const members = useStore((s) => s.snap?.project.tracks.filter((t) => t.vca === vca.id).length ?? 0);
  const [renaming, setRenaming] = useState(false);
  const update = (patch: Partial<Pick<Vca, "name" | "color" | "level_db" | "mute">>) => act("update_vca", { id: vca.id, patch });
  const parent = vcaSlotLabel(vcas, vca.parent);
  const nested = vcas.filter((v) => v.parent === vca.id).length;
  const openParent = (el: HTMLElement) =>
    openMenuAt(
      el,
      vcaMenu(vcas.filter((v) => v.id !== vca.id), vca.parent, (p) => act("set_vca_parent", { id: vca.id, parent: p }), undefined, (v) => canNest(vcas, vca.id, v.id)),
    );
  const menu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    openMenu(e.clientX, e.clientY, [
      { label: "Rename VCA", onClick: () => setRenaming(true) },
      { label: "VCA Color", children: colorMenu(vca.color, (c) => c && update({ color: c })) },
      { separator: true },
      { label: "Create New VCA", onClick: () => createVca() },
      { label: "Delete VCA", onClick: () => act("delete_vca", { id: vca.id }) },
    ]);
  };
  const count = members + nested;
  return (
    <div className="strip vca-strip" onContextMenu={menu}>
      <BlankRow />
      <div className="strip-section">
        <div className="strip-label">VCA</div>
        <div className="slot static" title={`${count ? `${count} ${count === 1 ? "strip" : "strips"}` : "No strips"}: the strips whose VCA slot is set to this VCA`}>
          <span className="slot-text">{count ? `${count} ${count === 1 ? "strip" : "strips"}` : "No strips"}</span>
        </div>
      </div>
      <BlankEq />
      <BlankSlots rows={rows.inserts} />
      <BlankSlots rows={rows.sends} />
      <BlankRow />
      <div className="strip-section">
        <Slot label={parent} assigned={!!parent} title={parent ? `Controlled by ${parent}` : "VCA: let another VCA control this one"} onOpen={openParent} />
      </div>
      <div className="strip-pan strip-blank" aria-hidden>
        <div className="auto-mode-spacer" />
        <div className="strip-icon-row" />
        <div className="strip-pan-knob" />
      </div>
      <div className="strip-readouts">
        <span className="readout">{formatDb(vca.level_db)}</span>
      </div>
      <div className="strip-fader" ref={faderBox}>
        <Fader db={vca.level_db} onChange={(db) => update({ level_db: db })} height={faderHeight} label={`${vca.name} level`} />
        <div style={{ width: METER_W }} />
      </div>
      <div className="strip-buttons">
        <span className="tbtn power ghost" />
        <div className="btn-row">
          <button
            className={"tbtn mute" + (vca.mute ? " on" : "")}
            aria-pressed={vca.mute}
            title="Mute every strip on this VCA"
            aria-label="Mute every strip on this VCA"
            {...onPress((e) => {
              e.stopPropagation();
              update({ mute: !vca.mute });
            })}
          >
            M
          </button>
          <span className="tbtn ghost" />
        </div>
        <div className="btn-row">
          <span className="tbtn ghost" />
          <span className="tbtn ghost" />
        </div>
      </div>
      <div className={"strip-name ink-" + plateInk(vca.color)} style={{ "--plate": vca.color } as React.CSSProperties} title={vca.name} onDoubleClick={() => setRenaming(true)}>
        {renaming ? (
          <input
            className="vca-rename"
            autoFocus
            defaultValue={vca.name}
            onMouseDown={(e) => e.stopPropagation()}
            onBlur={(e) => {
              setRenaming(false);
              const v = e.target.value.trim();
              if (v && v !== vca.name) update({ name: v });
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              if (e.key === "Escape") setRenaming(false);
            }}
          />
        ) : (
          <PlateName name={vca.name} />
        )}
      </div>
    </div>
  );
}
