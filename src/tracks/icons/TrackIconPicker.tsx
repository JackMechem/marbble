// The Track Icon picker: a grid of the icons by group, with Default (the icon the name or kind
// gives), in an in-window menu. It opens from the icon in a track header, the Track inspector
// and the track's right-click menu, and sets the icon on every selected track when the track is
// one of them.
import { useEffect, useRef } from "react";
import { type Id, type Track } from "../../engine/project";
import { api } from "../../engine/commands";
import { KEYBOARD_FOCUS_ATTR } from "../../keyboard/focusRing";
import { selectedTracks } from "../trackCommands";
import { getState } from "../../state/store";
import { TrackIconArt } from "./trackIconArt";
import { defaultIcon, pickerMove, TRACK_ICON_GROUPS } from "./trackIcons";
import { closeMenu, openMenuAt, type MenuItem } from "../../ui/Menu";

/** The tracks a pick on `track`'s icon sets: the selection when it's in it, else just it. */
function targets(track: Id): Id[] {
  const sel = selectedTracks();
  return sel.includes(track) ? sel : [track];
}

const ROWS = [1, ...TRACK_ICON_GROUPS.map((g) => g.icons.length)];

function IconGrid({ track, ids }: { track: Track; ids: () => Id[] }) {
  const ref = useRef<HTMLDivElement>(null);
  // Every track it sets has this icon chosen (null: all on Default); undefined when they differ.
  const tracks = getState().snap?.project.tracks ?? [];
  const chosen = ids().map((id) => tracks.find((t) => t.id === id)?.icon ?? null);
  const current = chosen.every((c) => c === chosen[0]) ? chosen[0] : undefined;
  const pick = (icon: string | null) => {
    closeMenu();
    api.setTrackIcon(ids(), icon);
  };
  // Focus starts on the current choice, so the arrows move from there. Closed from the keyboard,
  // the picker gives it back to the control that opened it.
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    const grid = ref.current;
    (grid?.querySelector<HTMLButtonElement>("button.on") ?? grid?.querySelector("button"))?.focus();
    return () => {
      if (before && document.activeElement === document.body && document.documentElement.hasAttribute(KEYBOARD_FOCUS_ATTR)) before.focus();
    };
  }, []);
  const key = (e: React.KeyboardEvent) => {
    if (e.key === "Escape" || e.key === "Tab") return;
    // Arrows, Return and Space belong to the grid, not the key commands.
    e.stopPropagation();
    const at = (e.target as HTMLElement).dataset;
    if (at.row === undefined) return;
    const [row, col] = pickerMove(ROWS, Number(at.row), Number(at.col), e.key);
    if (row === Number(at.row) && col === Number(at.col)) return;
    e.preventDefault();
    document.documentElement.setAttribute(KEYBOARD_FOCUS_ATTR, "");
    ref.current?.querySelector<HTMLButtonElement>(`button[data-row="${row}"][data-col="${col}"]`)?.focus();
  };
  return (
    <div ref={ref} className="icon-picker" role="group" aria-label="Track Icon" onKeyDown={key}>
      <button
        type="button"
        className={"icon-picker-default" + (current === null ? " on" : "")}
        data-row={0}
        data-col={0}
        aria-pressed={current === null}
        title="The icon the track's name or kind gives it"
        onClick={() => pick(null)}
      >
        <TrackIconArt id={defaultIcon(track)} size={18} />
        <span>Default</span>
      </button>
      {TRACK_ICON_GROUPS.map((g, r) => (
        <div key={g.label} className="icon-picker-group" role="group" aria-label={g.label}>
          <span className="icon-picker-label">{g.label}</span>
          <div className="icon-picker-row">
            {g.icons.map((icon, c) => (
              <button
                key={icon.id}
                type="button"
                className={"icon-picker-btn" + (current === icon.id ? " on" : "")}
                data-row={r + 1}
                data-col={c}
                title={icon.name}
                aria-label={icon.name}
                aria-pressed={current === icon.id}
                onClick={() => pick(icon.id)}
              >
                <TrackIconArt id={icon.id} size={18} />
              </button>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/** The picker as menu items, for `track` (and the selection with it), e.g. as a submenu. */
export function trackIconMenu(track: Track): MenuItem[] {
  return [{ content: <IconGrid track={track} ids={() => targets(track.id)} /> }];
}

/** Opens the picker below `el` (a track's icon). */
export function openTrackIconPicker(el: HTMLElement, track: Track) {
  openMenuAt(el, trackIconMenu(track));
}
