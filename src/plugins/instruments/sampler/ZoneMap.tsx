// The Sampler panel's zone map: every zone as a box over a keyboard, its keys across and its
// velocities up, to select, move and resize, with files dropped on a key placed there and the keys
// below to play.
import { useCallback, useEffect, useRef, useState } from "react";
import type { Id } from "../../../engine/project";
import { api, call } from "../../../engine/commands";
import { dragZone, grabOf, keyAt, keyName, zoneAt, zoneRect, type MapBox, type ZoneGrab, type ZoneView } from "./samplerMap";
import { getState } from "../../../state/store";
import { canvasFont } from "../../../ui/canvasText";
import { prepare, cssVar, useResize } from "./canvas";

const STRIP = 18;
const isBlack = (k: number) => [1, 3, 6, 8, 10].includes(k % 12);

export function ZoneMap({ zones, selected, playing, track, onSelect, onChange }: { zones: ZoneView[]; selected: number | null; playing: number; track: Id; onSelect: (id: number) => void; onChange: (z: ZoneView, final: boolean) => void }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ zone: ZoneView; grab: ZoneGrab; x: number; y: number; moved: boolean } | null>(null);
  const held = useRef<number | null>(null);
  const [draft, setDraft] = useState<ZoneView | null>(null);

  const box = (c: HTMLCanvasElement): MapBox => {
    const r = c.getBoundingClientRect();
    return { width: r.width, height: r.height, strip: STRIP };
  };
  const draw = useCallback(() => {
    const c = ref.current;
    const p = c && prepare(c);
    if (!c || !p) return;
    const { ctx, w, h } = p;
    const b = box(c);
    const px = w / 128;
    const accent = cssVar("--accent") || "#4c9eff";
    ctx.fillStyle = cssVar("--panel-2") || "#2c2c2f";
    ctx.fillRect(0, 0, w, h - STRIP);
    // Octave lines.
    ctx.fillStyle = "rgba(128,128,128,0.25)";
    for (let k = 0; k < 128; k += 12) ctx.fillRect(k * px, 0, 1, h - STRIP);
    for (const z of zones.map((x) => (draft && x.id === draft.id ? draft : x))) {
      const r = zoneRect(b, z);
      const on = z.id === selected;
      ctx.globalAlpha = z.id === playing ? 0.95 : on ? 0.8 : 0.45;
      ctx.fillStyle = z.error ? "#c8483d" : on ? accent : "#7f8a99";
      ctx.fillRect(r.x, r.y, r.w, r.h);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = on ? "#fff" : "rgba(0,0,0,0.5)";
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, Math.max(1, r.w - 1), Math.max(1, r.h - 1));
      // The root.
      ctx.fillStyle = "#fff";
      ctx.fillRect(z.root * px, r.y, Math.max(1, px), Math.min(4, r.h));
      if (r.w > 40 && r.h > 14) {
        ctx.fillStyle = "#fff";
        ctx.font = canvasFont("xs");
        ctx.fillText(z.name, r.x + 4, r.y + 11, r.w - 8);
      }
    }
    // The keys below. A piano's colours in every theme, as the instrument keyboard.
    ctx.fillStyle = "#e4e4e8";
    ctx.fillRect(0, h - STRIP, w, STRIP);
    for (let k = 0; k < 128; k++) {
      if (isBlack(k)) {
        ctx.fillStyle = "#26272b";
        ctx.fillRect(k * px, h - STRIP, Math.max(1, px), STRIP * 0.62);
      }
      if (k % 12 === 0) {
        ctx.fillStyle = "#26272b";
        ctx.font = canvasFont("xs");
        ctx.fillRect(k * px, h - STRIP, 1, STRIP);
        if (k % 24 === 0) ctx.fillText(keyName(k).replace("♯", "#"), k * px + 2, h - 2);
      }
    }
  }, [zones, selected, playing, draft]);
  useEffect(draw, [draw]);
  useResize(ref, draw);

  const local = (e: React.PointerEvent) => {
    const r = ref.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const midi = (status: number, note: number) => call("midi_event", { data: [status, note, status === 0x90 ? 100 : 0] });
  const noteOff = () => {
    if (held.current !== null) midi(0x80, held.current);
    held.current = null;
  };
  useEffect(() => noteOff, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <canvas
      ref={ref}
      className="smp-map"
      title="Drop files on a key to place them there. Drag a zone to move it, its edges to change its keys and velocities. Play the keys below."
      onPointerDown={(e) => {
        const c = ref.current!;
        const b = box(c);
        const { x, y } = local(e);
        c.setPointerCapture(e.pointerId);
        if (y >= b.height - STRIP) {
          const k = keyAt(b, x);
          held.current = k;
          if (getState().snap?.selected !== track) api.selectTrack(track);
          midi(0x90, k);
          return;
        }
        const z = zoneAt(b, zones, x, y);
        if (!z) return;
        onSelect(z.id);
        drag.current = { zone: z, grab: grabOf(b, z, x, y), x, y, moved: false };
      }}
      onPointerMove={(e) => {
        const c = ref.current!;
        const b = box(c);
        const { x, y } = local(e);
        if (held.current !== null && e.buttons & 1 && y >= b.height - STRIP) {
          const k = keyAt(b, x);
          if (k !== held.current) {
            midi(0x90, k);
            midi(0x80, held.current);
            held.current = k;
          }
          return;
        }
        const d = drag.current;
        if (!d) return;
        const dk = Math.round(((x - d.x) / b.width) * 128);
        const dv = Math.round(((d.y - y) / (b.height - STRIP)) * 127);
        if (dk === 0 && dv === 0 && !d.moved) return;
        d.moved = true;
        setDraft(dragZone(d.zone, d.grab, dk, dv));
      }}
      onPointerUp={() => {
        noteOff();
        if (drag.current?.moved && draft) onChange(draft, true);
        drag.current = null;
        setDraft(null);
      }}
      onPointerCancel={() => {
        noteOff();
        drag.current = null;
        setDraft(null);
      }}
    />
  );
}
