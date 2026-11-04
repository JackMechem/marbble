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
