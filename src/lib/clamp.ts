// Holding a number between two bounds, as most of the edit maths does.

/** `v` held within `lo`–`hi`; when `lo` is above `hi`, `hi` wins. */
export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
