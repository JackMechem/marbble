// Drawing a region on a canvas: its body, name bar, waveform or notes, fades, loop repetitions and
// selection outline. The Tracks area and the Audio Track Editor both draw through this.
import { type Id, type Track } from "../../engine/project";
import { type Note, type Pattern, type Region } from "../../engine/regions";
import { getState } from "../../state/store";
import { drawWave, waveCache } from "./waveform";
import { mix, theme } from "../../theme/canvasTheme";
import { loopPasses } from "./loops";
import { crossfadeLaws, crossfadesByRegion, fadeCurve, fadeHandles, resized, shapeOf, type Crossfade, type RegionCrossfades } from "./fades";
import { FOLDER_BADGE_W, folderLabel } from "../takes/comps";
import { isFolder } from "../takes/takes";
import { drawCompWave, drawFolderBadge } from "../takes/takeDraw";
import { drawnBar, regionBar, type RegionBar } from "./regionBar";
import { playedNotes } from "./regionParams";
import { drawPatternMini } from "../../editors/stepSequencer/model";
import { gainScale } from "./regionGain";
import { canvasFont } from "../../ui/canvasText";

export function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  r = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** A muted region's colour, and every region's on a muted track (the Tracks area). */
export const MUTED_COLOR = "#6c6c70";

type RegionStyle = typeof theme.region;
type AudioContent = Extract<Region["content"], { type: "audio" }>;

/** A region body's box: its left edge and width at `pps` pixels per frame, the strip under the
 *  name bar (`top`, `h`), the visible part of the canvas (`visX0`..`visX1`), and the frames
 *  trimmed off the front while dragging the left edge (`shift`). */
interface BodyBox {
  x: number;
  w: number;
  top: number;
  h: number;
  pps: number;
  visX0: number;
  visX1: number;
  shift: number;
}

/**
 * Body, name bar and content (waveform / notes) colours for each region style:
 *   glass — translucent tint over the lane (Studio)
 *   flat  — solid dark body under a solid name bar
 *   light — solid pale body for light themes
 */
function regionColors(style: RegionStyle, color: string, selected: boolean): { body: string; bar: string; content: string } {
  if (style === "glass") {
    return {
      body: selected ? mix(color, "#ffffff", 0.22, 0.9) : mix(color, theme.regionBase, 0.18, 0.72),
      bar: selected ? mix(color, "#ffffff", 0.45) : mix(color, "#ffffff", 0.12),
      content: selected ? "rgba(20,20,24,0.85)" : mix(color, "#ffffff", 0.62, 0.95),
    };
  }
  if (style === "flat") {
    return {
      body: selected ? mix(color, theme.regionBase, 0.12) : mix(color, theme.regionBase, 0.6),
      bar: selected ? mix(color, "#ffffff", 0.3) : color,
      content: selected ? mix(color, theme.regionBase, 0.72) : mix(color, "#ffffff", 0.28),
    };
  }
  return {
    body: selected ? mix(color, "#ffffff", 0.35) : mix(color, "#ffffff", 0.68),
    bar: selected ? mix(color, "#000000", 0.12) : color,
    content: mix(color, "#000000", selected ? 0.55 : 0.35),
  };
}

