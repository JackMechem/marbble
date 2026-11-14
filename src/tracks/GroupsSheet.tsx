// Logic's Groups window as a sheet: every group in a row with its on/off, colour, name, members
// and links side by side. A group's name opens its Group Settings (GroupSheet.tsx).
import type { Group } from "../engine/project";
import { setState, useStore } from "../state/store";
import { Sheet } from "../ui/Sheet";
import { openMenuAt } from "../ui/Menu";
import { keyHint } from "../keyboard/keymap";
import { colorMenu } from "./contextMenus";
import { GROUP_LINKS } from "./GroupSheet";
import { toggleGroupClutch, updateGroup } from "./trackCommands";

export function GroupsSheet() {
  const groups = useStore((s) => s.snap?.project.groups);
  const tracks = useStore((s) => s.snap?.project.tracks);
  const clutched = useStore((s) => !!s.snap?.groups_clutched);
  const close = () => setState({ dialog: null });
  const set = (g: Group, patch: Partial<Group>) => updateGroup({ ...g, ...patch });
  const key = keyHint("group-clutch");
  return (
    <Sheet title="Groups" onClose={close} width={780}>
      {groups?.length ? (
        <table className="groups-table">
          <thead>
            <tr>
              <th title="Group on">On</th>
              <th />
              <th className="groups-name">Name</th>
              <th title="Tracks in the group">Tracks</th>
              {GROUP_LINKS.map(([k, label, short]) => (
                <th key={k} title={label}>
                  {short}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {groups.map((g) => {
              const members = (tracks ?? []).filter((t) => t.group === g.id);
              return (
                <tr key={g.id} className={g.enabled ? "" : "off"}>
                  <td>
                    <input type="checkbox" aria-label={`${g.name} on`} checked={g.enabled} onChange={(e) => set(g, { enabled: e.target.checked })} />
                  </td>
                  <td>
                    <button
                      className="groups-color"
                      style={{ background: g.color }}
                      aria-label={`${g.name} colour`}
                      title="Group colour"
                      onClick={(e) => openMenuAt(e.currentTarget, colorMenu(g.color, (c) => c && set(g, { color: c })))}
                    />
                  </td>
                  <td className="groups-name">
                    <button className="groups-open" title={`Open ${g.name}'s settings`} onClick={() => setState({ dialog: null, groupSheet: g.id })}>
                      {g.name}
                    </button>
                  </td>
                  <td title={members.map((t) => t.name).join(", ")}>{members.length}</td>
                  {GROUP_LINKS.map(([k, label, , hint]) => (
                    <td key={k}>
                      <input type="checkbox" aria-label={`${g.name}: ${label}`} title={`${label}: ${hint}`} checked={g[k]} onChange={(e) => set(g, { [k]: e.target.checked })} />
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      ) : (
        <div className="pref-hint">No groups yet: select some tracks and choose Group Selected Tracks.</div>
      )}
      <div className="sheet-foot">
        <label className="check" title={`Suspend every group for now; each keeps its own on/off${key ? ` (${key})` : ""}`}>
          <input type="checkbox" checked={clutched} onChange={toggleGroupClutch} />
          Group Clutch
        </label>
        <div className="sheet-buttons">
          <button className="push default" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}
