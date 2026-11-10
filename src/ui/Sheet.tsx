// The `Sheet` frame the app's dialogs are built on: a titled box over the window, modal for key
// commands unless its `mode` says otherwise (keyboard/sheetKeys.ts).
import { useRef } from "react";
import { useSheetKeys } from "../keyboard/sheetKeys";
import type { SheetMode } from "../keyboard/sheetKeysModel";
import { sheetKey } from "./Form";

/** A sheet: modal for key commands unless `mode` says otherwise (sheetKeys.ts). */
export function Sheet({ title, children, onClose, width = 560, mode = "modal" }: { title: string; children: React.ReactNode; onClose: () => void; width?: number; mode?: SheetMode }) {
  const ref = useRef<HTMLDivElement>(null);
  useSheetKeys(ref, onClose, mode);
  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <div ref={ref} className="sheet" style={{ width }} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => sheetKey(e, onClose)}>
        <div className="sheet-title">{title}</div>
        {children}
      </div>
    </div>
  );
}
