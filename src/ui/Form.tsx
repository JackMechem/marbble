// Themed form controls. Native <select> and number inputs render as GTK widgets on Linux,
// which clash with the rest of the interface, so these draw their own.
import { useEffect, useState } from "react";
import { Icon } from "./icons";
import { openMenuAt } from "./Menu";
import { defaultButton } from "../keyboard/sheetKeys";

/**
 * onKeyDown for a sheet: keys typed in it stay there, so the main window's shortcuts don't
 * fire (sheetKeys.ts handles the keys pressed outside it). Esc closes it (an open menu takes
 * Esc first, in the capture phase), and Return presses its default button unless a field or
 * another button has the focus.
 */
export function sheetKey(e: React.KeyboardEvent, onClose: () => void) {
  e.stopPropagation();
  if (e.key === "Escape") onClose();
  else if (e.key === "Enter" && !(e.target as HTMLElement).closest("input, textarea, select, button")) {
    const b = defaultButton(e.currentTarget as HTMLElement);
    if (b) {
      e.preventDefault();
      b.click();
    }
  }
}

/**
 * A pop-up menu button in place of a native <select>. Arrow Up and Down step through the
 * options without opening it, as a closed native one does; Return or Space opens it.
 */
export function Select<T extends string | number>({
  value,
  options,
  onChange,
  width,
  title,
  className,
}: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
  width?: number | string;
  /** The tooltip; the chosen option's name when there's none. */
  title?: string;
  className?: string;
}) {
  const current = options.find((o) => o[0] === value);
  const open = (el: HTMLElement) =>
    openMenuAt(
      el,
      options.map(([v, label]) => ({ label, checked: v === value, onClick: () => onChange(v) })),
    );
  return (
    <button
      type="button"
      className={"select" + (className ? " " + className : "")}
      style={{ width }}
      title={title ?? current?.[1]}
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        open(e.currentTarget);
      }}
      // A click with no pointer behind it is Return or Space.
      onClick={(e) => e.detail === 0 && open(e.currentTarget)}
      onKeyDown={(e) => {
        if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
        e.preventDefault();
        const i = options.findIndex((o) => o[0] === value);
        const next = options[Math.min(options.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))];
        if (next && next[0] !== value) onChange(next[0]);
      }}
    >
      <span className="select-label">{current?.[1] ?? ""}</span>
      <span className="select-arrows">
        <Icon.chevron />
      </span>
    </button>
  );
}

export function NumberField({
  value,
  onChange,
  step = 1,
  min = -Infinity,
  max = Infinity,
  width = 76,
  resolution = step,
  title,
  format = String,
  parse = parseFloat,
}: {
  value: number;
  onChange: (v: number) => void;
  step?: number;
  min?: number;
  max?: number;
  width?: number;
  /** Typed values are rounded to this; by default, to `step`. */
  resolution?: number;
  /** A tooltip, for a field shown without its caption. */
  title?: string;
  /** How the value is shown (a note name, say), and how typed text is read back: a number, or
   *  null or NaN when it isn't one. */
  format?: (v: number) => string;
  parse?: (text: string) => number | null;
}) {
  const [text, setText] = useState(format(value));
  // On a new value only: a `format` made in the caller's render mustn't clobber typing.
  useEffect(() => setText(format(value)), [value]); // eslint-disable-line react-hooks/exhaustive-deps
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const commit = () => {
    const v = parse(text) ?? NaN;
    if (Number.isFinite(v)) {
      // Rounded to the resolution, then tidied of float noise (0.1 * 3 = 0.30000000000000004).
      const r = clamp(Math.round(Math.round(v / resolution) * resolution * 1e6) / 1e6);
      setText(format(r));
      onChange(r);
    } else setText(format(value));
  };
  const nudge = (d: number) => onChange(clamp(Math.round((value + d) * 1000) / 1000));
  return (
    <div className="numfield" style={{ width }} title={title}>
      <button type="button" className="numfield-step" tabIndex={-1} onClick={() => nudge(-step)} aria-label="Decrease">
        −
      </button>
      <input
        value={text}
        inputMode="decimal"
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") commit();
          if (e.key === "ArrowUp") nudge(step);
          if (e.key === "ArrowDown") nudge(-step);
        }}
      />
      <button type="button" className="numfield-step" tabIndex={-1} onClick={() => nudge(step)} aria-label="Increase">
        +
      </button>
    </div>
  );
}

/** A labelled checkbox in the sheets' style. */
export function Check({ checked, onChange, children, title }: { checked: boolean; onChange: (v: boolean) => void; children: React.ReactNode; title?: string }) {
  return (
    <label className="check" title={title}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {children}
    </label>
  );
}

/** A horizontal slider with its value shown beside it. `format` renders the value. */
export function Slider({
  value,
  onChange,
  min,
  max,
  step = 1,
  width = 160,
  format = (v) => String(v),
}: {
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  width?: number;
  format?: (v: number) => string;
}) {
  return (
    <div className="form-slider">
      <input type="range" min={min} max={max} step={step} value={value} style={{ width }} onChange={(e) => onChange(parseFloat(e.target.value))} />
      <span className="form-slider-value">{format(value)}</span>
    </div>
  );
}
