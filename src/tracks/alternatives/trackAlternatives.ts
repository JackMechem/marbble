// Track Alternatives (Track › Track Alternatives, the track header's pop-up): each is a set of
// regions for the track; one plays. Rust: crates/engine/src/session/tracks/alternatives.rs.
import { type Track } from "../../engine/project";
import { act } from "../../engine/commands";
import { activeTrackAlt, trackAltNames } from "../../project/alternativesModel";
import { askName } from "../../ui/NameSheet";
import type { MenuItem } from "../../ui/Menu";
import { getState, setState } from "../../state/store";

/** Tracks with regions can have alternatives. */
export function canHaveAlternatives(t: Track): boolean {
  return t.kind === "audio" || t.kind === "instrument";
}

/** The selected track, if it can have alternatives (for the native Track menu). */
export function trackForAlternatives(): Track | null {
  const s = getState().snap;
  const t = s?.project.tracks.find((x) => x.id === s.selected);
  if (!t || !canHaveAlternatives(t)) {
    setState({ toast: { text: "Select an audio or instrument track first", at: Date.now(), ms: 2500 } });
    return null;
  }
  return t;
}

export function newTrackAlternative(t: Track, duplicate: boolean) {
  return act("new_track_alternative", { track: t.id, duplicate });
}

function switchTrackAlternative(t: Track, name: string) {
  if (name !== activeTrackAlt(t)) return act("switch_track_alternative", { track: t.id, name });
}

export async function renameTrackAlternative(t: Track, name = activeTrackAlt(t)) {
  const taken = trackAltNames(t);
  const check = (n: string) =>
    !n.trim() ? "The name can't be empty." : taken.some((x) => x !== name && x.toLowerCase() === n.trim().toLowerCase()) ? `This track already has an alternative named “${n.trim()}”.` : null;
  const newName = await askName({ title: "Rename Track Alternative", value: name, ok: "Rename", check });
  if (newName && newName !== name) await act("rename_track_alternative", { track: t.id, name, newName });
}

export function deleteTrackAlternative(t: Track, name = activeTrackAlt(t)) {
  if (!t.alternatives?.length) {
    setState({ toast: { text: "A track keeps at least one alternative", at: Date.now(), ms: 2500 } });
    return;
  }
  return act("delete_track_alternative", { track: t.id, name });
}

/** The alternatives, the active one ticked, then New, Duplicate, Rename and Delete: the
 *  header pop-up and the track's right-click menu. */
export function trackAltMenu(t: Track): MenuItem[] {
  const frozen = !!t.frozen;
  return [
    ...trackAltNames(t).map((n) => ({ label: n, checked: n === activeTrackAlt(t), disabled: frozen, onClick: () => switchTrackAlternative(t, n) })),
    { separator: true },
    { label: "New Alternative", disabled: frozen, onClick: () => newTrackAlternative(t, false) },
    { label: "Duplicate Alternative", disabled: frozen, onClick: () => newTrackAlternative(t, true) },
    { label: "Rename Alternative…", disabled: frozen, onClick: () => renameTrackAlternative(t) },
    { label: "Delete Alternative", disabled: frozen || !t.alternatives?.length, onClick: () => deleteTrackAlternative(t) },
  ];
}
