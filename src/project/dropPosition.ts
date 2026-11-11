// Converting a native drag-and-drop position (Tauri's onDragDropEvent) to CSS pixels.
//
// Tauri labels the position a PhysicalPosition everywhere, but what it holds depends on the webview:
//   - WKWebView (macOS): AppKit points from NSDraggingInfo.draggingLocation, unscaled, so logical.
//   - WebKitGTK (Linux): GTK widget coordinates, logical too.
//   - WebView2 (Windows): client-area device pixels from ScreenToClient, physical.
// The interface size is the webview's page zoom. WebKit leaves it out of devicePixelRatio (which
// is the display's backing scale), while Chromium folds it in.

/** Which kind of coordinates a webview's native drops arrive in. */
export type DropUnits = "logical" | "physical";

/** The units for a user agent: only WebView2 (Windows) reports physical pixels. */
export function dropUnits(userAgent: string): DropUnits {
  return /Windows/i.test(userAgent) ? "physical" : "logical";
}

/**
 * A native drop position in CSS pixels. `uiScale` is the interface size (the page zoom) and
 * `devicePixelRatio` the webview's; logical positions divide by the zoom, physical ones by the
 * ratio, which on WebView2 already includes the zoom.
 */
export function dropToCss(
  p: { x: number; y: number },
  units: DropUnits,
  uiScale: number,
  devicePixelRatio: number,
): [number, number] {
  const f = (units === "physical" ? devicePixelRatio : uiScale) || 1;
  return [p.x / f, p.y / f];
}
