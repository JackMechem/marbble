// The panel a built-in instrument gets without any custom code: its parameters in the sections
// its spec lists, and a small keyboard to try it with. A model can have a layout of its own by
// registering it in FX_PANELS (src/fx.ts), as the effects do.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, call, type Id } from "../../api";
import type { FxParamSpec, FxPanelProps } from "../../fx";
import { getState } from "../../store";
import { moreBeyond, savedLayout, saveLayout, type PanelPlace, type SectionLayout } from "../../instLayout";
import { sectionView } from "../../instSections";
import { useFxLearn } from "../../learn";
import { keyboardKeys } from "../../pianoKeys";
import { FxChoice, FxGroup, FxKnob, FxMeter, FxToggle, isReductionMeter } from "./common";

export function InstrumentPanel(props: FxPanelProps) {
  const { info, track, meters } = props;
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<PanelPlace>("docked");
  const [layout, setLayout] = useState<SectionLayout>(() => savedLayout("docked"));
  // Floating or docking mounts the panel afresh, so this only needs to look once.
  useLayoutEffect(() => {
    const here: PanelPlace = ref.current?.closest(".fx-window") ? "floating" : "docked";
    setPlace(here);
    setLayout(savedLayout(here));
  }, []);
  const sections = useRef<HTMLDivElement>(null);
  const more = useMoreBeyond(sections, layout);
  const choose = (l: SectionLayout) => {
    setLayout(l);
    saveLayout(place, l);
  };
  return (
    <div ref={ref} className={"fx-panel inst-panel " + layout}>
      <div ref={sections} className="inst-sections" data-more={more ?? undefined} onWheel={wheelAcross}>
        {(info.sections ?? []).map((s) => (
          <InstSection key={s.title} {...props} title={s.title} params={s.params} />
        ))}
        {info.meters.length > 0 && (
          <FxGroup title="Meters" className="inst-section rows-1">
            {info.meters.map((m, i) => (
              <FxMeter key={m} label={m} value={meters[i] ?? NaN} kind={isReductionMeter(m) ? "reduction" : "level"} />
            ))}
          </FxGroup>
        )}
      </div>
      <div className="inst-footer">
        <div className="fx-segments inst-layout" title="How the sections are laid out">
          <button className={layout === "row" ? "on" : ""} onClick={() => choose("row")}>
            Row
          </button>
          <button className={layout === "grid" ? "on" : ""} onClick={() => choose("grid")}>
            Grid
          </button>
        </div>
        <PanelKeyboard track={track} />
      </div>
    </div>
  );
}

/**
 * One section of an instrument's panel: one header with the section's name (and its on/off
 * switch, when it has one), then its controls on a grid of equal cells, named without repeating
 * the header (instSections.ts). The Sampler's panel shows its sections with it too.
 */
