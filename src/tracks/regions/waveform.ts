// Waveform overviews and how they're drawn. Each clip's overview is fetched once as signed
// (min, max) pairs per 32 frames and kept as a pyramid: each level above halves the resolution,
// so drawing reads about one value per pixel whatever the zoom.
import { invoke } from "@tauri-apps/api/core";
import type { Id } from "../../engine/project";
import { getState, setState } from "../../state/store";

export type WaveStyle = "smooth" | "mirrored" | "rectified" | "blocks";

export const WAVE_STYLES: { id: WaveStyle; name: string; description: string }[] = [
  { id: "smooth", name: "Smooth", description: "The true shape, above and below the centre line" },
  { id: "mirrored", name: "Mirrored", description: "Loudness, mirrored around the centre line" },
  { id: "rectified", name: "Rectified", description: "Loudness, rising from the bottom of the region" },
  { id: "blocks", name: "Blocks", description: "One bar per bucket, the classic overview" },
];

export interface WaveLevel {
  /** Lowest sample per bucket, or null when only peak levels are known (shown as -max). */
  min: ArrayLike<number> | null;
  /** Highest sample per bucket (or the absolute peak when `min` is null). */
  max: ArrayLike<number>;
}

export interface Wave {
  frames: number;
  /** Frames per bucket at level 0; level i uses bucket * 2^i. */
  bucket: number;
  levels: WaveLevel[];
}

/** Waveform overviews by clip id. */
export const waveCache = new Map<Id, Wave>();

/** Clips on their way, with what to call when each arrives (several views may wait). */
const pending = new Map<Id, (() => void)[]>();

/** Fetches the clip's overview unless it's cached or on its way; `done` runs once it arrives. */
export function ensureWave(clip: Id, done: () => void) {
  if (waveCache.has(clip)) return;
  const waiting = pending.get(clip);
  if (waiting) {
    if (!waiting.includes(done)) waiting.push(done);
    return;
  }
  pending.set(clip, [done]);
  invoke<ArrayBuffer>("clip_waveform", { id: clip })
    .then((buf) => {
      if (buf.byteLength >= 8) {
        waveCache.set(clip, parseWave(buf));
        pending.get(clip)?.forEach((f) => f());
      }
    })
    .catch(() => {})
    .finally(() => pending.delete(clip));
}

function parseWave(buf: ArrayBuffer): Wave {
  const head = new DataView(buf, 0, 8);
  const frames = head.getUint32(0, true);
  const bucket = head.getUint32(4, true);
  const pairs = new Int16Array(buf.slice(8));
  const n = pairs.length >> 1;
  const min = new Float32Array(n);
  const max = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    min[i] = pairs[2 * i] / 32767;
    max[i] = pairs[2 * i + 1] / 32767;
  }
  const levels: WaveLevel[] = [{ min, max }];
  while (levels[levels.length - 1].max.length > 64) {
    const prev = levels[levels.length - 1];
    const pmin = prev.min as Float32Array;
    const pmax = prev.max as Float32Array;
    const len = Math.ceil(pmax.length / 2);
    const nmin = new Float32Array(len);
    const nmax = new Float32Array(len);
    for (let i = 0; i < len; i++) {
      const j = Math.min(2 * i + 1, pmax.length - 1);
      nmin[i] = Math.min(pmin[2 * i], pmin[j]);
      nmax[i] = Math.max(pmax[2 * i], pmax[j]);
    }
    levels.push({ min: nmin, max: nmax });
  }
  return { frames, bucket, levels };
}

// ---- the style setting ----------------------------------------------------------------------

const KEY = "marbble.waveStyle";

export function savedWaveStyle(): WaveStyle {
  try {
    const v = localStorage.getItem(KEY);
    return WAVE_STYLES.some((s) => s.id === v) ? (v as WaveStyle) : "smooth";
  } catch {
    return "smooth";
  }
}

export function applyWaveStyle(style: WaveStyle) {
  setState({ waveStyle: style });
  try {
    localStorage.setItem(KEY, style);
  } catch {
    // Storage can be unavailable; the style still applies for this session.
  }
}

// ---- drawing ----------------------------------------------------------------------------------

/** One level of a wave's pyramid, with the frames each of its buckets covers. */
interface Level extends WaveLevel {
  bucket: number;
}

/** Where a wave is drawn: from frame `offset` at `x`, at `pps` pixels per frame, over the
 *  pixel columns `x0`..`x1`. */
interface Span {
  offset: number;
  x: number;
  pps: number;
  x0: number;
  x1: number;
}

/** The coarsest level whose buckets are still at most one pixel wide. */
function levelFor(wave: Wave, pps: number): Level {
  const framesPerPx = 1 / pps;
  const li = Math.max(0, Math.min(wave.levels.length - 1, Math.floor(Math.log2(framesPerPx / wave.bucket))));
  return { ...wave.levels[li], bucket: wave.bucket * 2 ** li };
}

/**
 * Draws `wave` from frame `offset` into the box at x (width w, from `top`, height h) at `pps`
 * pixels per frame, only between clipX0 and clipX1. `scale` multiplies the amplitude (region gain,
 * waveform zoom); what goes past full height is cut off at the box edge.
 */
