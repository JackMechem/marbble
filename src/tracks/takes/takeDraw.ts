// Drawing take folders on the Tracks area's canvas: the disclosure triangle and take badge on the
// folder's name bar, the comp's waveform, and the open folder's rows, one per take.
import type { CompSegment, Region, Take, TakeFolder } from "../../engine/regions";
import { FOLDER_BADGE_BOX_W, FOLDER_BADGE_X, FOLDER_TRIANGLE_X, isMidiFolder, movedEdge, sectionAt } from "./comps";
import { MUTED_COLOR } from "../regions/regionDraw";
import { getState } from "../../state/store";
import { takeLabel } from "./takeRatings";
import { mix, theme } from "../../theme/canvasTheme";
import { canvasFont } from "../../ui/canvasText";
import { drawWave, waveCache } from "../regions/waveform";
import { isOpen, badgeText, takeShift, type CompDrag, TAKE_ROW_H } from "./takes";

/** The disclosure triangle and the take badge, at the start of the folder's name bar (`bar` px
 *  tall), both centred on it. */
export function drawFolderBadge(ctx: CanvasRenderingContext2D, r: Region & { takes: TakeFolder }, x: number, y: number, bar: number, ink: string) {
  const open = isOpen(r.id);
  const left = Math.max(x, 0);
  const cx = left + FOLDER_TRIANGLE_X;
  const cy = y + bar / 2;
  ctx.fillStyle = ink;
  ctx.beginPath();
  if (open) {
    ctx.moveTo(cx - 4, cy - 2.5);
    ctx.lineTo(cx + 4, cy - 2.5);
    ctx.lineTo(cx, cy + 2.5);
  } else {
    ctx.moveTo(cx - 2.5, cy - 4);
    ctx.lineTo(cx + 2.5, cy);
    ctx.lineTo(cx - 2.5, cy + 4);
  }
  ctx.closePath();
  ctx.fill();
  const bx = left + FOLDER_BADGE_X;
  const bh = 10;
  ctx.beginPath();
  ctx.roundRect(bx, cy - bh / 2, FOLDER_BADGE_BOX_W, bh, 2);
  ctx.globalAlpha *= 0.85;
  ctx.fill();
  ctx.globalAlpha /= 0.85;
  ctx.fillStyle = theme.region === "light" ? "#ffffff" : mix(ink, "#ffffff", 0.9);
  ctx.font = canvasFont("xs", 700);
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(badgeText(r.takes), bx + FOLDER_BADGE_BOX_W / 2, cy + 0.5);
  ctx.textAlign = "left";
}

/** A folder's waveform: each comp segment drawn from its own take. `shift` is the trim being
 *  dragged (frames off the front), as for plain regions. */
export function drawCompWave(
  ctx: CanvasRenderingContext2D,
  r: Region & { takes: TakeFolder },
  x: number,
  w: number,
  top: number,
  h: number,
  pps: number,
  color: string,
  visX0: number,
  visX1: number,
  shift: number,
  scale: number,
) {
  const tt0 = takeShift(r) + r.start + shift;
  for (const s of r.takes.comp) {
    const t = r.takes.takes.find((k) => k.id === s.take);
    const wave = t && waveCache.get(t.clip);
    if (!t || !wave) continue;
    const sx0 = x + (s.start - tt0) * pps;
    const sx1 = x + (s.end - tt0) * pps;
    drawWave(ctx, wave, t.offset + tt0, x, w, top, h, pps, color, Math.max(visX0, sx0), Math.min(visX1, sx1), undefined, scale * 10 ** (t.gain_db / 20));
  }
}

/** The take rows of an open folder, below its track's lane (`y` is the first row's top). Comp
 *  parts are in the region's colour (or the take's own), the rest dimmed, as in Logic. A rated
 *  take's name has its mark. `drag` is a swipe or an edge drag in progress; the selected comp
 *  section is outlined. */
