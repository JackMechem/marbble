// What the control bar's LCD shows, chosen from its chevron menu and kept in the app settings.

/** Beats & Project (position, tempo, signature, key), the same with the position as time, the
 *  selected audio track's input tuned, or the keys held on MIDI input named as a chord. */
export type LcdMode = "beats" | "time" | "tuner" | "chord";

export const LCD_MODES: { mode: LcdMode; label: string }[] = [
  { mode: "beats", label: "Beats & Project" },
  { mode: "time", label: "Time" },
  { mode: "tuner", label: "Tuner" },
  { mode: "chord", label: "Chord" },
];

/** The setting as saved; anything unknown (or not saved yet) is Beats & Project. */
export function lcdMode(saved: string | null | undefined): LcdMode {
  return LCD_MODES.some((m) => m.mode === saved) ? (saved as LcdMode) : "beats";
}

/** Within this many cents the tuner reads in tune (as the Tuner plug-in does). */
export const IN_TUNE = 2;

/** Where the tuner's needle sits across its scale, 0 to 100 (%), for -50 to +50 cents. */
export function needlePercent(cents: number): number {
  return Math.min(100, Math.max(0, 50 + cents));
}

/** Cents with a sign, to a tenth ("+3.2", "−0.4", "0.0"). */
export function centsText(cents: number): string {
  const s = Math.abs(cents).toFixed(1);
  return Number(s) === 0 ? s : (cents > 0 ? "+" : "−") + s;
}

/** A frequency to a tenth of a hertz, or a hundredth below 100 Hz (a bass's low strings). */
export function hzText(hz: number): string {
  return hz.toFixed(hz < 100 ? 2 : 1);
}
