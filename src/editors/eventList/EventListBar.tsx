// The Event List's two bars: the local bar (editor tabs, the Edit and View menus, the title and
// the MIDI Out, Catch Playhead and detach buttons) and the tools bar under it (add an event, what
// it adds, the type filters and the count).
import { detachMenuItems } from "../../app/detach/detach";
import { DetachButton } from "../../app/detach/DetachButton";
import { Icon } from "../../ui/icons";
import { fmt } from "../../keyboard/keymap";
import { onPress } from "../../keyboard/keyNav";
import { openMenuAt, type MenuItem } from "../../ui/Menu";
import { useBarFit } from "../../ui/useBarFit";
import { EditorTabs } from "../EditorTabs";
import type { Commands } from "./commands";
import * as el from "./model";
import { prefs, setPrefs, usePrefs } from "./prefs";

/** What the local bar gives way with, in order (barFit.ts); the buttons are in the View menu. */
const BAR_STEPS = ["tight", "detach", "catch", "midi-out", "tabs", "title"] as const;
/** And the bar under it: the count, then the type filters, which the View menu has too. */
const TOOLS_STEPS = ["count", "tight", "filters"] as const;

const typeName = (t: el.EventType) => el.eventType(t).name;
const toggleShown = (t: el.EventType) => setPrefs({ show: { ...prefs.show, [t]: !prefs.show[t] } });

/** `canAdd`: something is listed for + to add to; `count`: the rows listed. */
export function EventListBar({ title, canAdd, count, c }: { title: string; canAdd: boolean; count: number; c: Commands }) {
  const p = usePrefs();
  // Short of room, the bars keep to one row (barFit.ts); what leaves them is in the View menu.
  const fit = useBarFit(BAR_STEPS, title);
  const toolsFit = useBarFit(TOOLS_STEPS, typeName(p.addType));

  const typeItems = (): MenuItem[] => el.EVENT_TYPES.map((t) => ({ label: typeName(t.id), checked: prefs.addType === t.id, onClick: () => setPrefs({ addType: t.id }) }));
  const editItems = (): MenuItem[] => [
    { label: "Select All", hint: fmt("Mod+A"), onClick: c.selectAll },
    { label: "Deselect All", hint: "Esc", onClick: c.selectNone },
    { separator: true },
    { label: `Add ${typeName(prefs.addType)} at Playhead`, onClick: () => c.addEvent() },
    { label: "Delete", hint: fmt("Backspace"), disabled: !c.anySelected(), onClick: c.deleteEvents },
  ];
  const viewItems = (): MenuItem[] => [
    ...el.EVENT_TYPES.map((t) => ({ label: t.label, checked: prefs.show[t.id], onClick: () => toggleShown(t.id) })),
    { separator: true },
    { label: "Catch Playhead", checked: prefs.catchPlayhead, onClick: () => setPrefs({ catchPlayhead: !prefs.catchPlayhead }) },
    { label: "MIDI Out (Hear Edited Notes)", checked: prefs.midiOut, onClick: () => setPrefs({ midiOut: !prefs.midiOut }) },
    ...detachMenuItems("list"),
  ];

  return (
    <>
      <div ref={fit.ref} className={"local-bar editor-bar fit-bar" + fit.className}>
        <EditorTabs current="list" short={fit.has("tabs")} />
        <div className="local-menus">
          <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, editItems()))}>
            Edit
          </button>
          <button className="local-menu" {...onPress((e) => openMenuAt(e.currentTarget, viewItems()))}>
            View
          </button>
          {!fit.has("title") && <span className="local-title">{title}</span>}
        </div>
        <div className="local-right">
          {!fit.has("midi-out") && (
            <button className={"lb-btn" + (p.midiOut ? " on" : "")} aria-pressed={p.midiOut} title="MIDI Out (hear edited notes)" aria-label="MIDI Out (hear edited notes)" onClick={() => setPrefs({ midiOut: !p.midiOut })}>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
                <path d="M2 5.2h2.4L7.6 2.5v9L4.4 8.8H2zM9.8 4.6a3.4 3.4 0 0 1 0 4.8M11.4 3a5.6 5.6 0 0 1 0 8" />
              </svg>
            </button>
          )}
          {!fit.has("catch") && (
            <button className={"lb-btn catch" + (p.catchPlayhead ? " on" : "")} aria-pressed={p.catchPlayhead} title="Catch Playhead" aria-label="Catch Playhead" onClick={() => setPrefs({ catchPlayhead: !p.catchPlayhead })}>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
                <path d="M3 2v10M3 7h8M8 4l3 3-3 3" />
              </svg>
            </button>
          )}
          {!fit.has("detach") && <DetachButton panel="list" />}
        </div>
      </div>
      <div ref={toolsFit.ref} className={"el-tools fit-bar" + toolsFit.className}>
        <button className="lb-btn" title={`Add ${typeName(p.addType)} at Playhead`} aria-label={`Add ${typeName(p.addType)} at Playhead`} disabled={!canAdd} onClick={() => c.addEvent()}>
          <Icon.plus />
        </button>
        <button className="popup el-add-type" title="What + adds" {...onPress((e) => openMenuAt(e.currentTarget, typeItems()))}>
          {typeName(p.addType)} <Icon.chevron />
        </button>
        {!toolsFit.has("filters") && (
          <div className="el-filters">
            {el.EVENT_TYPES.map((t) => (
              <button key={t.id} className={"filter" + (p.show[t.id] ? " on" : "")} aria-pressed={!!p.show[t.id]} title={`Show ${t.label}`} onClick={() => setPrefs({ show: { ...p.show, [t.id]: !p.show[t.id] } })}>
                {t.label}
              </button>
            ))}
          </div>
        )}
        {!toolsFit.has("count") && <span className="el-count">{count === 1 ? "1 event" : `${count} events`}</span>}
      </div>
    </>
  );
}
