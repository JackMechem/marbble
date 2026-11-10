// The track headers column: a header per track in view (TrackHeader.tsx), reordering them by drag,
// resizing them, and the selected track for the panes that follow it.
import { Fragment, useEffect, useRef, useState } from "react";
import { isStack, type Id, type Track } from "../../engine/project";
import { act } from "../../engine/commands";
import { setState, useStore } from "../../state/store";
import { useLayout } from "../trackLayout";
import { onViewport, viewport } from "../viewport";
import { clickTrack } from "../trackCommands";
import { openMenu } from "../../ui/Menu";
import { OutputTrackHeader, SubLaneHeaders } from "./AutoLaneHeaders";
import { fmt } from "../../keyboard/keymap";
import { REORDER_SLOP, reorderIndex } from "./headerGeometry";
import { headerColumns } from "./headerColumns";
import { Header, INDENT } from "./TrackHeader";


export function TrackHeaders() {
  const project = useStore((s) => s.snap?.project);
  const selected = useStore((s) => s.snap?.selected ?? null);
  const multi = useStore((s) => s.selectedTracks);
  const selectedSet = new Set(selected !== null && multi.includes(selected) ? multi : selected !== null ? [selected] : []);
  const layout = useLayout();
  const dropTarget = useStore((s) => s.dropTarget);
  const hideMode = useStore((s) => s.showHidden);
  const fileDrop = dropTarget && dropTarget.kind !== "lane" ? dropTarget.index : null;
  // Button columns from the tracks in view (a collapsed stack's subtracks don't count).
  const columns = headerColumns(project?.tracks.filter((_, i) => layout.heights[i] > 0) ?? []);
  const inner = useRef<HTMLDivElement>(null);
  const [drop, setDrop] = useState<number | null>(null);

  useEffect(() => {
    const apply = () => {
      if (inner.current) inner.current.style.transform = `translateY(${-viewport().top}px)`;
    };
    apply();
    return onViewport(apply);
  }, []);

  // Dragging a track moves it with its subtracks. Where it lands decides its stack: it joins
  // the stack of the track it's dropped above (or the stack whose header it's dropped below).
  const startReorder = (e: React.MouseEvent, index: number, click?: () => void) => {
    if (e.button !== 0 || !project) return;
    clickTrack(project.tracks[index].id, e);
    const startY = e.clientY;
    const headerTop = (e.currentTarget as HTMLElement).closest(".track-header")!.getBoundingClientRect().top;
    const startContentY = layout.tops[index] + (startY - headerTop);
    let target = index;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientY - startY) < REORDER_SLOP) return;
      moved = true;
      target = reorderIndex(layout.tops, layout.heights, startContentY + (ev.clientY - startY));
      setDrop(target);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      setDrop(null);
      if (!moved) return click?.();
      const tracks = project.tracks;
      const id = tracks[index].id;
      const block = new Set<Id>([id]);
      for (const t of tracks) if (t.parent !== null && block.has(t.parent)) block.add(t.id);
      // Dropping inside its own block changes nothing.
      const blockEnd = index + block.size;
      if (target >= index && target <= blockEnd) return;
      const below = tracks.slice(target).find((t) => !block.has(t.id));
      const above = tracks.slice(0, target).reverse().find((t) => !block.has(t.id));
      let parent: Id | null = below ? below.parent : null;
      // Dropped right under an open stack's header: into the stack.
      if (above && isStack(above.kind) && !above.collapsed) parent = above.id;
      // A collapsed stack's hidden subtracks sit under its header: drop after them instead.
      const rest = tracks.filter((t) => !block.has(t.id));
      let at = rest.findIndex((t) => t.id === below?.id);
      if (at < 0) at = rest.length;
      const ids = rest.map((t) => t.id);
      ids.splice(at, 0, ...tracks.filter((t) => block.has(t.id)).map((t) => t.id));
      act("move_tracks", { ids, parents: [[id, parent]] });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  if (!project) return null;
  return (
    <div
      className="headers"
      onWheel={(e) => {
        // Forward wheel scrolling to the arrange view.
        const el = document.querySelector(".arrange-scroll") as HTMLElement | null;
        if (el) el.scrollTop += e.deltaY;
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) setState({ selectedRegions: [] });
      }}
      onContextMenu={(e) => {
        if (e.target !== e.currentTarget && !(e.target as HTMLElement).classList.contains("headers-inner")) return;
        e.preventDefault();
        openMenu(e.clientX, e.clientY, [{ label: "New Tracks…", hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) }]);
      }}
    >
      <div ref={inner} className="headers-inner">
        {project.tracks.map((t, i) =>
          layout.heights[i] > 0 ? (
            <Fragment key={t.id}>
              <Header
                track={t}
                index={i}
                selected={selectedSet.has(t.id)}
                primary={t.id === selected && selectedSet.size > 1}
                height={layout.heights[i]}
                depth={layout.depth[i]}
                group={t.group !== null ? project.groups.find((g) => g.id === t.group) : undefined}
                hideMode={hideMode}
                columns={columns}
                onReorderStart={startReorder}
              />
              {layout.autoRows[i] > 0 && <SubLaneHeaders track={t} height={layout.subLane[i]} indent={layout.depth[i] * INDENT} />}
            </Fragment>
          ) : null,
        )}
        {layout.output && <OutputTrackHeader row={layout.output} />}
        {drop !== null && <div className="reorder-line" style={{ top: (drop < layout.tops.length ? layout.tops[drop] : layout.total) - 1 }} />}
        {fileDrop !== null && (
          <div className="drop-new-track" style={{ top: (fileDrop < layout.tops.length ? layout.tops[fileDrop] : layout.total) - 1 }}>
            <span>New track</span>
          </div>
        )}
        {project.tracks.length === 0 && (
          <div className="headers-empty">
            <button className="big-add" onClick={() => setState({ dialog: "new-tracks" })}>
              New Track…
            </button>
            <span className="dim">{fmt("Mod+Alt+N")}</span>
          </div>
        )}
      </div>
    </div>
  );
}

export function useSelectedTrack(): Track | null {
  return useStore((s) => s.snap?.project.tracks.find((t) => t.id === s.snap?.selected) ?? null);
}

