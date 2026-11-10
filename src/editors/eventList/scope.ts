// What the Event List lists: the selected MIDI regions, or else every MIDI region of the selected
// software instrument track.
import type { Id, Project, Track } from "../../engine/project";
import type { Region } from "../../engine/regions";
import type { State } from "../../state/store";

// A pattern region plays its steps, not its notes: the Step Sequencer edits it.
const isMidi = (r: Region) => r.content.type === "midi" && !r.pattern;

export interface Scope {
  track: Track;
  regions: Region[];
  /** Every MIDI region of the track (none was selected). */
  all: boolean;
}

/** The selected MIDI regions (on the track of the first), or else every MIDI region of the
 *  selected instrument track. */
export function scopeOf(project: Project | undefined, selected: Id[], selectedTrack: Id | null): Scope | null {
  for (const t of project?.tracks ?? []) {
    const regions = t.regions.filter((r) => isMidi(r) && selected.includes(r.id));
    if (regions.length) return { track: t, regions, all: false };
  }
  const track = project?.tracks.find((t) => t.id === selectedTrack);
  return track?.kind === "instrument" ? { track, regions: track.regions.filter(isMidi), all: true } : null;
}

/** The scope of the store's selection. */
export const scopeOfState = (s: State) => scopeOf(s.snap?.project, s.selectedRegions, s.snap?.selected ?? null);

/** The track and regions listed, as a string: when it changes, the list starts over. */
export const scopeKey = (sc: Scope | null) => (sc ? `${sc.track.id}:${sc.regions.map((r) => r.id).join(",")}` : "");

/** What the list is called: the region's name when one was selected, else the track's. */
export const scopeName = (sc: Scope) => (sc.regions.length === 1 && !sc.all ? sc.regions[0].name : sc.track.name);
