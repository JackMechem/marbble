// Pack Take Folder across tracks: which tracks the selected regions come from, and which of
// them the pack leaves with no regions (offered for deletion afterwards). The pack itself is
// in the engine (crates/engine/src/session/takes/).
import type { Id, Track } from "../../engine/project";

/** The tracks holding any of `ids`, top first. The first gets the folder. */
export function packTracks(tracks: Track[], ids: Id[]): Id[] {
  return tracks.filter((t) => t.regions.some((r) => ids.includes(r.id))).map((t) => t.id);
}

/** Of the tracks a pack took regions from (`from`, top first), those it left with nothing: no
 *  regions, and no other track alternatives that might hold some. The folder's track stays. */
export function emptiedTracks(tracks: Track[], from: Id[]): Id[] {
  return from.slice(1).filter((id) => {
    const t = tracks.find((x) => x.id === id);
    return !!t && t.regions.length === 0 && !t.alternatives?.length;
  });
}

/** The question asked after a pack left `names` (track names) empty. */
export function emptiedQuestion(names: string[]): string {
  const quoted = names.map((n) => `“${n}”`);
  let which = quoted[0];
  if (names.length > 3) which = `these ${names.length} tracks`;
  else if (names.length > 1) which = `${quoted.slice(0, -1).join(", ")} and ${quoted[quoted.length - 1]}`;
  return `The takes are in the take folder now. Delete ${which}, left with no regions?`;
}
