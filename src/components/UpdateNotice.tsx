// "Marbble 0.1.0-alpha.8 is out" with View Release and Later, when the update check finds a
// newer version (src/updates.ts). Not a sheet: it sits above the status line, takes no keys and
// never takes the focus, so it can't get in the way of playing or recording.
import { useEffect, useState } from "react";
import { onTick } from "../store";
import { offerText } from "../updateCheck";
import { dismissUpdate, useUpdateOffer, viewRelease } from "../updates";

/** A press on its buttons leaves the focus where it was, so the keys keep reaching the app. */
const keepFocus = (e: React.MouseEvent) => e.preventDefault();

export function UpdateNotice() {
  const offer = useUpdateOffer();
  // Kept back while a recording runs, and shown once it stops.
  const [recording, setRecording] = useState(false);
  useEffect(() => onTick((t) => setRecording(t.recording)), []);
  if (!offer || recording) return null;
  return (
    <div className="update-notice" role="status">
      <span>{offerText(offer)}</span>
      <button className="push" onMouseDown={keepFocus} onClick={viewRelease}>
        View Release
      </button>
      <button className="push" onMouseDown={keepFocus} onClick={dismissUpdate}>
        Later
      </button>
    </div>
  );
}
