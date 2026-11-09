// When the keyboard focus ring shows. WebKit and Chromium ring a control focused by a click as soon
// as any key is pressed afterwards, and in this app that key is usually a shortcut, so the last
// button clicked would light up. The ring is kept for focus the keyboard moved: Tab turns it on,
// a mouse press turns it off (see base.css, which reads the attribute).

/** The attribute on <html> while focus is being moved with the keyboard. */
export const KEYBOARD_FOCUS_ATTR = "data-keyboard-focus";

interface Root {
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
}

/** Whether the keyboard is moving the focus now (Tab since the last mouse press). */
export const keyboardFocus = (root: { hasAttribute(name: string): boolean } = document.documentElement) => root.hasAttribute(KEYBOARD_FOCUS_ATTR);

/** Starts following Tab and the mouse; returns a function that stops. */
export function installFocusRing(win: EventTarget = window, root: Root = document.documentElement): () => void {
  const key = (e: Event) => {
    if ((e as KeyboardEvent).key === "Tab") root.setAttribute(KEYBOARD_FOCUS_ATTR, "");
  };
  const mouse = () => root.removeAttribute(KEYBOARD_FOCUS_ATTR);
  const capture = { capture: true };
  win.addEventListener("keydown", key, capture);
  win.addEventListener("mousedown", mouse, capture);
  return () => {
    win.removeEventListener("keydown", key, capture);
    win.removeEventListener("mousedown", mouse, capture);
  };
}

interface Focused {
  tagName: string;
  contains(other: unknown): boolean;
  blur(): void;
}

/**
 * A press outside a text field takes the keyboard from it, so the next key is a key command
 * again. Pressing in the Tracks area normally does this by itself, but the global tracks and
 * the ruler prevent the press's default (to keep text from being selected while dragging), and
 * that leaves the Library's search field or a name field focused. Leaves the field alone when
 * the press is inside it.
 */
export function releaseFieldFocus(target: unknown, active: Focused | null = document.activeElement as unknown as Focused | null): boolean {
  if (!active || !["INPUT", "SELECT", "TEXTAREA"].includes(active.tagName) || active.contains(target)) return false;
  active.blur();
  return true;
}
