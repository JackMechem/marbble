// The headers of a track's extra automation lanes and of the Output track.
import { MASTER_ID, type AutoParam, type Track } from "../../engine/project";
import { act } from "../../engine/commands";
import { outputTrack, paramLabel, VOLUME } from "../automation/params";
import { closeLane, paramMenu } from "../automation/menus";
import { useAutomation } from "../automation/useAutomation";
import { Icon } from "../../ui/icons";
import { clampHeight, setTrackHeight, type OutputRow } from "../trackLayout";
import { useStore } from "../../state/store";
import { headerTier } from "./headerGeometry";
import { AddLaneButton, AutomationControls } from "./AutomationHeader";
import { LevelSlider } from "./LevelSlider";
import { openMenuAt } from "../../ui/Menu";
import { LinesText } from "../../ui/PathText";
import { onPress } from "../../keyboard/keyNav";
import { dragWith } from "../../ui/pointerDrag";

/** The header of one of a track's extra automation lanes: its parameter pop-up, and buttons to
 *  add a lane below or close this one. */
function SubLaneHeader({ track, index, param, height, indent }: { track: Track; index: number; param: AutoParam; height: number; indent: number }) {
  return (
    <div className="auto-sublane" style={{ height, paddingLeft: indent }} onMouseDown={(e) => e.stopPropagation()}>
      <div className="th-color" style={{ background: track.color }} />
      <div className="th-auto">
        <button
          className="popup auto-param"
          title={`Parameter shown on this lane: ${paramLabel(param)}`}
          {...onPress(async (e) => {
            const el = e.currentTarget;
            openMenuAt(el, await paramMenu(track, index));
          })}
        >
          <span className="truncate">{paramLabel(param)}</span>
          <Icon.chevron />
        </button>
        <AddLaneButton track={track} />
        <button
          className="tbtn auto-lane-btn"
          title="Close this lane"
          aria-label="Close this lane"
          {...onPress((e) => {
            e.stopPropagation();
            closeLane(track, index);
          })}
        >
          −
        </button>
      </div>
    </div>
  );
}

/** A track's extra lanes, one header each, below its own header. */
export function SubLaneHeaders({ track, height, indent }: { track: Track; height: number; indent: number }) {
  return (
    <>
      {(track.auto_lanes ?? []).map((p, k) => (
        <SubLaneHeader key={k} track={track} index={k} param={p} height={height} indent={indent} />
      ))}
    </>
  );
}

/** The Output track's header (Track › Show Output Track): Stereo Out's volume and, with
 *  automation shown, its lanes. It has no regions. */
export function OutputTrackHeader({ row }: { row: OutputRow }) {
  const project = useStore((s) => s.snap?.project);
  const automation = useStore((s) => s.showAutomation);
  const strip = project ? outputTrack(project) : null;
  const [volume, touch] = useAutomation(strip, VOLUME, project?.master.volume_db ?? 0);
  if (!strip) return null;
  const tier = headerTier(row.height);
  const autoLine = automation && tier === "full";
  const name = <LinesText className="th-name" text="Stereo Out" max={1} title="Stereo Out" />;
  const volumeCell = (
    <div className="th-volume">
      <LevelSlider db={volume} onChange={(db) => act("set_master_volume", { db })} width={88} tier={tier} track={null} label="Stereo Out volume" {...touch} />
    </div>
  );
  const startResize = (e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const startY = e.clientY;
    const startH = row.height;
    const move = (ev: MouseEvent) => setTrackHeight(MASTER_ID, clampHeight(startH + ev.clientY - startY));
    const up = () => {
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "ns-resize";
    dragWith(move, up);
  };
  return (
    <div className="output-track" style={{ top: row.top }}>
      <div className={`track-header output-header tier-${tier}`} style={{ height: row.height }}>
        <div className="th-color" style={{ background: strip.color }} />
        <div className="th-num" />
        {tier !== "tiny" && (
          <div className="th-icon" title="Stereo Out">
            <Icon.waveform />
          </div>
        )}
        {/* With automation shown the slider moves up beside the name, as a track's sits beside its
            buttons, so the automation line below has the header's width (in the name's column it
            was cut to "Volu…" and ran into the slider). */}
        <div className="th-main">
          {autoLine ? (
            <div className="th-controls">
              {name}
              {volumeCell}
            </div>
          ) : (
            name
          )}
          {autoLine && <AutomationControls track={strip} />}
        </div>
        {!autoLine && volumeCell}
        <div
          className="th-resize"
          title={`Drag to resize · double-click to reset`}
          onMouseDown={startResize}
          onDoubleClick={(e) => {
            e.stopPropagation();
            setTrackHeight(MASTER_ID, null);
          }}
        />
      </div>
      {automation && row.autoRows > 0 && <SubLaneHeaders track={strip} height={row.subLane} indent={0} />}
    </div>
  );
}
