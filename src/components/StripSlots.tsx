// The pieces that keep channel strips level with each other: the boxes their insert and send
// slots sit in (as many rows as the strips beside them, see stripRows.ts), the "+N" for slots
// scrolled out of view, and blank stand-ins for sections a strip doesn't have (Stereo Out's input
// and sends, a folder's plug-ins, a VCA's everything), as tall as the real ones.

/** A strip's box of insert or send slots: `rows` slots tall, scrolling when it holds more. */
export function SlotBox({ rows, children }: { rows: number; children?: React.ReactNode }) {
  return (
    <div className="strip-slots" style={{ "--rows": rows } as React.CSSProperties}>
      {children}
    </div>
  );
}

/** "+N" beside a section's label: filled slots scrolled out of its box. */
export function MoreBadge({ n }: { n: number }) {
  return n > 0 ? (
    <span className="strip-more" title={`${n} more below: scroll the slots`}>
      +{n}
    </span>
  ) : null;
}

/** Stand-ins for sections a strip doesn't have, as tall as the real ones, so that the rows under
 *  them stay level with the neighbouring strips'. */
export function BlankSlots({ rows }: { rows: number }) {
  return (
    <div className="strip-section strip-blank" aria-hidden>
      <div className="strip-label">&nbsp;</div>
      <SlotBox rows={rows} />
    </div>
  );
}

export function BlankRow({ label = false }: { label?: boolean }) {
  return (
    <div className="strip-section strip-blank" aria-hidden>
      {label && <div className="strip-label">&nbsp;</div>}
      <div className="slot static" />
    </div>
  );
}

export function BlankEq() {
  return (
    <div className="strip-section strip-blank" aria-hidden>
      <div className="eq-thumb" />
    </div>
  );
}

/** The button block's place on Stereo Out, which has no buttons, so its fader ends where the
 *  others' do. */
export function BlankButtons() {
  return (
    <div className="strip-buttons strip-blank" aria-hidden>
      <span className="tbtn power ghost" />
      <div className="btn-row">
        <span className="tbtn ghost" />
        <span className="tbtn ghost" />
      </div>
      <div className="btn-row">
        <span className="tbtn ghost" />
        <span className="tbtn ghost" />
      </div>
    </div>
  );
}
