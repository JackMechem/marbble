// In-window menus (right-click, local and slot menus): `openMenu`, `openMenuAt` and the `MenuHost` that
// draws whichever is open. contextMenus.ts and the editors build the items.
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { Icon } from "./icons";
import { keyboardFocus } from "../keyboard/focusRing";
import { menuEdge, menuKey, menuStep, typeAheadText, TYPE_AHEAD_ATTR, type NavItem, type Typed } from "../keyboard/keyNav";

export interface MenuItem {
  /** The menu bar item it matches (src/app/menuBar/menuLabels.ts ids), so the menu bar can follow its state. */
  id?: string;
  label?: string;
  onClick?: () => void;
  children?: MenuItem[];
  checked?: boolean;
  disabled?: boolean;
  separator?: boolean;
  header?: boolean;
  hint?: string;
  /** Renders a colour swatch instead of the label text. */
  color?: string;
  /** Renders a grid of colour swatches (named in their tooltips) instead of a row. */
  swatches?: Swatch[];
  /** Renders this instead of a row: a picker of its own, which closes the menu when it's done. */
  content?: ReactNode;
}

/** One colour of a swatch grid. */
export interface Swatch {
  color: string;
  name: string;
  checked?: boolean;
  onClick: () => void;
}

/** A compact grid of colours, five to a row, each a button named for screen readers. */
function SwatchGrid({ swatches }: { swatches: Swatch[] }) {
  return (
    <div className="menu-swatches" role="group" aria-label="Colors">
      {swatches.map((sw) => (
        <button
          key={sw.color}
          type="button"
          className={"menu-swatch-btn" + (sw.checked ? " on" : "")}
          style={{ background: sw.color }}
          title={sw.name}
          aria-label={sw.name}
          aria-pressed={!!sw.checked}
          onClick={() => {
            closeMenu();
            sw.onClick();
          }}
        />
      ))}
    </div>
  );
}

interface Open {
  x: number;
  y: number;
  items: MenuItem[];
  minWidth?: number;
  /** Opened from the keyboard (Return on a focused slot or pop-up): its first item takes the focus. */
  keyboard: boolean;
}

let current: Open | null = null;
/** What had the focus when the menu opened: it gets it back when the menu closes from the keyboard
 *  or after a choice made with it. */
let opener: HTMLElement | null = null;
const subs = new Set<() => void>();
const emit = () => subs.forEach((s) => s());

export function openMenu(x: number, y: number, items: MenuItem[], minWidth?: number) {
  if (!current) opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  current = { x, y, items, minWidth, keyboard: keyboardFocus() };
  emit();
}

/** Opens a menu below an element, like a pop-up button. */
export function openMenuAt(el: HTMLElement, items: MenuItem[]) {
  const r = el.getBoundingClientRect();
  openMenu(r.left, r.bottom + 2, items, r.width);
}

/** Whether a menu is open (key handlers leave Esc and arrows to it). */
export const menuOpen = () => current !== null;

/**
 * Closes the menu. When the keyboard had moved the focus into it (or `restore` says so), the focus
 * goes back to what opened it, so Tab and the arrows go on from there; a click elsewhere passes
 * `restore` false and leaves the focus to what was clicked.
 */
export function closeMenu(restore = true) {
  const back = opener;
  const inMenu = document.activeElement instanceof Element && !!document.activeElement.closest(".menu-layer");
  current = null;
  opener = null;
  emit();
  if (restore && inMenu && back?.isConnected) back.focus({ preventScroll: true });
}

/** The keys typed for type-ahead, forgotten after a pause. */
let typed: Typed = { text: "", at: 0 };

/** `key` added to the type-ahead letters (keyNav.ts), or "" when it isn't one. */
function typedSoFar(key: string): string {
  const next = typeAheadText(typed, key, performance.now());
  if (!next) return "";
  typed = next;
  return next.text;
}

