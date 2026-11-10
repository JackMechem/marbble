// Region operations in the Tracks area, shared by the right-click menus, the local Edit menu, the
// native menu bar and the keyboard: the clipboard, repeat, join, split, mute, loops, selecting and
// creating regions. The menus that offer them are in contextMenus.ts.
import { open } from "../project/fileDialog";
import { type Id, type Track } from "../engine/project";
import { type Region } from "../engine/regions";
import { type Snapshot } from "../engine/snapshot";
import { act, actMade, call } from "../engine/commands";
import { isLooped, regionEnd } from "./regions/loops";
import { importMidiFiles } from "../project/midiFiles";
import { currentTick, getState, setState } from "../state/store";
import { tempoMap } from "../lib/tempo";
import { freePatternBar, templateRows } from "../editors/stepSequencer/model";
import { duplicateTracks } from "./trackCommands";

const project = () => getState().snap?.project;
const tracks = (): Track[] => project()?.tracks ?? [];
const playhead = () => currentTick()?.pos ?? 0;
const toast = (text: string, ms = 2500) => setState({ toast: { text, at: Date.now(), ms } });

export function trackOf(regionId: Id): Track | undefined {
  return tracks().find((t) => t.regions.some((r) => r.id === regionId));
}

export function regionById(id: Id): Region | undefined {
  for (const t of tracks()) for (const r of t.regions) if (r.id === id) return r;
  return undefined;
}

// ---- region operations --------------------------------------------------------------------

export const selectedRegions = () => getState().selectedRegions;

/** A region plus, when its track's group links editing, the regions at the same place on the
 *  group's other tracks. */
export function linkedRegions(id: Id): Id[] {
  const p = project();
  const t = trackOf(id);
  const r = t?.regions.find((x) => x.id === id);
  const g = t?.group != null ? p?.groups.find((x) => x.id === t.group) : undefined;
  if (!p || !r || !g?.enabled || !g.editing) return [id];
  const out = [id];
  for (const o of p.tracks) {
    if (o.id === t!.id || o.group !== g.id) continue;
    for (const x of o.regions) if (x.start < regionEnd(r) && regionEnd(x) > r.start) out.push(x.id);
  }
  return out;
}

export function selectRegions(ids: Id[] | null) {
  if (ids) setState({ selectedRegions: ids, focus: "regions" });
}

export function copyRegions(ids = selectedRegions()) {
  const all = tracks();
  const clip = ids.flatMap((id) => {
    const index = all.findIndex((t) => t.regions.some((r) => r.id === id));
    const region = all[index]?.regions.find((r) => r.id === id);
    return region ? [{ index, region }] : [];
  });
  if (clip.length) setState({ clipboard: clip, clipboardStart: null });
  return clip.length;
}

export function cutRegions(ids = selectedRegions()) {
  if (copyRegions(ids)) act("delete_regions", { ids }).then(() => setState({ selectedRegions: [] }));
}

/** Pastes at `at` (a track and position), or at the playhead on the selected track. The copied
 *  regions keep their spacing in time and across tracks. */
export async function pasteRegions(at?: { track: Id; pos: number }) {
  const s = getState();
  const clip = s.clipboard;
  const all = tracks();
  if (!clip.length || !all.length) return;
  const targetId = at?.track ?? s.snap?.selected ?? all[0].id;
  const target = Math.max(0, all.findIndex((t) => t.id === targetId));
  const top = Math.min(...clip.map((c) => c.index));
  const earliest = s.clipboardStart ?? Math.min(...clip.map((c) => c.region.start));
  const items = clip.flatMap(({ index, region }) => {
    const t = all[target + index - top];
    return t ? [[t.id, region] as [Id, Region]] : [];
  });
  const made = await actMade("paste_regions", { items, offset: (at?.pos ?? playhead()) - earliest, label: "Paste" });
  if (made && made.length < clip.length) toast("Some regions didn't fit the track they'd land on");
  selectRegions(made);
}

export const repeatRegions = (times = 1, ids = selectedRegions()) => ids.length && actMade("repeat_regions", { ids, times }).then(selectRegions);

export const joinRegions = (ids = selectedRegions()) => actMade("join_regions", { ids }).then(selectRegions);

export const splitAtPlayhead = (ids = selectedRegions()) => act("split_regions", { pos: playhead(), ids });

export function deleteRegions(ids = selectedRegions()) {
  if (ids.length) act("delete_regions", { ids }).then(() => setState({ selectedRegions: [] }));
}

