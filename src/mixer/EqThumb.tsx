// The Channel EQ thumbnail on a channel strip: the curve of the strip's first Channel EQ insert. A
// click opens or closes its panel, or inserts a Channel EQ when the strip has none.
import { useLayoutEffect, useRef, useState } from "react";
import type { ChannelEq, Id } from "../engine/project";
import { eqSections, isFlat, responseDb } from "../plugins/effects/eq/eqMath";
import { useStore } from "../state/store";
import { fxKey, usePanelShown } from "../plugins/panels/fxDockState";
import { pixelRatio } from "../ui/uiScale";
import { CHANNEL_EQ, channelEqClick, firstChannelEq } from "../plugins/fxHost";
import { valuesOf } from "../plugins/fxFormat";
import { eqFromValues } from "../plugins/effects/eq/eqParams";
import { slotProps } from "./slotKeys";
import { cssVar, xf } from "../plugins/effects/eq/EqEditor";

/** The settings of a strip's first Channel EQ insert (what its thumbnail shows), whether it is
 *  bypassed, and its instance id; null when the strip has none. */
function useFirstEq(trackId: Id): { eq: ChannelEq; bypass: boolean; uid: Id } | null {
  const first = useStore((s) => firstChannelEq(s.snap?.project, trackId)?.plugin);
  const info = useStore((s) => s.fxList.find((f) => f.id === CHANNEL_EQ));
  if (!first) return null;
  // Before the registry has loaded, the saved values (the engine writes them all) stand alone.
  const values = info ? valuesOf(info.params, first.params) : Object.fromEntries(first.params ?? []);
  return { eq: eqFromValues(values), bypass: first.bypass, uid: first.uid };
}

export function EqThumb({ trackId, name }: { trackId: Id; name: string }) {
  const first = useFirstEq(trackId);
  const eq = first?.eq;
  const on = !first?.bypass;
  const sr = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  const open = usePanelShown(first ? fxKey(first.uid) : null);
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  const ref = useRef<HTMLCanvasElement>(null);
  const [w, setW] = useState(90);
  useLayoutEffect(() => {
    const el = ref.current!.parentElement!;
    const ro = new ResizeObserver(([e]) => setW(Math.max(40, Math.floor(e.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const H = 30;
    const dpr = pixelRatio();
    cv.width = w * dpr;
    cv.height = H * dpr;
    cv.style.width = `${w}px`;
    cv.style.height = `${H}px`;
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, H);
    // No Channel EQ: just the label, which a click turns into one.
    if (!eq) return;
    const mid = H / 2;
    const scale = (H / 2 - 3) / 18;
    ctx.fillStyle = cssVar("--line");
    ctx.fillRect(0, Math.round(mid), w, 1);
    if (isFlat(eq)) return;
    const freqs = Array.from({ length: w }, (_, i) => xf(i, w));
    const db = responseDb(eqSections(eq, sr), freqs, sr).map((d) => d + eq.output_db);
    const accent = cssVar("--accent");
    ctx.beginPath();
    ctx.moveTo(0, mid);
    db.forEach((d, i) => ctx.lineTo(i, mid - Math.max(-18, Math.min(18, d)) * scale));
    ctx.lineTo(w, mid);
    ctx.closePath();
    ctx.globalAlpha = on ? 0.28 : 0.1;
    ctx.fillStyle = accent;
    ctx.fill();
    ctx.globalAlpha = on ? 1 : 0.4;
    ctx.beginPath();
    db.forEach((d, i) => (i ? ctx.lineTo : ctx.moveTo).call(ctx, i, mid - Math.max(-18, Math.min(18, d)) * scale));
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.25;
    ctx.stroke();
    ctx.globalAlpha = 1;
  }, [eq && JSON.stringify(eq), on, w, sr, themeId, uiScale]);
  const title = first ? `Channel EQ — ${name}` : `Channel EQ — ${name}: click to insert one at the top of Audio FX`;
  return (
    <div
      className={"eq-thumb" + (open ? " open" : "") + (first && !on ? " off" : "")}
      title={title}
      {...slotProps(first ? "Channel EQ" : "Channel EQ, none: insert one", { open: () => channelEqClick(trackId) })}
      aria-pressed={first ? open : undefined}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (e.button !== 0) return;
        channelEqClick(trackId);
      }}
    >
      <canvas ref={ref} />
      {(!eq || isFlat(eq)) && <span className="eq-thumb-label">EQ</span>}
    </div>
  );
}
