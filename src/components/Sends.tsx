// The channel strip's Sends section and Output slot.
import { invoke } from "@tauri-apps/api/core";
// The channel strip's Sends section and Output slot, laid out like Logic's.
import { useRef, useState } from "react";
import { SEND_SLOTS, type Send, type Track } from "../api";
import { defaultOutput, emptySendMenu, outputLabel, outputMenu, sendLabel, sendMenu, setSendLevel } from "../routing";
import { getState, useStore } from "../store";
import { dbToFader, faderToDb, formatDb } from "../time";
import { openMenuAt, type MenuItem } from "./Menu";
import { learnContext } from "../learn";
import { sliderAria } from "../sliderKeys";
import { sliderKeyDown } from "./Controls";
import { slotLayout } from "../stripRows";
import { MoreSlot, SlotBox } from "./StripSlots";

/** A small unipolar level knob, on the fader's taper. Shift-drag adjusts finely; double-click
 *  is 0 dB. */
function SendKnob({ db, pre, onChange, onDrag }: { db: number; pre: boolean; onChange: (db: number) => void; onDrag: (on: boolean) => void }) {
  // The unrounded knob position. A drag adds to it rather than to the rounded dB it reports,
  // so slow movements still add up.
  const pos = useRef(dbToFader(db));
  pos.current = dbToFader(db);
  const size = 16;
  const r = size / 2 - 2;
  const start = (e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    let last = e.clientY;
    onDrag(true);
    const move = (ev: MouseEvent) => {
      pos.current = Math.min(1, Math.max(0, pos.current + (last - ev.clientY) / (ev.shiftKey ? 600 : 120)));
      last = ev.clientY;
      onChange(Math.round(faderToDb(pos.current) * 10) / 10);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      onDrag(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  // -135° (off) .. +135° (+6 dB), clockwise from the top.
  const angle = -135 + dbToFader(db) * 270;
  const at = (a: number, rad = r) => {
    const t = ((a - 90) * Math.PI) / 180;
    return [size / 2 + rad * Math.cos(t), size / 2 + rad * Math.sin(t)];
  };
  const [x0, y0] = at(-135);
  const [x1, y1] = at(angle);
  const [px, py] = at(angle, r - 2.5);
  return (
    <div
      className={"send-knob" + (pre ? " pre" : "")}
      {...sliderAria({ label: "Send level", text: db <= -96 ? "minus infinity dB" : `${formatDb(db)} dB`, min: -96, max: 6, now: db })}
      onKeyDown={sliderKeyDown(dbToFader(db), db, (p) => Math.round(faderToDb(p) * 10) / 10, onChange, { onTouchStart: () => onDrag(true), onTouchEnd: () => onDrag(false) })}
      onMouseDown={start}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onChange(0);
      }}
      title={`Send level ${formatDb(db)} dB${pre ? " (pre fader)" : ""} — double-click for 0 dB`}
    >
      <svg width={size} height={size}>
        <circle cx={size / 2} cy={size / 2} r={r} className="send-knob-ring" />
        {angle > -134 && <path d={`M${x0} ${y0} A${r} ${r} 0 ${angle > 45 ? 1 : 0} 1 ${x1} ${y1}`} className="send-knob-arc" />}
        <circle cx={size / 2} cy={size / 2} r={r - 2} className="send-knob-body" />
        <line x1={size / 2} y1={size / 2} x2={px} y2={py} className="send-knob-pointer" />
      </svg>
    </div>
  );
}

function SendSlot({ track, slot, send }: { track: Track; slot: number; send: Send | null }) {
  const aux = useStore((s) => (send ? s.snap?.project.tracks.find((t) => t.id === send.bus) : undefined));
  const [dragging, setDragging] = useState(false);
  const menu = (e: React.MouseEvent) => {
    e.stopPropagation();
    const el = (e.currentTarget as HTMLElement).closest(".slot") as HTMLElement;
    openMenuAt(el, send ? sendMenu(track, slot, send) : emptySendMenu(track, slot));
  };
  if (!send)
    return (
      <div className="slot empty send" onMouseDown={menu} title="Add a send">
        <span className="slot-add" />
      </div>
    );
  const name = aux ? sendLabel(aux) : "—";
  return (
    <div className={"slot filled send" + (send.muted ? " muted" : "")} onContextMenu={learnContext({ type: "send", track: track.id, slot })}>
      <span className="slot-name" onMouseDown={menu} title={`${aux ? `${name} → ${aux.name}` : name}${send.pre_fader ? " · pre fader" : ""}${send.muted ? " · muted" : ""}`}>
        {dragging ? formatDb(send.level_db) : name}
      </span>
      <SendKnob db={send.level_db} pre={send.pre_fader} onChange={(db) => setSendLevel(track.id, slot, db)}
        onDrag={(on) => {
          setDragging(on);
          // Lets Touch/Latch automation know the send is held. Plain invoke, not call(): a
          // failure only costs the touch hint and isn't worth an error toast mid-drag.
          invoke("automation_touch", { track: track.id, param: { type: "send", value: slot }, touching: on }).catch(() => {});
        }}
      />
    </div>
  );
}

/** The menu behind a Sends "+N more": each send in `slots` with its slot's menu, then a send to
 *  add in the first free slot. */
function moreSendsMenu(track: Track, sends: (Send | null)[], slots: number[]): MenuItem[] {
  const tracks = getState().snap?.project.tracks ?? [];
  const items: MenuItem[] = slots.map((i) => {
    const send = sends[i]!;
    const aux = tracks.find((t) => t.id === send.bus);
    return { label: aux ? sendLabel(aux) : "—", hint: send.muted ? "Muted" : undefined, children: sendMenu(track, i, send) };
  });
  const free = sends.findIndex((s) => !s);
  if (free >= 0) items.push({ separator: true }, { label: "Add Send", children: emptySendMenu(track, free) });
  return items;
}

/** The Sends section, under Audio FX: `rows` slots (shared with the strips beside it, see
 *  stripRows.ts), the last one "+N more" when the filled ones need more. */
export function SendsSection({ track, rows }: { track: Track; rows: number }) {
  const sends = Array.from({ length: SEND_SLOTS }, (_, i) => track.sends?.[i] ?? null);
  const { shown, more } = slotLayout(sends, rows, SEND_SLOTS);
  return (
    <div className="strip-section">
      <div className="strip-label">Sends</div>
      <SlotBox rows={rows}>
        {sends.slice(0, shown).map((s, i) => (
          <SendSlot key={i} track={track} slot={i} send={s} />
        ))}
        {more.length > 0 && <MoreSlot n={more.length} shown={shown} noun={["send", "sends"]} menu={() => moreSendsMenu(track, sends, more)} />}
      </SlotBox>
    </div>
  );
}

/** The Output slot: Stereo Out, the track's summing stack, or an aux. A subtrack playing through
 *  its summing stack (no output of its own) says so, "→ Σ Drums", dashed, rather than a bus name. */
export function OutputSlot({ track }: { track: Track }) {
  const label = useStore((s) => (s.snap ? outputLabel(s.snap.project, track) : "Stereo Out"));
  const sum = useStore((s) => (s.snap && track.output === null ? defaultOutput(s.snap.project, track)?.name ?? null : null));
  return (
    <div
      className={"slot output output-pick" + (sum !== null ? " to-sum" : "")}
      onMouseDown={(e) => {
        e.stopPropagation();
        openMenuAt(e.currentTarget as HTMLElement, outputMenu(track));
      }}
      title={sum !== null ? `Output: through the summing stack “${sum}” · click to choose another` : `Output: ${label}`}
    >
      <span className="slot-name">{sum !== null ? `→ Σ ${sum}` : label}</span>
    </div>
  );
}
