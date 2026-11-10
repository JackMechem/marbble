// Painting the Tracks area's canvas. TracksCanvas.tsx calls this at most once per display frame; it reads the
// store and the theme itself, so nothing here is React state.
import { pixelRatio } from "../../ui/uiScale";
import { type Id, type Project, type Track } from "../../engine/project";
import { type Region } from "../../engine/regions";
import { getState, liveTakes } from "../../state/store";
import { offTracks } from "../trackOff";
import { drawWave } from "../regions/waveform";
import { PEAK_BUCKET, toBbt } from "../../lib/time";
import { snapToTracksGrid } from "../gridSnap";
import { forEachGridLine, tempoMap } from "../../lib/tempo";
import { mix, theme } from "../../theme/canvasTheme";
import { type Layout } from "../trackLayout";
import { viewport } from "../viewport";
import { isLooped, loopEnd, regionEnd } from "../regions/loops";
import { drawAutomation } from "../automation/canvas";
import { isFolder, isOpen } from "../takes/takes";
import { drawTakeRows } from "../takes/takeDraw";
import { drawnBar, regionBar } from "../regions/regionBar";
import { dragOutcome, type Outcome } from "../regions/dragModes";
import { movedStart } from "../regions/musicalMove";
import { newLaneId } from "../regions/newTrackDrag";
import { drawGainHandle, gainDragging, withLiveGain } from "../regions/regionGainDrag";
import { drawToolOverlay } from "../tools/trackTools";
import { drumMarks } from "../drumReplace/drumReplace";
import { drawLoops, drawNewLane, drawRegion, MUTED_COLOR, roundRect } from "../regions/regionDraw";
import { dragNewLanes, dragTarget, type TracksRefs } from "./drag";
import type { Viewport } from "../viewport";
import { canvasFont } from "../../ui/canvasText";

type Refs = TracksRefs & {
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  positionPlayhead: () => void;
};

/** One repaint of the whole canvas: lanes, grid, regions, take rows, automation, recordings in
 *  progress, and whatever a drag is showing. */
