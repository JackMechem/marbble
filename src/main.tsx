// The frontend's entry point: every window loads this and renders `App`, or just a panel's
// `DetachedWindow`. Key handling, theme and size are set up first so the first paint is right.
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/index.css";
import { applyTheme, savedThemeId } from "./themes";
import { applyUiScale, savedUiScale } from "./uiScale";
import { savedWaveStyle } from "./waveform";
import { setState } from "./store";
import { savedShowOverview } from "./components/Overview";
import "./panelKinds";
import { ownPanel } from "./detach";
import { DetachedWindow } from "./components/DetachedWindow";
import { installSheetKeys } from "./sheetKeys";
import { migrateStorage } from "./oldName";
import { installFocusRing } from "./focusRing";
import { installTooltips } from "./tooltip";
import { invoke } from "@tauri-apps/api/core";
import { KEY_STYLE, setGtkKeyNames, type GtkKeyNames } from "./keyFormat";

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
      {ownPanel ? <DetachedWindow /> : <App />}
    </React.StrictMode>,
  ),
);
