// The Mixer, the bottom section's channel strips: track stacks as brackets over their strips, strip
// widths and filters, the VCA strips and Stereo Out. The Inspector shows strips through the same
// rows (rowsFor, useStripFit). mixer/ holds the channel strip and its parts (slots, sends, VCAs,
// meters, latency) with the rules behind them: routing, strip settings, slot rows and widths.
import { useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { SEND_SLOTS, type Project, type Track } from "../engine/project";
import { Icon } from "../ui/icons";
import { setState, useStore } from "../state/store";
import { ChannelStrip } from "./ChannelStrip";
import { toggleCollapsed, toggleShowHidden } from "../tracks/trackCommands";
import { keyHint } from "../keyboard/keymap";
import { newAux } from "./routing";
import { resetClipIndicators } from "./stripSettings";
import { DetachButton } from "../app/detach/DetachButton";
import { createVca, useVcas, VcaStrip } from "./Vca";
import { openMenuAt } from "../ui/Menu";
import { useBarFit } from "../ui/useBarFit";
import { fitStripRows, SLOT_ROW_H, stripRows, stripWants, type SlotRows } from "./stripRows";
import {
  bracketRows,
  clampStripWidth,
  entryTracks,
  mixerEntries,
  plateInk,
  SLIM_W,
  stackKindLabel,
  STRIP_SIZES,
  stripKey,
  stripWidth,
  tint,
  tracksLabel,
  withStripWidth,
  type MixerEntry,
  type MixerView,
  type StripSize,
} from "./mixerLayout";
import { onPress } from "../keyboard/keyNav";

/** The insert and send rows shared by `tracks`' strips and Stereo Out's. */
export function rowsFor(project: Project, tracks: Track[]): SlotRows {
  return stripRows([...tracks.map((t) => stripWants(t.kind, t.inserts, t.sends, SEND_SLOTS)), stripWants(null, project.master.inserts, undefined, SEND_SLOTS)]);
}

type StripKind = "audio" | "instrument" | "aux";

/** The Mixer's strip filters: kind, button label, name in the Options menu and tooltips. */
const STRIP_KINDS: [StripKind, string, string][] = [
  ["audio", "Audio", "Audio"],
  ["instrument", "Inst", "Instrument"],
  ["aux", "Aux", "Aux"],
];

/**
 * Fits the Mixer's strips (or the Inspector's pair) to its height: fewer slot rows when it's
 * short, so the faders keep their room (fitStripRows), and when even one row each doesn't fit,
 * the strips scroll from the bottom, so the faders, meters and names show first. The view stays
 * at the bottom while it was there; once scrolled up it stays where it was put.
 */
export function useStripFit() {
  const ref = useRef<HTMLDivElement>(null);
  const shown = useRef<SlotRows>({ inserts: 1, sends: 1 });
  const atBottom = useRef(true);
  // Kept out of state: a resize renders the strips again only when the rows that fit change.
  const room = useRef<{ viewH: number; fixedH: number; faderMin: number } | null>(null);
  const wanted = useRef<SlotRows | null>(null);
  const [, refit] = useReducer((n: number) => n + 1, 0);
  const measure = () => {
    const el = ref.current;
    // The strip with the most above and below its fader (the least fader) sets the room, so no
    // strip is pushed taller than the view.
    let fader: HTMLElement | undefined;
    for (const f of el?.querySelectorAll<HTMLElement>(".strip-fader") ?? []) if (!fader || f.offsetHeight < fader.offsetHeight) fader = f;
    const strip = fader?.closest<HTMLElement>(".strip");
    if (!el || !fader || !strip) return;
    const faderMin = parseFloat(getComputedStyle(fader).minHeight) || 0;
    // The Mixer's stack brackets' rows above the strips (its row's top padding) aren't the
    // strips' room. The Inspector's strips sit straight in the box.
    const row = el.querySelector(":scope > .mixer-row");
    const brackets = row ? parseFloat(getComputedStyle(row).paddingTop) || 0 : 0;
    // The strip without its slot rows and with its fader at its least: the fader takes the
    // strip's spare height, and the slot boxes are SLOT_ROW_H a row.
    const fixedH = strip.offsetHeight - fader.offsetHeight + faderMin - (shown.current.inserts + shown.current.sends) * SLOT_ROW_H;
    const next = { viewH: el.clientHeight - brackets, fixedH, faderMin };
    const first = !room.current;
    room.current = next;
    const fit = wanted.current && fitStripRows(wanted.current, next.viewH, next.fixedH, next.faderMin);
    if (first || (fit && (fit.inserts !== shown.current.inserts || fit.sends !== shown.current.sends))) refit();
    if (atBottom.current) el.scrollTop = el.scrollHeight;
  };
  useLayoutEffect(measure);
  // The strips' box comes and goes with the project, so it's observed once it's there.
  const observed = useRef<{ el: Element; ro: ResizeObserver } | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || observed.current?.el === el) return;
    observed.current?.ro.disconnect();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    observed.current = { el, ro };
  });
  useEffect(() => () => observed.current?.ro.disconnect(), []);
  return {
    ref,
    onScroll: (e: React.UIEvent<HTMLDivElement>) => {
      const el = e.currentTarget;
      atBottom.current = el.scrollTop + el.clientHeight >= el.scrollHeight - 2;
    },
    rows: (want: SlotRows) => {
      wanted.current = want;
      const r = room.current;
      return (shown.current = r ? fitStripRows(want, r.viewH, r.fixedH, r.faderMin) : want);
    },
  };
}

