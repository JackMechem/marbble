// What a fader or knob shows while its parameter is automated.
import { useEffect, useRef, useState } from "react";
import { type AutoParam, type Track } from "../../engine/project";
import { act } from "../../engine/commands";
import { valueAt } from "./autoLanes";
import { currentTick, onTick } from "../../state/store";
import { laneOf } from "./params";

/**
 * What a fader or knob shows: the automation being played while the transport runs (and the
 * user isn't holding the control), otherwise the control's own value. Also returns the touch
 * handlers that tell the engine when the control is held, for Touch and Latch writing.
 */
export function useAutomation(track: Track | null, param: AutoParam, live: number): [number, { onTouchStart: () => void; onTouchEnd: () => void }] {
  const [touching, setTouching] = useState(false);
  const sent = useRef(false);
  const [, setFrame] = useState(0);
  const lane = track ? laneOf(track, param) : undefined;
  const follows = !!track && !!lane?.points.length && (track.auto_mode === "read" || track.auto_mode === "touch" || track.auto_mode === "latch");
  useEffect(() => {
    if (!follows) return;
    let was = false;
    return onTick((t) => {
      if (t.playing || was) setFrame((n) => n + 1);
      was = t.playing;
    });
  }, [follows]);
  const writing = !!track && (track.auto_mode === "touch" || track.auto_mode === "latch" || track.auto_mode === "write");
  const tick = currentTick();
  // In Read the lane wins even while the control is held, so it springs back as in Logic.
  const value = follows && !(touching && writing) && tick?.playing ? (valueAt(lane!.points, tick.pos) ?? live) : live;
  const touch = (on: boolean) => {
    setTouching(on);
    // Only the writing modes care (Read just plays the lane over the control); a release goes
    // wherever the grab went.
    if (track && (on ? writing : sent.current)) act("automation_touch", { track: track.id, param, touching: on });
    sent.current = on && writing;
  };
  return [value, { onTouchStart: () => touch(true), onTouchEnd: () => touch(false) }];
}
