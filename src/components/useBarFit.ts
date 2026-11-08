// A bar that keeps to one row (barFit.ts): it measures what its row needs after every render and
// whenever it or its parts change size, and steps through the bar's list of steps until it fits.
// The steps change before the browser paints (layout effects), so a step tried and taken back is
// never seen.
import { useLayoutEffect, useRef, useState } from "react";
import { fitOf, nextFit, rowNeed, type Edge, type Fit } from "../barFit";

/**
 * `steps` in the order the bar gives way; `content` is what can change the row's width besides
 * the steps and the room (a region name, a pop-up's value): when it changes, what each step
 * needed is measured again.
 */
export function useBarFit<S extends string>(steps: readonly S[], content = ""): Fit<S> & { ref: React.RefObject<HTMLDivElement | null> } {
  const ref = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState(0);
  // The steps the bar was last drawn with, and what the row needed with each step it overflowed at.
  const shown = useRef(0);
  const needs = useRef<(number | undefined)[]>([]);
  const lastContent = useRef(content);
  const measure = () => {
    const el = ref.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    // The bar's boxes and the boxes in them (a group's buttons), so one that overflows its group
    // counts too.
    const edges: Edge[] = [];
    for (const child of el.children) {
      for (const b of [child, ...child.children]) {
        const r = b.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) continue;
        edges.push({ right: r.right, marginRight: parseFloat(getComputedStyle(b).marginRight) || 0 });
      }
    }
    const need = rowNeed(box.left, edges, (parseFloat(cs.paddingRight) || 0) + (parseFloat(cs.borderRightWidth) || 0));
    const at = shown.current;
    if (need > box.width + 0.5) needs.current[at] = need;
    const next = nextFit(at, need, box.width, needs.current, steps.length);
    if (next !== at) setFit(next);
  };
  useLayoutEffect(() => {
    if (lastContent.current !== content) {
      lastContent.current = content;
      needs.current = [];
    }
    shown.current = fit;
    measure();
  });
  // The bar's own width changes with the window and the panes beside it; its parts' widths with
  // what they show (a tool's icon, a value), which doesn't always render the bar.
  const ro = useRef<ResizeObserver | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    ro.current ??= new ResizeObserver(() => measure());
    ro.current.observe(el);
    for (const child of el.children) ro.current.observe(child);
  });
  useLayoutEffect(
    () => () => {
      ro.current?.disconnect();
      ro.current = null;
    },
    [],
  );
  return { ...fitOf(steps, fit), ref };
}
