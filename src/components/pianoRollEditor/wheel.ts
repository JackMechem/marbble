// The mouse wheel over the Piano Roll's panes: zoom and scroll. Native listeners, because React's wheel
// events are passive and couldn't stop the pane from scrolling.
import { getSettings } from "../../prefs";
import { timeWheel } from "../../wheel";

export function installWheel(el: HTMLElement, lane: HTMLElement | null, ruler: HTMLElement | null, keys: HTMLElement | null, zoomX: (factor: number, anchorX?: number) => void, zoomY: (factor: number, anchorY?: number) => void) {
    const onNotes = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        zoomX(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - el.getBoundingClientRect().left);
      } else if (e.altKey) {
        e.preventDefault();
        zoomY(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientY - el.getBoundingClientRect().top);
      } else if (getSettings().ui.wheel_scrolls === "horizontal") {
        // Settings › Editing › Mouse Wheel Scrolls: the wheel moves through time, Shift+wheel
        // through the pitches. The default is left to native scrolling.
        e.preventDefault();
        const { dx, dy } = timeWheel(e, el.clientWidth);
        el.scrollLeft += dx;
        el.scrollTop += dy;
      }
    };
    // Over the ruler and the lane the wheel scrolls (or zooms) in time.
    const onTime = (e: WheelEvent) => {
      e.preventDefault();
      if (e.ctrlKey || e.metaKey) zoomX(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - el.getBoundingClientRect().left);
      else el.scrollLeft += e.deltaX || e.deltaY;
    };
    const onKeys = (e: WheelEvent) => {
      e.preventDefault();
      if (e.altKey || e.ctrlKey || e.metaKey) zoomY(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientY - el.getBoundingClientRect().top);
      else el.scrollTop += e.deltaY;
    };
    el.addEventListener("wheel", onNotes, { passive: false });
    lane?.addEventListener("wheel", onTime, { passive: false });
    ruler?.addEventListener("wheel", onTime, { passive: false });
    keys?.addEventListener("wheel", onKeys, { passive: false });
    return () => {
      el.removeEventListener("wheel", onNotes);
      lane?.removeEventListener("wheel", onTime);
      ruler?.removeEventListener("wheel", onTime);
      keys?.removeEventListener("wheel", onKeys);
    };
}