export function drawRegion(
  ctx: CanvasRenderingContext2D,
  r: Region,
  t: Track,
  x: number,
  y: number,
  w: number,
  h: number,
  selected: boolean,
  pps: number,
  W: number,
  /** Frames trimmed off (positive) or revealed at (negative) the front while dragging the left edge. */
  shift = 0,
  /** Fade lengths to show instead of the region's own (while dragging one). */
  fades?: [number, number],
  handles = false,
  /** A loop repetition: drawn without the name and selection outline. */
  ghost = false,
  /** Amplitude scale for the waveform (the Audio Track editor's gain and waveform zoom). */
  waveScale = 1,
) {
  const color = r.muted ? MUTED_COLOR : r.color ?? t.color;
  // Short tracks get a slimmer name bar, so the body keeps a strip of its own; the shortest
  // regions are the bar alone.
  const geom = regionBar(h);
  const style = theme.region;
  const { body, bar, content } = regionColors(style, color, selected);
  ctx.save();
  roundRect(ctx, x + 0.5, y + 0.5, w - 1, h - 1, style === "glass" ? 3 : 2);
  ctx.fillStyle = body;
  ctx.fill();
  ctx.clip();
  drawNameBar(ctx, r, x, y, w, h, geom, style, bar, ghost);
  // A region too short to read (the smallest track heights) shows no waveform, notes or fades.
  if (!geom.barOnly) {
    const box = { x, w, top: y + geom.bodyTop, h: geom.bodyH, pps, visX0: Math.max(0, x), visX1: Math.min(W, x + w), shift };
    if (r.content.type === "audio") drawAudioBody(ctx, r, r.content, box, content, waveScale, fades, handles, ghost);
    else if (r.pattern) drawPatternBody(ctx, r, r.pattern, box, content);
    else if (r.content.type === "midi") drawMidiBody(ctx, r, r.content.notes, box, content);
  }
  ctx.restore();
  if (selected && !ghost) {
    roundRect(ctx, x + 0.75, y + 0.75, w - 1.5, h - 1.5, style === "glass" ? 3 : 2);
    ctx.strokeStyle = theme.selectOutline;
    ctx.lineWidth = style === "glass" ? 1 : 1.5;
    ctx.stroke();
  }
}

/** The name bar across the region's top: a take folder's badge, then the name. */
function drawNameBar(ctx: CanvasRenderingContext2D, r: Region, x: number, y: number, w: number, h: number, geom: RegionBar, style: RegionStyle, bar: string, ghost: boolean) {
  const headerH = drawnBar(geom, h);
  ctx.fillStyle = bar;
  ctx.fillRect(x, y, w, headerH);
  if (style === "glass" && !geom.barOnly) {
    ctx.fillStyle = "rgba(0,0,0,0.18)";
    ctx.fillRect(x, y + headerH, w, 1);
  }
  const ink = style === "light" ? "#ffffff" : theme.ink;
  // A take folder's name bar starts with its disclosure triangle and take badge.
  const badge = isFolder(r) && !ghost && w > FOLDER_BADGE_W ? FOLDER_BADGE_W : 0;
  if (badge) drawFolderBadge(ctx, r as Region & { takes: NonNullable<Region["takes"]> }, x, y, headerH, ink);
  if (w <= 24 + badge || ghost) return;
  ctx.fillStyle = ink;
  ctx.font = canvasFont(geom.font, 600);
  ctx.textBaseline = "middle";
  // A take folder names the comp (or whole take) it plays, as Logic's "Audio 1: Comp 2".
  const label = folderLabel(r);
  const name = label ? `${r.name}: ${label}` : r.name;
  ctx.fillText(r.muted ? `${name} (muted)` : name, Math.max(x, 0) + 5 + badge, y + headerH / 2 + 0.5, Math.max(0, w - 8 - badge));
}

/** An audio region's body: its waveform (a take folder's comp), then its fades. */
function drawAudioBody(
  ctx: CanvasRenderingContext2D,
  r: Region,
  audio: AudioContent,
  b: BodyBox,
  color: string,
  waveScale: number,
  fades: [number, number] | undefined,
  handles: boolean,
  ghost: boolean,
) {
  const wave = waveCache.get(audio.clip);
  if (isFolder(r)) drawCompWave(ctx, r, b.x, b.w, b.top, b.h, b.pps, color, b.visX0, b.visX1, b.shift, waveScale * 10 ** (audio.gain_db / 20));
  else if (wave) drawWave(ctx, wave, audio.offset + b.shift, b.x, b.w, b.top, b.h, b.pps, color, b.visX0, b.visX1, undefined, waveScale * gainScale(audio.gain_db));
  const [fi, fo] = fades ?? [audio.fade_in, audio.fade_out];
  drawFades(ctx, r, b.x - b.shift * b.pps, b.x, b.w, b.top, b.h, fi, fo, b.pps, color, handles, ghost ? undefined : regionCrossfades(r.id));
}

