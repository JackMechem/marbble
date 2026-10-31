// The numbers behind the dynamics panels' controls (Compressor, Limiter): how a value reads and where
// it sits on a knob. Pure and tested in tests/dynScale.test.ts.
import type { FxParamSpec } from "./fx";

/** Formats a value in a parameter's unit, the way Logic's plug-in windows print them. */
export function formatDyn(spec: FxParamSpec | undefined, v: number): string {
  if (!spec) return String(v);
  if (spec.kind.type === "choice") return spec.kind.labels[Math.round(v)] ?? "";
  if (spec.kind.type === "toggle") return v >= 0.5 ? "On" : "Off";
  switch (spec.unit) {
    case "db":
      return `${v > 0.05 ? "+" : ""}${v.toFixed(1)} dB`;
    case "ms":
      return formatMs(v);
    case "hz":
      return v >= 1000 ? `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)} kHz` : `${Math.round(v)} Hz`;
    case "ratio":
      return `${v >= 10 ? v.toFixed(0) : v.toFixed(1)}:1`;
    case "percent":
      return `${Math.round(v * 100)} %`;
    default:
      return v.toFixed(2);
  }
}

export function formatMs(v: number): string {
  if (v < 1) return `${Math.round(v * 1000)} µs`;
  if (v < 10) return `${v.toFixed(1)} ms`;
  if (v < 1000) return `${Math.round(v)} ms`;
  return `${(v / 1000).toFixed(v < 10000 ? 2 : 1)} s`;
}

export const KNOB_SWEEP = 135;

export function toNorm(v: number, min: number, max: number, log: boolean) {
  if (max <= min) return 0;
  const n = log && min > 0 ? Math.log(v / min) / Math.log(max / min) : (v - min) / (max - min);
  return Math.min(1, Math.max(0, n));
}

export function fromNorm(n: number, min: number, max: number, log: boolean) {
  return log && min > 0 ? min * Math.pow(max / min, n) : min + (max - min) * n;
}

/** A point on a knob's circle, `deg` clockwise from the top. */
export function polar(c: number, r: number, deg: number): [number, number] {
  const a = ((deg - 90) * Math.PI) / 180;
  return [c + r * Math.cos(a), c + r * Math.sin(a)];
}

export function arc(c: number, r: number, from: number, to: number) {
  const [x0, y0] = polar(c, r, from);
  const [x1, y1] = polar(c, r, to);
  return `M${x0} ${y0} A${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x1} ${y1}`;
}
