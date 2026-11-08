// The side and bottom panes that aren't editors: Inspector, Mixer, Library and Browser.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { act, BUILTIN_SYNTH, call, INSTRUMENT_SLOT, SEND_SLOTS, type Project, type Region, type Track } from "../api";
import { Icon } from "../icons";
import { isAudioPath, startFileDrag } from "../dnd";
import { importMidiFiles, isMidiPath } from "../midiFiles";
import { currentTick, getState, setState, useStore } from "../store";
import { formatLengthBbt, formatTime, toBbt } from "../time";
import { ChannelStrip } from "./ChannelStrip";
import { FadeRows } from "./FadeRows";
import { RegionParamRows } from "./RegionParamRows";
import { TrackRows } from "./TrackInspector";
import { NumberField } from "./Form";
import { useSelectedTrack } from "./TrackHeaders";
import { isLooped } from "../loops";
import { toggleCollapsed, toggleLoop, toggleShowHidden } from "../ops";
import { keyHint } from "../keymap";
import { newAux } from "../routing";
import { resetClipIndicators } from "../stripSettings";
import { loadPlugin } from "../fxHost";
import { DetachButton } from "./DetachButton";
import { createVca, useVcas, VcaStrip } from "./Vca";
import { openMenuAt } from "./Menu";
import { useBarFit } from "./useBarFit";
import { fitStripRows, SLOT_ROW_H, stripRows, stripWants, type SlotRows } from "../stripRows";
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
} from "../mixerLayout";

/** The insert and send rows shared by `tracks`' strips and Stereo Out's. */
function rowsFor(project: Project, tracks: Track[]): SlotRows {
  return stripRows([...tracks.map((t) => stripWants(t.kind, t.inserts, t.sends, SEND_SLOTS)), stripWants(null, project.master.inserts, undefined, SEND_SLOTS)]);
}

function Disclosure({ title, children, initial = true }: { title: string; children: React.ReactNode; initial?: boolean }) {
  const [open, setOpen] = useState(initial);
  return (
    <div className="disclosure">
      <div className="disclosure-head" onClick={() => setOpen(!open)}>
        {Icon.disclosure(open)}
        <span>{title}</span>
      </div>
      {open && <div className="disclosure-body">{children}</div>}
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="insp-row">
      <span className="insp-label">{label}</span>
      <span className="insp-value">{children}</span>
    </div>
  );
}

export function Inspector() {
  const track = useSelectedTrack();
  const project = useStore((s) => s.snap?.project);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const regions: Region[] = useMemo(
    () => project?.tracks.flatMap((t) => t.regions).filter((r) => selectedRegions.includes(r.id)) ?? [],
    [project, selectedRegions],
  );
  // The strip pair fits its slot rows to its height as the Mixer's strips do.
  const fit = useStripFit();
  if (!project) return <div className="inspector" />;
  const rows = fit.rows(rowsFor(project, track ? [track] : []));
  const region = regions.length === 1 ? regions[0] : null;
  const regionTitle = regions.length > 1 ? `${regions.length} selected` : region ? region.name : "Default Settings";
  const bbt = (s: number) => {
    const b = toBbt(project, s);
    return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
  };
  return (
    <div className="inspector">
      <div className="inspector-top">
        <Disclosure title={`Region: ${regionTitle}`} initial={false}>
          {region ? (
            <>
              <Row label="Mute">
                <input type="checkbox" checked={region.muted} onChange={(e) => act("set_region_muted", { id: region.id, muted: e.target.checked })} />
              </Row>
              <Row label="Position">{bbt(region.start)}</Row>
              <Row label="Length">
                {/* Bars and beats like Position, the help tags and the Event List; the clock time after it. */}
                <span title={`${formatTime(project, region.length)} (minutes:seconds)`}>
                  {formatLengthBbt(project, region.start, region.length)} <span className="dim">· {formatTime(project, region.length)}</span>
                </span>
              </Row>
              <Row label="Loop">
                <input type="checkbox" checked={isLooped(region)} onChange={() => toggleLoop([region.id])} />
              </Row>
              <Row label="Type">{region.content.type === "audio" ? "Audio" : `MIDI · ${region.content.notes.length} notes`}</Row>
              <RegionParamRows regions={regions} />
              {region.content.type === "audio" && (
                <>
                  <Row label="Gain">
                    <NumberField value={Math.round(region.content.gain_db * 10) / 10} step={0.5} resolution={0.1} min={-96} max={30} width={70} onChange={(db) => act("set_region_gain", { ids: [region.id], db, delta: null })} />
                    <span className="dim"> dB</span>
                  </Row>
                  <FadeRows region={region} sampleRate={project.sample_rate} />
                </>
              )}
            </>
          ) : regions.length > 1 ? (
            <RegionParamRows regions={regions} />
          ) : (
            <div className="insp-empty">No region selected</div>
          )}
        </Disclosure>
        <Disclosure title={`Track: ${track?.name ?? "—"}`} initial={false}>
          {track ? (
            <TrackRows project={project} track={track} />
          ) : (
            <div className="insp-empty">No track selected</div>
          )}
        </Disclosure>
      </div>
      <div className="inspector-strips" ref={fit.ref} onScroll={fit.onScroll}>
        <div className="inspector-strip-row">
          {track ? <ChannelStrip track={track} selected rows={rows} /> : <div className="strip placeholder" />}
          <ChannelStrip track={null} rows={rows} />
        </div>
      </div>
    </div>
  );
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
function useStripFit() {
  const ref = useRef<HTMLDivElement>(null);
  const shown = useRef<SlotRows>({ inserts: 1, sends: 1 });
  const atBottom = useRef(true);
  const [room, setRoom] = useState<{ viewH: number; fixedH: number; faderMin: number } | null>(null);
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
    setRoom((r) => (r && r.viewH === next.viewH && Math.abs(r.fixedH - next.fixedH) < 1 && r.faderMin === next.faderMin ? r : next));
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
    rows: (wanted: SlotRows) => (shown.current = room ? fitStripRows(wanted, room.viewH, room.fixedH, room.faderMin) : wanted),
  };
}

