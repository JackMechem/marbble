// The tabs that switch between the MIDI editors (Piano Roll, Event List, Step Sequencer) on their
// bars.
import { ownPanel } from "../app/detach/detach";
import { setState } from "../state/store";

/** The MIDI editors, as their tabs name them: in full, and short for a bar short of room. */
const EDITOR_TABS = [
  { id: "editor", name: "Piano Roll", short: "Roll" },
  { id: "list", name: "Event List", short: "List" },
  { id: "steps", name: "Step Sequencer", short: "Steps" },
] as const;

/** Piano Roll / Event List / Step Sequencer, on the MIDI editors' bars; `short`, a word each with
 *  the full name in the tooltip. A panel's own window keeps its editor. */
export function EditorTabs({ current, short = false }: { current: "editor" | "list" | "steps"; short?: boolean }) {
  if (ownPanel) return null;
  return (
    <div className="editor-tabs">
      {EDITOR_TABS.map((t) => (
        <button key={t.id} className={"filter" + (current === t.id ? " on" : "")} aria-pressed={current === t.id} title={short ? t.name : undefined} aria-label={short ? t.name : undefined} onClick={() => setState({ bottom: t.id })}>
          {short ? t.short : t.name}
        </button>
      ))}
    </div>
  );
}
