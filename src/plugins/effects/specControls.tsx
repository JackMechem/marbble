// The controls the Reverb, Delay, Modulation and Distortion panels are built from (knob, toggle,
// segmented choice and note value, each driven by its parameter's spec), tempo-synced note times,
// and the canvas helpers their pictures use. They came before the shared ones in
// plugins/panels/panelControls.tsx and follow the same conventions: drag vertically (Shift for fine),
// double-click or Option-click for the default, and touch() around every drag so Touch/Latch
// automation records it.
import { useLayoutEffect, useRef } from "react";
import type { FxPanelProps, FxParamSpec } from "../builtinEffects";
import { openMenuAt } from "../../ui/Menu";
import { pixelRatio } from "../../ui/uiScale";
import { useStore } from "../../state/store";
import { sliderAria } from "../../keyboard/sliderKeys";
import { sliderKeyDown } from "../../ui/Controls";
import { onPress } from "../../keyboard/keyNav";
import { dragWith } from "../../ui/pointerDrag";

/** Tempo-sync note values, mirroring NOTE_LABELS in crates/engine/src/fx/delay/. */
const NOTE_LABELS = [
  "1/1", "1/1.", "1/1T", "1/2", "1/2.", "1/2T", "1/4", "1/4.", "1/4T",
  "1/8", "1/8.", "1/8T", "1/16", "1/16.", "1/16T", "1/32", "1/32.", "1/32T",
];

/** A note value's length in ms at a tempo (see `note_ms` in the engine). */
export function noteMs(index: number, tempo: number): number {
  const base = [4, 2, 1, 0.5, 0.25, 0.125][Math.min(5, Math.floor(index / 3))];
  const beats = index % 3 === 0 ? base : index % 3 === 1 ? base * 1.5 : (base * 2) / 3;
  return (beats * 60000) / Math.max(1, tempo);
}

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

function specOf(p: FxPanelProps, id: number): FxParamSpec {
  return p.info.params.find((s) => s.id === id)!;
}

/** A parameter's current value, falling back to its default. */
export function val(p: FxPanelProps, id: number): number {
  return p.values[id] ?? specOf(p, id).default;
}

function toNorm(s: FxParamSpec, v: number): number {
  if (s.log && s.min > 0) return clamp01(Math.log(v / s.min) / Math.log(s.max / s.min));
  return clamp01((v - s.min) / (s.max - s.min));
}

function fromNorm(s: FxParamSpec, n: number): number {
  if (s.log && s.min > 0) return s.min * Math.pow(s.max / s.min, n);
  return s.min + (s.max - s.min) * n;
}