/** A pattern region's body: its rows, with a dot where each note it plays starts. */
function drawPatternBody(ctx: CanvasRenderingContext2D, r: Region, pattern: Pattern, b: BodyBox, color: string) {
  const played = playedNotes(getState().snap?.played, r.id) ?? [];
  drawPatternMini(ctx, pattern, played, r.params?.transpose ?? 0, b.x - b.shift * b.pps, b.top, b.h, b.pps, color, b.visX0, b.visX1);
}

/** A MIDI region's notes as they play: with its playback parameters (quantize, transpose, ...)
 *  applied, over the pitch range they span. */
function drawMidiBody(ctx: CanvasRenderingContext2D, r: Region, own: Note[], b: BodyBox, color: string) {
  const notes = playedNotes(getState().snap?.played, r.id) ?? own;
  if (!notes.length) return;
  let lo = 127;
  let hi = 0;
  for (const n of notes) {
    lo = Math.min(lo, n.pitch);
    hi = Math.max(hi, n.pitch);
  }
  lo -= 2;
  hi += 2;
  const span = Math.max(12, hi - lo);
  const nh = Math.max(1.5, Math.min(4, b.h / span));
  ctx.fillStyle = color;
  for (const n of notes) {
    const nx = b.x + (n.start - b.shift) * b.pps;
    const nw = Math.max(2, n.length * b.pps);
    if (nx > b.visX1 || nx + nw < b.visX0) continue;
    const ny = b.top + b.h - ((n.pitch - lo) / span) * b.h - nh / 2;
    ctx.fillRect(nx, ny, nw - 0.5, nh);
  }
}

/** A lane a drag past the last track will create: the look of a file drag's new-track ghost
 *  (see the drop target in `draw`), named at its left edge. */
export function drawNewLane(ctx: CanvasRenderingContext2D, y: number, h: number, W: number, accent: string, ink: string) {
  ctx.fillStyle = accent;
  ctx.globalAlpha = 0.12;
  ctx.fillRect(0, y, W, h);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = accent;
  ctx.lineWidth = 2;
  ctx.setLineDash([6, 4]);
  ctx.strokeRect(1, y + 1, W - 2, h - 2);
  ctx.setLineDash([]);
  const label = "New track";
  ctx.font = canvasFont("sm", 600);
  const tagH = Math.min(18, h - 4);
  const tagY = y + Math.min(4, (h - tagH) / 2);
  ctx.beginPath();
  ctx.roundRect(4, tagY, ctx.measureText(label).width + 12, tagH, 4);
  ctx.fill();
  ctx.fillStyle = ink;
  ctx.textBaseline = "middle";
  ctx.fillText(label, 10, tagY + tagH / 2 + 0.5);
}

/** A looping region's repetitions after its body (`x` is the timeline origin in pixels): the
 *  region drawn again, lighter, with a notch at each repetition boundary as in Logic. The
 *  fade-out sits at the end of the last repetition, where the loop stops. */
export function drawLoops(
  ctx: CanvasRenderingContext2D,
  r: Region,
  t: Track,
  originX: number,
  start: number,
  length: number,
  loop: number | null | undefined,
  y: number,
  h: number,
  selected: boolean,
  pps: number,
  W: number,
  shift: number,
) {
  const passes = loopPasses(start, length, loop);
  const fo = r.content.type === "audio" ? r.content.fade_out : 0;
  const alpha = ctx.globalAlpha;
  passes.forEach(([ps, pl], k) => {
    const x = originX + ps * pps;
    const w = Math.max(2, pl * pps);
    if (x > W || x + w < 0) return;
    const last = k === passes.length - 1;
    ctx.globalAlpha = alpha * 0.5;
    drawRegion(ctx, r, t, x, y, w, h, selected, pps, W, shift, [0, last ? Math.min(fo, pl) : 0], false, true);
    ctx.globalAlpha = alpha;
    // The notch: a small wedge at the top edge and a hairline down the name bar.
    const nx = Math.round(x) + 0.5;
    ctx.fillStyle = "rgba(0,0,0,0.45)";
    ctx.fillRect(nx - 0.5, y, 1, drawnBar(regionBar(h), h));
    ctx.beginPath();
    ctx.moveTo(nx - 4, y);
    ctx.lineTo(nx + 4, y);
    ctx.lineTo(nx, y + 5);
    ctx.closePath();
    ctx.fill();
  });
}