export function drawTracks({ canvasRef, live, dragRef, swipeRef, marqueeRef, hoverRef, positionPlayhead }: Refs) {
  const cv = canvasRef.current;
  const { project, selectedTrack, selectedRegions, pps, layout } = live.current;
  if (!cv || !project) return;
  const vp = viewport();
  const dpr = pixelRatio();
  const W = vp.width;
  const H = vp.height;
  if (cv.width !== Math.round(W * dpr) || cv.height !== Math.round(H * dpr)) {
    cv.width = Math.round(W * dpr);
    cv.height = Math.round(H * dpr);
    cv.style.width = `${W}px`;
    cv.style.height = `${H}px`;
  }
  const ctx = cv.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = theme.arrangeBg;
  ctx.fillRect(0, 0, W, H);

  // Lanes a drag past the last track will create, below it.
  const drag = dragRef.current;
  const newLaneOf = dragNewLanes(layout, drag, project.tracks);
  const newLaneH = getState().trackHeight;
  const newLanesBottom = layout.total + newLaneOf.size * newLaneH;
  const newLaneTop = (lane: number) => layout.total + lane * newLaneH - vp.top;
  // Lanes (hidden tracks, shown in hide mode, are dimmed)
  const multi = getState().selectedTracks;
  project.tracks.forEach((t, i) => {
    const y = layout.tops[i] - vp.top;
    const th = layout.heights[i];
    if (th === 0 || y > H || y + th < 0) return;
    ctx.fillStyle = t.id === selectedTrack || (selectedTrack !== null && multi.includes(selectedTrack) && multi.includes(t.id)) ? theme.laneSelected : theme.lane;
    if (t.hidden) ctx.globalAlpha = 0.45;
    ctx.fillRect(0, y, W, th - 1);
    ctx.globalAlpha = 1;
  });
  // The Output track's lane (it has no regions; automation draws over it).
  if (layout.output) {
    ctx.fillStyle = theme.lane;
    ctx.fillRect(0, layout.output.top - vp.top, W, layout.output.height + layout.output.autoRows - 1);
  }
  for (let k = 0; k < newLaneOf.size; k++) {
    ctx.fillStyle = theme.lane;
    ctx.fillRect(0, newLaneTop(k), W, newLaneH - 1);
  }

  // Grid, through the tempo map: bar lines, and beats when they are far enough apart.
  const gridBottom = Math.min(H, newLanesBottom - vp.top);
  forEachGridLine(tempoMap(project), Math.max(0, vp.left / pps), (vp.left + W) / pps, pps, { beatsMinPx: 9 }, (pos, kind) => {
    ctx.fillStyle = kind === "bar" ? theme.barLine : theme.beatLine;
    ctx.fillRect(kind === "bar" ? Math.round(pos * pps - vp.left) + 0.5 : Math.round(pos * pps - vp.left + 0.5), 0, 1, gridBottom);
  });
  // Cycle range tint, as Logic does when cycle is on.
  if (project.cycle.enabled) {
    const cx = project.cycle.start * pps - vp.left;
    const cw = (project.cycle.end - project.cycle.start) * pps;
    ctx.fillStyle = theme.cycleTint;
    ctx.fillRect(cx, 0, cw, gridBottom);
  }

  // A stack's lane shows where its subtracks have regions.
  drawStackLanes(ctx, project, layout, vp, pps, W, H);

  // The new lanes' ghosts, as a file dragged below the last track draws its own.
  if (newLaneOf.size) {
    const css = getComputedStyle(document.documentElement);
    for (let k = 0; k < newLaneOf.size; k++) drawNewLane(ctx, newLaneTop(k), newLaneH, W, css.getPropertyValue("--accent").trim() || "#4c9eff", css.getPropertyValue("--accent-ink").trim() || "#fff");
  }

  // Regions
  // In the No Overlap and Shuffle drag modes, where the drag will leave every region.
  let outcome: Map<Id, Outcome> | null = null;
  // Where each region draws, with any drag in progress applied.
  const off = offTracks(project.tracks);
  const place = (r: Region, i: number, still = false) => {
    let start = r.start;
    let length = r.length;
    let loop = r.loop_end;
    let ti = i;
    // Past the last track: the new lane it lands on.
    let lane: number | undefined;
    let shift = 0;
    let fades: [number, number] | undefined;
    if (drag && !still && drag.ids.includes(r.id)) {
      if (drag.kind === "move") {
        const from = getGrabbedStart();
        start = movedStart(tempoMap(project), r.start, from, Math.max(0, snapToTracksGrid(project, from + drag.dx)), getState().snapMode !== "off");
        if (loop != null) loop += start - r.start;
        lane = newLaneOf.get(i);
        if (lane === undefined) ti = dragTarget(layout, i, drag.dTrack, project.tracks);
      } else if (r.id === drag.grabbed && drag.fade !== undefined && r.content.type === "audio") {
        fades = drag.kind === "fadeIn" ? [drag.fade, r.content.fade_out] : [r.content.fade_in, drag.fade];
      } else if (r.id === drag.grabbed && drag.loopEnd !== undefined) {
        loop = drag.loopEnd;
      } else if (drag.spans?.has(r.id)) {
        // A trim: every selected region's edge moves with the grabbed one's.
        const s = drag.spans.get(r.id)!;
        shift = s.start - r.start;
        start = s.start;
        length = s.length;
      }
    }
    const o = still ? undefined : outcome?.get(r.id);
    if (o?.start !== undefined) {
      if (loop != null) loop += o.start - start;
      start = o.start;
    }
    const gone = o?.len !== undefined && o.len <= 0;
    if (o?.len !== undefined && o.len < length) {
      length = Math.max(0, o.len);
      loop = null;
    } else if (o?.len !== undefined) loop = start + o.len;
    // A looping region's fade-out is drawn where the loop ends.
    if (r.content.type === "audio" && loopEnd(start, length, loop) > start + length) fades = [fades?.[0] ?? r.content.fade_in, 0];
    const y = (lane !== undefined ? newLaneTop(lane) : layout.tops[ti] - vp.top) + 1;
    // Open take folders' rows sit below the lane; regions keep the lane's own height.
    const th = lane !== undefined ? newLaneH : layout.heights[ti] - layout.takeRows[ti];
    const dim = project.tracks[ti].hidden || off.has(project.tracks[ti].id);
    return { start, length, loop, ti, lane, shift, fades, y, th, hidden: lane === undefined && dim, gone };
  };
  const mode = project.drag_mode ?? "overlap";
  if (drag && !drag.copy && mode !== "overlap" && mode !== "xfade") {
    const lanes = new Map(project.tracks.map((t) => [t.id, t.regions.map((r) => ({ id: r.id, start: r.start, len: regionEnd(r) - r.start }))]));
    for (const lane of newLaneOf.values()) lanes.set(newLaneId(lane), []);
    const grabbed = project.tracks.flatMap((t) => t.regions).find((r) => r.id === drag.grabbed);
    if (drag.kind === "move") {
      const moved = project.tracks.flatMap((t, i) =>
        t.regions
          .filter((r) => drag.ids.includes(r.id))
          .map((r) => {
            const g = place(r, i);
            return { id: r.id, track: g.lane !== undefined ? newLaneId(g.lane) : project.tracks[g.ti].id, start: g.start };
          }),
      );
      outcome = dragOutcome(mode, lanes, { kind: "move", moved });
    } else if (grabbed && (drag.kind === "left" || drag.kind === "right") && drag.start !== undefined && drag.length !== undefined) {
      const len = loopEnd(drag.start, drag.length, grabbed.loop_end) - drag.start;
      outcome = dragOutcome(mode, lanes, { kind: "resize", id: drag.grabbed, start: drag.start, len });
    }
  }
  // A muted track's regions are drawn grey, as a muted region is (without "(muted)" in the name).
  const shown = (r: Region, t: Track): Region => (t.mute && !r.muted ? { ...r, color: MUTED_COLOR } : r);
  // Loop repetitions first: region bodies cover them, as they do in playback.
  // A copy drag (Alt) leaves the originals in place and draws the copies where they'll land.
  const passes = (r: Region) => (drag?.copy && drag.ids.includes(r.id) ? [true, false] : [false]);
  project.tracks.forEach((t, i) => {
    for (const r of t.regions) for (const still of passes(r)) {
      if (!isLooped(r) && !(drag?.kind === "loop" && drag.grabbed === r.id)) continue;
      const g = place(r, i, still);
      if (g.gone) continue;
      const x0 = (g.start + g.length) * pps - vp.left;
      const x1 = loopEnd(g.start, g.length, g.loop) * pps - vp.left;
      if (g.th === 0 || x0 > W || x1 < 0 || g.y > H || g.y + g.th < 0) continue;
      ctx.globalAlpha = g.hidden ? 0.4 : 1;
      drawLoops(ctx, shown(withLiveGain(r), t), t, -vp.left, g.start, g.length, g.loop, g.y, g.th - 3, selectedRegions.includes(r.id), pps, W, g.shift);
      ctx.globalAlpha = 1;
    }
  });
  project.tracks.forEach((t, i) => {
    for (const r of t.regions) for (const still of passes(r)) {
      const { start, length, shift, fades, y, th, hidden, gone } = place(r, i, still);
      const x = start * pps - vp.left;
      const w = Math.max(2, length * pps);
      if (gone || th === 0 || x > W || x + w < 0 || y > H || y + th < 0) continue;
      const sel = selectedRegions.includes(r.id);
      ctx.globalAlpha = hidden ? 0.4 : 1;
      drawRegion(ctx, shown(withLiveGain(r), t), t, x, y, w, th - 3, sel, pps, W, shift, fades, r.content.type === "audio" && (sel || hoverRef.current === r.id) && !t.frozen);
      if (!t.frozen && !drag && (hoverRef.current === r.id || gainDragging(r.id))) drawGainHandle(ctx, withLiveGain(r), x, y, w, th - 3, W);
      if (t.frozen) {
        // Frozen: a frosted veil over the region, which can't be edited.
        ctx.globalAlpha = 0.28;
        ctx.fillStyle = theme.arrangeBg;
        ctx.fillRect(x, y, w, th - 3);
      }
      ctx.globalAlpha = 1;
    }
  });

  // Open take folders: a row per take below the lane.
  project.tracks.forEach((t, i) => {
    if (!layout.takeRows[i]) return;
    const rowsTop = layout.tops[i] + layout.heights[i] - layout.takeRows[i] - vp.top;
    if (rowsTop > H || rowsTop + layout.takeRows[i] < 0) return;
    for (const r of t.regions) {
      if (!isFolder(r) || !isOpen(r.id)) continue;
      const x = r.start * pps - vp.left;
      const w = Math.max(2, r.length * pps);
      if (x > W || x + w < 0) continue;
      const sw = swipeRef.current;
      drawTakeRows(ctx, r, r.muted || t.mute ? MUTED_COLOR : r.color ?? t.color, x, w, rowsTop, pps, W, sw && sw.region === r.id ? sw : null);
    }
  });

  // Drum Replacement's hits, while its sheet is open.
  drawDrumMarks(ctx, project, layout, vp, pps, W);

  if (getState().showAutomation) drawAutomation(ctx, { project, layout, pps, vp });

  // Recording in progress
  drawLiveTakes(ctx, project, layout, vp, pps, W);
  // Where dragged audio files will land: the target lane (or a ghost lane for a new track),
  // a line at the snapped position, and a bar/beat tag.
  drawDropTarget(ctx, project, layout, vp, pps, W);
  drawToolOverlay(ctx, { project, layout, pps, vp, W, H });
  if (marqueeRef.current) drawMarquee(ctx, marqueeRef.current, vp);
  // A hoisted function so `place` above can use it: the other dragged regions follow the
  // grabbed one's snapped move, which keeps their spacing.
  function getGrabbedStart() {
    for (const t of project!.tracks) for (const r of t.regions) if (r.id === drag!.grabbed) return r.start;
    return 0;
  }
  positionPlayhead();
}

