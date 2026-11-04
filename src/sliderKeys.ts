// Keyboard control of the custom knobs, faders and sliders (Controls.tsx, LevelSlider.tsx, the effect
// panels' knobs), following the WAI-ARIA slider pattern. Pure, so it can be tested.

/** The keys a focused slider takes. */
export const SLIDER_KEYS = ["ArrowUp", "ArrowRight", "ArrowDown", "ArrowLeft", "PageUp", "PageDown", "Home", "End"] as const;

/** One arrow press moves a control this far along its travel (0..1); Shift moves a tenth as far,
 *  Page Up and Page Down ten times as far. */
export const KEY_STEP = 0.01;

/**
 * Where a key moves a control at `pos` (0..1 along its travel), or null for a key the control
 * doesn't take. Up and Right raise, Down and Left lower, Home and End go to the ends.
 */
export function keyTarget(key: string, pos: number, fine = false, step = KEY_STEP): number | null {
  const s = fine ? step / 10 : step;
  const clamp = (p: number) => Math.min(1, Math.max(0, p));
  switch (key) {
    case "ArrowUp":
    case "ArrowRight":
      return clamp(pos + s);
    case "ArrowDown":
    case "ArrowLeft":
      return clamp(pos - s);
    case "PageUp":
      return clamp(pos + s * 10);
    case "PageDown":
      return clamp(pos - s * 10);
    case "Home":
      return 0;
    case "End":
      return 1;
    default:
      return null;
  }
}

/**
 * The value a key gives a control: `value` maps a position (0..1) to the value the control would
 * report there (rounded, or snapped to a switch's positions), and `current` is what it reports now.
 * A step that rounds back to `current` keeps going in the same direction until the value changes,
 * so a press is never lost to rounding. Null when the key isn't the control's or the value can't
 * move (already at that end).
 */
export function keyValue(key: string, pos: number, current: number, value: (pos: number) => number, fine = false, step = KEY_STEP): number | null {
  const first = keyTarget(key, pos, fine, step);
  if (first === null) return null;
  let p = first;
  let v = value(p);
  if (key !== "Home" && key !== "End") {
    const d = first - pos;
    // A few hundred tries covers the coarsest rounding over the whole travel.
    for (let i = 0; v === current && d !== 0 && p > 0 && p < 1 && i < 1000; i++) {
      p = Math.min(1, Math.max(0, p + d));
      v = value(p);
    }
  }
  return v === current ? null : v;
}

/** The ARIA attributes and key handler of a slider-like control. `pos` is where it sits (0..1),
 *  `current` the value it reports and `value` maps a position to a value, as for `keyValue`.
 *  `onKey` gets the new value; the caller sends it down the same path a drag does. */
export interface SliderA11y {
  label: string;
  /** The value as a person reads it ("-6.0 dB", "L 12"). */
  text: string;
  min: number;
  max: number;
  now: number;
}

/** Props for a focusable slider element: role, name, range and value. */
export function sliderAria({ label, text, min, max, now }: SliderA11y) {
  return {
    role: "slider",
    tabIndex: 0,
    "aria-label": label,
    "aria-valuemin": min,
    "aria-valuemax": max,
    "aria-valuenow": Number.isFinite(now) ? Math.round(now * 1000) / 1000 : min,
    "aria-valuetext": text,
  } as const;
}