export function drawTakeRows(
  ctx: CanvasRenderingContext2D,
  r: Region & { takes: TakeFolder },
  folderColor: string,
  x: number,
  w: number,
  y: number,
  pps: number,
  W: number,
  drag: CompDrag | null,
) {
  const tt0 = takeShift(r) + r.start;
  const preview = drag && "take" in drag ? drag : null;
  const comp: CompSegment[] = drag && "edge" in drag ? movedEdge(r.takes, drag.edge, drag.to) : r.takes.comp;
  const sel = getState().compSection;
  const selected = sel?.region === r.id && !drag ? sectionAt(r.takes, sel.at + takeShift(r)) : null;
  const visX0 = Math.max(0, x);
  const visX1 = Math.min(W, x + w);
  const light = theme.region === "light";
  r.takes.takes.forEach((t, k) => {
    // A muted folder's rows stay grey.
    const color = folderColor === MUTED_COLOR ? folderColor : (t.color ?? folderColor);
    const ry = y + k * TAKE_ROW_H;
    const rh = TAKE_ROW_H - 2;
    // The take's own extent within the folder.
    const tx0 = Math.max(x, x + (t.start - tt0) * pps);
    const tx1 = Math.min(x + w, x + (t.end - tt0) * pps);
    ctx.save();
    ctx.beginPath();
    ctx.rect(visX0, ry, Math.max(0, visX1 - visX0), rh + 1);
    ctx.clip();
    ctx.fillStyle = mix(color, theme.regionBase, light ? 0.75 : 0.82, 0.9);
    ctx.fillRect(tx0, ry + 0.5, Math.max(0, tx1 - tx0), rh);
    // A MIDI take shows its notes, bright when it's the one heard.
    if (isMidiFolder(r)) {
      if (r.takes.comp.some((s) => s.take === t.id)) {
        ctx.fillStyle = light ? mix(color, "#ffffff", 0.45) : mix(color, theme.regionBase, 0.2, 0.9);
        ctx.fillRect(tx0, ry + 0.5, Math.max(0, tx1 - tx0), rh);
      }
      drawTakeNotes(ctx, t, x, tt0, ry + 2, rh - 4, pps, light ? mix(color, "#000000", 0.5) : theme.ink, visX0, visX1);
      ctx.fillStyle = light ? "rgba(0,0,0,0.6)" : "rgba(255,255,255,0.75)";
      ctx.font = canvasFont("sm", 600);
      ctx.textBaseline = "middle";
      ctx.fillText(takeLabel(t), visX0 + 4, ry + rh / 2 + 0.5);
      ctx.restore();
      return;
    }
    const wave = waveCache.get(t.clip);
    const scale = 10 ** (t.gain_db / 20);
    const segs = comp.filter((s) => s.take === t.id).map((s) => [s.start, s.end]);
    if (preview && preview.take === t.id) segs.push([preview.start, preview.end]);
    // Selected stretches: a bright body under the waveform.
    for (const [a, b] of segs) {
      const sx0 = Math.max(tx0, x + (a - tt0) * pps);
      const sx1 = Math.min(tx1, x + (b - tt0) * pps);
      if (sx1 <= sx0) continue;
      ctx.fillStyle = light ? mix(color, "#ffffff", 0.45) : mix(color, theme.regionBase, 0.2, 0.9);
      ctx.fillRect(sx0, ry + 0.5, sx1 - sx0, rh);
    }
    if (wave) {
      ctx.globalAlpha = 0.45;
      drawWave(ctx, wave, t.offset + tt0, x, w, ry + 2, rh - 4, pps, light ? mix(color, "#000000", 0.4) : mix(color, "#ffffff", 0.55), tx0, tx1, undefined, scale);
      ctx.globalAlpha = 1;
      for (const [a, b] of segs) {
        const sx0 = Math.max(tx0, x + (a - tt0) * pps);
        const sx1 = Math.min(tx1, x + (b - tt0) * pps);
        if (sx1 > sx0) drawWave(ctx, wave, t.offset + tt0, x, w, ry + 2, rh - 4, pps, light ? mix(color, "#000000", 0.55) : theme.ink, sx0, sx1, undefined, scale);
      }
    }
    // The selected comp section: an outline around it in its take's row.
    if (selected?.take === t.id) {
      const sx0 = Math.max(tx0, x + (selected.start - tt0) * pps);
      const sx1 = Math.min(tx1, x + (selected.end - tt0) * pps);
      ctx.strokeStyle = light ? mix(color, "#000000", 0.6) : theme.ink;
      ctx.lineWidth = 1.5;
      ctx.strokeRect(sx0 + 0.75, ry + 1.25, Math.max(0, sx1 - sx0 - 1.5), rh - 1.5);
    }
    ctx.fillStyle = light ? "rgba(0,0,0,0.6)" : "rgba(255,255,255,0.75)";
    ctx.font = canvasFont("sm", 600);
    ctx.textBaseline = "middle";
    ctx.fillText(takeLabel(t), visX0 + 4, ry + rh / 2 + 0.5);
    ctx.restore();
  });
}

/** A MIDI take's notes in its row, spread over the take's pitch range. */
function drawTakeNotes(ctx: CanvasRenderingContext2D, t: Take, x: number, tt0: number, top: number, h: number, pps: number, ink: string, visX0: number, visX1: number) {
  const notes = t.notes ?? [];
  if (!notes.length) return;
  let lo = 127;
  let hi = 0;
  for (const n of notes) {
    lo = Math.min(lo, n.pitch);
    hi = Math.max(hi, n.pitch);
  }
  const span = Math.max(12, hi - lo + 4);
  const nh = Math.max(1.5, Math.min(3, h / span));
  ctx.fillStyle = ink;
  ctx.globalAlpha = 0.8;
  for (const n of notes) {
    const nx = x + (n.start - tt0) * pps;
    const nw = Math.max(2, n.length * pps);
    if (nx > visX1 || nx + nw < visX0) continue;
    ctx.fillRect(nx, top + h - ((n.pitch - lo + 2) / span) * h - nh / 2, nw - 0.5, nh);
  }
  ctx.globalAlpha = 1;
}
