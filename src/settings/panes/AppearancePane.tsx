// Settings › Appearance: the theme (each shown as a small preview), the interface size, the
// waveform style and where the CPU meter shows.
import { useStore } from "../../state/store";
import { mix } from "../../theme/canvasTheme";
import { applyTheme, THEMES, type Theme } from "../../theme/themes";
import { applyUiScale, UI_SCALES } from "../../ui/uiScale";
import { applyWaveStyle, WAVE_STYLES } from "../../tracks/regions/waveform";
import { Foot, setUi } from "./parts";
import { useMeterPlace } from "../../app/cpuMeter/CpuMeter";
import type { MeterPlace } from "../../app/cpuMeter/cpuMeterModel";
import { fmt } from "../../keyboard/keymap";

function ThemePreview({ t }: { t: Theme }) {
  const c = t.css;
  const cv = t.canvas;
  const region = (color: string) =>
    cv.region === "light" ? mix(color, "#ffffff", 0.6) : cv.region === "flat" ? mix(color, cv.regionBase, 0.55) : mix(color, cv.regionBase, 0.25, 0.85);
  return (
    <div className="theme-preview" style={{ background: c["--bg"], boxShadow: `0 0 0 1px ${c["--edge"]}` }}>
      <div className="tp-bar" style={{ background: c["--cb-bg"], borderBottom: `1px solid ${c["--cb-edge"]}` }}>
        <i style={{ background: c["--btn"] }} />
        <i style={{ background: c["--btn"] }} />
        <b style={{ background: c["--lcd-bg"], boxShadow: c["--lcd-shadow"] }} />
        <i style={{ background: c["--cycle"] }} />
        <i style={{ background: c["--accent"] }} />
      </div>
      <div className="tp-body">
        <div className="tp-heads" style={{ background: c["--headers-bg"], borderRight: `1px solid ${c["--edge"]}` }}>
          <div style={{ background: c["--header-sel"], borderBottom: `1px solid ${c["--header-line"]}` }} />
          <div style={{ background: c["--header"], borderBottom: `1px solid ${c["--header-line"]}` }} />
          <div style={{ background: c["--header"] }} />
        </div>
        <div className="tp-lanes" style={{ background: cv.arrangeBg }}>
          <div style={{ background: cv.laneSelected, borderBottom: `1px solid ${cv.arrangeBg}` }} />
          <div style={{ background: cv.lane, borderBottom: `1px solid ${cv.arrangeBg}` }} />
          <div style={{ background: cv.lane }} />
          <span className="tp-region" style={{ left: "6%", width: "52%", top: 2, background: region("#3b8fd9") }} />
          <span className="tp-region" style={{ left: "38%", width: "50%", top: "calc(33.3% + 2px)", background: region("#f2a63b") }} />
          <span className="tp-region" style={{ left: "6%", width: "40%", top: "calc(66.6% + 2px)", background: region("#4fae4c") }} />
          <span style={{ position: "absolute", left: "64%", top: 0, bottom: 0, width: 1, background: cv.playhead }} />
        </div>
      </div>
    </div>
  );
}

export function AppearancePane({ close }: { close: () => void }) {
  const current = useStore((s) => s.themeId);
  const scale = useStore((s) => s.uiScale);
  const waveStyle = useStore((s) => s.waveStyle);
  const meter = useMeterPlace();
  const places: [MeterPlace, string][] = [
    ["status_line", "Status Line"],
    ["control_bar", "Control Bar"],
  ];
  return (
    <>
      <div className="pref-section">
        <div className="pref-title">Interface Size</div>
        <div className="segmented">
          {UI_SCALES.map((s) => (
            <button key={s} className={Math.abs(s - scale) < 0.001 ? "on" : ""} aria-pressed={Math.abs(s - scale) < 0.001} onClick={() => applyUiScale(s)}>
              {Math.round(s * 100)}%
            </button>
          ))}
        </div>
        <div className="pref-hint">Also in View › Larger / Smaller Interface ({fmt("Mod+=")} / {fmt("Mod+-")}, {fmt("Mod+0")} for actual size).</div>
      </div>
      <div className="pref-section">
        <div className="pref-title">Waveforms</div>
        <div className="segmented">
          {WAVE_STYLES.map((s) => (
            <button key={s.id} className={s.id === waveStyle ? "on" : ""} aria-pressed={s.id === waveStyle} title={s.description} onClick={() => applyWaveStyle(s.id)}>
              {s.name}
            </button>
          ))}
        </div>
        <div className="pref-hint">{WAVE_STYLES.find((s) => s.id === waveStyle)?.description}.</div>
      </div>
      <div className="pref-section">
        <div className="pref-title">CPU Meter</div>
        <div className="segmented">
          {places.map(([id, name]) => (
            <button key={id} className={id === meter ? "on" : ""} aria-pressed={id === meter} onClick={() => setUi({ cpu_meter: id })}>
              {name}
            </button>
          ))}
        </div>
        <div className="pref-hint">
          {meter === "status_line"
            ? "At the bottom of the window, beside the memory Marbble is using and the threads playing tracks."
            : "At the top, after the master volume, as in Logic Pro. The status line still shows memory and threads."}
        </div>
      </div>
      <div className="pref-title">Theme</div>
      <div className="theme-grid">
        {THEMES.map((t) => (
          <button key={t.id} className={"theme-card" + (t.id === current ? " on" : "")} aria-pressed={t.id === current} onClick={() => applyTheme(t.id)}>
            <ThemePreview t={t} />
            <span className="theme-name">{t.name}</span>
            <span className="theme-desc">{t.description}</span>
          </button>
        ))}
      </div>
      <Foot close={close} />
    </>
  );
}
