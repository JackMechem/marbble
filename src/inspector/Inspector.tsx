// The Inspector, the left side pane as in Logic: the selected region's section, the track's
// section, and the track's channel strip beside Stereo Out's. inspector/ holds it and its rows: the
// track's (TrackInspector.tsx), a region's fades (FadeRows.tsx) and playback parameters
// (RegionParamRows.tsx).
import { useMemo, useState } from "react";
import { type Region } from "../engine/regions";
import { act } from "../engine/commands";
import { Icon } from "../ui/icons";
import { useStore } from "../state/store";
import { formatLengthBbt, formatTime, toBbt } from "../lib/time";
import { ChannelStrip } from "../mixer/ChannelStrip";
import { FadeRows } from "./FadeRows";
import { RegionParamRows } from "./RegionParamRows";
import { TrackRows } from "./TrackInspector";
import { NumberField } from "../ui/Form";
import { useSelectedTrack } from "../tracks/headers/TrackHeaders";
import { isLooped } from "../tracks/regions/loops";
import { toggleLoop } from "../tracks/regionCommands";
import { useStripFit, rowsFor } from "../mixer/Mixer";

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