/** Sets the Mixer's strip size or its strips' own widths (remembered per project, see layout.ts). */
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
            onMouseDown={(e) =>
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
            }
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

/** Logic's Library lists patches for the selected track; here it lists plug-ins to load. */
export function Library() {
  const track = useSelectedTrack();
  const plugins = useStore((s) => s.plugins);
  const [q, setQ] = useState("");
  const [rescanning, setRescanning] = useState(false);
  const isInst = track?.kind === "instrument";
  const fxList = useStore((s) => s.fxList);
  const instList = useStore((s) => s.instList);
  const instGroups = new Map<string, typeof instList>();
  for (const i of instList.filter((i) => !q || `${i.name} ${i.category} ${i.about}`.toLowerCase().includes(q.toLowerCase()))) {
    if (!instGroups.has(i.category)) instGroups.set(i.category, []);
    instGroups.get(i.category)!.push(i);
  }
  // Built-in effects first, by category, as in Logic's Library.
  const builtins = isInst ? [] : fxList.filter((f) => !q || `${f.name} ${f.category}`.toLowerCase().includes(q.toLowerCase()));
  const fxGroups = new Map<string, typeof builtins>();
  for (const f of builtins) {
    if (!fxGroups.has(f.category)) fxGroups.set(f.category, []);
    fxGroups.get(f.category)!.push(f);
  }
  const list = plugins
    .filter((p) => p.is_instrument === isInst)
    .filter((p) => !q || `${p.name} ${p.vendor} ${p.subcategories}`.toLowerCase().includes(q.toLowerCase()));
  const groups = new Map<string, typeof list>();
  for (const p of list) {
    const g = p.vendor || "Unknown";
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g)!.push(p);
  }
  const load = (id: string) => {
    if (!track) return;
    if (isInst) loadPlugin(track.id, INSTRUMENT_SLOT, id, true);
    else {
      const slot = track.inserts.findIndex((s) => !s);
      if (slot < 0) setState({ toast: { text: "All Audio FX slots are in use", at: Date.now() } });
      else loadPlugin(track.id, slot, id, true);
    }
  };
  const current = (t: Track | null) => (t?.kind === "instrument" ? t.instrument?.plugin_id : undefined);
  return (
    <div className="library">
      <div className="pane-title">Library</div>
      <input className="search" placeholder="Search" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
      {!track ? (
        <div className="insp-empty">Select a track</div>
      ) : track.kind === "folder" ? (
        // A folder stack has no channel strip of its own to put plug-ins on.
        <div className="insp-empty">Folder stacks have no plug-ins</div>
      ) : (
        <div className="lib-list">
          <div className="lib-group">{isInst ? "Instruments" : "Audio FX"}</div>
          {isInst && !instList.length && (
            <div className={"lib-item" + (current(track) === BUILTIN_SYNTH ? " on" : "")} onClick={() => load(BUILTIN_SYNTH)}>
              Marbble Synth
            </div>
          )}
          {isInst &&
            [...instGroups.entries()]
              .sort((a, b) => (a[0] === "Marbble" ? -1 : b[0] === "Marbble" ? 1 : a[0].localeCompare(b[0])))
              .map(([group, is]) => (
                <div key={`inst:${group}`}>
                  <div className="lib-vendor">{group}</div>
                  {is.map((i) => (
                    <div key={i.id} className={"lib-item" + (current(track) === i.id ? " on" : "")} onClick={() => load(i.id)} title={i.about}>
                      {i.name}
                    </div>
                  ))}
                </div>
              ))}
          {[...fxGroups.entries()]
            .sort((a, b) => a[0].localeCompare(b[0]))
            .map(([cat, fs]) => (
              <div key={`fx:${cat}`}>
                <div className="lib-vendor">{cat}</div>
                {fs.map((f) => (
                  <div key={f.id} className="lib-item" onClick={() => load(f.id)} title={`Marbble ${f.category}`}>
                    {f.name}
                  </div>
                ))}
              </div>
            ))}
          {[...groups.entries()].map(([vendor, ps]) => (
            <div key={vendor}>
              <div className="lib-vendor">{vendor}</div>
              {ps.map((p) => (
                <div key={p.id} className={"lib-item" + (current(track) === p.id ? " on" : "")} onClick={() => load(p.id)} title={p.subcategories}>
                  {p.name}
                </div>
              ))}
            </div>
          ))}
          {list.length === 0 && builtins.length === 0 && (!isInst || instGroups.size === 0) && <div className="insp-empty">No matching plug-ins</div>}
        </div>
      )}
      <div className="pane-foot">
        <span className="dim">{plugins.length} plug-ins</span>
        <button
          className="small-btn"
          disabled={rescanning}
          onClick={async () => {
            setRescanning(true);
            const l = await call<typeof plugins>("rescan_plugins");
            if (l) setState({ plugins: l });
            setRescanning(false);
          }}
        >
          {rescanning ? "Scanning…" : "Rescan"}
        </button>
      </div>
    </div>
  );
}

