// How the Diagnostics window writes its figures, the same on screen and in the copied report.
// Unknown values are a dash, never 0.

/** What an unknown value shows as. */
export const DASH = "—";

const known = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);

/** A share as a percentage: 0.234 → "23%"; `digits` decimals. */
export function pct(v: number | null | undefined, digits = 0): string {
  return known(v) ? `${(v * 100).toFixed(digits)}%` : DASH;
}

/** Milliseconds: "5.3 ms". */
export function ms(v: number | null | undefined, digits = 1): string {
  return known(v) ? `${v.toFixed(digits)} ms` : DASH;
}

/** A whole number, or a dash. */
export function num(v: number | null | undefined): string {
  return known(v) ? String(Math.round(v)) : DASH;
}

/** A size given in kB: "512 kB", "84.2 MB", "1.25 GB". */
export function kb(v: number | null | undefined): string {
  if (!known(v)) return DASH;
  if (v < 1024) return `${Math.round(v)} kB`;
  if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} MB`;
  return `${(v / (1024 * 1024)).toFixed(2)} GB`;
}

/** A size given in bytes, as `kb` writes it. */
export function bytes(v: number | null | undefined): string {
  return known(v) ? kb(v / 1024) : DASH;
}

/** A frame count with its duration at `rate`: "512 (10.7 ms)". */
export function frames(n: number | null | undefined, rate: number): string {
  if (!known(n)) return DASH;
  return rate > 0 ? `${n} (${((n / rate) * 1000).toFixed(1)} ms)` : String(n);
}

/** The thread that played a strip: the audio callback is 0, `marbble-dsp-n` is n. */
export function threadName(t: number | null | undefined): string {
  if (!known(t)) return DASH;
  return t === 0 ? "Audio" : `DSP ${t}`;
}

/** A time since launch: "1:02.345", or "1:02:03.456" past the hour. */
export function sinceLaunch(msTotal: number): string {
  const total = Math.max(0, Math.floor(msTotal));
  const milli = String(total % 1000).padStart(3, "0");
  const s = Math.floor(total / 1000);
  const sec = String(s % 60).padStart(2, "0");
  const min = Math.floor(s / 60);
  return min >= 60 ? `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}:${sec}.${milli}` : `${min}:${sec}.${milli}`;
}

/** "Yes" or "No", or a dash for unknown. */
/** `1 error`, `2 errors`. */
export const plural = (n: number, one: string, many = one + "s") => `${n} ${n === 1 ? one : many}`;

export function yesNo(v: boolean | null | undefined): string {
  return v === true ? "Yes" : v === false ? "No" : DASH;
}

/** A list, or a dash when it's empty. */
export function list(items: (string | number)[]): string {
  return items.length ? items.join(", ") : DASH;
}

/** "128" when given what was asked, else "256 (asked 128)". */
export function asked(given: number, wanted: number): string {
  return given === wanted || !wanted ? String(given || DASH) : `${given || DASH} (asked ${wanted})`;
}
