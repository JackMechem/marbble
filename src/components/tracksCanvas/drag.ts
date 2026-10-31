// What the Tracks canvas's drawing and pointer code share: the values live for them, and the shape of a
// drag in progress with where it would land.
import { type Id, type Project, type Track } from "../../api";
import { stepVisible, type Layout } from "../../layout";
import { newLanes, sourceTracks } from "../../newTrackDrag";
import type { GrabKind } from "../../regionGrab";

/** The latest values for the imperative draw and pointer code. */
export interface TracksLive {
  project: Project | undefined;
  selectedTrack: Id | null;
  selectedRegions: Id[];
  pps: number;
  layout: Layout;
}

/** The refs both read: what is live, and the gestures in progress. */
export interface TracksRefs {
  live: React.RefObject<TracksLive>;
  dragRef: React.RefObject<Drag | null>;
  /** A Quick Swipe Comping drag: the take and take-time range. */
  swipeRef: React.RefObject<{ region: Id; take: Id; start: number; end: number } | null>;
  /** A rubber-band selection, in content coordinates. */
  marqueeRef: React.RefObject<{ left: number; top: number; right: number; bottom: number } | null>;
  /** The region under the pointer, which shows its fade handles. */
  hoverRef: React.RefObject<Id | null>;
}

export interface Drag {
  kind: GrabKind;
  ids: Id[];
  dx: number;
  /** Tracks moved, counted in visible tracks (collapsed and hidden ones are skipped). */
  dTrack: number;
  /** Alt (Option) is held: the move drops copies, as in Logic. */
  copy?: boolean;
  /** For trims, the new start/length of the grabbed region. */
  start?: number;
  length?: number;
  /** For fade drags, the new fade length. */
  fade?: number;
  /** For loop drags, the new loop end. */
  loopEnd?: number;
  grabbed: Id;
}

/** The track a region on track `i` lands on when a drag moves it `dTrack` visible tracks. A
 *  region can only go onto a track of its own kind; otherwise it stays on its track, as
 *  `move_region` does. */
export function dragTarget(layout: Layout, i: number, dTrack: number, tracks: Track[]): number {
  const ti = stepVisible(layout, i, dTrack);
  return tracks[ti]?.kind === tracks[i].kind ? ti : i;
}

/** The new lanes a move in progress makes, by the index of the track each comes from. */
export function dragNewLanes(layout: Layout, d: Drag | null, tracks: Track[]): Map<number, number> {
  if (d?.kind !== "move") return new Map();
  return newLanes(layout.heights, sourceTracks(tracks, d.ids), d.dTrack);
}