export function InstSection({ info, values, set, touch, title, params, className }: FxPanelProps & { title: string; params: number[]; className?: string }) {
  const byId = new Map(info.params.map((p) => [p.id, p]));
  const specs = params.flatMap((id) => byId.get(id) ?? []);
  const view = sectionView(info.id, title, specs);
  const on = (id: number) => values[id] >= 0.5;
  const power = view.power === null ? null : byId.get(view.power);
  const control = (spec: FxParamSpec, label: string) => {
    const change = (v: number) => set(spec.id, v);
    if (spec.kind.type === "toggle")
      return (
        <div className="inst-toggle">
          <div className="fx-label" />
          <FxToggle spec={spec} value={values[spec.id]} onChange={change} label={label} />
        </div>
      );
    if (spec.kind.type === "choice") return <FxChoice spec={spec} value={values[spec.id]} onChange={change} label={label} />;
    return <FxKnob spec={spec} value={values[spec.id]} onChange={change} onTouch={(t) => touch(spec.id, t)} size={40} label={label} />;
  };
  return (
    <div className={`fx-group inst-section rows-${view.rows}` + (power && !on(power.id) ? " off" : "") + (className ? " " + className : "")} role="group" aria-label={title}>
      <div className="fx-group-title inst-section-title">
        {power && <LedSwitch spec={power} on={on(power.id)} onChange={(v) => set(power.id, v)} className="inst-power" />}
        <span className="inst-section-name">{title}</span>
      </div>
      <div className="fx-group-body">
        {view.cells.map((c) => {
          const spec = byId.get(c.id)!;
          if (c.type === "control") return <div key={c.id} className="inst-cell">{control(spec, c.label)}</div>;
          const toggle = byId.get(c.toggle)!;
          return (
            <div key={c.id} className={"inst-cell switched" + (on(c.toggle) ? "" : " off")}>
              <FxKnob
                spec={spec}
                value={values[spec.id]}
                onChange={(v) => set(spec.id, v)}
                onTouch={(t) => touch(spec.id, t)}
                size={40}
                label={c.label}
                head={<LedSwitch spec={toggle} on={on(c.toggle)} onChange={(v) => set(toggle.id, v)} className="inst-switch" text={c.label} />}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** An on/off switch with a light, for a section's or a knob's part: its name in the tooltip. */
function LedSwitch({ spec, on, onChange, className, text }: { spec: FxParamSpec; on: boolean; onChange: (v: number) => void; className: string; text?: string }) {
  const learn = useFxLearn(spec.id);
  return (
    <button className={className + (on ? " on" : "")} aria-pressed={on} aria-label={spec.name} title={spec.name} onClick={() => onChange(on ? 0 : 1)} onContextMenu={learn}>
      <span className="dyn-led" />
      {text && <span className="inst-switch-name">{text}</span>}
    </button>
  );
}

/**
 * Which side of the scrolling sections has more out of view (instLayout.ts), kept up to date as
 * the box scrolls or resizes, so a fade on that edge shows the sections go on under the keyboard
 * bar or past the panel's edge.
 */
export function useMoreBeyond(ref: React.RefObject<HTMLDivElement | null>, layout: SectionLayout) {
  const [more, setMore] = useState<"below" | "right" | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const check = () => setMore(moreBeyond(layout, el));
    check();
    el.addEventListener("scroll", check, { passive: true });
    const ro = new ResizeObserver(check);
    ro.observe(el);
    for (const c of el.children) ro.observe(c);
    return () => {
      el.removeEventListener("scroll", check);
      ro.disconnect();
    };
  }, [ref, layout]);
  return more;
}

/** In a row wider than the panel, a wheel moves along the sections. */
export function wheelAcross(e: React.WheelEvent<HTMLDivElement>) {
  const el = e.currentTarget;
  if (e.deltaX === 0 && el.scrollWidth > el.clientWidth) el.scrollLeft += e.deltaY;
}

/** Two octaves to try a sound with, played through live MIDI on the instrument's track. */
function PanelKeyboard({ track }: { track: Id }) {
  const [base, setBase] = useState(48);
  const [down, setDown] = useState<number | null>(null);
  const held = useRef<number | null>(null);
  const send = (status: number, note: number) => call("midi_event", { data: [status, note, status === 0x90 ? 100 : 0] });
  const off = () => {
    if (held.current !== null) send(0x80, held.current);
    held.current = null;
    setDown(null);
  };
  const on = (note: number) => {
    if (held.current === note) return;
    const previous = held.current;
    held.current = note;
    setDown(note);
    // Live MIDI plays the selected track.
    if (getState().snap?.selected !== track) api.selectTrack(track);
    send(0x90, note);
    // Note on first, so sliding across the keys plays legato.
    if (previous !== null) send(0x80, previous);
  };
  // A key mustn't stay down because the panel closed under the pointer.
  useEffect(() => off, []); // eslint-disable-line react-hooks/exhaustive-deps

  const keys = keyboardKeys(base, 25);
  return (
    <div className="inst-keyboard" onPointerUp={off} onPointerLeave={off} onPointerCancel={off}>
      <button className="fx-toggle" title="An octave down" aria-label="An octave down" disabled={base <= 12} onClick={() => setBase(base - 12)}>
        −
      </button>
      <div className="inst-keys" style={{ width: `${keys.whites * 22}px` }}>
        {keys.list.map((k) => (
          <div
            key={k.note}
            className={"inst-key " + (k.black ? "black" : "white") + (down === k.note ? " down" : "")}
            style={{ left: `${(k.at / keys.whites) * 100}%`, width: `${(k.black ? 0.62 : 1) * (100 / keys.whites)}%` }}
            onPointerDown={(e) => {
              e.preventDefault();
              // Pointer events go to the key under the pointer as it slides, with none captured.
              (e.target as Element).releasePointerCapture?.(e.pointerId);
              on(k.note);
            }}
            onPointerEnter={(e) => {
              if (e.buttons & 1) on(k.note);
            }}
            title={k.name}
          />
        ))}
      </div>
      <button className="fx-toggle" title="An octave up" aria-label="An octave up" disabled={base >= 84} onClick={() => setBase(base + 12)}>
        +
      </button>
    </div>
  );
}
