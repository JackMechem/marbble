// Take folders in the arrange area, as in Logic: a folder region shows its comp, a disclosure
// triangle opens it into one row per take below the track's lane, and swiping across a take
// row puts that stretch of the take in the comp (Quick Swipe Comping). The comp logic lives in
// the engine (crates/engine/src/edit/takes/); this file has the folders' layout and sends the edits.
// Drawing them is takeDraw.ts, their menu takeFolderMenu.ts.
import { ask } from "@tauri-apps/plugin-dialog";
import { type Id, type Track } from "../../engine/project";
import type { CompSegment, Region, Take, TakeFolder } from "../../engine/regions";
import { act, actMade } from "../../engine/commands";
import { isMidiFolder, neighbourTake, sectionAt } from "./comps";
import { emptiedQuestion, emptiedTracks, packTracks } from "./packTakes";
import { getState, setState } from "../../state/store";
import { ensureWave } from "../regions/waveform";

/** Height of one take row in an open folder. */
export const TAKE_ROW_H = 26;

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
