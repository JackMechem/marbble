// Fitting text to its box, the parts CSS can't do alone. A long path is shortened from its start
// at a separator ("…/Music/Marbble"), never mid-name; the whole path goes in the tooltip. The
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
