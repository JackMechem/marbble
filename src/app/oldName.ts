// Marbble was called Fermata up to 0.1.0-alpha.5, and its view preferences were kept in
// localStorage under `fermata.` keys. The backend moves the webview's storage across at launch
// (crates/engine/src/old_name.rs); this renames the keys in it, once, before anything reads them.

const OLD = "fermata.";
const NEW = "marbble.";

/** Renames every `fermata.` key to its `marbble.` name, keeping a value already saved under the new name. */
export function migrateStorage(storage: Storage = localStorage) {
  const old: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const k = storage.key(i);
    if (k?.startsWith(OLD)) old.push(k);
  }
  for (const k of old) {
    const next = NEW + k.slice(OLD.length);
    const v = storage.getItem(k);
    if (storage.getItem(next) === null && v !== null) storage.setItem(next, v);
    storage.removeItem(k);
  }
}

/** The track heights remembered per project folder (trackLayout.ts) follow the projects folder when
 *  Settings › General renames `~/Music/Fermata`. A value already kept under the new path wins. */
export function moveFolderKeys(from: string, to: string, storage: Storage = localStorage) {
  const prefix = "marbble.trackHeights:";
  const keys: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const k = storage.key(i);
    if (!k?.startsWith(prefix)) continue;
    const path = k.slice(prefix.length);
    if (path === from || path.startsWith(from + "/") || path.startsWith(from + "\\")) keys.push(k);
  }
  for (const k of keys) {
    const next = prefix + to + k.slice(prefix.length + from.length);
    const v = storage.getItem(k);
    if (storage.getItem(next) === null && v !== null) storage.setItem(next, v);
    storage.removeItem(k);
  }
}
