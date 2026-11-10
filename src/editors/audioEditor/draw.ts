// Painting the Audio Track Editor: the lane's regions drawn large with clip marks and transients, the
// selected range, its ruler and its dB scale.
import { type Id, type Project, type Track } from "../../engine/project";
import { type Region } from "../../engine/regions";
import * as ed from "./state";
import { getState } from "../../state/store";
import { theme } from "../../theme/canvasTheme";
import { forEachGridLine, tempoMap } from "../../lib/tempo";
import { pixelRatio } from "../../ui/uiScale";
import { ensureWave, waveCache, type Wave } from "../../tracks/regions/waveform";
import { drawRegion } from "../../tracks/regions/regionDraw";
import { dbGain, DB_MARKS, CYCLE_STRIP, geometry, lowerBound, RULER, SCALE_W, type Drag } from "./model";
import { canvasFont } from "../../ui/canvasText";

/** The latest values for the imperative draw and pointer code. */
export interface AudioLive {
  project: Project | undefined;
  track: Track;
  selectedRegions: Id[];
  pps: number;
  amp: number;
  showTransients: boolean;
  accent: string;
  total: number;
}

/** Red marks at the top and bottom of the waveform where the audio, after `gain`, reaches
 *  0 dBFS. Reads the same overview level drawWave uses, one value per pixel column. */
function drawClipMarks(ctx: CanvasRenderingContext2D, wave: Wave, offset: number, x: number, w: number, pps: number, gain: number, top: number, h: number, W: number) {
  if (gain <= 0) return;
  const limit = 0.999 / gain;
  if (limit > 1) return; // the overview stops at full scale, so nothing below it can clip
  const li = Math.max(0, Math.min(wave.levels.length - 1, Math.floor(Math.log2(1 / pps / wave.bucket))));
  const { min, max } = wave.levels[li];
  const bucket = wave.bucket * 2 ** li;
  const n = max.length;
  const x0 = Math.max(0, Math.floor(x));
  const x1 = Math.min(W, Math.ceil(x + w));
  ctx.fillStyle = theme.red;
  let run = -1;
  const flush = (px: number) => {
    if (run < 0) return;
    ctx.fillRect(run, top, px - run, 3);
    ctx.fillRect(run, top + h - 3, px - run, 3);
    run = -1;
  };
  for (let px = x0; px < x1; px++) {
    const f0 = offset + (px - x) / pps;
    let b = Math.max(0, Math.floor(f0 / bucket));
    const b1 = Math.min(n, Math.max(b + 1, Math.ceil((f0 + 1 / pps) / bucket)));
    let hot = false;
    for (; b < b1 && !hot; b++) hot = max[b] >= limit || (min ? -min[b] : max[b]) >= limit;
    if (hot && run < 0) run = px;
    else if (!hot) flush(px);
  }
  flush(x1);
}

function sizeCanvas(cv: HTMLCanvasElement, w: number, h: number, dpr: number) {
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
    cv.style.width = `${w}px`;
    cv.style.height = `${h}px`;
  }
  const ctx = cv.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return ctx;
}

/** The lane's layout (model.ts `geometry`). */
type Geometry = ReturnType<typeof geometry>;

/** The canvas's size and how the timeline maps onto it: `X` takes a frame to a canvas x. */
interface View {
  W: number;
  H: number;
  left: number;
  pps: number;
  X: (frame: number) => number;
}

/** The background: bars, beats and (zoomed in) sixteenths through the tempo map, and the cycle. */
function drawBackground(ctx: CanvasRenderingContext2D, { W, H, left, pps, X }: View, project: Project) {
  ctx.fillStyle = theme.editorBg;
  ctx.fillRect(0, 0, W, H);
  forEachGridLine(tempoMap(project), Math.max(0, left / pps), (left + W) / pps, pps, { beatsMinPx: 9.01, subdiv: 4, subMinPx: 30.01 }, (pos, kind) => {
    ctx.fillStyle = kind === "bar" ? theme.gridBar : theme.gridBeat;
    ctx.fillRect(Math.round(X(pos)), RULER, 1, H - RULER);
  });
  if (project.cycle.enabled) {
    ctx.fillStyle = theme.cycleTint;
    ctx.fillRect(X(project.cycle.start), RULER, (project.cycle.end - project.cycle.start) * pps, H - RULER);
  }
}