/** Fade curves over a region body: the faded part shaded, the curve drawn (dashed for a
 *  Speed Up or Slow Down), and (when `handles`) the grab points at the top. `origin` is where
 *  the region's own start is drawn. A crossfade replaces the fade on its side and shows both
 *  regions' curves crossing; each region draws the part over itself. */
function drawFades(
  ctx: CanvasRenderingContext2D,
  r: Region,
  origin: number,
  x: number,
  w: number,
  top: number,
  h: number,
  fi: number,
  fo: number,
  pps: number,
  ink: string,
  handles: boolean,
  xf: RegionCrossfades | undefined,
) {
  const shape = shapeOf(r.content);
  // `gain(u)` for u from 0 at the left of the fade to 1 at its right.
  const side = (x0: number, len: number, gain: (u: number) => number, dashed: boolean, shade: boolean) => {
    if (len < 1) return;
    const steps = Math.max(8, Math.min(64, Math.round(len / 3)));
    const pt = (k: number) => [x0 + (k / steps) * len, top + h * (1 - gain(k / steps))] as const;
    if (shade) {
      ctx.beginPath();
      ctx.moveTo(x0, top);
      for (let k = 0; k <= steps; k++) ctx.lineTo(...pt(k));
      ctx.lineTo(x0 + len, top);
      ctx.closePath();
      ctx.fillStyle = "rgba(0,0,0,0.28)";
      ctx.fill();
    }
    ctx.beginPath();
    for (let k = 0; k <= steps; k++) (k ? ctx.lineTo : ctx.moveTo).call(ctx, ...pt(k));
    ctx.strokeStyle = ink;
    ctx.lineWidth = 1.2;
    if (dashed) ctx.setLineDash([3, 2]);
    ctx.stroke();
    ctx.setLineDash([]);
  };
  const at = (frame: number) => origin + (frame - r.start) * pps;
  if (!xf?.in) side(x, fi * pps, (u) => fadeCurve(u, shape.in_curve), shape.in_type === "speed_up", true);
  if (!xf?.out) side(x + w - fo * pps, fo * pps, (u) => fadeCurve(1 - u, shape.out_curve), shape.out_type === "slow_down", true);
  for (const c of [xf?.in, xf?.out]) {
    if (!c) continue;
    const [outLaw, inLaw] = crossfadeLaws(c);
    side(at(c.fade_out[0]), c.fade_out[1] * pps, (u) => outLaw(1 - u), false, c === xf?.out);
    side(at(c.fade_in[0]), c.fade_in[1] * pps, inLaw, false, c === xf?.in);
  }
  if (handles) {
    const hd = fadeHandles(r.start, r.length, fi, fo, xf);
    ctx.fillStyle = ink;
    if (hd.fadeIn !== null) ctx.fillRect(Math.round(x + hd.fadeIn * pps) - 3, top, 6, 6);
    ctx.fillRect(Math.round(xf?.out ? at(r.start + hd.fadeOut) : x + w - fo * pps) - 3, top, 6, 6);
  }
}

/** A crossfade whose handle is being dragged: its outgoing region and the length shown. */
let crossfadeDrag: { region: Id; length: number } | null = null;

export function setCrossfadeDrag(v: { region: Id; length: number } | null) {
  crossfadeDrag = v;
}

/** The crossfades a region takes part in, with a handle drag in progress applied. */
export function regionCrossfades(id: Id): RegionCrossfades | undefined {
  const x = crossfadesByRegion(getState().snap?.crossfades).get(id);
  const d = crossfadeDrag;
  if (!x || !d) return x;
  const fix = (c?: Crossfade) => (c && c.out_region === d.region ? resized(c, d.length) : c);
  return { out: fix(x.out), in: fix(x.in) };
}
