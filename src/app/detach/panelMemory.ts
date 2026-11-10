// What the main window remembers about detached panels from one launch to the next, in
// localStorage: where each kind's window was (one place per kind, or per effect type), and which
// panels to reopen. A panel window measures its own geometry here and sends it to the main
// window (detach.ts), which keeps it; panel windows never write the memory.
import { availableMonitors, getCurrentWindow } from "@tauri-apps/api/window";
import { cascade, fitToMonitors, parseMemory, type PanelMemory, type Rect } from "./detachModel";
import { IS_MAC } from "../../keyboard/keyFormat";
import { getState } from "../../state/store";
import { kindOf } from "./panelRegistry";

/**
 * Window positions and sizes are kept in physical pixels, except on macOS: there the screens
 * share one coordinate space in points, while a physical position is converted with the scale
 * of whichever screen the window is on now, so a Retina screen next to a standard one would put
 * a window on the wrong screen.
 */
const LOGICAL = IS_MAC;

const STORAGE_KEY = "marbble.detachedPanels";

function loadMemory(): PanelMemory {
  try {
    return parseMemory(localStorage.getItem(STORAGE_KEY));
  } catch {
    return parseMemory(null);
  }
}

const memory = loadMemory();

/** The panels that were detached when Marbble quit. */
export function panelsToReopen(): string[] {
  return memory.open;
}

/** Stores the memory with the panels detached now (the kinds that reopen). Main window only. */
export function saveMemory() {
  memory.open = getState().detached.filter((k) => kindOf(k).k?.restore);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(memory));
  } catch {
    // Not critical: the panels open in the main window next time.
  }
}

/** The `memory.geometry` entry a panel's window uses. */
function memoryKey(key: string): string {
  const { k, kind, arg } = kindOf(key);
  return k?.memoryKey?.(arg, getState()) ?? kind;
}

/** Remembers where panel `key`'s window is, as its window reported it. Main window only. */
export function rememberGeometry(key: string, rect: Rect) {
  memory.geometry[memoryKey(key)] = rect;
  saveMemory();
}

/** Where panel `key`'s window was last, or null the first time. Another window remembered in
 *  the same place opens a little lower and to the right. */
export function rememberedRect(key: string): Rect | null {
  const mem = memoryKey(key);
  const saved = memory.geometry[mem];
  if (!saved) return null;
  const others = getState().detached.filter((d) => d !== key && memoryKey(d) === mem).length;
  return cascade(saved, others);
}

/** The monitors' work areas, in the units geometry is kept in. */
async function monitorRects(): Promise<Rect[]> {
  try {
    return (await availableMonitors()).map((m) => {
      const f = LOGICAL ? m.scaleFactor : 1;
      const a = m.workArea;
      return { x: a.position.x / f, y: a.position.y / f, w: a.size.width / f, h: a.size.height / f };
    });
  } catch {
    return [];
  }
}

/** A remembered rect moved onto the monitors connected now, as `open_panel_window` takes it. */
export async function onMonitors(rect: Rect): Promise<Rect & { logical: boolean }> {
  return { ...fitToMonitors(rect, await monitorRects()), logical: LOGICAL };
}

/** Where this panel window is now, or null while it is minimized, zoomed or full screen (not
 *  where it should open next time) or closing. */
export async function currentGeometry(): Promise<Rect | null> {
  const w = getCurrentWindow();
  try {
    if ((await w.isMinimized()) || (await w.isMaximized()) || (await w.isFullscreen())) return null;
    const [pos, size, f] = await Promise.all([w.outerPosition(), w.innerSize(), LOGICAL ? w.scaleFactor() : Promise.resolve(1)]);
    return { x: Math.round(pos.x / f), y: Math.round(pos.y / f), w: Math.round(size.width / f), h: Math.round(size.height / f) };
  } catch {
    return null;
  }
}
