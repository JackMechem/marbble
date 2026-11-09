// The pieces that keep channel strips level with each other: the boxes their insert and send
// slots sit in (as many rows as the strips beside them, see stripRows.ts), the "+N more" row for
// filled slots past them, and blank stand-ins for sections a strip doesn't have (Stereo Out's input
// and sends, a folder's plug-ins, a VCA's everything), as tall as the real ones.
import { Icon } from "../icons";
import { openMenuAt, type MenuItem } from "./Menu";
import { LinesText } from "./PathText";
import { slotProps } from "../slotKeys";

/** A strip's box of insert or send slots: `rows` slots tall. */
export function SlotBox({ rows, children }: { rows: number; children?: React.ReactNode }) {
  return (
    <div className="strip-slots" style={{ "--rows": rows } as React.CSSProperties}>
      {children}
    </div>
  );
}

/** The last row of a strip with more filled slots than rows: "+N more", a menu of the rest
 *  (`menu`, made when it opens so it lists them as they are then). */
export function MoreSlot({ n, shown, noun, menu }: { n: number; shown: number; noun: [string, string]; menu: () => MenuItem[] }) {
  // With slots above it, "+2 more"; as the only row, "2 plug-ins".
  const text = shown > 0 ? `+${n} more` : `${n} ${noun[n === 1 ? 0 : 1]}`;
  return (
    <div
      className="slot more"
      title={`${n} more ${noun[n === 1 ? 0 : 1]} than fit: click for them`}
      {...slotProps(`${n} more ${noun[n === 1 ? 0 : 1]}`, { open: (el) => openMenuAt(el, menu()) }, { popup: true })}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (e.button === 0) openMenuAt(e.currentTarget, menu());
      }}
    >
      <span className="slot-name">{text}</span>
      <Icon.chevron />
    </div>
  );
}

/** Stand-ins for sections a strip doesn't have, as tall as the real ones, so that the rows under
 *  them stay level with the neighbouring strips'. */
export function BlankSlots({ rows }: { rows: number }) {
  return (
    <div className="strip-section strip-blank" aria-hidden>
      <div className="strip-label">&nbsp;</div>
      <SlotBox rows={rows} />
    </div>
  );
}

export function BlankRow({ label = false }: { label?: boolean }) {
  return (
    <div className="strip-section strip-blank" aria-hidden>
      {label && <div className="strip-label">&nbsp;</div>}
      <div className="slot static" />
    </div>
  );
}

export function BlankEq() {
  return (
    <div className="strip-section strip-blank" aria-hidden>
      <div className="eq-thumb" />
    </div>
  );
}

/** The button block's place on Stereo Out, which has no buttons, so its fader ends where the
 *  others' do. */
export function BlankButtons() {
  return (
    <div className="strip-buttons strip-blank" aria-hidden>
      <span className="tbtn power ghost" />
      <div className="btn-row">
        <span className="tbtn ghost" />
        <span className="tbtn ghost" />
      </div>
      <div className="btn-row">
        <span className="tbtn ghost" />
        <span className="tbtn ghost" />
      </div>
    </div>
  );
}

/** A strip's name on its name plate: up to two lines, broken at spaces only (`fitLines`), each
 *  line ending in "…" when cut (strip.css). The plate's tooltip has the whole name. */
export function PlateName({ name }: { name: string }) {
  return <LinesText text={name} className="strip-name-text" />;
}
