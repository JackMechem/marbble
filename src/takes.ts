// Take folders in the arrange area, as in Logic: a folder region shows its comp, a disclosure
// triangle opens it into one row per take below the track's lane, and swiping across a take
// row puts that stretch of the take in the comp (Quick Swipe Comping). The comp logic lives in
// the engine (crates/engine/src/takes.rs); this file draws folders and sends the edits.
import { ask } from "@tauri-apps/plugin-dialog";
import { act, actMade, type CompSegment, type Id, type Region, type Take, type TakeFolder, type TakeRating, type Track } from "./api";
import { compNames, currentComp, isMidiFolder, movedEdge, neighbourTake, sectionAt } from "./comps";
import type { MenuItem } from "./components/Menu";
import { askName, notEmpty } from "./components/NameSheet";
import { colorMenu } from "./ops";
import { emptiedQuestion, emptiedTracks, packTracks } from "./packTakes";
import { MUTED_COLOR } from "./regionDraw";
import { getState, setState } from "./store";
import { hasBadTakes, RATING_MARKS, takeLabel, toggledRating } from "./takeRatings";
import { mix, theme } from "./theme";
import { drawWave, ensureWave, waveCache } from "./waveform";

/** Height of one take row in an open folder. */
export const TAKE_ROW_H = 26;
/** Width of the triangle and take badge at the start of a folder's name bar. */
export const FOLDER_BADGE_W = 34;

/** A comp edit being dragged in an open folder's take rows, in take time: a swipe of `take` over
 *  `start..end`, or the comp section edge at `edge` moved to `to`. */
export type CompDrag = { region: Id; take: Id; start: number; end: number } | { region: Id; edge: number; to: number };

/** An audio or MIDI take folder. */
export const isFolder = (r: Region): r is Region & { takes: TakeFolder } => !!r.takes;

/** Folder-relative offset: a timeline position `p` is take time `p + shift(r)`. */
export const takeShift = (r: Region) => (r.content.type === "audio" ? r.content.offset : (r.takes?.offset ?? 0)) - r.start;

/** The take heard throughout when the comp is one whole take, else null (a comp). */
export function wholeTake(f: TakeFolder): Id | null {
  if (f.comp.length !== 1) return null;
  const s = f.comp[0];
  const t = f.takes.find((x) => x.id === s.take);
  return t && t.start === s.start && t.end === s.end ? t.id : null;
}

/** What the folder's badge says: the whole take's number, or "C" for a comp. */
export function badgeText(f: TakeFolder): string {
  const w = wholeTake(f);
  if (w === null) return f.comp.length ? "C" : "–";
  return String(f.takes.findIndex((t) => t.id === w) + 1);
}

// ---- open folders: a view setting, like track heights ---------------------------------------

export function isOpen(id: Id): boolean {
  return getState().openFolders.includes(id);
}

export function toggleFolder(id: Id) {
  const open = getState().openFolders;
  setState({ openFolders: open.includes(id) ? open.filter((x) => x !== id) : [...open, id] });
}

/** Extra lane height a track gets for its open folders (the tallest one). */
export function takeRowsHeight(t: Track, open: Id[]): number {
  let rows = 0;
  for (const r of t.regions) if (r.takes && open.includes(r.id)) rows = Math.max(rows, r.takes.takes.length);
  return rows * TAKE_ROW_H;
}

/** Loads the waveforms of every take, not just each folder's own clip. */
export function ensureTakeWaves(tracks: Track[], done: () => void) {
  for (const t of tracks) for (const r of t.regions) if (r.content.type === "audio") for (const k of r.takes?.takes ?? []) ensureWave(k.clip, done);
}

// ---- drawing ----------------------------------------------------------------------------------

