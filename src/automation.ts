// Track automation in the arrange area: the lanes' maths (values, scales, labels), drawing each
// track's shown lanes (the main one over its dimmed regions, extra ones below it, and the Output
// track's), and editing points and curves with the mouse. The Tracks area hands over drawing and
// pointer events while the automation view is on. The pure parts are in autoLanes.ts.
//
// The code is in automation/: params.ts (what a parameter is), values.ts (reading them), menus.ts
// (what's shown), useAutomation.ts (faders following a lane) and canvas.ts (drawing and editing).
// Everything is re-exported here, so the rest of the app imports from one place.
export { automatable, isOutput, laneOf, MODE_LABELS, MODES, outputTrack, paramLabel, sameParam, stripOf, valueAt, VOLUME, fromUnit, toUnit } from "./automation/params";
export { formatValue, shownParam } from "./automation/values";
export { addLane, closeLane, modeMenu, paramMenu, pluginParams, showUsedAutomation } from "./automation/menus";
export { useAutomation } from "./automation/useAutomation";
export { automationHover, automationLeave, automationMenu, automationMouseDown, deleteSelectedPoints, drawAutomation, type AutoInput, type AutoView } from "./automation/canvas";
