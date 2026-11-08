// Fitting text to its box, the parts CSS can't do alone. A long path is shortened from its start
// at a separator ("…/Music/Marbble"), never mid-name; the whole path goes in the tooltip. A strip's
// name breaks onto two lines at spaces only (`fitLines`). The
// CSS side (truncate, nowrap) is src/styles/text-fit.css. Pure, so it can be unit tested.

/** What a shortened path starts with. */
export const PATH_ELLIPSIS = "…";

/**
 * `path` as it fits, by `fits` (whether a string fits the box): whole, or as many of its last
 * names as fit after "…/". When not even the last name fits, "…/" and the last name, which the
 * box's own text-overflow then ends with "…".
 */
export function fitPath(path: string, fits: (s: string) => boolean): string {
  if (!path || fits(path)) return path;
  const sep = path.includes("\\") && !path.includes("/") ? "\\" : "/";
  const names = path.split(sep).filter((s, i) => s !== "" || i === 0);
  let best = PATH_ELLIPSIS + sep + names[names.length - 1];
  for (let k = 2; k < names.length; k++) {
    const s = PATH_ELLIPSIS + sep + names.slice(-k).join(sep);
    if (!fits(s)) break;
    best = s;
  }
  return best;
}

/**
 * A name on `max` lines (a channel strip's name plate), broken only at spaces: each line takes as
 * many words as fit (`fits`), at least one, and the last line takes the rest. A word too wide for
 * the box, or a last line that doesn't fit, is left for the box's own text-overflow to end in "…".
 */
export function fitLines(text: string, fits: (s: string) => boolean, max = 2): string[] {
  const words = text.trim().split(/\s+/).filter(Boolean);
  const whole = words.join(" ");
  if (words.length <= 1 || fits(whole)) return [whole];
  const lines: string[] = [];
  let rest = words;
  while (rest.length && lines.length < max - 1) {
    let n = 1;
    while (n < rest.length && fits(rest.slice(0, n + 1).join(" "))) n++;
    lines.push(rest.slice(0, n).join(" "));
    rest = rest.slice(n);
  }
  if (rest.length) lines.push(rest.join(" "));
  return lines;
}
