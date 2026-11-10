// The small controls the dynamics panels (Compressor, Limiter) are built from: knob, segmented choice,
// toggle, framed section, level bar and the gain-reduction meters.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FxPanelProps } from "../../builtinEffects";
import { onFrame } from "../../../state/frameLoop";
import { useStore } from "../../../state/store";
import { theme } from "../../../theme/canvasTheme";
import { arc, formatDyn, fromNorm, KNOB_SWEEP, polar, toNorm } from "./dynScale";
import { sliderAria } from "../../../keyboard/sliderKeys";
import { sliderKeyDown } from "../../../ui/Controls";

export function cssVar(name: string, fallback = "#888"): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export interface DynKnobProps {
  label: string;
  value: number;
  min: number;
  max: number;
  log?: boolean;
  /** Double-click resets to this. */
  def: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
  onGrab?: (held: boolean) => void;
  /** Switch positions: the knob snaps to the nearest and draws a tick at each. */
  steps?: number[];
  size?: number;
  disabled?: boolean;
  title?: string;
  /** Text shown instead of the value when disabled. */
  note?: string;
}

/**
 * A rotary control dragged vertically (Shift for fine); double-click resets. Works out each
 * move from the value at mouse-down, so rounding never eats small drags.
 */
export function DynKnob({ label, value, min, max, log = false, def, format, onChange, onGrab, steps, size = 40, disabled, title, note }: DynKnobProps) {
  const n = toNorm(value, min, max, log);
  const angle = -KNOB_SWEEP + n * 2 * KNOB_SWEEP;
  const c = size / 2;
  const r = c - 3;
  const snap = (v: number) => {
    if (!steps?.length) return v;
    const nv = toNorm(v, min, max, log);
    return steps.reduce((best, s) => (Math.abs(toNorm(s, min, max, log) - nv) < Math.abs(toNorm(best, min, max, log) - nv) ? s : best), steps[0]);
  };
  const down = (e: React.MouseEvent) => {
    if (disabled || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const n0 = n;
    let px = 0;
    let last = e.clientY;
    onGrab?.(true);
    const move = (ev: MouseEvent) => {
      px += (last - ev.clientY) * (ev.shiftKey ? 0.15 : 1);
      last = ev.clientY;
      const nn = Math.min(1, Math.max(0, n0 + px / 180));
      const v = snap(fromNorm(nn, min, max, log));
      if (v !== value) onChange(v);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      onGrab?.(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  return (
    <div
      className={"dyn-knob" + (disabled ? " disabled" : "")}
      title={title ?? `${label} · drag · Shift for fine · double-click to reset`}
      {...(disabled ? {} : sliderAria({ label, text: format(value), min, max, now: value }))}
      onKeyDown={disabled ? undefined : sliderKeyDown(n, value, (p) => snap(fromNorm(p, min, max, log)), onChange, onGrab && { onTouchStart: () => onGrab(true), onTouchEnd: () => onGrab(false) })}
    >
      <svg
        width={size}
        height={size}
        onMouseDown={down}
        onDoubleClick={() => {
          if (disabled) return;
          onGrab?.(true);
          onChange(snap(def));
          onGrab?.(false);
        }}
      >
        <path d={arc(c, r, -KNOB_SWEEP, KNOB_SWEEP)} className="dyn-knob-track" />
        {!disabled && n > 0.002 && <path d={arc(c, r, -KNOB_SWEEP, angle)} className="dyn-knob-arc" />}
        {steps?.map((s) => {
          const [x, y] = polar(c, r, -KNOB_SWEEP + toNorm(s, min, max, log) * 2 * KNOB_SWEEP);
          return <circle key={s} cx={x} cy={y} r={1.1} className="dyn-knob-step" />;
        })}
        <circle cx={c} cy={c} r={r - 4.5} className="dyn-knob-body" />
        <line x1={polar(c, r * 0.18, angle)[0]} y1={polar(c, r * 0.18, angle)[1]} x2={polar(c, r - 6, angle)[0]} y2={polar(c, r - 6, angle)[1]} className="dyn-knob-pointer" />
      </svg>
      <div className="dyn-knob-label">{label}</div>
      <div className="dyn-knob-value">{disabled ? note ?? "—" : format(value)}</div>
    </div>
  );
}

/** A knob bound to one of the effect's parameters, optionally with a narrower range. */
export function ParamKnob({
  props,
  id,
  label,
  range,
  ...rest
}: { props: FxPanelProps; id: number; label?: string; range?: [number, number] } & Partial<Omit<DynKnobProps, "value" | "onChange" | "onGrab" | "min" | "max">>) {
  const spec = props.info.params.find((p) => p.id === id);
  if (!spec) return null;
  const [min, max] = range ?? [spec.min, spec.max];
  const raw = props.values[id] ?? spec.default;
  return (
    <DynKnob
      label={label ?? spec.name}
      value={Math.min(max, Math.max(min, raw))}
      min={min}
      max={max}
      log={spec.log}
      def={Math.min(max, Math.max(min, spec.default))}
      format={(v) => formatDyn(spec, v)}
      onChange={(v) => props.set(id, v)}
      onGrab={(on) => props.touch(id, on)}
      {...rest}
    />
  );
}

/** A row of buttons for a choice, lit on the current one. */
export function Segmented({ labels, value, onChange, title, vertical }: { labels: string[]; value: number; onChange: (i: number) => void; title?: string; vertical?: boolean }) {
  return (
    <div className={"dyn-seg" + (vertical ? " vertical" : "")} title={title}>
      {labels.map((l, i) => (
        <button key={l + i} className={i === Math.round(value) ? "on" : ""} aria-pressed={i === Math.round(value)} onClick={() => onChange(i)}>
          {l}
        </button>
      ))}
    </div>
  );
}

export function ParamChoice({ props, id, labels, vertical }: { props: FxPanelProps; id: number; labels?: string[]; vertical?: boolean }) {
  const spec = props.info.params.find((p) => p.id === id);
  if (!spec || spec.kind.type !== "choice") return null;
  return (
    <Segmented
      labels={labels ?? spec.kind.labels}
      value={props.values[id] ?? spec.default}
      title={spec.name}
      vertical={vertical}
      onChange={(i) => {
        props.touch(id, true);
        props.set(id, i);
        props.touch(id, false);
      }}
    />
  );
}

export function ParamToggle({ props, id, label, title }: { props: FxPanelProps; id: number; label?: string; title?: string }) {
  const spec = props.info.params.find((p) => p.id === id);
  if (!spec) return null;
  const on = (props.values[id] ?? spec.default) >= 0.5;
  return (
    <button
      className={"dyn-toggle" + (on ? " on" : "")}
      aria-pressed={on}
      title={title ?? spec.name}
      onClick={() => {
        props.touch(id, true);
        props.set(id, on ? 0 : 1);
        props.touch(id, false);
      }}
    >
      <span className="dyn-led" />
      {label ?? spec.name}
    </button>
  );
}

/** A labelled group of controls, like the framed sections on Logic's plug-in faces. */
export function Section({ title, children, className }: { title: string; children: React.ReactNode; className?: string }) {
  return (
    <div className={"dyn-section" + (className ? " " + className : "")}>
      <div className="dyn-section-title">{title}</div>
      <div className="dyn-section-body">{children}</div>
    </div>
  );
}

const BAR_FLOOR = -60;

/**
 * A level meter in dBFS for one meter value, with a 1.5 s peak hold and a readout that holds
 * the maximum until clicked. The value arrives about 30 times a second; the bar eases between.
 * With `grow`, `length` is the bar's smallest and it fills the space its column has.
 */
export function LevelBar({ db, label, horizontal = false, length = 120, grow = false }: { db: number; label: string; horizontal?: boolean; length?: number; grow?: boolean }) {
  const bar = useRef<HTMLDivElement>(null);
  const fill = useRef<HTMLDivElement>(null);
  const hold = useRef<HTMLDivElement>(null);
  const target = useRef(db);
  target.current = db;
  const [max, setMax] = useState(-Infinity);
  // The channel meters' colours, so the bar follows the theme.
  useStore((s) => s.themeId);
  const gradient = `linear-gradient(to ${horizontal ? "right" : "top"}, ${theme.meterStops.join(", ")})`;
  useEffect(() => {
    let shown = BAR_FLOOR;
    let held = BAR_FLOOR;
    let heldAt = 0;
    let last = performance.now();
    return onFrame((now) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const t = Math.max(BAR_FLOOR, target.current);
      shown = t > shown ? t : Math.max(t, shown - 20 * dt);
      if (t >= held || now - heldAt > 1500) {
        held = t;
        heldAt = now;
      }
      const f = (v: number) => `${((Math.min(6, v) - BAR_FLOOR) / (6 - BAR_FLOOR)) * 100}%`;
      if (fill.current) fill.current.style[horizontal ? "width" : "height"] = f(shown);
      if (hold.current) {
        hold.current.style[horizontal ? "left" : "bottom"] = f(held);
        hold.current.classList.toggle("over", held > 0);
      }
    });
  }, [horizontal]);
  useEffect(() => {
    if (db > max) setMax(db);
  }, [db, max]);
  // A growing bar's colours stretch with it, so the zones stay at the same levels.
  useLayoutEffect(() => {
    const el = bar.current;
    if (!grow || !el) return;
    const ro = new ResizeObserver(([e]) => el.style.setProperty("--dyn-bar-len", `${horizontal ? e.contentRect.width : e.contentRect.height}px`));
    ro.observe(el);
    return () => ro.disconnect();
  }, [grow, horizontal]);
  const dim = horizontal ? "width" : "height";
  const barStyle = grow ? { [horizontal ? "minWidth" : "minHeight"]: length, flex: "1 1 auto" } : { [dim]: length, ["--dyn-bar-len" as string]: `${length}px` };
  return (
    <div className={"dyn-level" + (horizontal ? " horizontal" : "")}>
      <div className="dyn-level-bar" ref={bar} style={barStyle}>
        <div className="dyn-level-fill" ref={fill} style={{ backgroundImage: gradient }} />
        <div className="dyn-level-hold" ref={hold} />
        <div className="dyn-level-zero" style={horizontal ? { left: `${(-BAR_FLOOR / (6 - BAR_FLOOR)) * 100}%` } : { bottom: `${(-BAR_FLOOR / (6 - BAR_FLOOR)) * 100}%` }} />
      </div>
      <div className="dyn-level-label">{label}</div>
      <button className={"dyn-level-peak" + (max > 0 ? " over" : "")} title="Peak · click to reset" onClick={() => setMax(-Infinity)}>
        {max <= -100 || !isFinite(max) ? "−∞" : max.toFixed(1)}
      </button>
    </div>
  );
}

/** GR meter scale: where `gr` dB sits, 0..1 from no reduction to the end of the scale. */
function grPos(gr: number, top: number) {
  return Math.log10(1 + Math.max(0, gr)) / Math.log10(1 + top);
}

/**
 * Gain reduction on a bar that grows leftwards from 0 dB, as on Logic's Platinum face and the
 * Limiter. Eases at VU-like speed so it reads as movement rather than flicker.
 */
export function GrBar({ gr, top = 30, marks = [0, 1, 2, 3, 6, 10, 20, 30] }: { gr: number; top?: number; marks?: number[] }) {
  const fill = useRef<HTMLDivElement>(null);
  const target = useRef(gr);
  target.current = gr;
  useEffect(() => {
    let shown = 0;
    let last = performance.now();
    return onFrame((now) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const t = target.current;
      shown += (t - shown) * Math.min(1, dt / (t > shown ? 0.03 : 0.12));
      if (fill.current) fill.current.style.width = `${grPos(shown, top) * 100}%`;
    });
  }, [top]);
  return (
    <div className="dyn-grbar">
      <div className="dyn-grbar-track">
        <div className="dyn-grbar-fill" ref={fill} />
      </div>
      <div className="dyn-grbar-scale">
        {marks.map((m) => (
          <span key={m} style={{ right: `${grPos(m, top) * 100}%` }}>
            {m}
          </span>
        ))}
      </div>
    </div>
  );
}

/**
 * An analogue gain-reduction meter: a needle resting at 0 on the right that swings left with
 * reduction, with VU ballistics (about 300 ms to settle).
 */
export function GrNeedle({ gr }: { gr: number }) {
  const needle = useRef<SVGLineElement>(null);
  const target = useRef(gr);
  target.current = gr;
  const W = 300;
  const H = 104;
  const cx = W / 2;
  const cy = H + 46;
  const R = 128;
  const SWEEP = 42;
  const top = 20;
  const angle = (g: number) => SWEEP - grPos(g, top) * 2 * SWEEP;
  useEffect(() => {
    let shown = 0;
    let vel = 0;
    let last = performance.now();
    return onFrame((now) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      // A damped mass on a spring, like a moving-coil movement: a slight, quick overshoot.
      const k = 260;
      const damp = 26;
      vel += (k * (target.current - shown) - damp * vel) * dt;
      shown = Math.max(-0.4, Math.min(top + 1, shown + vel * dt));
      if (needle.current) needle.current.setAttribute("transform", `rotate(${angle(shown)} ${cx} ${cy})`);
    });
  }, []);
  const marks = [0, 1, 2, 3, 5, 7, 10, 15, 20];
  return (
    <svg className="dyn-vu" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMid meet">
      <path d={`M${polar(cx, R, -SWEEP - 2).join(" ")} A${R} ${R} 0 0 1 ${polar(cx, R, SWEEP + 2).join(" ")}`} transform={`translate(0 ${cy - cx})`} className="dyn-vu-arc" />
      <path d={`M${polar(cx, R, -SWEEP).join(" ")} A${R} ${R} 0 0 1 ${polar(cx, R, angle(10)).join(" ")}`} transform={`translate(0 ${cy - cx})`} className="dyn-vu-arc hot" />
      {marks.map((m) => {
        const a = angle(m);
        const [x0, y0] = polar(cx, R, a);
        const [x1, y1] = polar(cx, R + (m % 5 === 0 || m < 4 ? 9 : 5), a);
        const [tx, ty] = polar(cx, R + 19, a);
        return (
          <g key={m} transform={`translate(0 ${cy - cx})`}>
            <line x1={x0} y1={y0} x2={x1} y2={y1} className="dyn-vu-tick" />
            <text x={tx} y={ty} className="dyn-vu-num">
              {m}
            </text>
          </g>
        );
      })}
      <text x={cx} y={H - 30} className="dyn-vu-legend">
        GAIN REDUCTION · dB
      </text>
      <line ref={needle} x1={cx} y1={cy} x2={cx} y2={cy - R - 8} className="dyn-vu-needle" transform={`rotate(${SWEEP} ${cx} ${cy})`} />
      <rect x={0} y={H - 14} width={W} height={14} className="dyn-vu-foot" />
    </svg>
  );
}