/** The menu's rows as navigation sees them (keyNav.ts). */
const navItems = (items: MenuItem[]): NavItem[] => items.map((it) => ({ label: it.label, disabled: it.disabled, separator: it.separator, header: it.header, custom: !!(it.swatches || it.content) }));

/** Focuses row `i` of a menu element. */
function focusRow(menu: Element | null, i: number) {
  if (i >= 0) (menu?.querySelector(`[data-i="${i}"]`) as HTMLElement | null)?.focus();
}

/** How each open submenu closes back to its parent, by depth (Esc and Left). */
const backs = new Map<number, () => void>();

/** The row with the focus in a menu element, or -1. */
function focusedRow(menu: HTMLElement): number {
  const el = document.activeElement?.closest("[data-i]");
  return el && menu.contains(el) ? Number(el.getAttribute("data-i")) : -1;
}

function List({ items, x, y, minWidth, depth, autoFocus, onBack }: { items: MenuItem[]; x: number; y: number; minWidth?: number; depth: number; autoFocus?: boolean; onBack?: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [sub, setSub] = useState<{ i: number; x: number; y: number; focus: boolean } | null>(null);
  const timer = useRef<number>(0);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    let nx = x;
    let ny = y;
    if (nx + r.width > window.innerWidth - 4) nx = depth > 0 ? x - r.width - (minWidth ?? 0) : window.innerWidth - r.width - 4;
    if (ny + r.height > window.innerHeight - 4) ny = Math.max(4, window.innerHeight - r.height - 4);
    setPos({ x: nx, y: ny });
  }, [x, y, depth, minWidth]);

  // Opened from the keyboard: the checked item takes the focus (a pop-up's current choice), or the first.
  useEffect(() => {
    if (!autoFocus) return;
    const nav = navItems(items);
    const checked = items.findIndex((it, i) => it.checked && menuStep(nav, i - 1, 1) === i);
    focusRow(ref.current, checked >= 0 ? checked : menuEdge(nav, "first"));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps -- only when it opens

  useEffect(() => {
    if (!onBack) return;
    backs.set(depth, onBack);
    return () => {
      if (backs.get(depth) === onBack) backs.delete(depth);
    };
  }, [depth, onBack]);

  const openSub = (i: number, focus: boolean) => {
    const row = ref.current?.querySelector(`[data-i="${i}"]`);
    if (!row || !items[i]?.children) return;
    const r = row.getBoundingClientRect();
    setSub({ i, x: r.right - 2, y: r.top - 5, focus });
  };
  const choose = (it: MenuItem, i: number, keyboard: boolean) => {
    if (it.disabled) return;
    if (it.children) {
      if (keyboard) openSub(i, true);
      return;
    }
    closeMenu();
    it.onClick?.();
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    const menu = ref.current;
    const target = e.target as Element;
    // Keys pressed in a submenu are its own, and a picker's (a swatch, an icon grid) are the picker's.
    if (!menu || target.closest(".menu") !== menu || (target !== menu && !target.closest("[data-i]"))) return;
    const from = focusedRow(menu);
    const k = menuKey(navItems(items), from, e, typedSoFar(e.key));
    if (!k) return;
    e.preventDefault();
    e.stopPropagation();
    if ("move" in k) focusRow(menu, k.move);
    else if ("choose" in k) choose(items[from], from, true);
    else if ("open" in k) openSub(from, true);
    else if ("back" in k) onBack?.();
  };
  // Back from a submenu: it closes and its item has the focus again.
  const back = (i: number) => () => {
    setSub(null);
    focusRow(ref.current, i);
  };

  return (
    <>
      <div
        ref={ref}
        className="menu"
        role="menu"
        tabIndex={-1}
        data-depth={depth}
        style={{ left: pos.x, top: pos.y, minWidth: depth === 0 ? minWidth : undefined }}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        {items.map((it, i) =>
          it.separator ? (
            <div key={i} className="menu-sep" role="separator" />
          ) : it.swatches ? (
            <SwatchGrid key={i} swatches={it.swatches} />
          ) : it.content ? (
            <div key={i}>{it.content}</div>
          ) : it.header ? (
            <div key={i} className="menu-header" role="presentation">
              {it.label}
            </div>
          ) : (
            <div
              key={i}
              data-i={i}
              {...{ [TYPE_AHEAD_ATTR]: "menu" }}
              tabIndex={-1}
              role={it.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
              aria-checked={it.checked !== undefined ? it.checked : undefined}
              aria-disabled={it.disabled || undefined}
              aria-haspopup={it.children ? "menu" : undefined}
              aria-expanded={it.children ? sub?.i === i : undefined}
              className={"menu-item" + (it.disabled ? " disabled" : "") + (sub?.i === i ? " open" : "")}
              onMouseEnter={(e) => {
                window.clearTimeout(timer.current);
                // While the keyboard is in the menu, the pointer takes the focus along with it.
                if (ref.current?.contains(document.activeElement) && !it.disabled) e.currentTarget.focus();
                if (it.children) {
                  timer.current = window.setTimeout(() => openSub(i, false), 90);
                } else {
                  timer.current = window.setTimeout(() => setSub(null), 120);
                }
              }}
              onClick={() => choose(it, i, false)}
            >
              <span className="menu-check">{it.checked ? "✓" : ""}</span>
              {it.color ? (
                <span className="menu-swatch" style={{ background: it.color }} />
              ) : (
                <span className="menu-label">{it.label}</span>
              )}
              {it.hint && <span className="menu-hint">{it.hint}</span>}
              {it.children && (
                <span className="menu-arrow">
                  <Icon.chevronRight />
                </span>
              )}
            </div>
          ),
        )}
      </div>
      {sub && items[sub.i]?.children && (
        <List key={sub.i} items={items[sub.i].children!} x={sub.x} y={sub.y} depth={depth + 1} minWidth={ref.current?.offsetWidth} autoFocus={sub.focus} onBack={back(sub.i)} />
      )}
    </>
  );
}

export function MenuHost() {
  const open = useSyncExternalStore(
    (l) => {
      subs.add(l);
      return () => subs.delete(l);
    },
    () => current,
  );
  useEffect(() => {
    if (!open) return;
    // Capture phase, so dialogs that stop propagation can't keep a menu open; clicks inside a
    // menu are left alone.
    const down = (e: Event) => {
      if (!(e.target instanceof Element && e.target.closest(".menu"))) closeMenu(false);
    };
    // Taken in the capture phase, before the sheet or editor under the menu sees them: Esc closes
    // the menu (a submenu's Esc only that, see List), Tab closes it, and Up, Down, Home and End
    // move into a menu opened with the mouse. Keys in the menu are then its own (List's onKeyDown).
    const key = (e: KeyboardEvent) => {
      const at = document.activeElement instanceof Element ? document.activeElement.closest(".menu") : null;
      const depth = Number(at?.getAttribute("data-depth") ?? 0);
      if (e.key === "Escape" || e.key === "Tab") {
        e.preventDefault();
        e.stopPropagation();
        const back = e.key === "Escape" ? backs.get(depth) : undefined;
        if (back) back();
        else closeMenu();
        return;
      }
      if (at || e.altKey || e.ctrlKey || e.metaKey || !["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
      const nav = navItems(current?.items ?? []);
      const i = e.key === "ArrowDown" || e.key === "Home" ? menuEdge(nav, "first") : menuEdge(nav, "last");
      e.preventDefault();
      e.stopPropagation();
      focusRow(document.querySelector('.menu-layer .menu[data-depth="0"]'), i);
    };
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key, true);
    window.addEventListener("blur", down);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("blur", down);
    };
  }, [open]);
  if (!open) return null;
  return (
    <div className="menu-layer">
      <List key={`${open.x},${open.y}`} items={open.items} x={open.x} y={open.y} minWidth={open.minWidth} depth={0} autoFocus={open.keyboard} />
    </div>
  );
}
