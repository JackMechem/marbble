// One name per command across the menus. The menu bar (src-tauri/src/menu.rs) and the Tracks
// area's Edit, Functions and View menus use the names here, by the menu bar's item ids; the
// Edit menus also share their order (EDIT_MENU). Right-click menus act on what was clicked, so
// they drop "Regions" from a name (SHORT_LABELS). tests/menuLabels.test.ts checks menu.rs
// against this file. Pure, so the tests can import it.

export const MENU_LABELS = {
  undo: "Undo",
  redo: "Redo",
  cut: "Cut",
  copy: "Copy",
  paste: "Paste",
  "paste-original": "Paste at Original Position",
  "paste-replace": "Paste Replace",
  duplicate: "Duplicate",
  "repeat:1": "Once",
  "repeat:2": "2 Times",
  "repeat:3": "3 Times",
  "repeat:4": "4 Times",
  "repeat:8": "8 Times",
  "repeat:16": "16 Times",
  split: "Split Regions at Playhead",
  join: "Join Regions",
  "join-per-track": "Join Regions per Track",
  loop: "Loop Regions",
  "convert-loops": "Convert Loops to Real Copies",
  "pack-takes": "Pack Take Folder",
  "set-locators": "Set Locators by Regions",
  "strip-silence": "Strip Silence…",
  "section-insert-silence": "Insert Silence Between Locators",
  "section-cut": "Cut Section Between Locators",
  "section-repeat": "Repeat Section Between Locators",
  "section-insert-cut": "Insert Cut Section at Left Locator",
  "trim-start-playhead": "Trim Region Start to Playhead",
  "trim-end-playhead": "Trim Region End to Playhead",
  "trim-end-next": "Trim Region End to Next Region",
  "fill-locators": "Fill Within Locators",
  "remove-overlaps": "Remove Overlaps",
  "same-length": "Resize to Same Length",
  "crop-outside": "Crop Regions Outside Locators",
  "move-to-playhead": "Move Region to Playhead",
  "remove-gaps": "Remove Gaps",
  "delete-shuffle": "Delete and Shuffle",
  "name-regions-by-track": "Name Regions by Track",
  "name-tracks-by-region": "Name Tracks by Region",
  "color-regions-by-track": "Color Regions by Track",
  "color-tracks-by-region": "Color Tracks by Region",
  "zero-crossings": "Snap Edits to Zero Crossings",
  "select-all": "Select All",
  "sel-following": "All Following",
  "sel-following-track": "All Following of Same Track",
  "sel-inside-locators": "Inside Locators",
  "sel-muted": "Muted Regions",
  "sel-overlapped": "Overlapped Regions",
  "sel-same-color": "Same Colored Regions",
  "sel-equal": "Equal Regions",
  "sel-similar": "Similar Regions",
  "sel-invert": "Invert Selection",
  delete: "Delete",
  // Track (the Tracks area's Functions menu).
  "new-tracks": "New Tracks…",
  "duplicate-track": "Duplicate Track",
  "same-settings": "New Track with Same Settings",
  "folder-stack": "Create Folder Stack",
  "summing-stack": "Create Summing Stack",
  group: "Group Selected Tracks",
  "output-track": "Show Output Track",
  "hide-tracks": "Hide Track",
  "unhide-all": "Unhide All Tracks",
  "delete-track": "Delete Track",
  "delete-unused": "Delete Unused Tracks",
  // File (also in Functions).
  "import-audio": "Import Audio File…",
  "import-midi": "Import MIDI File…",
  "export-midi": "Export Selection as MIDI File…",
  // View.
  library: "Library",
  inspector: "Inspector",
  mixer: "Mixer",
  browser: "Browsers",
  automation: "Show Automation",
  "used-automation": "Show Used Automation",
  "global-tracks": "Show Global Tracks",
  overview: "Show Overview",
  "show-hidden": "Show Hidden Tracks",
  "zoom-in": "Zoom In",
  "zoom-out": "Zoom Out",
} as const;

export type MenuId = keyof typeof MENU_LABELS;

/** A command's name in the menus. */
export const menuLabel = (id: MenuId): string => MENU_LABELS[id];

/** Names in a region's right-click menu: the menu name without "Regions", since the menu acts
 *  on the regions clicked. */
export const SHORT_LABELS = {
  split: "Split at Playhead",
  join: "Join",
  loop: "Loop",
  "set-locators": "Set Locators",
} as const satisfies Partial<Record<MenuId, string>>;

/** A menu's layout: item ids, "-" for a separator, and submenus. */
export type MenuShape = (MenuId | "-" | { sub: string; items: MenuShape })[];

const REPEAT: MenuShape = ["repeat:1", "repeat:2", "repeat:3", "repeat:4", "repeat:8", "repeat:16"];

/** The Edit menu, in the menu bar and in the Tracks area alike. */
export const EDIT_MENU: MenuShape = [
  "undo",
  "redo",
  "-",
  "cut",
  "copy",
  "paste",
  "paste-original",
  "paste-replace",
  "duplicate",
  { sub: "Repeat", items: REPEAT },
  "-",
  "split",
  "join",
  "join-per-track",
  "loop",
  "convert-loops",
  "pack-takes",
  "set-locators",
  "strip-silence",
  "-",
  { sub: "Section Between Locators", items: ["section-insert-silence", "section-cut", "section-repeat", "section-insert-cut"] },
  { sub: "Trim", items: ["trim-start-playhead", "trim-end-playhead", "trim-end-next", "fill-locators", "remove-overlaps", "same-length", "-", "crop-outside"] },
  { sub: "Move", items: ["move-to-playhead", "remove-gaps", "delete-shuffle"] },
  { sub: "Name and Color", items: ["name-regions-by-track", "name-tracks-by-region", "color-regions-by-track", "color-tracks-by-region"] },
  "-",
  "zero-crossings",
  "-",
  "select-all",
  {
    sub: "Select",
    items: ["sel-following", "sel-following-track", "sel-inside-locators", "-", "sel-muted", "sel-overlapped", "sel-same-color", "sel-equal", "sel-similar", "-", "sel-invert"],
  },
  "-",
  "delete",
];

/** Lays a menu out by its shape: `item` makes each command's entry (its label, state and
 *  action), `submenu` each submenu. Items `item` returns null for are left out. */
export function layMenu<T>(shape: MenuShape, item: (id: MenuId) => T | null, submenu: (label: string, items: T[]) => T, separator: T): T[] {
  return shape.flatMap((s) => {
    if (s === "-") return [separator];
    if (typeof s === "object") return [submenu(s.sub, layMenu(s.items, item, submenu, separator))];
    const it = item(s);
    return it === null ? [] : [it];
  });
}
