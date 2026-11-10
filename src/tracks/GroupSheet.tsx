// Logic's Group Settings sheet: a track group's name, colour and what its tracks share.
import { useEffect, useState } from "react";
import type { Group } from "../engine/project";
import { act } from "../engine/commands";
import { updateGroup } from "./trackCommands";
import { TRACK_COLORS } from "./trackColors";
import { setState, useStore } from "../state/store";
import { Sheet } from "../ui/Sheet";

/** Logic's Group Settings: name, colour and what the group's tracks share. */
export function GroupSheet() {
  const id = useStore((s) => s.groupSheet);
  const group = useStore((s) => s.snap?.project.groups.find((g) => g.id === s.groupSheet));
  // Select the stable track list and filter it here: a selector that builds a new array on
  // every call makes useSyncExternalStore re-render forever, which blanks the window.
  const tracks = useStore((s) => s.snap?.project.tracks);
  const members = (tracks ?? []).filter((t) => t.group === id);
  const close = () => setState({ groupSheet: null });
  const [name, setName] = useState(group?.name ?? "");
  useEffect(() => setName(group?.name ?? ""), [id]);
  if (!group) return null;
  const set = (patch: Partial<Group>) => updateGroup({ ...group, ...patch });
  const commitName = () => name.trim() && name !== group.name && set({ name: name.trim() });
  // Closing (Done, Esc or a click outside) keeps a name still being typed: the field may not
  // get its blur before the sheet goes.
  const done = () => {
    commitName();
    close();
  };
  const links: [keyof Group, string, string][] = [
    ["volume", "Volume", "Moving one fader moves the others by the same amount"],
    ["pan", "Pan", "Pan moves together, by the same amount"],
    ["mute", "Mute", ""],
    ["solo", "Solo", ""],
    ["record", "Record Enable", ""],
    ["select", "Track Selection", "Clicking one track selects them all"],
    ["editing", "Editing (Selection)", "Selecting a region selects the regions at the same place on the other tracks"],
  ];
  return (
    <Sheet title="Group Settings" onClose={done} width={460}>
      <div className="group-sheet">
        <div className="group-row">
          <input
            className="group-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
          <label className="check">
            <input type="checkbox" checked={group.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
            Group on
          </label>
        </div>
        <div className="group-colors">
          {TRACK_COLORS.map((c) => (
            <button key={c} className={"group-swatch" + (c === group.color ? " on" : "")} aria-pressed={c === group.color} style={{ background: c }} onClick={() => set({ color: c })} />
          ))}
        </div>
        <div className="pref-title">Shared by the group's tracks</div>
        <div className="group-links">
          {links.map(([k, label, hint]) => (
            <label key={k} className="check" title={hint}>
              <input type="checkbox" checked={group[k] as boolean} onChange={(e) => set({ [k]: e.target.checked })} />
              {label}
              {hint && <span className="dim"> — {hint}</span>}
            </label>
          ))}
        </div>
        <div className="pref-hint">
          {members.length} track{members.length === 1 ? "" : "s"}: {members.map((t) => t.name).join(", ")}
        </div>
      </div>
      <div className="sheet-foot">
        <button className="push" onClick={() => act("delete_group", { id: group.id }).then(close)}>
          Delete Group
        </button>
        <div className="sheet-buttons">
          <button className="push default" onClick={done}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}
