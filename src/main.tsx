// The frontend's entry point: every window loads this and renders `App`, or just a panel's
// `DetachedWindow`, or the Diagnostics window's view. Key handling, theme and size are set up first so the first paint is right.
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./app/App";
import "./styles/index.css";
import { applyTheme, savedThemeId } from "./theme/themes";
import { applyUiScale, savedUiScale } from "./ui/uiScale";
import { savedWaveStyle } from "./tracks/regions/waveform";
import { setState } from "./state/store";
import { savedShowOverview } from "./tracks/overview/Overview";
import "./app/detach/panelKinds";
import { ownPanel } from "./app/detach/detach";
import { DetachedWindow } from "./app/detach/DetachedWindow";
import { DiagnosticsWindow } from "./diagnostics/DiagnosticsWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { installSheetKeys } from "./keyboard/sheetKeys";
import { migrateStorage } from "./app/oldName";
import { installFocusRing } from "./keyboard/focusRing";
import { installTooltips } from "./ui/tooltip";
import { invoke } from "@tauri-apps/api/core";
import { KEY_STYLE, setGtkKeyNames, type GtkKeyNames } from "./keyboard/keyFormat";

// Before anything reads its preferences.
migrateStorage();

// First, so open sheets see every key press before anything else does.
installSheetKeys();
installFocusRing();
// Marbble's own tooltips for every `title`, in this window (a detached panel's too).
installTooltips();

applyTheme(savedThemeId(), false);
const scale = savedUiScale();
if (scale !== 1) applyUiScale(scale, false);
setState({ waveStyle: savedWaveStyle() });
setState({ showOverview: savedShowOverview() });

/** On Linux, GTK's own (translated) names for keys, so menus and tooltips write keys as the
 *  menu bar does (keyFormat.ts). A moment at most: without them keys read as in English GTK. */
async function loadKeyNames() {
  if (KEY_STYLE !== "gtk") return;
  try {
    const late = new Promise<null>((done) => window.setTimeout(() => done(null), 500));
    const names = await Promise.race([invoke<GtkKeyNames | null>("gtk_key_names"), late]);
    if (names) setGtkKeyNames(names);
  } catch {
    // Not in a Tauri window (a test page): the untranslated names.
  }
}

void loadKeyNames().finally(() =>
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      {/* A detached panel's window (detach.ts) shows just that panel. */}
      {getCurrentWindow().label === "diagnostics" ? <DiagnosticsWindow /> : ownPanel ? <DetachedWindow /> : <App />}
    </React.StrictMode>,
  ),
);
