// A track header's volume slider, with the track's level meter in its line.
import { useMemo, useRef } from "react";
import type { Id } from "../../engine/project";
import { dbToFader, faderToDb, formatDb } from "../../lib/decibels";
import { fmt } from "../../keyboard/keymap";
import { levelSliderSize, meterScale, thumbCenter, type HeaderTier } from "./headerGeometry";
import { Meter, sliderKeyDown, useDrag, type TouchProps } from "../../ui/Controls";
import { sliderAria } from "../../keyboard/sliderKeys";

/**
 * A track header's volume slider, as in Logic's track headers: a slim line, brighter up to a
 * small round thumb, with the track's level meter drawn in the line. The meter is on the slider's
 * scale (see `meterScale`), so the thumb marks where a full-scale signal lands. The slider takes
 * presses over its whole height, not just on the line. Drag to move (Shift for fine), double-click or Alt-click for
 * 0 dB. `track` is null for a folder stack, which has no meter of its own.
 */
export function LevelSlider({
  db,
  onChange,
  width,
  tier,
  track,
  label = "Volume",
  onTouchStart,
  onTouchEnd,
}: {
  db: number;
  onChange: (db: number) => void;
  width: number;
  tier: HeaderTier;
  track: Id | null;
  /** Starts the tooltip, which goes on with the level. */
  label?: string;
} & TouchProps) {
  const { groove, thumb, height } = levelSliderSize(tier);
  // Where the drag has got to, from where it was grabbed (see useDrag).
  const pos = useRef(dbToFader(db));
  const travel = Math.max(1, width - thumb);
  const start = useDrag((d, fine) => {
    pos.current = Math.min(1, Math.max(0, pos.current + d / travel / (fine ? 6 : 1)));
    onChange(Math.round(faderToDb(pos.current) * 10) / 10);
  }, onTouchEnd);
  // Stable per size, so the meter isn't restarted on every render.
  const scale = useMemo(() => meterScale(width, thumb, dbToFader), [width, thumb]);
  const center = thumbCenter(dbToFader(db), width, thumb);
  return (
    <div
      className="level-slider"
      style={{ width, height }}
      title={`${label}: ${formatDb(db)} dB · double-click or ${fmt("Alt")}-click for 0 dB`}
      {...sliderAria({ label, text: db <= -96 ? "minus infinity dB" : `${formatDb(db)} dB`, min: -96, max: 6, now: db })}
      onKeyDown={sliderKeyDown(dbToFader(db), db, (p) => Math.round(faderToDb(p) * 10) / 10, onChange, { onTouchStart, onTouchEnd })}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (e.button !== 0) return;
        if (e.altKey) {
          e.preventDefault();
          onChange(0);
          return;
        }
        pos.current = dbToFader(db);
        onTouchStart?.();
        start(e, "x");
      }}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onChange(0);
      }}
    >
      <div className="level-groove" style={{ height: groove }}>
        <div className="level-fill" style={{ width: center }} />
        {track !== null && <Meter source={{ kind: "track", id: track }} height={width} width={groove} horizontal scale={scale} className="level-meter" clear />}
      </div>
      <div className="level-thumb" style={{ left: center - thumb / 2, top: (height - thumb) / 2, width: thumb, height: thumb }} />
    </div>
  );
}