/** Sets the Mixer's strip size or its strips' own widths (remembered per project, see trackLayout.ts). */
const setMixerView = (f: (v: MixerView) => MixerView) => setState((s) => ({ mixerView: f(s.mixerView) }));

/**
 * A strip in the Mixer at its width, with the handle on its right edge: drag to give the strip a
 * width of its own, double-click to give it back its size's. While dragging, `onDrag` has the
 * width so far; it goes to the store (shared with the other windows) when the drag ends.
 */
function StripCell({ k, width, onDrag, children }: { k: string; width: number; onDrag: (d: { key: string; w: number } | null) => void; children: React.ReactNode }) {
  const start = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const from = e.clientX;
    let w = width;
    document.body.style.cursor = "col-resize";
    el.classList.add("dragging");
    const move = (ev: PointerEvent) => {
      w = clampStripWidth(width + ev.clientX - from);
      onDrag({ key: k, w });
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      el.classList.remove("dragging");
      document.body.style.cursor = "";
      onDrag(null);
      if (w !== width) setMixerView((v) => withStripWidth(v, k, w));
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  return (
    <div className={"mixer-cell" + (width < SLIM_W ? " slim" : "")} style={{ width }}>
      {children}
      <div
        className="strip-resize"
        role="separator"
        aria-orientation="vertical"
        aria-label="Strip width"
        title="Drag to set this strip's width · double-click to reset it"
        onPointerDown={start}
        onDoubleClick={() => setMixerView((v) => withStripWidth(v, k, null))}
      />
    </div>
  );
}

/** What the Mixer's bar gives way with, in order (barFit.ts): the strip widths, then the strip
 *  filters, which its Options menu has too. */
const MIXER_BAR_STEPS = ["tight", "widths", "filters"] as const;

export function Mixer() {
  const project = useStore((s) => s.snap?.project);
  const selected = useStore((s) => s.snap?.selected);
  const [filter, setFilter] = useState<Record<StripKind, boolean>>({ audio: true, instrument: true, aux: true });
  const flip = (k: StripKind) => setFilter((f) => ({ ...f, [k]: !f[k] }));
  const showHidden = useStore((s) => s.showHidden);
  const view = useStore((s) => s.mixerView);
  const [drag, setDrag] = useState<{ key: string; w: number } | null>(null);
  const vcas = useVcas();
  const fit = useStripFit();
  const barFit = useBarFit(MIXER_BAR_STEPS);
  if (!project) return null;
  const shown = project.tracks.filter((t) => (t.hidden ? showHidden : true) && (t.kind === "audio" || t.kind === "instrument" || t.kind === "aux" ? filter[t.kind] : true));
  // Auxes come after the tracks, as in Logic's mixer.
  const tracks = [...shown.filter((t) => t.kind !== "aux"), ...shown.filter((t) => t.kind === "aux")];
  // Stacks draw as brackets over their strips; a collapsed one (in the Tracks area too, as in
  // Logic, whose Mixer follows the stack's open state) as its own strip and a marker.
  const entries = mixerEntries(project.tracks, tracks);
  const rows = fit.rows(rowsFor(project, entryTracks(entries)));
  const setSize = (size: StripSize) => setMixerView((v) => ({ ...v, size }));
  const sized = Object.keys(view.widths).length > 0;
  const cell = (k: string, folder: boolean, strip: React.ReactNode) => (
    <StripCell key={k} k={k} width={drag?.key === k ? drag.w : stripWidth(view, k, folder)} onDrag={setDrag}>
      {strip}
    </StripCell>
  );
  const trackCell = (t: Track) => cell(stripKey.track(t.id), t.kind === "folder", <ChannelStrip track={t} selected={t.id === selected} rows={rows} />);
  const entry = (e: MixerEntry): React.ReactNode => {
    if (e.kind === "strip") return trackCell(e.track);
    const t = e.track;
    const what = `${t.name}: ${t.kind === "summing" ? "summing" : "folder"} stack, ${tracksLabel(e.count)}`;
    return (
      <div key={`stack-${t.id}`} className="mixer-stack" style={{ "--stack-depth": e.depth, "--stack-tint": tint(t.color, 0.1) } as React.CSSProperties}>
        <button
          type="button"
          className={"mixer-bracket ink-" + plateInk(t.color)}
          style={{ "--plate": t.color } as React.CSSProperties}
          aria-expanded={e.open}
          title={`${what} · click to ${e.open ? "collapse" : "expand"} it`}
          onClick={() => toggleCollapsed(t)}
        >
          {Icon.disclosure(e.open)}
          <span className="mixer-bracket-name">{t.name}</span>
          {/* Collapsed, the marker has the count, so the name gets the room. */}
          {e.open && <span className="mixer-bracket-kind">{stackKindLabel(t, e.count)}</span>}
        </button>
        {trackCell(t)}
        {e.open ? (
          e.members.map(entry)
        ) : (
          <button type="button" className="mixer-stack-more" aria-expanded={false} title={`Show the ${tracksLabel(e.count)} in ${t.name}`} onClick={() => toggleCollapsed(t)}>
            <span>{tracksLabel(e.count)}</span>
            {Icon.disclosure(false)}
          </button>
        )}
      </div>
    );
  };
  return (
    <div className="mixer">
      <div ref={barFit.ref} className={"local-bar fit-bar mixer-bar" + barFit.className}>
        <span className="local-title mixer-title">Mixer</span>
        <div className="local-menus">
          <button
            className="local-menu"
            {...onPress((e) =>
              openMenuAt(e.currentTarget, [
                { label: "Create New VCA", onClick: () => createVca() },
                { label: "New Aux", onClick: () => newAux() },
                { separator: true },
                ...STRIP_KINDS.map(([k, , name]) => ({ label: `Show ${name} Strips`, checked: filter[k], onClick: () => flip(k) })),
                { label: "Show Hidden Tracks", hint: keyHint("show-hidden"), checked: showHidden, onClick: toggleShowHidden },
                { separator: true },
                ...STRIP_SIZES.map(([k, label]) => ({ label: `${label} Strips`, checked: view.size === k, onClick: () => setSize(k) })),
                { label: "Reset Strip Widths", disabled: !sized, onClick: () => setMixerView((v) => ({ ...v, widths: {} })) },
                { separator: true },
                { label: "Reset All Clip Indicators", hint: keyHint("reset-clips"), onClick: () => resetClipIndicators(null) },
              ])
            )}
          >
            Options
          </button>
        </div>
        {!barFit.has("filters") && (
          <div className="mixer-filters" role="group" aria-label="Strips shown">
            {STRIP_KINDS.map(([k, label, name]) => (
              <button key={k} className={"filter" + (filter[k] ? " on" : "")} aria-pressed={filter[k]} title={`${filter[k] ? "Hide" : "Show"} ${name.toLowerCase()} strips`} onClick={() => flip(k)}>
                {label}
              </button>
            ))}
          </div>
        )}
        {!barFit.has("widths") && (
          <div className="mixer-widths" role="group" aria-label="Strip width">
            {STRIP_SIZES.map(([k, label]) => (
              <button
                key={k}
                className={"filter" + (view.size === k ? " on" : "")}
                aria-pressed={view.size === k}
                title={`${label} strips${sized ? " (strips given a width of their own keep it: Options › Reset Strip Widths)" : ""}`}
                onClick={() => setSize(k)}
              >
                {label}
              </button>
            ))}
          </div>
        )}
        <div className="local-right">
          <DetachButton panel="mixer" />
        </div>
      </div>
      <div className="mixer-strips" ref={fit.ref} onScroll={fit.onScroll}>
        <div className="mixer-row" style={{ "--bracket-rows": bracketRows(entries) } as React.CSSProperties}>
          {entries.map(entry)}
          {/* VCA strips come after the auxes, before Stereo Out, as in Logic. */}
          {vcas.map((v) => cell(stripKey.vca(v.id), false, <VcaStrip vca={v} rows={rows} />))}
          <div className="mixer-gap" />
          {cell(stripKey.master, false, <ChannelStrip track={null} rows={rows} />)}
        </div>
      </div>
    </div>
  );
}

