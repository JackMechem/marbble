// The native menu bar's state, from the in-app menus and the key commands: pure parts of
// menuBar.ts, so the tests can run them.
import { MENU_LABELS } from "./menuLabels.ts";

/** An item's state as menu/mod.rs `set_menu_state` takes it. Every field but `id` is optional: only
 *  what changed is sent. */
export interface ItemState {
  id: string;
  enabled?: boolean;
  checked?: boolean;
  /** The label, when it isn't the item's usual one (or back to it). */
  text?: string;
  /** The key command's key shown beside the item, as keymap.ts writes it; "" for none. */
  key?: string;
  /** The same key as the platform writes it (the label on Windows). */
  keyText?: string;
}

/** The parts of an in-app menu item (ui/Menu.tsx) the menu bar follows. */
export interface MenuLike {
  id?: string;
  label?: string;
  disabled?: boolean;
  checked?: boolean;
  children?: MenuLike[];
}

/** The states of the items that have a menu bar id, submenus included: enabled, ticked (when the
 *  item has a tick) and the label. */
export function statesOf(items: MenuLike[], out = new Map<string, ItemState>()): Map<string, ItemState> {
  for (const it of items) {
    if (it.children) statesOf(it.children, out);
    if (!it.id) continue;
    const st: ItemState = { id: it.id, enabled: !it.disabled };
    if (it.checked !== undefined) st.checked = it.checked;
    if (it.label !== undefined) st.text = it.label;
    out.set(it.id, st);
  }
  return out;
}

/** The menu bar item that shows a key command's key: the same id, but Edit › Delete for the
 *  Delete Regions command. */
export const keyItemFor = (command: string): string => (command === "delete-regions" ? "delete" : command);

/** The keys to show: for each key command, its first key on the item with its id. */
export function keyStates(commands: string[], keys: (id: string) => string[], format: (k: string) => string): Map<string, ItemState> {
  const out = new Map<string, ItemState>();
  for (const c of commands) {
    const k = keys(c)[0] ?? "";
    out.set(keyItemFor(c), { id: keyItemFor(c), key: k, keyText: k ? format(k) : "" });
  }
  return out;
}

/** Merges states by id, later ones adding to and overriding earlier ones field by field. */
export function mergeStates(...maps: Map<string, ItemState>[]): Map<string, ItemState> {
  const out = new Map<string, ItemState>();
  for (const m of maps) for (const [id, st] of m) out.set(id, { ...out.get(id), ...st });
  return out;
}

const FIELDS = ["enabled", "checked", "text", "key", "keyText"] as const;

/** What to send: the fields that differ from what was sent last (`sent`, updated in place). An
 *  item starts out as menu/mod.rs builds it: enabled, unticked, with its usual label and no key. */
export function changedStates(next: Map<string, ItemState>, sent: Map<string, ItemState>): ItemState[] {
  const out: ItemState[] = [];
  for (const [id, st] of next) {
    const was: ItemState = sent.get(id) ?? { id, enabled: true, checked: false, text: (MENU_LABELS as Record<string, string>)[id], key: "", keyText: "" };
    const diff: ItemState = { id };
    let any = false;
    for (const f of FIELDS) {
      if (st[f] !== undefined && st[f] !== was[f]) {
        (diff as unknown as Record<string, unknown>)[f] = st[f];
        any = true;
      }
    }
    if (any) out.push(diff);
    sent.set(id, { ...was, ...st });
  }
  return out;
}
