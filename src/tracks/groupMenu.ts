// The pop-up behind a Group slot (Mixer strips, the Track inspector), as in Logic: No Group, the
// project's groups, New Group…, and the ways to the settings.
import type { Id, Track } from "../engine/project";
import { fmt, keyHint } from "../keyboard/keymap";
import { getState, setState } from "../state/store";
import type { MenuItem } from "../ui/Menu";
import { groupTracks, selectedTracks, setGroup, toggleGroupClutch } from "./trackCommands";

/** The tracks a Group slot on `track` acts on: the selection when the track is in it. */
export function slotTracks(track: Track): Id[] {
  const sel = selectedTracks();
  return sel.includes(track.id) ? sel : [track.id];
}

/** The Group slot's menu for `track`, assigning `ids`. */
export function groupMenu(track: Track, ids: Id[] = slotTracks(track)): MenuItem[] {
  const s = getState();
  const groups = s.snap?.project.groups ?? [];
  const items: MenuItem[] = [{ label: "No Group", checked: track.group === null, onClick: () => setGroup(ids, null) }];
  if (groups.length) items.push({ separator: true });
  for (const g of groups) items.push({ label: g.name, checked: track.group === g.id, onClick: () => setGroup(ids, g.id) });
  items.push(
    { separator: true },
    { label: "New Group…", hint: fmt("Mod+G"), onClick: () => groupTracks(ids) },
    { label: "Group Settings…", disabled: track.group === null, onClick: () => setState({ groupSheet: track.group }) },
    { id: "groups", label: "Groups…", disabled: !groups.length, onClick: () => setState({ dialog: "groups" }) },
    { id: "group-clutch", label: "Group Clutch", hint: keyHint("group-clutch"), checked: !!s.snap?.groups_clutched, onClick: toggleGroupClutch },
  );
  return items;
}