/** The disclosure triangle and the take badge, at the start of the folder's name bar. */
export function drawFolderBadge(ctx: CanvasRenderingContext2D, r: Region & { takes: TakeFolder }, x: number, y: number, ink: string) {
  const open = isOpen(r.id);
  const tx = Math.max(x, 0) + 5;
  ctx.fillStyle = ink;
  ctx.beginPath();
  if (open) {
    ctx.moveTo(tx, y + 5);
    ctx.lineTo(tx + 8, y + 5);
    ctx.lineTo(tx + 4, y + 10);
  } else {
    ctx.moveTo(tx + 1, y + 3);
    ctx.lineTo(tx + 6, y + 7);
    ctx.lineTo(tx + 1, y + 11);
  }
  ctx.closePath();
  ctx.fill();
  const bx = tx + 12;
  ctx.beginPath();
  ctx.roundRect(bx, y + 2, 16, 10, 2);
  ctx.globalAlpha *= 0.85;
  ctx.fill();
  ctx.globalAlpha /= 0.85;
  ctx.fillStyle = theme.region === "light" ? "#ffffff" : mix(ink, "#ffffff", 0.9);
  ctx.font = "700 9px Inter, 'SF Pro Text', system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.fillText(badgeText(r.takes), bx + 8, y + 7.5);
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
      ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
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
    ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
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

// ---- edits --------------------------------------------------------------------------------------

/** Puts `take` in the comp over timeline `start..end` of the folder (a swipe). */
export function swipeComp(r: Region, take: Id, start: number, end: number) {
  act("set_comp_range", { region: r.id, take, start: Math.round(Math.min(start, end)), end: Math.round(Math.max(start, end)) });
}

/** The take row under content y, for an open folder whose rows start at `rowsTop`. */
export function takeAtRow(r: Region & { takes: TakeFolder }, y: number, rowsTop: number): Take | null {
  const k = Math.floor((y - rowsTop) / TAKE_ROW_H);
  return k >= 0 && k < r.takes.takes.length ? r.takes.takes[k] : null;
}

/** Drags the comp edge at timeline `at` to `to`: a border moves both sections, an outer edge
 *  lengthens or shortens its own. */
export function moveCompEdge(r: Region, at: number, to: number) {
  if (at !== to) act("move_comp_edge", { region: r.id, at: Math.round(at), to: Math.round(to) });
}

/** The selected comp section, while its folder is open and selected: the folder and the section. */
export function selectedSection(): { r: Region & { takes: TakeFolder }; s: CompSegment; at: number } | null {
  const st = getState();
  const sel = st.compSection;
  if (!sel || !st.selectedRegions.includes(sel.region) || !isOpen(sel.region)) return null;
  const r = st.snap?.project.tracks.flatMap((t) => t.regions).find((x) => x.id === sel.region);
  if (!r || !isFolder(r) || isMidiFolder(r)) return null;
  const s = sectionAt(r.takes, sel.at + takeShift(r));
  return s ? { r, s, at: sel.at } : null;
}

/** Moves the selected comp section to the take above (-1) or below (1); false when no section is
 *  selected, so the arrow keys fall back to selecting tracks. */
export function moveSelectedSection(delta: number): boolean {
  const cur = selectedSection();
  if (!cur) return false;
  if (neighbourTake(cur.r.takes, cur.s, delta) !== null) act("move_comp_section", { region: cur.r.id, at: cur.at, delta });
  return true;
}

/** Removes the selected comp section (it falls silent); false when none is selected. */
export function removeSelectedSection(): boolean {
  const cur = selectedSection();
  if (!cur) return false;
  act("remove_comp_section", { region: cur.r.id, at: cur.at });
  setState({ compSection: null });
  return true;
}

/** The comp section part of a take row's menu: for the section of `take` heard at timeline `at`. */
function sectionItems(r: Region & { takes: TakeFolder }, take: Id, at: number): MenuItem[] {
  const s = sectionAt(r.takes, at + takeShift(r));
  if (!s || s.take !== take) return [];
  const move = (delta: number) => ({
    disabled: neighbourTake(r.takes, s, delta) === null,
    onClick: () => {
      setState({ compSection: { region: r.id, at } });
      act("move_comp_section", { region: r.id, at, delta });
    },
  });
  return [
    { label: "Comp Section", header: true },
    { label: "Move Section to Take Above", ...move(-1) },
    { label: "Move Section to Take Below", ...move(1) },
    { label: "Remove Section", onClick: () => act("remove_comp_section", { region: r.id, at }) },
    { separator: true },
  ];
}

async function renameTake(r: Region, t: Take) {
  const name = await askName({ title: "Rename Take", value: t.name, ok: "Rename", check: notEmpty });
  if (name && name !== t.name) act("rename_take", { region: r.id, take: t.id, name });
}

async function renameComp(r: Region & { takes: TakeFolder }, index: number) {
  const old = compNames(r.takes)[index];
  const name = await askName({ title: "Rename Comp", value: old, ok: "Rename", check: notEmpty });
  if (name && name !== old) act("rename_comp", { region: r.id, index, name });
}

/** The comps part of an audio folder's menu, as in Logic: the comps to switch between, then
 *  Create New Comp, Duplicate Comp, Rename Comp and Delete Comp. */
function compItems(r: Region & { takes: TakeFolder }): MenuItem[] {
  const names = compNames(r.takes);
  const current = currentComp(r.takes);
  return [
    { label: "Comps", header: true },
    ...names.map((name, index) => ({ label: name, checked: index === current, onClick: () => act("select_comp", { region: r.id, index }) })),
    { separator: true },
    { label: "Create New Comp", onClick: () => act("create_comp", { region: r.id }) },
    { label: "Duplicate Comp", onClick: () => act("duplicate_comp", { region: r.id }) },
    { label: "Rename Comp…", onClick: () => renameComp(r, current) },
    { label: "Delete Comp", disabled: names.length < 2, onClick: () => act("delete_comp", { region: r.id, index: current }) },
    { separator: true },
  ];
}

/** Good and Bad for one take, ticked when it has them; choosing the ticked one clears it. */
function ratingItems(r: Region, t: Take): MenuItem[] {
  const rate = (rating: TakeRating) => act("set_take_rating", { region: r.id, take: t.id, rating: toggledRating(t.rating, rating) });
  return [
    { label: `${RATING_MARKS.good} Good`, checked: t.rating === "good", onClick: () => rate("good") },
    { label: `${RATING_MARKS.bad} Bad`, checked: t.rating === "bad", onClick: () => rate("bad") },
  ];
}

/** A take's colour menu: the folder's colour, or one of its own. */
function takeColorItems(r: Region, t: Take): MenuItem[] {
  return colorMenu(t.color, (color) => act("set_take_color", { region: r.id, take: t.id, color }), "Folder Color");
}

/** The take folder menu (the badge, or Take Folder in the region menu). A MIDI folder plays
 *  whole takes, so it has no comps, and Flatten and Merge is just Flatten. Right-clicking a take
 *  row (`row`: its take and the timeline position) puts that take's rating and colour first,
 *  then the edits of the comp section there. */
export function takeFolderMenu(r: Region & { takes: TakeFolder }, row?: { take: Id; at: number }): MenuItem[] {
  const f = r.takes;
  const midi = isMidiFolder(r);
  const whole = midi ? (f.comp[0]?.take ?? f.takes[f.takes.length - 1]?.id) : wholeTake(f);
  const select = (ids: Id[] | null) => ids && setState({ selectedRegions: ids, focus: "regions" });
  const active = whole === null ? "Comp" : "Take";
  const rowTake = row && f.takes.find((t) => t.id === row.take);
  const own: MenuItem[] = rowTake
    ? [{ label: takeLabel(rowTake), header: true }, ...ratingItems(r, rowTake), { label: "Color", children: takeColorItems(r, rowTake) }, { separator: true }]
    : [];
  return [
    ...own,
    ...(row && !midi ? sectionItems(r, row.take, row.at) : []),
    ...(midi ? [] : compItems(r)),
    { label: "Takes", header: true },
    ...f.takes.map((t, k) => ({ label: `${k + 1}  ${takeLabel(t)}`, checked: whole === t.id, onClick: () => act("select_take", { region: r.id, take: t.id }) })),
    { separator: true },
    { label: isOpen(r.id) ? "Close Take Folder" : "Open Take Folder", onClick: () => toggleFolder(r.id) },
    { label: "Rename Take", children: f.takes.map((t) => ({ label: t.name, onClick: () => renameTake(r, t) })) },
    { label: "Rate Take", children: f.takes.map((t) => ({ label: takeLabel(t), children: ratingItems(r, t) })) },
    { label: "Take Color", children: f.takes.map((t) => ({ label: takeLabel(t), children: takeColorItems(r, t) })) },
    { label: "Delete Take", disabled: f.takes.length < 2, children: f.takes.map((t) => ({ label: t.name, onClick: () => act("delete_take", { region: r.id, take: t.id }) })) },
    { label: "Delete Unused Takes", disabled: f.comp.length === 0 || new Set(f.comp.map((s) => s.take)).size === f.takes.length, onClick: () => act("delete_unused_takes", { region: r.id }) },
    { label: "Delete Bad Takes", disabled: !hasBadTakes(f), onClick: () => act("delete_bad_takes", { region: r.id }) },
    { separator: true },
    { label: "Flatten", onClick: () => actMade("flatten_take_folder", { region: r.id }).then(select) },
    ...(midi ? [] : [{ label: "Flatten and Merge", onClick: () => actMade("merge_take_folder", { region: r.id }).then(select) }]),
    { label: "Unpack to New Tracks", onClick: () => actMade("unpack_take_folder", { region: r.id }) },
    { label: "Unpack to Existing Tracks", onClick: () => actMade("unpack_to_existing_tracks", { region: r.id }) },
    { separator: true },
    // As in Logic, the items name the comp when one is heard rather than a whole take.
    { label: `Export Active ${active} to New Track`, onClick: () => actMade("export_active_take", { region: r.id, remove: false }) },
    { label: `Move Active ${active} to New Track`, onClick: () => actMade("export_active_take", { region: r.id, remove: true }) },
  ];
}

/** Previous Take (-1) and Next Take (1): the selected take folders switch one take up or down
 *  (the engine takes grouped folders along). */
export function stepTake(delta: number) {
  const s = getState();
  const folders = (s.snap?.project.tracks ?? []).flatMap((t) => t.regions.filter((r) => r.takes && s.selectedRegions.includes(r.id)).map((r) => r.id));
  if (folders.length) act("step_take", { regions: folders, delta });
}

/** Pack Take Folder: the selected audio regions become a folder, on the topmost of their
 *  tracks. Tracks the pack leaves empty can then be deleted, as a step of its own. */
export async function packTakeFolder(ids: Id[] = getState().selectedRegions) {
  if (ids.length < 2) return;
  const from = packTracks(getState().snap?.project.tracks ?? [], ids);
  const made = await actMade("pack_take_folder", { ids });
  if (!made) return;
  setState({ selectedRegions: made, focus: "regions" });
  const empty = emptiedTracks(getState().snap?.project.tracks ?? [], from);
  if (!empty.length) return;
  const names = empty.map((id) => getState().snap?.project.tracks.find((t) => t.id === id)?.name ?? "");
  const ok = await ask(emptiedQuestion(names), {
    title: "Pack Take Folder",
    kind: "info",
    okLabel: empty.length > 1 ? "Delete Tracks" : "Delete Track",
    cancelLabel: "Keep",
  });
  if (ok && (await act("delete_tracks", { ids: empty }))) setState({ selectedTracks: getState().selectedTracks.filter((id) => !empty.includes(id)) });
}