function drawStackLanes(ctx: CanvasRenderingContext2D, project: Project, layout: Layout, vp: Viewport, pps: number, W: number, H: number) {
  project.tracks.forEach((t, i) => {
    if (t.kind !== "folder" && t.kind !== "summing") return;
    const y = layout.tops[i] - vp.top;
    const th = layout.heights[i];
    if (th === 0 || y > H || y + th < 0) return;
    const inside = new Set<number>([t.id]);
    for (const c of project.tracks) if (c.parent !== null && inside.has(c.parent)) inside.add(c.id);
    const subs = project.tracks.filter((c) => c.id !== t.id && inside.has(c.id) && c.regions.length);
    // A row per subtrack down the lane; on short lanes the margins shrink and the rows
    // thin out (down to a pixel), clipped so they never spill into the next lane.
    const pad = th >= 30 ? 5 : 3;
    const rowH = Math.min(8, (th - 1 - 2 * pad) / Math.max(1, subs.length));
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, y + pad, W, Math.max(0, th - 1 - 2 * pad));
    ctx.clip();
    subs.forEach((c, k) => {
      ctx.fillStyle = mix(c.color, theme.regionBase, 0.25, 0.85);
      for (const r of c.regions) {
        const x = r.start * pps - vp.left;
        const w = Math.max(2, (regionEnd(r) - r.start) * pps);
        if (x > W || x + w < 0) continue;
        ctx.fillRect(x, y + pad + k * rowH, w, Math.max(1, rowH - 1));
      }
    });
    ctx.restore();
  });
}

