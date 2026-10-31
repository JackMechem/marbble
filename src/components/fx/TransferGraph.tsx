// The Compressor panel's transfer curve, drawn from the same model as the engine's.
import { useLayoutEffect, useRef } from "react";
import { useStore } from "../../store";
import { pixelRatio } from "../../uiScale";
import { ABI_SHIFT, Circuit, P, staticReduction } from "../../compressorModel";
import { cssVar } from "./dynControls";

const G_MIN = -60;
const G_MAX = 0;

export function TransferGraph({ v, level, size = 150 }: { v: (id: number) => number; level: number; size?: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  const key = [P.circuit, P.threshold, P.ratio, P.knee, P.fetRatio, P.optoMode].map(v).join(",");
  useLayoutEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const dpr = pixelRatio();
    cv.width = size * dpr;
    cv.height = size * dpr;
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size, size);
    const px = (db: number) => ((db - G_MIN) / (G_MAX - G_MIN)) * size;
    const py = (db: number) => size - px(db);
    const line = cssVar("--line");
    const faint = cssVar("--faint");
    const accent = cssVar("--accent");

    ctx.lineWidth = 1;
    for (let db = G_MIN; db <= G_MAX; db += 12) {
      ctx.strokeStyle = line;
      ctx.globalAlpha = 0.6;
      ctx.beginPath();
      ctx.moveTo(Math.round(px(db)) + 0.5, 0);
      ctx.lineTo(Math.round(px(db)) + 0.5, size);
      ctx.moveTo(0, Math.round(py(db)) + 0.5);
      ctx.lineTo(size, Math.round(py(db)) + 0.5);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    // Unity, for reference.
    ctx.setLineDash([3, 3]);
    ctx.strokeStyle = faint;
    ctx.beginPath();
    ctx.moveTo(0, size);
    ctx.lineTo(size, 0);
    ctx.stroke();
    ctx.setLineDash([]);
    // Threshold.
    const thr = v(P.threshold) - (Math.round(v(P.circuit)) === Circuit.Fet && Math.round(v(P.fetRatio)) >= 4 ? ABI_SHIFT : 0);
    ctx.fillStyle = accent;
    ctx.globalAlpha = 0.18;
    ctx.fillRect(px(thr), 0, 1, size);
    ctx.globalAlpha = 1;

    ctx.beginPath();
    for (let i = 0; i <= size; i++) {
      const x = G_MIN + (i / size) * (G_MAX - G_MIN);
      const y = x - staticReduction(v, x);
      if (i) ctx.lineTo(i, py(y));
      else ctx.moveTo(i, py(y));
    }
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.font = "9px Inter, system-ui, sans-serif";
    ctx.fillStyle = faint;
    ctx.textBaseline = "top";
    ctx.fillText("out", 4, 4);
    ctx.textBaseline = "bottom";
    ctx.textAlign = "right";
    ctx.fillText("in", size - 4, size - 3);
    ctx.textAlign = "left";
  }, [key, size, themeId, uiScale]);

  const showDot = level > G_MIN;
  const out = level - staticReduction(v, level);
  const dotX = ((Math.min(G_MAX, level) - G_MIN) / (G_MAX - G_MIN)) * 100;
  const dotY = ((Math.min(G_MAX, Math.max(G_MIN, out)) - G_MIN) / (G_MAX - G_MIN)) * 100;
  return (
    <div className="dyn-graph" style={{ width: size, height: size }} title="Transfer curve: output level against input level">
      <canvas ref={ref} style={{ width: size, height: size }} />
      {showDot && <div className="dyn-graph-dot" style={{ left: `${dotX}%`, bottom: `${dotY}%` }} />}
    </div>
  );
}
