// Canvas helpers the Sampler panel's zone map and waveform share.
import { useEffect } from "react";

export const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** Sizes a canvas to its box and hands back its context in CSS pixels. */
export function prepare(c: HTMLCanvasElement): { ctx: CanvasRenderingContext2D; w: number; h: number } | null {
  const ctx = c.getContext("2d");
  if (!ctx) return null;
  const { width: w, height: h } = c.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return { ctx, w, h };
}

/** Redraws when its box changes size (the dock being dragged, say). */
export function useResize(ref: React.RefObject<HTMLElement | null>, draw: () => void) {
  useEffect(() => {
    if (!ref.current) return;
    const ob = new ResizeObserver(draw);
    ob.observe(ref.current);
    return () => ob.disconnect();
  }, [ref, draw]);
}