/** Where Drum Replacement will put its notes: a line down the audio track's lane at each hit
 *  the threshold keeps, with a tick at the top. */
function drawDrumMarks(ctx: CanvasRenderingContext2D, project: Project, layout: Layout, vp: Viewport, pps: number, W: number) {
  const marks = drumMarks();
  const i = marks ? project.tracks.findIndex((t) => t.id === marks.track) : -1;
  if (!marks || i < 0 || !layout.heights[i]) return;
  const y = layout.tops[i] - vp.top + 1;
  const h = layout.heights[i] - layout.takeRows[i] - 3;
  ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#4c9eff";
  for (const at of marks.at) {
    const x = Math.round(at * pps - vp.left);
    if (x < -4 || x > W + 4) continue;
    ctx.globalAlpha = 0.85;
    ctx.fillRect(x, y, 1, h);
    ctx.globalAlpha = 1;
    ctx.beginPath();
    ctx.moveTo(x - 3.5, y);
    ctx.lineTo(x + 4.5, y);
    ctx.lineTo(x + 0.5, y + 5);
    ctx.fill();
  }
}

/** A take being recorded on its track. */
function drawLiveTakes(ctx: CanvasRenderingContext2D, project: Project, layout: Layout, vp: Viewport, pps: number, W: number) {
  for (const [trackId, take] of liveTakes) {
    const i = project.tracks.findIndex((t) => t.id === trackId);
    if (i < 0) continue;
    // Cycle recording: show the pass in progress, over the cycle.
    const c = project.cycle;
    const pass = !c.enabled || take.start < c.start || take.start >= c.end ? 0 : project.punch.enabled ? Math.min(project.punch.end, c.end) - Math.max(project.punch.start, c.start) : c.end - c.start;
    const passFrom = pass > 0 ? Math.floor(take.frames / pass) * pass : 0;
    const x = take.start * pps - vp.left;
    const w = Math.max(2, (take.frames - passFrom) * pps);
    const y = layout.tops[i] - vp.top + 1;
    const h = layout.heights[i] - layout.takeRows[i] - 3;
    ctx.save();
    roundRect(ctx, x, y, w, h, 2);
    ctx.fillStyle = theme.region === "light" ? mix(theme.red, "#ffffff", 0.6) : mix(theme.red, theme.regionBase, 0.5);
    ctx.fill();
    ctx.clip();
    ctx.fillStyle = theme.red;
    const g = regionBar(h);
    ctx.fillRect(x, y, w, drawnBar(g, h));
    if (!g.barOnly) drawWave(ctx, { frames: take.frames, bucket: PEAK_BUCKET, levels: [{ min: null, max: take.peaks }] }, passFrom, x, w, y + g.bodyTop, g.bodyH, pps, theme.region === "light" ? mix(theme.red, "#000000", 0.35) : mix(theme.red, "#ffffff", 0.4), 0, W);
    ctx.restore();
  }
}

