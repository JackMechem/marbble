// The Left-click Tool and Command-click Tool pop-ups in the Tracks area's bar (Logic's), with
// the tools' icons. Choosing and using the tools is in trackTools.ts.
import { Icon } from "../../ui/icons";
import { fmt, keyHint } from "../../keyboard/keymap";
import { useStore } from "../../state/store";
import type { TrackTool } from "./tools";
import { toolMenuItems } from "./trackTools";
import { openMenuAt } from "../../ui/Menu";
import { ToolIcon } from "../../editors/pianoRoll/ToolIcon";
import { onPress } from "../../keyboard/keyNav";

/** A Tracks-area tool's icon: the Piano Roll's where the two share a tool. */
function TrackToolIcon({ tool }: { tool: TrackTool }) {
  const p = { width: 14, height: 14, viewBox: "0 0 14 14", fill: "none", stroke: "currentColor", strokeWidth: 1.3, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  switch (tool) {
    case "fade":
      return (
        <svg {...p}>
          <path d="M2 11.5C6.5 11.5 9.5 7.5 12 2.5M2 11.5h10" />
        </svg>
      );
    case "marquee":
      return (
        <svg {...p}>
          <rect x="2" y="3" width="10" height="8" strokeDasharray="2 1.6" />
          <path d="M7 5.5v3M5.5 7h3" />
        </svg>
      );
    default:
      return <ToolIcon tool={tool} />;
  }
}

/** `cmd` false leaves out the Command-click Tool (a bar short of room has it in its View menu). */
export function ToolPopups({ cmd = true }: { cmd?: boolean }) {
  const tools = useStore((s) => s.tools);
  const hint = keyHint("tool-menu");
  return (
    <>
      <button className="popup tool-popup left" title={`Left-click Tool${hint ? ` (${hint})` : ""}`} aria-label={`Left-click Tool${hint ? ` (${hint})` : ""}`} {...onPress((e) => openMenuAt(e.currentTarget, toolMenuItems("left")))}>
        <TrackToolIcon tool={tools.left} /> <Icon.chevron />
      </button>
      {cmd && (
        <button className="popup tool-popup cmd" title={`${fmt("Mod")}-click Tool`} aria-label={`${fmt("Mod")}-click Tool`} {...onPress((e) => openMenuAt(e.currentTarget, toolMenuItems("cmd")))}>
          <TrackToolIcon tool={tools.cmd} /> <Icon.chevron />
        </button>
      )}
    </>
  );
}