export function toggleRegionMute(ids = selectedRegions()) {
  const regs = ids.map(regionById).filter((r): r is Region => !!r);
  const muted = !regs.every((r) => r.muted);
  regs.forEach((r) => act("set_region_muted", { id: r.id, muted }));
}

export function setLocatorsByRegions(ids = selectedRegions()) {
  const regs = ids.map(regionById).filter((r): r is Region => !!r);
  if (!regs.length) return;
  const start = Math.min(...regs.map((r) => r.start));
  const end = Math.max(...regs.map(regionEnd));
  act("set_cycle", { enabled: true, start, end });
}

/** L, Logic's Loop switch: loops each region up to the next region on its track (or four times
 *  its length), or turns looping off when they all loop already. */
export function toggleLoop(ids = selectedRegions()) {
  const regs = ids.map(regionById).filter((r): r is Region => !!r);
  if (regs.length) act("set_regions_looped", { ids: regs.map((r) => r.id), on: !regs.every(isLooped) });
}

/** Turns loops into plain regions, one per repetition, and selects them all. */
export function convertLoops(ids = selectedRegions()) {
  const looped = ids.filter((id) => {
    const r = regionById(id);
    return r && isLooped(r);
  });
  if (looped.length) actMade("convert_loops", { ids: looped }).then((made) => made && selectRegions([...new Set([...ids, ...made])]));
}

export function selectOnTrack(trackId: Id, from?: number) {
  const t = tracks().find((x) => x.id === trackId);
  if (t) selectRegions(t.regions.filter((r) => from === undefined || r.start >= from).map((r) => r.id));
}

export function selectAllRegions() {
  const showHidden = getState().showHidden;
  selectRegions(tracks().flatMap((t) => (t.hidden && !showHidden ? [] : t.regions.map((r) => r.id))));
}

export async function createMidiRegion(track: Id, pos: number) {
  const p = project();
  if (!p) return;
  const m = tempoMap(p);
  const bar = m.barAt(pos);
  const start = Math.round(m.barStart(bar));
  selectRegions(await actMade("create_midi_region", { track, start, length: Math.round(m.barStart(bar + 1)) - start, key: null }));
}

/** Create Pattern Region: a bar-long pattern region at the bar containing `pos`, selected and
 *  open in the Step Sequencer. With `free` (made at the playhead rather than where the user
 *  clicked), it goes in the first bar from there that no region covers. */
export async function createPatternRegion(track: Id, pos: number, free = false) {
  const p = project();
  if (!p) return;
  const m = tempoMap(p);
  let start: number, length: number;
  if (free) [start, length] = freePatternBar(m, p.tracks.find((t) => t.id === track)?.regions ?? [], pos);
  else {
    const bar = m.barAt(pos);
    start = Math.round(m.barStart(bar));
    length = Math.round(m.barStart(bar + 1)) - start;
  }
  // Rows to suit the instrument: drums for a kit, notes for a synth (editors/stepSequencer/model.ts).
  const t = p.tracks.find((x) => x.id === track);
  const rows = t ? templateRows(t, p.key) : null;
  const made = await actMade("create_pattern_region", { track, start, length, rows });
  if (made) setState({ selectedRegions: made, focus: "regions", bottom: "steps" });
}

/** Track › New Pattern Region: on the selected software instrument track, at the playhead (or
 *  the first free bar after it). */
export function newPatternRegion() {
  const t = tracks().find((x) => x.id === getState().snap?.selected);
  if (t?.kind === "instrument") createPatternRegion(t.id, playhead(), true);
  else toast("Select a software instrument track for the pattern region");
}

export async function importAt(track: Track, pos: number) {
  const file = await open({ title: "Import Audio File", multiple: true });
  const paths = typeof file === "string" ? [file] : file ?? [];
  if (!paths.length) return;
  const res = await call<{ snapshot: Snapshot; errors: string[] }>("import_audio_files", { paths, track: track.id, start: Math.round(pos), index: null });
  if (res) {
    setState({ snap: res.snapshot });
    if (res.errors.length) toast(res.errors.join(" · "), 6000);
  }
}

export async function importMidiAt(track: Track, pos: number) {
  const file = await open({ title: "Import MIDI File", multiple: true, filters: [{ name: "MIDI Files", extensions: ["mid", "midi", "smf", "kar"] }] });
  const paths = typeof file === "string" ? [file] : file ?? [];
  if (paths.length) importMidiFiles(paths, track.id, pos, null);
}

/** ⌘D: duplicates whatever was last worked on, the tracks or the regions. */
export function duplicate() {
  const s = getState();
  if (s.focus === "regions" && s.selectedRegions.length) repeatRegions(1);
  else duplicateTracks(true);
}
