// The bottom section while it shows docked plug-in panels (BottomPane "fx"): the panel header
// (PanelHeader.tsx) with a tab per docked panel where the name goes, and the shown panel filling
// the rest of the section's height (the splitter above sets it). At most MAX_DOCKED panels are
// docked (fxDock.ts); tabs that don't fit scroll sideways.
import { useEffect, useRef, useState } from "react";
import { dockedPanels, type PanelKey } from "../../fxDock";
import { closeDocked, closePanel, selectDocked } from "../../fxDockState";
import { useStore } from "../../store";
import { openMenu } from "../Menu";
import { PanelContent, usePanelTitle } from "./FxPanel";
import { PanelHeader } from "./PanelHeader";

export function FxDock() {
  const panels = useStore((s) => s.fxDock);
  const tabs = dockedPanels(panels);
  const active = panels.active;
  const strip = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState<string | undefined>(undefined);
  // The front tab stays in view when the tabs scroll (the strip only: scrollIntoView would also
  // scroll the window's clipped boxes).
  useEffect(() => {
    const el = strip.current;
    const tab = el?.querySelector<HTMLElement>(".fx-tab.on");
    if (!el || !tab) return;
    const left = tab.getBoundingClientRect().left - el.getBoundingClientRect().left + el.scrollLeft;
    if (left < el.scrollLeft) el.scrollLeft = left;
    else if (left + tab.offsetWidth > el.scrollLeft + el.clientWidth) el.scrollLeft = left + tab.offsetWidth - el.clientWidth;
  }, [active, tabs.length]);
  // Which ends have tabs scrolled out of view, for a fade there.
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const check = () => {
      const l = el.scrollLeft > 1;
      const r = el.scrollLeft + el.clientWidth < el.scrollWidth - 1;
      setMore(l && r ? "both" : l ? "left" : r ? "right" : undefined);
    };
    check();
    el.addEventListener("scroll", check, { passive: true });
    const ro = new ResizeObserver(check);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", check);
      ro.disconnect();
    };
  }, [active === null, tabs.length]); // eslint-disable-line react-hooks/exhaustive-deps
  if (active === null) return <div className="fx-dock" />;
  const tabStrip = (
    <div
      ref={strip}
      className="fx-tabs"
      data-more={more}
      role="tablist"
      onWheel={(e) => {
        const el = e.currentTarget;
        if (e.deltaX === 0 && el.scrollWidth > el.clientWidth) el.scrollLeft += e.deltaY;
      }}
    >
      {tabs.map((k) => (
        <DockTab key={k} panelKey={k} on={k === active} count={tabs.length} />
      ))}
    </div>
  );
  return (
    <div className="fx-dock">
      <PanelHeader panelKey={active} place="docked" tabs={tabStrip} />
      <PanelContent key={active} panelKey={active} />
    </div>
  );
}

/**
 * A docked panel's tab. The front tab is closed by the header's ×; the others show their own ×
 * on hover. A middle-click closes a tab, and the right-click menu closes it, the others or all.
 */
function DockTab({ panelKey, on, count }: { panelKey: PanelKey; on: boolean; count: number }) {
  const title = usePanelTitle(panelKey);
  if (!title) return null;
  const menu = (e: React.MouseEvent) => {
    e.preventDefault();
    openMenu(e.clientX, e.clientY, [
      { label: "Close", onClick: () => closePanel(panelKey) },
      { label: "Close Other Tabs", disabled: count < 2, onClick: () => closeDocked(panelKey) },
      { label: "Close All Tabs", onClick: () => closeDocked() },
    ]);
  };
  return (
    <button
      role="tab"
      aria-selected={on}
      className={"fx-tab" + (on ? " on" : "")}
      title={`${title.name} — ${title.track}`}
      onClick={() => selectDocked(panelKey)}
      onAuxClick={(e) => e.button === 1 && closePanel(panelKey)}
      onContextMenu={menu}
    >
      <span className="fx-tab-name">{title.name}</span>
      <span className="fx-tab-track">{title.track}</span>
      {!on && (
        <span
          className="fx-tab-close"
          title={`Close ${title.name}`}
          onClick={(e) => {
            e.stopPropagation();
            closePanel(panelKey);
          }}
        >
          ×
        </span>
      )}
    </button>
  );
}
