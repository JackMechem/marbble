// Track operations, shared by the right-click menus, the local Edit menu, the native menu bar and
// the keyboard: the track selection, duplicating and deleting tracks, stacks, hiding, groups and
// the switches across tracks. Region operations are in regionCommands.ts, the menus in
// contextMenus.ts.
import { type Group, type Id, type Track } from "../engine/project";
import { act, actMade, api } from "../engine/commands";
import { currentLayout, stepVisible } from "./trackLayout";
import { openBounceInPlace } from "../project/bounce/bounce";
import { getState, setState } from "../state/store";
import { onTracks } from "./regions/regionSelect";

const project = () => getState().snap?.project;
const tracks = (): Track[] => project()?.tracks ?? [];
const toast = (text: string, ms = 2500) => setState({ toast: { text, at: Date.now(), ms } });

const inOrder = (ids: Id[]) => tracks().map((t) => t.id).filter((id) => ids.includes(id));

// ---- track selection ------------------------------------------------------------------------

/** The selected tracks, in track order. Falls back to the inspector's track. */
export function selectedTracks(): Id[] {
  const s = getState();
  const cur = s.snap?.selected ?? null;
  if (cur !== null && s.selectedTracks.includes(cur)) return inOrder(s.selectedTracks);
  return cur !== null ? [cur] : [];
}

/** Adds the members of groups that link selection. */
function withGroups(ids: Id[]): Id[] {
  const p = project();
  if (!p) return ids;
  const out = new Set(ids);
  for (const id of ids) {
    const t = p.tracks.find((x) => x.id === id);
    const g = t?.group != null ? p.groups.find((x) => x.id === t.group) : undefined;
    if (g?.enabled && g.select) p.tracks.filter((x) => x.group === g.id).forEach((x) => out.add(x.id));
  }
  return inOrder([...out]);
}

/** A click on a track header: plain selects, ⌘/Ctrl toggles, Shift extends. */
export function clickTrack(id: Id, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) {
  const s = getState();
  const all = tracks().map((t) => t.id);
  let sel: Id[];
  const cur = s.snap?.selected ?? null;
  if (e.shiftKey && cur !== null) {
    const [a, b] = [all.indexOf(cur), all.indexOf(id)].sort((x, y) => x - y);
    // Tracks out of sight (hidden, or in a collapsed stack) stay out of the range.
    const heights = currentLayout().heights;
    sel = all.slice(a, b + 1).filter((x, k) => heights[a + k] > 0 || x === id || x === cur);
  } else if (e.ctrlKey || e.metaKey) {
    const base = selectedTracks();
    sel = base.includes(id) ? base.filter((x) => x !== id) : [...base, id];
    if (!sel.length) sel = [id];
  } else {
    sel = selectedTracks().includes(id) && selectedTracks().length > 1 ? selectedTracks() : [id];
  }
  const chosen = withGroups(sel);
  setState({ selectedTracks: chosen, focus: "tracks" });
  keepRegionsOn(chosen);
  if (cur !== id) api.selectTrack(id);
}

/** Choosing tracks by hand deselects the regions on other tracks, as a click on an empty lane
 *  does, so the Inspector's Region and Track sections always describe the same track. */
export function keepRegionsOn(trackIds: Id[]) {
  const sel = getState().selectedRegions;
  const kept = onTracks(tracks(), sel, trackIds);
  if (kept.length !== sel.length) setState({ selectedRegions: kept });
}

// ---- track operations ---------------------------------------------------------------------

async function selectMadeTracks(ids: Id[] | null) {
  if (ids?.length) setState({ selectedTracks: ids, focus: "tracks" });
}

export const duplicateTracks = (content = true) => actMade("duplicate_tracks", { ids: selectedTracks(), content }).then(selectMadeTracks);

export function deleteTracks(ids = selectedTracks()) {
  if (ids.length) act("delete_tracks", { ids }).then(() => setState({ selectedTracks: [] }));
}

export async function deleteUnusedTracks() {
  const n = await actMade("delete_unused_tracks");
  toast(n?.[0] ? `Deleted ${n[0]} unused track${n[0] > 1 ? "s" : ""}` : "No unused tracks");
}

export async function createStack(summing: boolean, ids = selectedTracks()) {
  const made = await actMade("create_stack", { ids, summing });
  if (made) setState({ selectedTracks: made });
}

export const flattenStack = (id: Id) => act("flatten_stack", { id });

/** Moves tracks out of their stack, one level up. */
export function removeFromStack(ids = selectedTracks()) {
  const p = project();
  if (!p) return;
  const parents = ids.map((id) => {
    const t = p.tracks.find((x) => x.id === id);
    const up = t?.parent != null ? p.tracks.find((x) => x.id === t.parent)?.parent ?? null : null;
    return [id, up] as [Id, Id | null];
  });
  act("move_tracks", { ids: p.tracks.map((t) => t.id), parents });
}

/** Track › Bounce in Place: the selected regions when they were last worked on (like ⌘D),
 *  otherwise the selected tracks. */
export function bounceInPlace(ids?: Id[]) {
  const s = getState();
  if (!ids && s.focus === "regions" && s.selectedRegions.length) openBounceInPlace([], s.selectedRegions);
  else openBounceInPlace(ids ?? selectedTracks());
}

export const toggleCollapsed = (t: Track) => api.updateTrack(t.id, { collapsed: !t.collapsed });

export const setHidden = (ids: Id[], hidden: boolean) => act("set_tracks_hidden", { ids, hidden });

/** ⌃H, as Logic's Hide Current Track and Select Next Track: hides the selected tracks (shows
 *  them again when they're all hidden already) and moves the selection to the next track left
 *  in view. */
export async function hideTracks(ids = selectedTracks()) {
  if (!ids.length) return;
  const hide = !tracks().filter((t) => ids.includes(t.id)).every((t) => t.hidden);
  if (!(await setHidden(ids, hide)) || !hide || getState().showHidden) return;
  const all = tracks();
  const l = currentLayout();
  const last = Math.max(...ids.map((id) => all.findIndex((t) => t.id === id)));
  let next = stepVisible(l, last, 1);
  if (l.heights[next] === 0) next = stepVisible(l, last, -1);
  const t = l.heights[next] > 0 ? all[next] : undefined;
  setState({ selectedTracks: t ? [t.id] : [] });
  if (t) api.selectTrack(t.id);
}

export function unhideAllTracks() {
  const ids = tracks().filter((t) => t.hidden).map((t) => t.id);
  if (ids.length) setHidden(ids, false);
}

/** H: Logic's hide mode, which shows hidden tracks (dimmed) and an H button on every header. */
export const toggleShowHidden = () => setState({ showHidden: !getState().showHidden });

export async function groupTracks(ids = selectedTracks()) {
  if (!ids.length) return;
  const made = await actMade("create_group", { ids });
  if (made?.[0] !== undefined) setState({ groupSheet: made[0] });
}

export const setGroup = (ids: Id[], group: Id | null) => act("set_track_group", { ids, group });
export const updateGroup = (group: Group) => act("update_group", { group });

/** Applies a switch to every selected track, the way the clicked track goes. */
export function switchAll(field: "mute" | "solo" | "armed" | "off", ids = selectedTracks()) {
  const sel = tracks().filter((t) => ids.includes(t.id));
  const on = !sel.every((t) => t[field]);
  sel.forEach((t) => api.updateTrack(t.id, { [field]: on }));
}