interface DirListing {
  path: string;
  parent: string | null;
  entries: { name: string; path: string; is_dir: boolean }[];
}

export function Browser() {
  const [dir, setDir] = useState<DirListing | null>(null);
  const load = (path: string | null) => call<DirListing>("list_dir", { path }).then((d) => d && setDir(d));
  useEffect(() => {
    load(null);
  }, []);
  const importAt = async (path: string) => {
    const snap = getState().snap;
    const pos = currentTick()?.pos ?? 0;
    // MIDI files go onto the selected instrument track, or a new one.
    if (isMidiPath(path)) {
      const inst = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "instrument");
      await importMidiFiles([path], inst?.id ?? null, pos, null);
      return;
    }
    if (!isAudioPath(path)) {
      setState({ toast: { text: "Only audio and MIDI files can be imported", at: Date.now() } });
      return;
    }
    const track = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "audio");
    if (!track) {
      setState({ toast: { text: "Select an audio track to import into", at: Date.now() } });
      return;
    }
    act("import_audio", { path, track: track.id, start: pos });
  };
  return (
    <div className="browser">
      <div className="pane-title">All Files</div>
      <div className="browser-path">
        <button className="small-btn icon" disabled={!dir?.parent} onClick={() => dir?.parent && load(dir.parent)} title="Enclosing folder" aria-label="Enclosing folder">
          <Icon.up />
        </button>
        <span title={dir?.path}>{dir?.path.split("/").pop() || dir?.path}</span>
      </div>
      <div className="browser-list">
        {dir?.entries.map((e) => (
          <div
            key={e.path}
            className={"browser-item" + (e.is_dir ? " dir" : "")}
            onMouseDown={(ev) => !e.is_dir && startFileDrag(ev, e.path, e.name)}
            onDoubleClick={() => (e.is_dir ? load(e.path) : importAt(e.path))}
            title={e.is_dir ? "Double-click to open" : "Drag onto a track (or the track list for a new track), or double-click to import at the playhead"}
          >
            {e.is_dir ? <Icon.folder /> : <Icon.file />}
            <span>{e.name}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
