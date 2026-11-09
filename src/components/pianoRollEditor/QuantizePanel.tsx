// The Time Quantize pop-over under the Piano Roll's Quantize button.
import { useEffect, useRef } from "react";
import * as me from "../../midiEdit";
import { menuOpen } from "../Menu";
import { setPrefs, type Prefs } from "./prefs";

/** Logic's Time Quantize settings: value, strength, swing and whether ends are quantized. */
export function QuantizePanel({ anchor, prefs: p, onClose, onQuantize }: { anchor: HTMLElement | null; prefs: Prefs; onClose: () => void; onQuantize: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: MouseEvent) => {
      if (ref.current?.contains(e.target as Node) || anchor?.contains(e.target as Node)) return;
      onClose();
    };
    // Esc closes just the panel: capture phase, ahead of the editor's Esc (deselect all).
    const key = (e: KeyboardEvent) => {
      // A menu over the panel takes the Esc first (it marks it handled).
      if (e.key !== "Escape" || e.defaultPrevented || menuOpen()) return;
      e.stopPropagation();
      onClose();
    };
    window.addEventListener("mousedown", down);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", key, true);
    };
  }, [anchor, onClose]);
  const r = anchor?.getBoundingClientRect();
  const left = Math.min((r?.left ?? 0) - 40, window.innerWidth - 270);
  return (
    <div className="pr-qpanel" ref={ref} style={{ left: Math.max(4, left), top: (r?.bottom ?? 0) + 4 }}>
      <div className="pr-qtitle">Time Quantize</div>
      <div className="pr-qgrid">
        {me.GRID_VALUES.map((g) => (
          <button key={g} className={"pr-qval" + (g === p.qValue ? " on" : "")} aria-pressed={g === p.qValue} onClick={() => setPrefs({ qValue: g })}>
            {g}
          </button>
        ))}
      </div>
      <label className="pr-qrow">
        <span>Strength</span>
        <input type="range" min={0} max={100} value={p.qStrength} onChange={(e) => setPrefs({ qStrength: parseInt(e.target.value) })} />
        <span className="pr-qnum">{p.qStrength}%</span>
      </label>
      <label className="pr-qrow">
        <span>Swing</span>
        <input type="range" min={0} max={100} value={p.qSwing} onChange={(e) => setPrefs({ qSwing: parseInt(e.target.value) })} />
        <span className="pr-qnum">{p.qSwing}%</span>
      </label>
      <label className="pr-qcheck">
        <input type="checkbox" checked={p.qEnds} onChange={(e) => setPrefs({ qEnds: e.target.checked })} />
        Quantize note ends
      </label>
      <div className="pr-qfoot">
        <button className="pr-qbtn" onClick={onQuantize}>
          Quantize
        </button>
      </div>
    </div>
  );
}
