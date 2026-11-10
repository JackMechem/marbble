// The themes the app offers (each in a file of its own: a map of CSS variables and one of canvas
// colours) and `applyTheme`, which sets one on the window. Flat spreads Studio's maps and the others
// spread Flat's, so a new variable only needs a value in Studio. theme/ also holds the canvas
// colours views draw with (canvasTheme.ts) and keeping the appearance settings (appearance.ts).
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setState } from "../state/store";
import { theme, type CanvasTheme } from "./canvasTheme";
import { flatCss, flatCanvas } from "./flat";
import { contrastCss, contrastCanvas } from "./highContrast";
import { lightCss, lightCanvas } from "./light";
import { midnightCss, midnightCanvas } from "./midnight";
import { studioCss, studioCanvas } from "./studio";

export interface Theme {
  id: string;
  name: string;
  description: string;
  /** CSS custom properties applied to :root. */
  css: Record<string, string>;
  canvas: CanvasTheme;
}

export const THEMES: Theme[] = [
  { id: "studio", name: "Studio", description: "Soft depth and gradients. The default.", css: studioCss, canvas: studioCanvas },
  { id: "flat", name: "Flat", description: "Solid fills and hairline borders.", css: flatCss, canvas: flatCanvas },
  { id: "midnight", name: "Midnight", description: "Flat, with a deep blue tint.", css: midnightCss, canvas: midnightCanvas },
  { id: "light", name: "Light", description: "Bright surfaces for daylight sessions.", css: lightCss, canvas: lightCanvas },
  { id: "contrast", name: "High Contrast", description: "Maximum legibility: black, white and bold edges.", css: contrastCss, canvas: contrastCanvas },
];

const KEY = "marbble.theme";

export function savedThemeId(): string {
  try {
    return localStorage.getItem(KEY) ?? "studio";
  } catch {
    return "studio";
  }
}

/** The window's own theme follows the app's, so on Linux GTK draws the menu bar's text and menus
 *  light or dark to match (gtk-application-prefer-dark-theme), and on macOS the title bar does.
 *  The window's background, which GTK's menu bar is drawn on (it has none of its own in Adwaita),
 *  is the theme's `--bg`: left at the dark one the window opens with, Light's dark text sat on a
 *  near-black bar. */
function windowTheme(kind: "light" | "dark", background: string) {
  try {
    const w = getCurrentWindow();
    w.setTheme(kind).catch(() => {});
    w.setBackgroundColor(background).catch(() => {});
  } catch {
    // Not in a Tauri window (a test page): nothing to theme.
  }
}

/** The custom properties other themes set that this one doesn't. A theme only lists what it
 *  changes, so these must be cleared when switching to it, or the last theme's values stay. */
export function staleThemeKeys(id: string): string[] {
  const own = new Set(Object.keys((THEMES.find((t) => t.id === id) ?? THEMES[0]).css));
  return [...new Set(THEMES.flatMap((t) => Object.keys(t.css)))].filter((k) => !own.has(k));
}

export function applyTheme(id: string, persist = true) {
  const t = THEMES.find((t) => t.id === id) ?? THEMES[0];
  const root = document.documentElement;
  for (const k of staleThemeKeys(t.id)) root.style.removeProperty(k);
  for (const [k, v] of Object.entries(t.css)) root.style.setProperty(k, v);
  root.dataset.theme = t.id;
  root.style.colorScheme = t.id === "light" ? "light" : "dark";
  windowTheme(t.id === "light" ? "light" : "dark", t.css["--bg"]);
  Object.assign(theme, t.canvas);
  if (persist) {
    try {
      localStorage.setItem(KEY, t.id);
    } catch {
      // Storage can be unavailable; the theme still applies for this session.
    }
  }
  setState({ themeId: t.id });
}
