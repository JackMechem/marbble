// The region gain field in the Audio Track Editor's bar.
import { useEffect, useRef, useState } from "react";
import { type Id } from "../../engine/project";
import { type Region } from "../../engine/regions";
import { act } from "../../engine/commands";
import { coalesced } from "../../state/frameLoop";
import { formatDb } from "../../lib/decibels";

/** Region gain in the local bar: drag up or down (Shift for fine steps), scroll, or
 *  double-click for 0 dB. A drag is one undo step. */
export function GainField({ region }: { region: Region | null }) {
  const audio = region?.content.type === "audio" ? region.content : null;
  const db = audio?.gain_db ?? 0;
  const want = useRef<{ id: Id; db: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const [send] = useState(() =>
    coalesced(() => {
      const w = want.current;
      // Once the store has the value, the shown one is current again.
      if (w) act("set_region_gain", { ids: [w.id], db: w.db, delta: null }).then(() => want.current === w && (want.current = null));
    }),
  );
  const set = (v: number) => {
    if (!region) return;
    want.current = { id: region.id, db: Math.round(Math.min(30, Math.max(-96, v)) * 10) / 10 };
    send();
  };
  // The wheel steps from the value last asked for, not the one shown: fast ticks arrive before
  // the store catches up. A native listener, because React's wheel events are passive and
  // can't stop the pane from scrolling.
  const live = useRef({ region, db, set });
  live.current = { region, db, set };
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      const { region, db, set } = live.current;
      if (region?.content.type !== "audio") return;
      e.preventDefault();
      const from = want.current?.id === region.id ? want.current.db : db;
      set(from + (e.deltaY < 0 ? 0.5 : -0.5));
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, []);
  return (
    <div
      ref={ref}
      className={"aed-gain" + (audio ? "" : " disabled")}
      title="Region gain: drag, scroll, or double-click for 0 dB"
      onMouseDown={(e) => {
        if (!audio) return;
        e.preventDefault();
        const y0 = e.clientY;
        let v = db;
        let last = y0;
        const move = (ev: MouseEvent) => {
          v += ((last - ev.clientY) * (ev.shiftKey ? 0.05 : 0.25));
          last = ev.clientY;
          set(v);
        };
        const up = () => {
          window.removeEventListener("mousemove", move);
          window.removeEventListener("mouseup", up);
        };
        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
      }}
      onDoubleClick={() => set(0)}
    >
      {audio ? `${formatDb(db)} dB` : "—"}
    </div>
  );
}