/** Where region `r` is drawn while `drag` moves, trims or fades it: its start and length, the
 *  frames trimmed off its front, and the fades to show instead of its own. */
function draggedRegion(r: Region, drag: Drag | null): { start: number; length: number; shift: number; fades?: [number, number] } {
  const own = { start: r.start, length: r.length, shift: 0 };
  if (!drag || drag.id !== r.id) return own;
  if (drag.kind === "move" && drag.start !== undefined) return { ...own, start: drag.start };
  if (drag.fade !== undefined && r.content.type === "audio") {
    return { ...own, fades: drag.kind === "fadeIn" ? [drag.fade, r.content.fade_out] : [r.content.fade_in, drag.fade] };
  }
  if (drag.start !== undefined && drag.length !== undefined) return { start: drag.start, length: drag.length, shift: drag.start - r.start };
  return own;
}

/** Transient marks over a region's body drawn at `x` (width `w`) from clip frame `offset`: a
 *  faint line and a small wedge at the bottom for each. */
function drawTransients(ctx: CanvasRenderingContext2D, list: number[], offset: number, x: number, w: number, { W, pps }: View, g: Geometry) {
  const from = offset + (Math.max(0, x) - x) / pps;
  const to = offset + (Math.min(W, x + w) - x) / pps;
  const bottom = g.bodyTop + g.bodyH;
  ctx.fillStyle = theme.selectOutline;
  for (let i = lowerBound(list, from); i < list.length && list[i] <= to; i++) {
    const tx = Math.round(x + (list[i] - offset) * pps);
    ctx.globalAlpha = 0.35;
    ctx.fillRect(tx, g.bodyTop, 1, g.bodyH);
    ctx.globalAlpha = 0.9;
    ctx.beginPath();
    ctx.moveTo(tx - 3.5, bottom);
    ctx.lineTo(tx + 4.5, bottom);
    ctx.lineTo(tx + 0.5, bottom - 5);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
}

/** The selected range over the lane: a tint with a line at each end. */
function drawRangeSel(ctx: CanvasRenderingContext2D, { H, pps, X }: View, s: ed.RangeSel, accent: string) {
  const sx = X(s.start);
  const sw = Math.max(1, (s.end - s.start) * pps);
  ctx.fillStyle = accent;
  ctx.globalAlpha = 0.2;
  ctx.fillRect(sx, RULER, sw, H - RULER);
  ctx.globalAlpha = 0.9;
  ctx.fillRect(Math.round(sx), RULER, 1, H - RULER);
  ctx.fillRect(Math.round(sx + sw), RULER, 1, H - RULER);
  ctx.globalAlpha = 1;
}

export type DrawRefs = {
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  scaleRef: React.RefObject<HTMLCanvasElement | null>;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  playheadRef: React.RefObject<HTMLDivElement | null>;
  dragRef: React.RefObject<Drag | null>;
  hoverRef: React.RefObject<Id | null>;
  live: React.RefObject<AudioLive>;
  requestDraw: () => void;
  positionPlayhead: () => void;
};

export function editorDrawer({ canvasRef, scaleRef, scrollRef, playheadRef, dragRef, hoverRef, live, requestDraw, positionPlayhead }: DrawRefs) {
  const draw = () => {
    const cv = canvasRef.current;
    const sc = scaleRef.current;
    const el = scrollRef.current;
    const { project, track, accent, pps } = live.current;
    if (!cv || !sc || !el || !project || pps <= 0) return;
    const dpr = pixelRatio();
    const W = el.clientWidth;
    const H = el.clientHeight;
    const left = el.scrollLeft;
    const ctx = sizeCanvas(cv, W, H, dpr);
    const g = geometry(H);
    const v: View = { W, H, left, pps, X: (f) => f * pps - left };
    drawBackground(ctx, v, project);
    drawRegions(ctx, v, g);
    if (!track.regions.length) {
      ctx.fillStyle = theme.rulerTick;
      ctx.font = canvasFont("md");
      ctx.textAlign = "center";
      ctx.fillText("This track has no regions yet. Record or drop an audio file on it.", W / 2, g.top + g.h / 2);
      ctx.textAlign = "left";
    }
    // The selected range: over the lane, and as a bar along the foot of the ruler.
    const s = ed.rangeSel();
    const sel = s && s.track === track.id ? s : null;
    if (sel) drawRangeSel(ctx, v, sel, accent);
    drawRuler(ctx, W, left);
    if (sel) {
      ctx.fillStyle = accent;
      ctx.fillRect(v.X(sel.start), RULER - 4, Math.max(1, (sel.end - sel.start) * pps), 3);
    }
    drawScale(sc, H, dpr);
    const ph = playheadRef.current;
    if (ph) ph.style.height = `${H}px`;
    positionPlayhead();
  };

  /** The track's regions, as in the arrange but tall, with gain and waveform zoom applied, and
   *  over each audio one where it clips and (when shown) its transients. */
  const drawRegions = (ctx: CanvasRenderingContext2D, v: View, g: Geometry) => {
    const { track, selectedRegions, pps, amp, showTransients } = live.current;
    const { W } = v;
    const drag = dragRef.current;
    for (const r of track.regions) {
      const { start, length, shift, fades } = draggedRegion(r, drag);
      const x = v.X(start);
      const w = Math.max(2, length * pps);
      if (x > W || x + w < 0) continue;
      const audio = r.content.type === "audio" ? r.content : null;
      const isSel = selectedRegions.includes(r.id);
      if (audio && !waveCache.has(audio.clip)) ensureWave(audio.clip, requestDraw);
      // drawRegion scales the waveform by the region's gain itself.
      drawRegion(ctx, r, track, x, g.top, w, g.h, isSel, pps, W, shift, fades, !!audio && (isSel || hoverRef.current === r.id), false, amp);
      if (!audio) continue;
      const wave = waveCache.get(audio.clip);
      const offset = audio.offset + shift;
      ctx.save();
      ctx.beginPath();
      ctx.rect(Math.max(0, x), g.bodyTop, Math.min(W, x + w) - Math.max(0, x), g.bodyH);
      ctx.clip();
      if (wave && !r.muted) drawClipMarks(ctx, wave, offset, x, w, pps, dbGain(audio.gain_db), g.bodyTop, g.bodyH, W);
      const transients = showTransients ? ed.transientsFor(audio.clip, requestDraw) : null;
      if (transients) drawTransients(ctx, transients, offset, x, w, v, g);
      ctx.restore();
    }
  };

  const drawRuler = (ctx: CanvasRenderingContext2D, W: number, left: number) => {
    const { project, pps } = live.current;
    if (!project) return;
    ctx.fillStyle = theme.ruler;
    ctx.fillRect(0, 0, W, RULER);
    ctx.fillStyle = theme.rulerCycleZone;
    ctx.fillRect(0, 0, W, CYCLE_STRIP);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, RULER - 1, W, 1);
    const c = project.cycle;
    if (c.end > c.start) {
      ctx.fillStyle = c.enabled ? theme.cycleOn : theme.cycleOff;
      ctx.beginPath();
      ctx.roundRect(c.start * pps - left, 1, (c.end - c.start) * pps, CYCLE_STRIP - 2, 2);
      ctx.fill();
    }
    const map = tempoMap(project);
    // Label spacing from the bar length where the view starts.
    const every = [1, 2, 4, 8, 16, 32, 64, 128].find((n) => map.samplesPerBarAt(left / pps) * pps * n >= 34) ?? 256;
    ctx.textBaseline = "alphabetic";
    for (const bar of map.bars(Math.max(0, left / pps), (left + W) / pps)) {
      const b = bar.bar - 1;
      const x = Math.round(bar.start * pps - left) + 0.5;
      const barPx = (bar.end - bar.start) * pps;
      const beatPx = barPx / bar.num;
      const beatX = (k: number) => Math.round(map.sampleAt(bar.startQ + (k * 4) / bar.den) * pps - left + 0.5);
      if (b % every === 0) {
        ctx.fillStyle = theme.rulerTick;
        ctx.fillRect(x, CYCLE_STRIP + 1, 1, RULER - CYCLE_STRIP - 2);
        ctx.fillStyle = theme.rulerText;
        ctx.font = canvasFont("sm", 500);
        ctx.fillText(String(b + 1), x + 3, RULER - 5);
      } else if (barPx >= 4) {
        ctx.fillStyle = theme.rulerMinor;
        ctx.fillRect(x, RULER - 7, 1, 6);
      }
      if (beatPx < 10) continue;
      ctx.font = canvasFont("xs");
      for (let k = 1; k < bar.num; k++) {
        const bx = beatX(k);
        ctx.fillStyle = theme.rulerMinor;
        ctx.fillRect(bx, RULER - 5, 1, 4);
        if (beatPx >= 48) {
          ctx.fillStyle = theme.rulerTick;
          ctx.fillText(`${b + 1}.${k + 1}`, bx + 3, RULER - 5);
        }
      }
      if (beatPx >= 48) {
        ctx.fillStyle = theme.rulerMinor;
        for (let k = 1; k < bar.num * 4; k++) if (k % 4) ctx.fillRect(beatX(k / 4), RULER - 3, 1, 2);
      }
    }
  };

  /** The dB scale beside the lane: post-gain level, so it moves with the waveform zoom only. */
  const drawScale = (cv: HTMLCanvasElement, H: number, dpr: number) => {
    const { amp } = live.current;
    const ctx = sizeCanvas(cv, SCALE_W, H, dpr);
    const g = geometry(H);
    ctx.fillStyle = theme.editorBg;
    ctx.fillRect(0, 0, SCALE_W, H);
    ctx.fillStyle = theme.ruler;
    ctx.fillRect(0, 0, SCALE_W, RULER);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, RULER - 1, SCALE_W, 1);
    ctx.font = canvasFont("xs");
    ctx.textBaseline = "middle";
    ctx.textAlign = "right";
    ctx.fillStyle = theme.rulerTick;
    ctx.fillText("dB", SCALE_W - 7, RULER / 2 + 3);
    const rectified = getState().waveStyle === "rectified";
    const mid = g.bodyTop + g.bodyH / 2;
    const half = g.bodyH / 2 - 1;
    const base = g.bodyTop + g.bodyH;
    const yFor = (lin: number) => (rectified ? base - lin * amp * (g.bodyH - 1) : mid - lin * amp * half);
    const mark = (y: number, label: string) => {
      ctx.fillStyle = theme.rulerMinor;
      ctx.fillRect(SCALE_W - 4, Math.round(y), 4, 1);
      ctx.fillStyle = theme.rulerText;
      ctx.fillText(label, SCALE_W - 7, y);
    };
    let lastY = -Infinity;
    const stop = rectified ? base - 12 : mid - 10;
    for (const d of DB_MARKS) {
      const lin = Math.pow(10, d / 20);
      if (lin * amp > 1.0001) continue;
      const y = yFor(lin);
      if (y > stop) break;
      if (y - lastY < 13) continue;
      mark(y, d === 0 ? "0" : String(d));
      if (!rectified) mark(2 * mid - y, d === 0 ? "0" : String(d));
      lastY = y;
    }
    mark(rectified ? base - 1 : mid, "-∞");
    ctx.textAlign = "left";
  };

  return draw;
}
