// A file or folder path in a box: shortened from the start at a separator when it doesn't fit
// (textFit.ts), with the whole path in the tooltip.
import { useLayoutEffect, useRef, useState } from "react";
import { fitPath } from "../textFit";

let ctx: CanvasRenderingContext2D | null = null;

/** The width `text` takes in `el`'s font. */
export function textWidth(el: HTMLElement, text: string): number {
  ctx ??= document.createElement("canvas").getContext("2d");
  if (!ctx) return 0;
  const cs = getComputedStyle(el);
  ctx.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  return ctx.measureText(text).width;
}

export function PathText({ path, className, title }: { path: string; className?: string; title?: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [shown, setShown] = useState(path);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      const cs = getComputedStyle(el);
      const room = el.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
      setShown(room > 0 ? fitPath(path, (s) => textWidth(el, s) <= room) : path);
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [path]);
  return (
    <span ref={ref} className={"path-text" + (className ? " " + className : "")} title={title ?? path}>
      <span className="truncate">{shown}</span>
    </span>
  );
}
