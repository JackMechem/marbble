// Lists from the keyboard: the Library's plug-ins, the Project Chooser's recent projects, Project
// Alternatives and the Marker List. A list is one Tab stop (a roving tabindex: its selected row,
// else the row last focused), its rows take the real focus, and keyNav.ts's listKey decides what
// the keys do on one. As with the slots, the keys are taken only while Tab moves the focus
// (focusRing.ts): a row focused by a click leaves Return, Delete and the arrows as they were.
import { useEffect, useRef, useState } from "react";
import { keyboardFocus } from "./focusRing";
import { listKey, typeAheadText, type Typed } from "./keyNav";

export interface ListKeys {
  /** The rows' keys, in the order they're shown. */
  keys: readonly string[];
  /** The rows' names, for type-ahead (in the order of `keys`). */
  labels?: readonly string[];
  /** The selected row, where selection follows the focus: the Tab stop sits on it. */
  selected?: string | null;
  /** The row the Tab stop starts on when none is selected or has had the focus. */
  start?: string | null;
  /** The keyboard moved to row `key` (select it where selection follows the focus). */
  move?: (key: string) => void;
  /** Return or Space on row `key`: what a click or double-click does there. */
  open?: (key: string, el: HTMLElement) => void;
  /** Delete or Backspace on row `key`: what the row's remove button does. */
  remove?: (key: string) => void;
}

/** The attribute that marks a row and holds its key. */
const ROW = "data-list-row";

/**
 * Keys for a list. Spread `listProps` on the element around the rows (with its role and name) and
 * `rowProps(key)` on each row; `stop` is the row that is the Tab stop.
 */
export function useListKeys(o: ListKeys) {
  const box = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState<string | null>(null);
  const typed = useRef<Typed>({ text: "", at: 0 });
  // A row removed from the keyboard, and the row that takes the focus once it has gone.
  const removed = useRef<{ key: string; next: string | null } | null>(null);
  const has = (k: string | null | undefined): k is string => k != null && o.keys.includes(k);
  const stop = has(o.selected) ? o.selected : has(active) ? active : has(o.start) ? o.start : (o.keys[0] ?? null);

  const rowEl = (k: string) => [...(box.current?.querySelectorAll<HTMLElement>(`[${ROW}]`) ?? [])].find((el) => el.getAttribute(ROW) === k);
  const focusRow = (k: string) => {
    const el = rowEl(k);
    if (!el) return;
    el.focus();
    setActive(k);
    o.move?.(k);
  };

  const shown = o.keys.join("\n");
  useEffect(() => {
    const r = removed.current;
    if (!r || o.keys.includes(r.key)) return;
    removed.current = null;
    // Unless the focus went somewhere else meanwhile (a removal that asked first).
    const a = document.activeElement;
    if (r.next && o.keys.includes(r.next) && (!a || a === document.body || box.current?.contains(a))) focusRow(r.next);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLElement>) => {
    const t = e.target as HTMLElement;
    const k = t.getAttribute(ROW);
    // Keys in a row's own fields and buttons are theirs.
    if (k === null || !keyboardFocus()) return;
    const from = o.keys.indexOf(k);
    let text = "";
    if (o.labels && !e.altKey && !e.ctrlKey && !e.metaKey) {
      const next = typeAheadText(typed.current, e.key, performance.now());
      if (next) typed.current = next;
      text = next?.text ?? "";
    }
    const what = listKey(o.keys.length, from, e.nativeEvent, text, o.labels);
    if (!what) return;
    // Every list key stays in the list, even one it has no use for (Delete in the Library mustn't
    // delete the selected regions, a letter mustn't run a key command).
    e.preventDefault();
    e.stopPropagation();
    if ("move" in what) {
      if (what.move >= 0 && what.move !== from) focusRow(o.keys[what.move]);
    } else if ("open" in what) o.open?.(k, t);
    else if (o.remove) {
      removed.current = { key: k, next: o.keys[from + 1] ?? o.keys[from - 1] ?? null };
      o.remove(k);
    }
  };

  const shortcuts = ["Enter", o.remove && "Delete"].filter(Boolean).join(" ");
  const rowProps = (k: string) => ({
    [ROW]: k,
    tabIndex: k === stop ? 0 : -1,
    "aria-keyshortcuts": shortcuts,
    onFocus: (e: React.FocusEvent<HTMLElement>) => {
      if (e.target === e.currentTarget) setActive(k);
    },
  });

  return { listProps: { ref: box, onKeyDown }, rowProps, stop };
}

/** What `rowProps` gives a row, for a row drawn by a component of its own. */
export type RowProps = ReturnType<ReturnType<typeof useListKeys>["rowProps"]>;
