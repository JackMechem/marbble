// Detached panel windows, the runtime: the Mixer, the Editors, the Channel EQ or an effect panel
// shown in a window of its own (for another monitor), labelled `panel-<key>`. Every window runs
// this file; the main window decides what is detached and remembers it, opens and closes the
// windows (src-tauri/src/windows.rs) and runs the commands the panels' keys ask for, and every
// window passes the UI state they share and its edits' snapshots to the others.
//
// Events meant for one window are listened for on that window (`getCurrentWebviewWindow()
// .listen`): a plain `listen()` also hears events sent to other windows. Only the engine's
// `tick` and `project`, which every window wants, use plain `listen`.
//
// The pure part (keys, the shared fields, geometry) is detachModel.ts; the panel kinds are in
// panelRegistry.ts (registered from panelKinds.tsx, so this file imports no components); what is
// remembered between launches, and where windows go, is panelMemory.ts.
import { invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { type Snapshot, type Tick } from "../../engine/snapshot";
import { call } from "../../engine/commands";
import type { MenuItem } from "../../ui/Menu";
import { cleanPatch, keyOfLabel, panelLabel, syncedState, syncPatch, withDetached, withoutDetached, type Rect } from "./detachModel";
import { followBottom, kindOf } from "./panelRegistry";
import { currentGeometry, onMonitors, panelsToReopen, rememberedRect, rememberGeometry, saveMemory } from "./panelMemory";
import { commandForEvent } from "../../keyboard/keymap";
import { getSettings, onSettingsChange, receiveSettings, type AppSettings } from "../../settings/appSettings";
import { getState, pushTick, setState, subscribeState, type State } from "../../state/store";
import { applyTheme } from "../../theme/themes";
import { applyUiScale } from "../../ui/uiScale";
import { keyboardFocus } from "../../keyboard/focusRing";
import { leavesKeyToFocus } from "../../keyboard/keyNav";
import { keepsKeys, textFieldMenu, typingInField } from "../../keyboard/textKeys";

const LABEL = getCurrentWindow().label;

/** The panel this window shows, or null in the main window. */
export const ownPanel: string | null = keyOfLabel(LABEL);
export const isMain = ownPanel === null;

// ---- this window's state ----------------------------------------------------------------------

/** Above zero while applying another window's change, which must not be sent back. */
let remote = 0;
/** The state as of the last change seen, to compare the next one with. */
let prev: State = getState();
/** The panel window shows its panel; from now on, its panel going away closes the window. */
let booted = false;
let broadcastTimer = 0;
let lastBroadcast = 0;
/** In the main window: runs a key command a panel window passed on (App's). */
let keyAction: (id: string, repeat: boolean) => void = () => {};
/** In the main window: the panel window (its label) whose text field is being typed in. */
let typingIn: string | null = null;
/** In the main window: the panels detached when Marbble quit have been reopened. */
let restored = false;

// ---- shared state ---------------------------------------------------------------------------

/** Whether there are other windows to tell. */
const hasPeers = (s: State) => !isMain || s.detached.length > 0;

function remotely(f: () => void) {
  remote++;
  try {
    f();
  } finally {
    remote--;
  }
}

/** Other windows get the session's snapshot after this one's edit, at most every 30 ms. */
function broadcastSoon() {
  if (broadcastTimer) return;
  broadcastTimer = window.setTimeout(
    () => {
      broadcastTimer = 0;
      lastBroadcast = performance.now();
      invoke("broadcast_snapshot").catch(() => {});
    },
    Math.max(0, lastBroadcast + 30 - performance.now()),
  );
}

function onStateChange() {
  const s = getState();
  const was = prev;
  prev = s;
  followBottom(s.bottom);
  if (remote === 0 && hasPeers(s)) {
    const patch = syncPatch(was, s);
    // Only the main window decides what is detached.
    if (patch && !isMain) delete patch.detached;
    if (patch && Object.keys(patch).length) emit("ui-sync", { from: LABEL, patch }).catch(() => {});
    if (s.snap !== was.snap) broadcastSoon();
  }
  // A sheet opened from a panel window (its menu bar, a key command) opens here: show it.
  const sheet = (s.dialog && s.dialog !== was.dialog) || (s.showControllerAssignments && !was.showControllerAssignments);
  if (isMain && sheet && s.detached.length) getCurrentWindow().setFocus().catch(() => {});
  // A track or effect went away: so does its window.
  if (isMain && s.snap && s.snap !== was.snap) {
    for (const key of s.detached) {
      const { k, arg } = kindOf(key);
      if (k?.alive && !k.alive(arg, s)) return closeDetached(key);
    }
  }
  if (!isMain && booted) {
    const { k, arg } = kindOf(ownPanel!);
    const fix = k?.own?.(arg, s);
    if (fix) return setState(fix);
  }
  // A detached panel is shown only in its own window: shown here (its key, its slot), it goes
  // back out and its window comes to the front. Each setState re-runs this with fresh state.
  for (const key of s.detached) {
    if (key === ownPanel) continue;
    const { k, arg } = kindOf(key);
    if (k?.shown(arg, s)) {
      setState(k.hide(arg, s));
      invoke("focus_panel_window", { label: panelLabel(key) }).catch(() => {});
      return;
    }
  }
  // The panel was closed in its own window (its ×, Esc): so is the window.
  if (!isMain && booted) {
    const { k, arg } = kindOf(ownPanel!);
    if (k && !k.shown(arg, getState())) {
      booted = false;
      send({ type: "close", key: ownPanel! });
    }
  }
}

/** A snapshot from the engine or another window: stored, not passed on. */
export function receiveSnapshot(snap: Snapshot) {
  remotely(() => setState({ snap }));
}

interface SyncMsg {
  from: string;
  patch: Record<string, unknown>;
}

function applySync({ from, patch }: SyncMsg) {
  if (from === LABEL) return;
  const p = cleanPatch(patch);
  // Only the main window decides what is detached.
  if (isMain) delete p.detached;
  remotely(() => {
    if (typeof p.themeId === "string" && p.themeId !== getState().themeId) applyTheme(p.themeId, isMain);
    if (typeof p.uiScale === "number" && p.uiScale !== getState().uiScale) void applyUiScale(p.uiScale, isMain);
    delete p.themeId;
    delete p.uiScale;
    if (Object.keys(p).length) setState(p as Partial<State>);
  });
}

function onSettings() {
  if (remote === 0 && hasPeers(getState())) emit("ui-settings", { from: LABEL, settings: getSettings() }).catch(() => {});
}

function applySettings({ from, settings }: { from: string; settings: AppSettings }) {
  if (from !== LABEL) remotely(() => receiveSettings(settings));
}

// ---- messages to the main window ------------------------------------------------------------

type PanelMsg =
  | { type: "hello" }
  | { type: "attach" | "detach" | "close"; key: string }
  | { type: "key"; id: string; repeat?: boolean }
  | { type: "typing"; on: boolean }
  | { type: "geometry"; key: string; rect: Rect };

function send(m: PanelMsg): Promise<void> {
  return emitTo("main", "panel-msg", { from: LABEL, ...m }).catch(() => {});
}

/**
 * The window of a detached panel with a text field being typed in, or null. A key command a MIDI
 * controller fires is held back then, as it is for a field in the main window (App's
 * runMidiCommand).
 */
export function typingPanel(): string | null {
  const label = typingIn;
  return label && getState().detached.some((k) => panelLabel(k) === label) ? label : null;
}

function onPanelMsg(m: PanelMsg & { from: string }) {
  switch (m.type) {
    case "typing":
      if (m.on) typingIn = m.from;
      else if (typingIn === m.from) typingIn = null;
      break;
    case "hello":
      emitTo(m.from, "ui-sync", { from: LABEL, patch: syncedState(getState()) }).catch(() => {});
      emitTo(m.from, "ui-settings", { from: LABEL, settings: getSettings() }).catch(() => {});
      break;
    case "attach":
      attachPanel(m.key);
      break;
    case "detach":
      detachPanel(m.key);
      break;
    case "close":
      closeDetached(m.key);
      break;
    case "key":
      keyAction(m.id, !!m.repeat);
      break;
    case "geometry":
      rememberGeometry(m.key, m.rect);
      break;
  }
}

// ---- detaching and attaching ----------------------------------------------------------------

/** Opens panel `key`'s window where it was last (the first time, at its kind's size where the
 *  system puts it); if it can't, the panel goes back in the main window. */
async function openWindow(key: string) {
  const { k, arg } = kindOf(key);
  if (!k) return;
  const remembered = rememberedRect(key);
  const geometry = remembered ? await onMonitors(remembered) : null;
  const size = k.sizeOf?.(arg, getState()) ?? k.size;
  try {
    await invoke("open_panel_window", { label: panelLabel(key), title: k.title(arg, getState()), width: size.w, height: size.h, geometry });
  } catch (e) {
    const s = getState();
    setState({ detached: withoutDetached(s.detached, key), ...k.show(arg, s), toast: { text: `Couldn't open a window: ${e}`, at: Date.now() } });
    saveMemory();
  }
}

/** Shows a panel in its own window (or brings its window to the front). */
export function detachPanel(key: string) {
  const { k, arg } = kindOf(key);
  if (!k) return;
  const s = getState();
  if (!isMain) {
    if (k.shown(arg, s)) setState(k.hide(arg, s));
    send({ type: "detach", key });
    return;
  }
  if (s.detached.includes(key)) {
    invoke("focus_panel_window", { label: panelLabel(key) }).catch(() => {});
    return;
  }
  setState({ ...(k.shown(arg, s) ? k.hide(arg, s) : {}), detached: withDetached(s.detached, key) });
  saveMemory();
  void openWindow(key);
}

/** Puts a detached panel back in the main window and closes its window. */
export function attachPanel(key: string) {
  // From the panel's window, which is about to close: it doesn't hide the panel first, so the
  // main window's copy starts its meters after this one last stopped them.
  if (!isMain) {
    send({ type: "attach", key });
    return;
  }
  const s = getState();
  if (!s.detached.includes(key)) return;
  const { k, arg } = kindOf(key);
  const rest = { ...s, detached: withoutDetached(s.detached, key) };
  setState({ detached: rest.detached, ...(k && !k.shown(arg, rest) ? k.show(arg, rest) : {}) });
  invoke("close_panel_window", { label: panelLabel(key) }).catch(() => {});
  saveMemory();
}

/** Closes a detached panel's window without showing the panel in the main window. */
export function closeDetached(key: string) {
  if (!isMain) {
    send({ type: "close", key });
    return;
  }
  const s = getState();
  if (!s.detached.includes(key)) return;
  setState({ detached: withoutDetached(s.detached, key) });
  invoke("close_panel_window", { label: panelLabel(key) }).catch(() => {});
  saveMemory();
}

/** Window › "Mixer in Separate Window" and the like. */
export function toggleDetached(key: string) {
  if (getState().detached.includes(key)) attachPanel(key);
  else detachPanel(key);
}

/**
 * In a panel's window, runs key command `id` in the main window instead and returns true: for
 * what opens a sheet there (Controller Assignments). False in the main window.
 */
export function runInMain(id: string): boolean {
  if (isMain) return false;
  send({ type: "key", id });
  return true;
}

/** The end of a panel's View menu: open it in its own window, or attach it from there. */
export function detachMenuItems(key: string): MenuItem[] {
  return [
    { separator: true },
    ownPanel === key ? { label: "Attach to Main Window", onClick: () => attachPanel(key) } : { label: "Open in Separate Window", onClick: () => detachPanel(key) },
  ];
}

// ---- installing ---------------------------------------------------------------------------------

/**
 * The main window's part (App, once): it hears the panels, and reopens the panels that were
 * detached when Marbble quit. `key` runs a key command a panel window passed on.
 */
export function installDetach(opts: { key: (id: string, repeat: boolean) => void }): () => void {
  keyAction = opts.key;
  prev = getState();
  const offs = [subscribeState(onStateChange), onSettingsChange(onSettings)];
  const wv = getCurrentWebviewWindow();
  const pending = [
    wv.listen<PanelMsg & { from: string }>("panel-msg", (e) => onPanelMsg(e.payload)),
    wv.listen<SyncMsg>("ui-sync", (e) => applySync(e.payload)),
    wv.listen<{ from: string; settings: AppSettings }>("ui-settings", (e) => applySettings(e.payload)),
    wv.listen<Snapshot>("project-sync", (e) => receiveSnapshot(e.payload)),
  ];
  let live = true;
  // Once the panels can be heard: their first message is "hello".
  Promise.all(pending).then(() => {
    if (!live) return;
    if (!restored) {
      restored = true;
      for (const key of panelsToReopen()) if (kindOf(key).k?.restore) detachPanel(key);
    }
    // Panels that outlived a reload of this window catch up.
    const s = getState();
    if (s.detached.length) emit("ui-sync", { from: LABEL, patch: syncedState(s) }).catch(() => {});
  });
  return () => {
    live = false;
    offs.forEach((off) => off());
    pending.forEach((p) => p.then((off) => off()));
  };
}

/** Sends where a panel window is to the main window, which remembers it per kind. */
async function reportGeometry() {
  const rect = await currentGeometry();
  if (rect) send({ type: "geometry", key: ownPanel!, rect });
}

/**
 * A panel window's part (DetachedWindow, once). `menu` handles a native menu item that belongs
 * to the panel (Select All in a Piano Roll) and returns false for the rest, which go to the
 * main window.
 */
export function installPanelWindow(opts: { menu: (id: string) => boolean }): () => void {
  const key = ownPanel!;
  const { k, arg } = kindOf(key);
  prev = getState();
  const offs = [subscribeState(onStateChange), onSettingsChange(onSettings)];
  const wv = getCurrentWebviewWindow();
  const win = getCurrentWindow();
  let geomTimer = 0;
  const geometrySoon = () => {
    window.clearTimeout(geomTimer);
    geomTimer = window.setTimeout(reportGeometry, 400);
  };
  const pending = [
    // The main window already shows the engine's warnings.
    listen<Tick>("tick", (e) => pushTick({ ...e.payload, warning: null })),
    listen<Snapshot>("project", (e) => receiveSnapshot(e.payload)),
    wv.listen<Snapshot>("project-sync", (e) => receiveSnapshot(e.payload)),
    wv.listen<SyncMsg>("ui-sync", (e) => applySync(e.payload)),
    wv.listen<{ from: string; settings: AppSettings }>("ui-settings", (e) => applySettings(e.payload)),
    wv.listen<string>("menu", (e) => {
      // A focused text field here takes its edits first, and drops the other accelerators
      // (textKeys.ts).
      if (!textFieldMenu(e.payload) && !opts.menu(e.payload)) emitTo("main", "menu", e.payload).catch(() => {});
    }),
    // The window's close button attaches the panel again (quitting closes it without asking).
    win.onCloseRequested(async () => {
      booted = false;
      await send({ type: "attach", key });
    }),
    win.onMoved(geometrySoon),
    win.onResized(geometrySoon),
  ];
  const stopKeys = forwardKeys();
  const stopTyping = reportTypingChanges();
  if (k) remotely(() => setState(k.show(arg, getState())));
  booted = !!k;
  let live = true;
  Promise.all(pending).then(() => {
    if (!live) return;
    send({ type: "hello" });
    // `call`, not `act`: a snapshot from `act` would be sent on to the other windows.
    call<Snapshot>("get_state").then((snap) => snap && live && receiveSnapshot(snap));
  });
  return () => {
    live = false;
    booted = false;
    window.clearTimeout(geomTimer);
    stopKeys();
    stopTyping();
    offs.forEach((off) => off());
    pending.forEach((p) => p.then((off) => off()));
  };
}

/** In a panel window: key commands run in the main window, as if pressed there; the panel's own
 *  keys (the Piano Roll's) are handled first, in the capture phase, and prevent the default.
 *  Returns the function that stops it. */
function forwardKeys(): () => void {
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || getState().dialog) return;
    const t = e.target as HTMLElement | null;
    if (keepsKeys(t)) return;
    if (leavesKeyToFocus(e, t, keyboardFocus())) return;
    const id = commandForEvent(e);
    if (!id) return;
    e.preventDefault();
    send({ type: "key", id, repeat: e.repeat });
  };
  window.addEventListener("keydown", onKey);
  return () => window.removeEventListener("keydown", onKey);
}

/** In a panel window: tells the main window when a text field here starts or stops being typed
 *  in (the window gains or loses the focus, or a field does), so a MIDI controller's key commands
 *  wait. Returns the function that stops it. */
function reportTypingChanges(): () => void {
  let typing = false;
  const reportTyping = () => {
    const now = typingInField();
    if (now !== typing) {
      typing = now;
      send({ type: "typing", on: now });
    }
  };
  // After the event: while focus moves, neither the old element nor the new one has it.
  const typingSoon = () => window.setTimeout(reportTyping, 0);
  const focusEvents = ["focusin", "focusout", "focus", "blur"] as const;
  focusEvents.forEach((ev) => window.addEventListener(ev, typingSoon));
  return () => focusEvents.forEach((ev) => window.removeEventListener(ev, typingSoon));
}
