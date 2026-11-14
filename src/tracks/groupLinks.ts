// Track groups' links on the UI side: which other tracks a change reaches through a group. The
// engine applies the mixer links itself (crates/engine/src/session/tracks/groups/); the UI needs
// them for the selection links and for touching partners' automation. Tested in
// tests/tracks/groupLinks.test.ts.
import type { AutoMode, AutoParam, Group, Id, Track } from "../engine/project";
import type { Region } from "../engine/regions";
import { regionEnd } from "./regions/loops.ts";

/** What a group can link, as in its settings (Group's boolean fields). */
export type GroupLink = "volume" | "pan" | "mute" | "solo" | "record" | "select" | "editing";

type Members = Pick<Track, "id" | "group">;

/** The groups that link anything now: none while the group clutch (⇧G) is engaged. */
export function activeGroups(snap: { project: { groups: Group[] }; groups_clutched?: boolean } | null | undefined): Group[] {
  return snap && !snap.groups_clutched ? snap.project.groups : [];
}

/** The group `track` is in, when it is on and shares `link`. */
export function linkingGroup(groups: readonly Group[], track: Members | undefined, link: GroupLink): Group | undefined {
  const g = track?.group != null ? groups.find((x) => x.id === track.group) : undefined;
  return g?.enabled && g[link] ? g : undefined;
}

/** The other tracks `link` reaches from `id`, in track order. */
export function linkedPeers<T extends Members>(tracks: readonly T[], groups: readonly Group[], id: Id, link: GroupLink): T[] {
  const g = linkingGroup(groups, tracks.find((t) => t.id === id), link);
  return g ? tracks.filter((t) => t.group === g.id && t.id !== id) : [];
}

const WRITES: AutoMode[] = ["touch", "latch", "write"];

/** Whether holding `param` on `id` should tell the engine: its own track writes automation, or a
 *  partner the group moves with it does. */
export function touchWrites(tracks: readonly (Members & Pick<Track, "auto_mode">)[], groups: readonly Group[], id: Id, param: AutoParam): boolean {
  const own = tracks.find((t) => t.id === id);
  if (own && WRITES.includes(own.auto_mode)) return true;
  const link = param.type === "volume" || param.type === "pan" || param.type === "mute" ? param.type : null;
  return !!link && linkedPeers(tracks, groups, id, link).some((t) => WRITES.includes(t.auto_mode));
}

/** `ids` plus the members of the groups that link selection, in track order. */
export function withSelectionPeers(tracks: readonly Members[], groups: readonly Group[], ids: readonly Id[]): Id[] {
  const out = new Set(ids);
  for (const id of ids) linkedPeers(tracks, groups, id, "select").forEach((t) => out.add(t.id));
  return tracks.map((t) => t.id).filter((id) => out.has(id));
}

/** ⌘/Ctrl-click on track `id`: takes it out of `selected` with the members its group selects
 *  along with it, or adds it. */
export function toggleTrack(tracks: readonly Members[], groups: readonly Group[], selected: readonly Id[], id: Id): Id[] {
  if (!selected.includes(id)) return [...selected, id];
  const out = new Set([id, ...linkedPeers(tracks, groups, id, "select").map((t) => t.id)]);
  return selected.filter((x) => !out.has(x));
}

type Lane = Members & { regions: readonly Pick<Region, "id" | "start" | "length" | "loop_end">[] };

/** Region `id` plus, when its track's group links editing, the regions overlapping it on the
 *  group's other tracks. */
export function linkedRegionIds(tracks: readonly Lane[], groups: readonly Group[], id: Id): Id[] {
  const t = tracks.find((x) => x.regions.some((r) => r.id === id));
  const r = t?.regions.find((x) => x.id === id);
  if (!t || !r) return [id];
  const out = [id];
  for (const o of linkedPeers(tracks, groups, t.id, "editing")) {
    for (const x of o.regions) if (x.start < regionEnd(r) && regionEnd(x) > r.start) out.push(x.id);
  }
  return out;
}

/** Shift-click on region `id`: adds it and `linked` (its linked regions) to `selected`, or takes
 *  them all out when it is selected already. */
export function toggleRegion(selected: readonly Id[], id: Id, linked: readonly Id[]): Id[] {
  if (selected.includes(id)) return selected.filter((x) => !linked.includes(x));
  return [...selected, ...linked.filter((x) => !selected.includes(x))];
}
