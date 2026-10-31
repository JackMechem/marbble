// What every Settings pane is made of: the window's frame and tab glyphs, a pane's footer, rows and
// sections, and the helpers that write a change to settings.json.
import { useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { updateSettings, type Prefs, type UiPrefs } from "../../prefs";
import { setState } from "../../store";
import { sheetKey } from "../Form";
import { useSheetKeys } from "../../sheetKeys";

export const toast = (text: string, ms = 3000) => setState({ toast: { text, at: Date.now(), ms } });

export function SheetFrame({ title, children, onClose, width }: { title: string; children: React.ReactNode; onClose: () => void; width: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useSheetKeys(ref, onClose);
  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <div ref={ref} className="sheet" style={{ width }} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => sheetKey(e, onClose)}>
        <div className="sheet-title">{title}</div>
        {children}
      </div>
    </div>
  );
}


/** 20px tab glyphs, drawn like the ones in icons.tsx. */
function Glyph({ children }: { children: React.ReactNode }) {
  return (
    <svg width={20} height={20} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round">
      {children}
    </svg>
  );
}

export const TabIcon = {
  general: () => (
    <Glyph>
      <circle cx="10" cy="10" r="2.6" />
      <path d="M10 2.8v2.1M10 15.1v2.1M2.8 10h2.1M15.1 10h2.1M4.9 4.9l1.5 1.5M13.6 13.6l1.5 1.5M4.9 15.1l1.5-1.5M13.6 6.4l1.5-1.5" />
    </Glyph>
  ),
  recording: () => (
    <Glyph>
      <circle cx="10" cy="10" r="7" />
      <circle cx="10" cy="10" r="3.6" fill="currentColor" stroke="none" />
    </Glyph>
  ),
  metronome: () => (
    <Glyph>
      <path d="M7.2 3h5.6l3 14H4.2z" />
      <path d="M10 14.2 14.6 5.4M6.4 12.5h7.2" />
    </Glyph>
  ),
  midi: () => (
    <Glyph>
      <circle cx="10" cy="10" r="7.2" />
      <circle cx="6.4" cy="10.4" r=".7" fill="currentColor" stroke="none" />
      <circle cx="13.6" cy="10.4" r=".7" fill="currentColor" stroke="none" />
      <circle cx="7.5" cy="7" r=".7" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="7" r=".7" fill="currentColor" stroke="none" />
      <circle cx="10" cy="5.8" r=".7" fill="currentColor" stroke="none" />
      <path d="M8.6 15.2h2.8" />
    </Glyph>
  ),
  editing: () => (
    <Glyph>
      <circle cx="6" cy="14.5" r="2.3" />
      <circle cx="14" cy="14.5" r="2.3" />
      <path d="M7.5 12.8 14 3.5M12.5 12.8 6 3.5" />
    </Glyph>
  ),
  plugins: () => (
    <Glyph>
      <path d="M7 2.8v4M13 2.8v4M5 6.8h10v3.4a5 5 0 0 1-10 0zM10 15.2v2.2" />
    </Glyph>
  ),
  project: () => (
    <Glyph>
      <rect x="3.5" y="3" width="13" height="14" rx="1.5" />
      <path d="M6.5 7h7M6.5 10h7M6.5 13h4" />
    </Glyph>
  ),
  notes: () => (
    <Glyph>
      <path d="M5 3h7.5L15.5 6v11H5z M12.5 3v3h3M7.5 9.5h5M7.5 12.5h5" />
    </Glyph>
  ),
};


/** The usual pane footer: settings save themselves, so there is only Done. */
export function Foot({ close, note = "Changes apply immediately and are remembered.", children }: { close: () => void; note?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="sheet-foot">
      <span className="dim">{note}</span>
      <div className="sheet-buttons">
        {children}
        <button className="push default" onClick={close}>
          Done
        </button>
      </div>
    </div>
  );
}

export function Row({ label, children, note }: { label: string; children: React.ReactNode; note?: React.ReactNode }) {
  return (
    <>
      <label>{label}</label>
      <div className="pref-cell">
        {children}
        {note && <div className="pref-note">{note}</div>}
      </div>
    </>
  );
}

export function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="pref-group">
      <div className="pref-group-title">{title}</div>
      <div className="pref-grid">{children}</div>
    </div>
  );
}

/** Updates one section of the session preferences. */
export function setPrefs<K extends keyof Prefs>(key: K, patch: Partial<Prefs[K]>) {
  updateSettings((s) => ({ ...s, prefs: { ...s.prefs, [key]: { ...s.prefs[key], ...patch } } }));
}

export function setUi(patch: Partial<UiPrefs>) {
  updateSettings((s) => ({ ...s, ui: { ...s.ui, ...patch } }));
}

export function FolderField({ value, placeholder, onChange, title }: { value: string | null; placeholder: string; onChange: (v: string | null) => void; title: string }) {
  return (
    <div className="row-inline">
      <span className={"path-field pref-path" + (value ? "" : " placeholder")} title={value ?? placeholder}>
        <bdi>{value ?? placeholder}</bdi>
      </span>
      <button
        className="push"
        onClick={async () => {
          const dir = await open({ directory: true, title, defaultPath: value ?? undefined });
          if (typeof dir === "string") onChange(dir);
        }}
      >
        Choose…
      </button>
      {value && (
        <button className="push" onClick={() => onChange(null)}>
          Default
        </button>
      )}
    </div>
  );
}
