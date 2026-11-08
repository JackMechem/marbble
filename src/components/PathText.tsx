// Text fitted to its box (textFit.ts): a file or folder path shortened from the start at a
// separator, with the whole path in the tooltip (PathText), and a name on up to a few lines broken
// at spaces (LinesText).
import { useLayoutEffect, useRef, useState, type HTMLAttributes } from "react";
import { fitLines, fitPath } from "../textFit";

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

/**
 * `text` on up to `max` lines, broken at spaces only (`fitLines`), each line ending in "…" when cut
 * (`.truncate`). `className` lays the lines out (a column, its line height). `title` is the tooltip
 * while something is cut; the box has none while the whole text shows.
 */
export function LinesText({
  text,
  max = 2,
  className,
  title,
  ...rest
}: { text: string; max?: number; className?: string; title?: string } & Omit<HTMLAttributes<HTMLSpanElement>, "title" | "className" | "children">) {
  const ref = useRef<HTMLSpanElement>(null);
  const [shown, setShown] = useState<{ lines: string[]; cut: boolean }>({ lines: [text], cut: false });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      const room = el.clientWidth;
      const fits = (s: string) => textWidth(el, s) <= room;
      const lines = room > 0 ? fitLines(text, fits, max) : [text];
      const cut = room > 0 && lines.some((l) => !fits(l));
      setShown((p) => (p.cut === cut && p.lines.length === lines.length && p.lines.every((x, i) => x === lines[i]) ? p : { lines, cut }));
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [text, max]);
  return (
    <span ref={ref} className={className} title={shown.cut ? title : undefined} {...rest}>
      {shown.lines.map((l, i) => (
        <span key={i} className="truncate">
          {l}
        </span>
      ))}
    </span>
  );
}