function drawDropTarget(ctx: CanvasRenderingContext2D, project: Project, layout: Layout, vp: Viewport, pps: number, W: number) {
  const drop = getState().dropTarget;
  if (drop && drop.kind !== "headers") {
    const css = getComputedStyle(document.documentElement);
    const accent = css.getPropertyValue("--accent").trim() || "#4c9eff";
    const ink = css.getPropertyValue("--accent-ink").trim() || "#fff";
    const x = Math.round(drop.sample * pps - vp.left);
    const n = project.tracks.length;
    const isNew = drop.kind === "new";
    const y = (drop.kind === "lane" ? layout.tops[drop.index] : drop.index < n ? layout.tops[drop.index] : layout.total) - vp.top;
    const h = drop.kind === "lane" ? layout.heights[drop.index] - 1 : getState().trackHeight;
    ctx.fillStyle = accent;
    ctx.globalAlpha = isNew ? 0.12 : 0.2;
    ctx.fillRect(0, y, W, h);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    if (isNew) ctx.setLineDash([6, 4]);
    ctx.strokeRect(1, y + 1, W - 2, h - 2);
    ctx.setLineDash([]);
    ctx.fillRect(x - 1, y, 2, h);
    const b = toBbt(project, drop.sample);
    const label = `${isNew ? "New track · " : ""}${b.bar} ${b.beat}`;
    ctx.font = canvasFont("sm", 600);
    const tw = ctx.measureText(label).width + 12;
    // The tag shrinks to fit the shortest lanes.
    const tagH = Math.min(18, h - 4);
    const tagY = y + Math.min(4, (h - tagH) / 2);
    ctx.beginPath();
    ctx.roundRect(x + 4, tagY, tw, tagH, 4);
    ctx.fill();
    ctx.fillStyle = ink;
    ctx.textBaseline = "middle";
    ctx.fillText(label, x + 10, tagY + tagH / 2 + 0.5);
  }
}

/** The rubber band from empty lane space. */
function drawMarquee(ctx: CanvasRenderingContext2D, mq: { left: number; top: number; right: number; bottom: number }, vp: Viewport) {
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#4c9eff";
  const x = Math.round(mq.left - vp.left) + 0.5;
  const y = Math.round(mq.top - vp.top) + 0.5;
  const w = Math.round(mq.right - mq.left);
  const h = Math.round(mq.bottom - mq.top);
  ctx.fillStyle = accent;
  ctx.globalAlpha = 0.12;
  ctx.fillRect(x, y, w, h);
  ctx.globalAlpha = 0.8;
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1;
  ctx.strokeRect(x, y, w, h);
  ctx.globalAlpha = 1;
}
