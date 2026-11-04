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

// Before anything reads its preferences.
migrateStorage();

// First, so open sheets see every key press before anything else does.
installSheetKeys();

applyTheme(savedThemeId(), false);
const scale = savedUiScale();
if (scale !== 1) applyUiScale(scale, false);
setState({ waveStyle: savedWaveStyle() });
setState({ showOverview: savedShowOverview() });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {/* A detached panel's window (detach.ts) shows just that panel. */}
    {ownPanel ? <DetachedWindow /> : <App />}
  </React.StrictMode>,
);
