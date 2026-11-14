// A channel strip's Group slot, as in Logic: the group's name on its colour, or empty; a click
// opens the group pop-up (src/tracks/groupMenu.ts).
import type { Track } from "../engine/project";
import { useStore } from "../state/store";
import { openMenuAt } from "../ui/Menu";
import { groupMenu } from "../tracks/groupMenu";
import { slotProps } from "./slotKeys";

export function GroupSlot({ track }: { track: Track }) {
  const group = useStore((s) => (track.group === null ? undefined : s.snap?.project.groups.find((g) => g.id === track.group)));
  const clutched = useStore((s) => !!s.snap?.groups_clutched);
  const open = (el: HTMLElement) => openMenuAt(el, groupMenu(track));
  const on = !!group && group.enabled && !clutched;
  const title = group ? `${group.name}${clutched ? " (suspended by the Group Clutch)" : group.enabled ? "" : " (off)"}: click to change the group` : "Group: put this strip in a group";
  return (
    <div
      className={"slot static group-slot" + (group ? " assigned" : "") + (group && !on ? " off" : "")}
      style={group ? ({ "--group": group.color } as React.CSSProperties) : undefined}
      title={title}
      {...slotProps(`Group: ${group?.name ?? "none"}`, { open }, { popup: true })}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (e.button === 0) open(e.currentTarget);
      }}
    >
      {group ? <span className="slot-text">{group.name}</span> : <span className="slot-name dim">Group</span>}
    </div>
  );
}
