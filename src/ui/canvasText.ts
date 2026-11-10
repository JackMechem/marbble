// Canvas text on the interface's type scale: the six sizes of tokens.css, in the interface's font.
// Canvases draw in CSS pixels, their contexts scaled by pixelRatio() (uiScale.ts), which folds in
// the interface size and the display's density, so a scale size here comes out the same size as
// the stylesheet's text at every interface size, and as sharp.

/** The type scale's steps, `--fs-xs` to `--fs-xxl`. As in the stylesheet, text uses "sm" and up;
 *  "xs" is for tiny numbers and captions in fixed-size boxes (a graph's axis, a note's name, the
 *  slim region bar's name). */
export type TypeSize = "xs" | "sm" | "md" | "lg" | "xl" | "xxl";

/** The scale's sizes in px as tokens.css sets them, for when the tokens can't be read. */
export const TYPE_SCALE_PX: Record<TypeSize, number> = { xs: 10, sm: 11, md: 12, lg: 13, xl: 15, xxl: 17 };

/** The interface's font, as base.css sets it on the body. */
export const UI_FONT = `Inter, "SF Pro Text", "Helvetica Neue", "Segoe UI", Cantarell, system-ui, sans-serif`;

/** The scale from its tokens, read with `get` (a token's value, "" when unset); a size whose
 *  token is missing or not in px keeps its default. */
export function readTypeScale(get: (token: string) => string): Record<TypeSize, number> {
  const out = { ...TYPE_SCALE_PX };
  for (const k of Object.keys(out) as TypeSize[]) {
    const m = /^\s*([\d.]+)px\s*$/.exec(get(`--fs-${k}`));
    if (m) out[k] = Number(m[1]);
  }
  return out;
}

// The tokens aren't themed and don't change at run time, so they're read once, once the
// stylesheet has set them.
let scale: Record<TypeSize, number> | null = null;

function typePx(size: TypeSize): number {
  if (!scale) {
    if (typeof document === "undefined") return TYPE_SCALE_PX[size];
    const root = getComputedStyle(document.documentElement);
    if (!root.getPropertyValue("--fs-md")) return TYPE_SCALE_PX[size];
    scale = readTypeScale((t) => root.getPropertyValue(t));
  }
  return scale[size];
}

/** A canvas `font` for text of scale size `size` at `weight`. */
export function canvasFont(size: TypeSize, weight = 400): string {
  return `${weight} ${typePx(size)}px ${UI_FONT}`;
}
