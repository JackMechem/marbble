// Painting the overview strip: the project's regions drawn small, track by track, on a layer drawn
// again only when the project or the layout changes, and the playhead and visible-area box over it.
import type { Project, Track } from "../../engine/project";
import type { Region } from "../../engine/regions";
import type { Layout } from "../trackLayout";
import { regionEnd } from "../regions/loops";
import { mix, theme } from "../../theme/canvasTheme";
import { OVERVIEW_BAND, overviewRows, timeToX, type StripMap } from "./overviewModel";

/** Each stack's subtracks (all levels), for drawing a closed stack's row. */
function stackContents(tracks: Track[]): Map<number, Track[]> {
  const out = new Map<number, Track[]>();
  const byId = new Map(tracks.map((t) => [t.id, t]));
  for (const t of tracks) {
    for (let p = t.parent != null ? byId.get(t.parent) : undefined, d = 0; p && d < 32; p = p.parent != null ? byId.get(p.parent) : undefined, d++) {
      const list = out.get(p.id) ?? [];
      list.push(t);
      out.set(p.id, list);
    }
  }
  return out;
}

/** A region's bar colour: its colour (or its track's), softened toward the lane so a strip of
 *  saturated colours stays calm. Muted regions are grey, as in the Tracks area. */
function barColor(r: Region, t: Track): string {
  const c = r.muted ? "#6c6c70" : r.color ?? t.color;
  return theme.region === "light" ? mix(c, "#ffffff", 0.12) : mix(c, theme.lane, 0.28);
}

export interface Geometry {
  /** Strip size in CSS px and the device pixel ratio. */
  W: number;
  H: number;
  dpr: number;
  map: StripMap;
}

/**
 * The region layer: lanes, the area past the project end, the cycle range, markers and
 * every region as a bar on its track's row. Drawn in device pixels so hairlines stay sharp.
 */
export function drawStatic(ctx: CanvasRenderingContext2D, g: Geometry, p: Project, layout: Layout, regionsEnd: number) {
  const { dpr, map } = g;
  const DW = ctx.canvas.width;
  const DH = ctx.canvas.height;
  const X = (t: number) => Math.round(timeToX(t, map.span, map.width) * dpr);
  const px = Math.max(1, Math.round(dpr));
  const light = theme.region === "light";
  ctx.setTransform(1, 0, 0, 1, 0, 0);

  // The project, on the lane colour; the timeline past its end a shade darker.
  ctx.fillStyle = theme.lane;
  ctx.fillRect(0, 0, DW, DH);
  const endX = regionsEnd > 0 ? X(regionsEnd) : DW;
  if (endX < DW) {
    ctx.fillStyle = theme.arrangeBg;
    ctx.fillRect(endX, 0, DW - endX, DH);
    ctx.fillStyle = light ? "rgba(0,0,0,0.035)" : "rgba(0,0,0,0.22)";
    ctx.fillRect(endX, 0, DW - endX, DH);
  }

  // Cycle range: a thin bar in the top band (yellow when on, grey when off), and the same
  // faint tint the tracks get when it's on.
  const c = p.cycle;
  if (c.end > c.start) {
    const x0 = X(c.start);
    const w = Math.max(px, X(c.end) - x0);
    if (c.enabled) {
      ctx.fillStyle = theme.cycleTint;
      ctx.fillRect(x0, 0, w, DH);
    }
    ctx.fillStyle = c.enabled ? theme.cycleOn : theme.cycleOff;
    ctx.fillRect(x0, 0, w, Math.round(2 * dpr));
  }

  // Regions. Rows thinner than a pixel still get a pixel, so a big session reads as texture
  // rather than vanishing; rows with room are inset to leave a gap between tracks.
  const rows = overviewRows(layout.tops, layout.heights, layout.total, map.rowsTop, map.rowsH);
  const stacks = stackContents(p.tracks);
  p.tracks.forEach((t, i) => {
    if (!layout.heights[i]) return;
    const y0 = Math.round(rows[i].y * dpr);
    const rh = Math.max(1, Math.round((rows[i].y + rows[i].h) * dpr) - y0);
    const inset = rh >= 5 ? Math.max(1, Math.round(rh * 0.16)) : 0;
    const y = y0 + inset;
    const h = Math.max(1, rh - inset * 2);
    const alpha = t.hidden ? 0.45 : 1;
    const bars = (owner: Track, a: number) => {
      for (const r of owner.regions) {
        const x0 = X(r.start);
        const own = X(r.start + r.length);
        const end = X(regionEnd(r));
        if (end < 0 || x0 > DW) continue;
        ctx.fillStyle = barColor(r, owner);
        // A one-pixel gap after each region so neighbours stay apart when there's room.
        const gap = own - x0 > 3 * px ? px : 0;
        ctx.globalAlpha = a;
        ctx.fillRect(x0, y, Math.max(px, own - x0 - gap), h);
        if (end > own) {
          ctx.globalAlpha = a * 0.45;
          ctx.fillRect(own, y, Math.max(px, end - own - gap), h);
        }
      }
    };
    bars(t, alpha);
    // A stack's row shows its subtracks' regions, like its lane in the Tracks area; faintly
    // when the stack is open and they have rows of their own.
    if (t.kind === "folder" || t.kind === "summing") for (const s of stacks.get(t.id) ?? []) bars(s, alpha * (t.collapsed ? 1 : 0.35));
  });
  ctx.globalAlpha = 1;

  // Markers: ticks in the top band, in their colour.
  for (const m of p.markers) {
    ctx.fillStyle = m.color ?? theme.rulerText;
    ctx.fillRect(X(m.pos), 0, px, Math.round(OVERVIEW_BAND * dpr));
  }

  // The project end.
  if (endX < DW) {
    ctx.fillStyle = theme.rulerTick;
    ctx.fillRect(endX, 0, px, DH);
  }
}

/** The visible-area box and the playhead over the region layer. Everything outside the box is
 *  veiled, so the box reads as the lit part, as in Ardour. */
export function drawOverlay(ctx: CanvasRenderingContext2D, layer: HTMLCanvasElement, box: [number, number, number, number], phx: number, dpr: number) {
  const DW = ctx.canvas.width;
  const DH = ctx.canvas.height;
  const px = Math.max(1, Math.round(dpr));
  const light = theme.region === "light";
  const [x0, y0, x1, y1] = box;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.drawImage(layer, 0, 0);

  ctx.fillStyle = light ? "rgba(0,0,0,0.07)" : "rgba(0,0,0,0.3)";
  ctx.fillRect(0, 0, x0, DH);
  ctx.fillRect(x1, 0, DW - x1, DH);
  ctx.fillRect(x0, 0, x1 - x0, y0);
  ctx.fillRect(x0, y1, x1 - x0, DH - y1);

  ctx.fillStyle = mix(theme.text, theme.text, 0, light ? 0.05 : 0.07);
  ctx.fillRect(x0, y0, x1 - x0, y1 - y0);
  ctx.fillStyle = mix(theme.text, theme.text, 0, light ? 0.55 : 0.6);
  ctx.fillRect(x0, y0, x1 - x0, px);
  ctx.fillRect(x0, y1 - px, x1 - x0, px);
  ctx.fillRect(x0, y0, px, y1 - y0);
  ctx.fillRect(x1 - px, y0, px, y1 - y0);

  if (phx >= 0 && phx < DW) {
    ctx.fillStyle = theme.playhead;
    ctx.fillRect(phx, 0, px, DH);
  }
}
