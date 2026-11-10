// The strip under the Audio Track Editor's bar: the region's details and the selected range.
import { useEffect, useState } from "react";
import { type Id, type Project, type Track } from "../../engine/project";
import { type Region } from "../../engine/regions";
import { call } from "../../engine/commands";
import * as ed from "./state";
import { formatLengthBbt, formatTime, toBbt } from "../../lib/time";
import { formatDb } from "../../lib/decibels";

const bbt = (p: Project, pos: number) => {
  const b = toBbt(p, pos);
  return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
};

const clipInfos = new Map<Id, { frames: number; channels: number; sample_rate: number; path: string } | null>();

/** The region's details, and the selected range when there is one. */
export function InfoStrip({ project, track, region, sel }: { project: Project; track: Track; region: Region | null; sel: ed.RangeSel | null }) {
  const clip = region?.content.type === "audio" ? region.content.clip : null;
  const [, setLoaded] = useState(0);
  useEffect(() => {
    if (clip === null || clipInfos.has(clip)) return;
    call<{ frames: number; channels: number; sample_rate: number; path: string }>("clip_info", { id: clip }).then((info) => {
      clipInfos.set(clip, info);
      setLoaded((n) => n + 1);
    });
  }, [clip]);
  const info = clip !== null ? clipInfos.get(clip) : undefined;
  const file = info?.path.split(/[\\/]/).pop();
  return (
    <div className="aed-info">
      {region ? (
        <>
          <span>
            <b>{region.name}</b>
            {region.muted ? " (muted)" : ""}
          </span>
          <span>
            Start <b>{bbt(project, region.start)}</b>
          </span>
          <span>
            Length <b>{formatLengthBbt(project, region.start, region.length)}</b> · {formatTime(project, region.length)}
          </span>
          {region.content.type === "audio" && (
            <span>
              Gain <b>{formatDb(region.content.gain_db)} dB</b>
            </span>
          )}
          {info && (
            <span title={info.path}>
              {(info.sample_rate / 1000).toFixed(1)} kHz · {info.channels === 1 ? "Mono" : info.channels === 2 ? "Stereo" : `${info.channels} ch`} · {file}
            </span>
          )}
        </>
      ) : (
        <span>
          {track.regions.length ? `${track.regions.length} region${track.regions.length > 1 ? "s" : ""} on this track. Click one to select it, or drag across the waveform to select a range.` : "No regions."}
        </span>
      )}
      {sel && (
        <span className="aed-sel">
          Selection <b>{bbt(project, sel.start)}</b> – <b>{bbt(project, sel.end)}</b> · {formatTime(project, sel.end - sel.start)}
        </span>
      )}
    </div>
  );
}