export function formatMs(v: number): string {
  if (v >= 1000) return `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)} s`;
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ms`;
}

function formatValue(s: FxParamSpec, v: number): string {
  if (s.kind.type === "choice") return s.kind.labels[Math.round(v)] ?? "";
  switch (s.unit) {
    case "ms":
      return formatMs(v);
    case "hz":
      if (v >= 1000) return `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)} kHz`;
      return v < 10 ? `${v.toFixed(2)} Hz` : `${Math.round(v)} Hz`;
    case "percent":
      return `${Math.round(v * 100)} %`;
    case "db":
      return `${v.toFixed(1)} dB`;
    default:
      return v.toFixed(2);
  }
}

function arcPoint(cx: number, cy: number, r: number, deg: number): [number, number] {
  const a = (deg * Math.PI) / 180;
  return [cx + r * Math.sin(a), cy - r * Math.cos(a)];
}

function arc(cx: number, cy: number, r: number, from: number, to: number): string {
  const [x0, y0] = arcPoint(cx, cy, r, from);
  const [x1, y1] = arcPoint(cx, cy, r, to);
  return `M${x0} ${y0} A${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x1} ${y1}`;
}

/** A rotary control for one parameter. */
export function SpKnob({
  p,
  id,
  label,
  size = 38,
  format,
  disabled,
}: {
  p: FxPanelProps;
  id: number;
  label?: string;
  size?: number;
  format?: (v: number) => string;
  disabled?: boolean;
}) {
  const s = specOf(p, id);
  const v = val(p, id);
  const n = toNorm(s, v);
  const start = (e: React.MouseEvent) => {
    if (e.button !== 0 || disabled) return;
    e.preventDefault();
    if (e.altKey) {
      p.set(id, s.default);
      return;
    }
    p.touch(id, true);
    let pos = n;
    let last = e.clientY;
    const move = (ev: MouseEvent) => {
      pos = clamp01(pos + (last - ev.clientY) / (ev.shiftKey ? 900 : 180));
      last = ev.clientY;
      p.set(id, fromNorm(s, pos));
    };
    const up = () => {
      p.touch(id, false);
    };
    dragWith(move, up);
  };
  const c = size / 2;
  const r = c - 3;
  const angle = -135 + 270 * n;
  return (
    <div
      className={"sp-knob" + (size >= 56 ? " big" : "") + (disabled ? " disabled" : "")}
      title={`${s.name}: ${formatValue(s, v)}`}
      {...(disabled ? {} : sliderAria({ label: label ?? s.name, text: format ? format(v) : formatValue(s, v), min: s.min, max: s.max, now: v }))}
      onKeyDown={disabled ? undefined : sliderKeyDown(n, v, (q) => fromNorm(s, q), (x) => p.set(id, x), { onTouchStart: () => p.touch(id, true), onTouchEnd: () => p.touch(id, false) })}
    >
      <div className="sp-knob-label">{label ?? s.name}</div>
      <svg width={size} height={size} onMouseDown={start} onDoubleClick={() => !disabled && p.set(id, s.default)}>
        <path d={arc(c, c, r, -135, 135)} className="sp-knob-track" />
        {n > 0.003 && <path d={arc(c, c, r, -135, angle)} className="sp-knob-arc" />}
        <circle cx={c} cy={c} r={r - 4} className="knob-body" />
        <line x1={c} y1={c} x2={arcPoint(c, c, r - 5, angle)[0]} y2={arcPoint(c, c, r - 5, angle)[1]} className="knob-pointer" />
      </svg>
      <div className="sp-knob-value">{format ? format(v) : formatValue(s, v)}</div>
    </div>
  );
}

/** An on/off button for a toggle parameter. */
export function SpToggle({ p, id, label, title }: { p: FxPanelProps; id: number; label?: string; title?: string }) {
  const on = val(p, id) >= 0.5;
  return (
    <button type="button" className={"sp-toggle" + (on ? " on" : "")} aria-pressed={on} title={title} onClick={() => p.set(id, on ? 0 : 1)}>
      {label ?? specOf(p, id).name}
    </button>
  );
}

/** A segmented control for a choice parameter. */
export function SpSegments({ p, id }: { p: FxPanelProps; id: number }) {
  const s = specOf(p, id);
  const labels = s.kind.type === "choice" ? s.kind.labels : [];
  const v = Math.round(val(p, id));
  return (
    <div className="segmented sp-segments">
      {labels.map((label, i) => (
        <button type="button" key={label} className={i === v ? "on" : ""} aria-pressed={i === v} onClick={() => p.set(id, i)}>
          {label}
        </button>
      ))}
    </div>
  );
}

/** A note-value pop-up for a synced time, showing what it comes to at the project tempo. */
export function SpNote({ p, id, label }: { p: FxPanelProps; id: number; label: string }) {
  const tempo = useStore((s) => s.snap?.project.tempo ?? 120);
  const v = Math.round(val(p, id));
  return (
    <div className="sp-knob sp-note-wrap">
      <div className="sp-knob-label">{label}</div>
      <button
        type="button"
        className="sp-note"
        {...onPress((e) => {
          e.preventDefault();
          e.stopPropagation();
          openMenuAt(
            e.currentTarget,
            NOTE_LABELS.flatMap((l, i) => [
              ...(i > 0 && i % 3 === 0 ? [{ separator: true }] : []),
              { label: l, hint: formatMs(noteMs(i, tempo)), checked: i === v, onClick: () => p.set(id, i) },
            ]),
          );
        })}
      >
        {NOTE_LABELS[v]}
      </button>
      <div className="sp-knob-value">{formatMs(noteMs(v, tempo))}</div>
    </div>
  );
}

export function cssVar(name: string, fallback = "#888"): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

/** Sizes a canvas to its parent at the device pixel ratio and returns a 2D context and size. */
export function fitCanvas(cv: HTMLCanvasElement): { ctx: CanvasRenderingContext2D; w: number; h: number } {
  const parent = cv.parentElement!;
  const w = parent.clientWidth;
  const h = parent.clientHeight;
  const dpr = pixelRatio();
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
    cv.style.width = `${w}px`;
    cv.style.height = `${h}px`;
  }
  const ctx = cv.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return { ctx, w, h };
}

/** Redraws a canvas when its inputs, the theme or its size change. */
export function useCanvas(draw: (cv: HTMLCanvasElement) => void, deps: unknown[]) {
  const ref = useRef<HTMLCanvasElement>(null);
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  const drawRef = useRef(draw);
  drawRef.current = draw;
  useLayoutEffect(() => {
    if (ref.current) drawRef.current(ref.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, themeId, uiScale]);
  useLayoutEffect(() => {
    const cv = ref.current;
    if (!cv?.parentElement) return;
    const ro = new ResizeObserver(() => drawRef.current(cv));
    ro.observe(cv.parentElement);
    return () => ro.disconnect();
  }, []);
  return ref;
}
