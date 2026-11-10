// The selected zone's waveform in the Sampler panel: its start, end and loop points to drag, and
// the wheel to zoom into part of the sample.
import { useCallback, useEffect, useRef, useState } from "react";
import type { Id } from "../../../engine/project";
import { call } from "../../../engine/commands";
import { moveHandle, pointsOf, zoomed, type SamplerWave, type WaveHandle, type ZoneView } from "./samplerMap";
import { canvasFont } from "../../../ui/canvasText";
import { prepare, cssVar, useResize } from "./canvas";

export function ZoneWaveform({ track, zone, onChange }: { track: Id; zone: ZoneView; onChange: (z: ZoneView) => void }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [range, setRange] = useState<[number, number]>([0, zone.frames]);
  const [wave, setWave] = useState<SamplerWave | null>(null);
  const [draft, setDraft] = useState<ZoneView | null>(null);
  const grab = useRef<WaveHandle | null>(null);
  const shown = draft ?? zone;

  useEffect(() => setRange([0, zone.frames]), [zone.id, zone.frames]);
  useEffect(() => {
    let live = true;
    const width = Math.max(64, Math.round(ref.current?.getBoundingClientRect().width ?? 500));
    if (zone.error || !zone.frames) {
      setWave(null);
      return;
    }
    call<SamplerWave>("sampler_waveform", { track, zone: zone.id, from: range[0], to: range[1], points: width }).then((w) => live && setWave(w));
    return () => {
      live = false;
    };
  }, [track, zone.id, zone.file, zone.error, zone.frames, range]);

  const draw = useCallback(() => {
    const c = ref.current;
    const p = c && prepare(c);
    if (!c || !p) return;
    const { ctx, w, h } = p;
    const accent = cssVar("--accent") || "#4c9eff";
    ctx.fillStyle = cssVar("--panel-2") || "#2c2c2f";
    ctx.fillRect(0, 0, w, h);
    if (shown.error) {
      ctx.fillStyle = "#e0695e";
      ctx.font = canvasFont("md");
      ctx.fillText(`${shown.file}: ${shown.error}`, 10, h / 2, w - 20);
      return;
    }
    if (!wave) return;
    const [from, to] = range;
    const xOf = (frame: number) => ((frame - from) / Math.max(1, to - from)) * w;
    const pts = pointsOf(shown);
    // The loop's region, and its crossfade going in.
    if (shown.loop_on) {
      const a = xOf(pts.loop_start);
      const b = xOf(pts.loop_end);
      ctx.fillStyle = "rgba(76,190,120,0.18)";
      ctx.fillRect(a, 0, b - a, h);
      const x = Math.min(shown.xfade, pts.loop_start, pts.loop_end - pts.loop_start);
      if (x > 0) {
        ctx.fillStyle = "rgba(76,190,120,0.35)";
        ctx.beginPath();
        ctx.moveTo(xOf(pts.loop_end - x), h);
        ctx.lineTo(xOf(pts.loop_end), 0);
        ctx.lineTo(xOf(pts.loop_end), h);
        ctx.fill();
      }
    }
    // The audio, dim outside what plays.
    const mid = h / 2;
    const n = wave.peaks.length;
    for (let i = 0; i < n; i++) {
      const [lo, hi] = wave.peaks[i];
      const x = (i / n) * w;
      const frame = from + (i / n) * (to - from);
      ctx.fillStyle = frame < pts.start || frame >= pts.end ? "rgba(150,150,155,0.35)" : accent;
      ctx.fillRect(x, mid - hi * mid, Math.max(1, w / n), Math.max(1, (hi - lo) * mid));
    }
    // Handles.
    const handle = (frame: number, label: string, color: string, top: boolean) => {
      const x = xOf(frame);
      if (x < -1 || x > w + 1) return;
      ctx.fillStyle = color;
      ctx.fillRect(x - 0.5, 0, 1.5, h);
      ctx.fillRect(x - 0.5, top ? 0 : h - 12, 12, 12);
      ctx.fillStyle = "#000";
      ctx.font = canvasFont("xs", 700);
      ctx.fillText(label, x + 2, top ? 9 : h - 3);
    };
    handle(pts.start, "S", "#f0d264", true);
    handle(pts.end, "E", "#f0d264", true);
    if (shown.loop_on) {
      handle(pts.loop_start, "L", "#5fd38d", false);
      handle(pts.loop_end, "R", "#5fd38d", false);
    }
  }, [shown, wave, range]);
  useEffect(draw, [draw]);
  useResize(ref, draw);

  const frameAt = (e: React.PointerEvent | React.WheelEvent) => {
    const r = ref.current!.getBoundingClientRect();
    return range[0] + ((e.clientX - r.left) / r.width) * (range[1] - range[0]);
  };
  return (
    <canvas
      ref={ref}
      className="smp-wave-canvas"
      title="Drag S and E for where the zone starts and ends, L and R for its loop. Scroll to zoom, double-click to see it all."
      onWheel={(e) => {
        // It zooms the waveform; the row behind it stays where it is.
        e.stopPropagation();
        e.preventDefault();
        if (zone.frames) setRange(zoomed(range[0], range[1], zone.frames, frameAt(e), e.deltaY < 0 ? 0.75 : 1.33));
      }}
      onDoubleClick={() => setRange([0, zone.frames])}
      onPointerDown={(e) => {
        const c = ref.current!;
        const r = c.getBoundingClientRect();
        const x = e.clientX - r.left;
        const xOf = (f: number) => ((f - range[0]) / Math.max(1, range[1] - range[0])) * r.width;
        const pts = pointsOf(shown);
        const candidates: [WaveHandle, number][] = [["start", pts.start], ["end", pts.end]];
        if (shown.loop_on) candidates.push(["loop_start", pts.loop_start], ["loop_end", pts.loop_end]);
        let best: WaveHandle | null = null;
        let near = 10;
        for (const [h, f] of candidates) {
          const d = Math.abs(xOf(f) - x);
          if (d < near) {
            near = d;
            best = h;
          }
        }
        grab.current = best;
        if (best) c.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (grab.current) setDraft(moveHandle(shown, grab.current, frameAt(e)));
      }}
      onPointerUp={() => {
        if (grab.current && draft) onChange(draft);
        grab.current = null;
        setDraft(null);
      }}
    />
  );
}
