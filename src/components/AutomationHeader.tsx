// A track header's automation controls: the mode pop-up, the parameter pop-up and the + for another lane.
import type { Track } from "../api";
import { addLane, MODE_LABELS, modeMenu, paramLabel, paramMenu, VOLUME } from "../automation";
import { Icon } from "../icons";
import { useStore } from "../store";
import { openMenuAt } from "./Menu";
import { autoParamTarget } from "../controllers";
import { learnContext } from "../learn";
import { onPress } from "../keyNav";

/** The automation mode pop-up: green in Read, orange-red in the writing modes, as in Logic. */
export function AutoModeButton({ track, compact }: { track: Track; compact?: boolean }) {
  return (
    <button
      className={`auto-mode ${track.auto_mode}` + (compact ? " compact" : "")}
      title="Automation mode"
      {...onPress((e) => {
        e.stopPropagation();
        openMenuAt(e.currentTarget, modeMenu(track));
      })}
    >
      {MODE_LABELS[track.auto_mode]}
    </button>
  );
}

/** A track header's automation row: which parameter the lane shows, and the mode. `width`
 *  stacks them in a column that wide instead (the mode and + above the parameter), where a
 *  short header's volume slider was. */
export function AutomationControls({ track, width }: { track: Track; width?: number }) {
  const param = useStore((s) => s.autoLane[track.id]) ?? VOLUME;
  const label = paramLabel(param);
  return (
    <div className={"th-auto" + (width ? " stacked" : "")} style={width ? { width } : undefined} onMouseDown={(e) => e.stopPropagation()}>
      <AutoModeButton track={track} compact />
      <button
        className="popup auto-param"
        title={`Parameter shown on the lane: ${label}`}
        onContextMenu={learnContext(autoParamTarget(track.id, param))}
        {...onPress(async (e) => {
          const el = e.currentTarget;
          openMenuAt(el, await paramMenu(track));
        })}
      >
        <span className="truncate">{label}</span>
        <Icon.chevron />
      </button>
      <AddLaneButton track={track} />
    </div>
  );
}

/** The "+" that shows another automation lane below the track's, as Logic's disclosure
 *  triangle on a lane header does. */
export function AddLaneButton({ track }: { track: Track }) {
  return (
    <button
      className="tbtn auto-lane-btn"
      title="Show another automation lane"
      aria-label="Show another automation lane"
      {...onPress((e) => {
        e.stopPropagation();
        addLane(track);
      })}
    >
      +
    </button>
  );
}
