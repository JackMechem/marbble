// A channel strip's slots from the keyboard: each slot (Setting, Input, Channel EQ, the inserts,
// the sends, Output, VCA, "+N more") is a Tab stop, and the keys keyNav.ts gives a slot do what
// the mouse does there. Taken only while Tab moves the focus (focusRing.ts): a slot focused by a
// click leaves Space, Delete and the arrows to the app's key commands.
import { keyboardFocus } from "./focusRing";
import { slotKey, stepIndex } from "./keyNav";

/** What the keys do on a slot; each gets the slot's element, for a menu to open at. */
export interface SlotActions {
  /** Return and Space: what a click on the slot does (open the plug-in, or the slot's menu). */
  open: (el: HTMLElement) => void;
  /** Alt+Down: the slot's pop-up menu, when it has one besides `open`. */
  menu?: (el: HTMLElement) => void;
  /** The context-menu key and Shift+F10: its right-click menu. */
  context?: (el: HTMLElement) => void;
  /** Delete and Backspace: empty the slot, as its menu's No Plug-in or Remove Send does. */
  remove?: () => void;
}

/** The attribute that marks a slot for Up and Down. */
const NAV = "data-slot-nav";

/** Focuses the slot `dir` from `el` in its strip (they stop at the ends). */
function stepSlot(el: HTMLElement, dir: -1 | 1) {
  const strip = el.closest(".strip") ?? el.parentElement;
  const slots = strip ? [...strip.querySelectorAll<HTMLElement>(`[${NAV}]`)] : [];
  slots[stepIndex(slots.length, slots.indexOf(el), dir)]?.focus();
}

/**
 * Props that make a slot element a Tab stop with the slot keys. `label` names it for screen
 * readers ("Audio FX 2: Channel EQ"). A slot with buttons or a knob of its own is a group; the
 * others are buttons that open a menu.
 */
export function slotProps(label: string, a: SlotActions, opts: { group?: boolean; popup?: boolean } = {}) {
  const keys = ["Enter", a.menu && "Alt+ArrowDown", a.remove && "Delete"].filter(Boolean).join(" ");
  return {
    tabIndex: 0,
    "data-slot-nav": "",
    role: opts.group ? "group" : "button",
    "aria-label": label,
    "aria-haspopup": opts.popup && !opts.group ? ("menu" as const) : undefined,
    "aria-keyshortcuts": keys,
    onKeyDown: (e: React.KeyboardEvent<HTMLElement>) => {
      // Keys on a slot's own power button or send knob are theirs.
      if (e.target !== e.currentTarget || !keyboardFocus()) return;
      const k = slotKey(e.nativeEvent);
      if (!k) return;
      // Every slot key stays in the strip, even one this slot has no use for (Delete on Output
      // mustn't delete the selected regions).
      e.preventDefault();
      e.stopPropagation();
      const el = e.currentTarget;
      if (k === "previous" || k === "next") stepSlot(el, k === "next" ? 1 : -1);
      else if (k === "open") a.open(el);
      else if (k === "menu") (a.menu ?? a.open)(el);
      else if (k === "context") a.context?.(el);
      else if (k === "remove") a.remove?.();
    },
  };
}
