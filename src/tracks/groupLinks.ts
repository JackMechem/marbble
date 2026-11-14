// Track groups' links on the UI side: which other tracks a change reaches through a group. The
// engine applies the mixer links itself (crates/engine/src/session/tracks/groups/); the UI needs
// them for the selection links and for touching partners' automation. Tested in
// tests/tracks/groupLinks.test.ts.
import type { AutoMode, AutoParam, Group, Id, Track } from "../engine/project";

/** What a group can link, as in its settings (Group's boolean fields). */
export type GroupLink = "volume" | "pan" | "mute" | "solo" | "record" | "select" | "editing";

type Members = Pick<Track, "id" | "group">;

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