export function drawWave(
  ctx: CanvasRenderingContext2D,
  wave: Wave,
  offset: number,
  x: number,
  w: number,
  top: number,
  h: number,
  pps: number,
  color: string,
  clipX0: number,
  clipX1: number,
  style: WaveStyle = getState().waveStyle,
  scale = 1,
) {
  const level = levelFor(wave, pps);
  const x0 = Math.max(Math.floor(x), Math.floor(clipX0));
  const x1 = Math.min(Math.ceil(x + w), Math.ceil(clipX1));
  if (x1 <= x0 || level.max.length === 0) return;
  const span = { offset, x, pps, x0, x1 };
  ctx.fillStyle = color;
  if (style === "blocks") {
    drawBlocks(ctx, level, span, top, h, scale);
    return;
  }
  // The other styles trace an outline through one value per half pixel (finer on HiDPI it
  // wouldn't show) and fill it, so edges are smooth and antialiased.
  const { hi, lo } = outlineValues(level, span, style, scale);
  ctx.beginPath();
  if (style === "rectified") traceRectified(ctx, hi, x0, top, h);
  else traceAroundCentre(ctx, hi, lo, x0, top, h);
  ctx.closePath();
  ctx.fill();
}

/** One rect per pixel column, each the peak of the buckets under it. */
function drawBlocks(ctx: CanvasRenderingContext2D, { min, max, bucket }: Level, { offset, x, pps, x0, x1 }: Span, top: number, h: number, scale: number) {
  const n = max.length;
  const mid = top + h / 2;
  const half = h / 2 - 1;
  ctx.beginPath();
  for (let px = x0; px < x1; px++) {
    const f0 = offset + (px - x) / pps;
    const f1 = offset + (px + 1 - x) / pps;
    let b = Math.floor(f0 / bucket);
    const b1 = Math.max(b + 1, Math.ceil(f1 / bucket));
    let m = 0;
    for (; b < b1 && b < n; b++) {
      if (b < 0) continue;
      m = Math.max(m, Math.abs(max[b]), min ? Math.abs(min[b]) : 0);
    }
    const a = Math.max(0.25, Math.min(1, m * scale) * half);
    ctx.rect(px, mid - a, 1, a * 2);
  }
  ctx.fill();
}

/** The outline's step, in pixels. */
const STEP = 0.5;

/**
 * The wave's top and bottom (`hi`, `lo`, in full-scale units after `scale`) every STEP pixels
 * from `x0`; the styles other than smooth keep only the loudness, mirrored.
 */
function outlineValues({ min, max, bucket }: Level, { offset, x, pps, x0, x1 }: Span, style: WaveStyle, scale: number) {
  const n = max.length;
  const count = Math.ceil((x1 - x0) / STEP) + 1;
  const hi = new Float32Array(count);
  const lo = new Float32Array(count);
  // When buckets are wider than a step (deep zoom), interpolate between bucket centres.
  const coarse = bucket * pps > STEP;
  for (let i = 0; i < count; i++) {
    const px = x0 + i * STEP;
    let top = -Infinity;
    let bot = Infinity;
    if (coarse) {
      const u = (offset + (px - x) / pps) / bucket - 0.5;
      const b = Math.floor(u);
      const t = u - b;
      const ba = Math.min(n - 1, Math.max(0, b));
      const bb = Math.min(n - 1, Math.max(0, b + 1));
      top = max[ba] + (max[bb] - max[ba]) * t;
      bot = min ? min[ba] + (min[bb] - min[ba]) * t : -top;
    } else {
      const f0 = offset + (px - x) / pps;
      const f1 = f0 + STEP / pps;
      let b = Math.max(0, Math.floor(f0 / bucket));
      const b1 = Math.min(n, Math.max(b + 1, Math.ceil(f1 / bucket)));
      for (; b < b1; b++) {
        if (max[b] > top) top = max[b];
        const m = min ? min[b] : -max[b];
        if (m < bot) bot = m;
      }
      if (top === -Infinity) top = bot = 0;
    }
    top *= scale;
    bot *= scale;
    if (style === "smooth") {
      hi[i] = top;
      lo[i] = bot;
    } else {
      const a = Math.max(Math.abs(top), Math.abs(bot));
      hi[i] = a;
      lo[i] = -a;
    }
  }
  return { hi, lo };
}

/** The rectified outline: rising from the bottom edge of the box. */
function traceRectified(ctx: CanvasRenderingContext2D, hi: Float32Array, x0: number, top: number, h: number) {
  const base = top + h;
  const full = h - 1;
  const count = hi.length;
  ctx.moveTo(x0, base);
  for (let i = 0; i < count; i++) ctx.lineTo(x0 + i * STEP, base - Math.max(0.5, Math.min(1, hi[i]) * full));
  ctx.lineTo(x0 + (count - 1) * STEP, base);
}

/** The smooth and mirrored outline: along the top left to right and back along the bottom, at
 *  least a pixel thick everywhere, so silence still shows as a hairline. */
function traceAroundCentre(ctx: CanvasRenderingContext2D, hi: Float32Array, lo: Float32Array, x0: number, top: number, h: number) {
  const mid = top + h / 2;
  const half = h / 2 - 1;
  const count = hi.length;
  const clamp = (v: number) => Math.max(-1, Math.min(1, v));
  for (let i = 0; i < count; i++) {
    let a = mid - clamp(hi[i]) * half;
    let b = mid - clamp(lo[i]) * half;
    if (b - a < 1) {
      const c = (a + b) / 2;
      a = c - 0.5;
      b = c + 0.5;
    }
    hi[i] = a;
    lo[i] = b;
  }
  ctx.moveTo(x0, hi[0]);
  for (let i = 1; i < count; i++) ctx.lineTo(x0 + i * STEP, hi[i]);
  for (let i = count - 1; i >= 0; i--) ctx.lineTo(x0 + i * STEP, lo[i]);
}
