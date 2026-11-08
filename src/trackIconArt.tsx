// The track icons' drawings, by the ids in trackIcons.ts: 1.4px strokes on a 20px grid in
// currentColor, like the rest of icons.tsx, simple enough to read at a compact header's 16px.
import type { ReactNode } from "react";
import type { Track } from "./api";
import { shownIcon } from "./trackIcons";

/** A vertical guitar-family body, turned to lie along the diagonal (head top right). */
const Tilted = ({ children }: { children: ReactNode }) => <g transform="rotate(45 10 10)">{children}</g>;

const dot = (cx: number, cy: number, r = 0.9) => <circle cx={cx} cy={cy} r={r} fill="currentColor" stroke="none" />;

const ART: Record<string, ReactNode> = {
  // The kinds' own icons.
  audio: <path d="M2 10h1.5M4.5 7v6M7 4v12M9.5 6.5v7M12 3v14M14.5 7.5v5M17 9v2" />,
  instrument: (
    <>
      <rect x="2" y="5" width="16" height="10" rx="1.5" />
      <path d="M6 5v10M10 5v10M14 5v10" />
      <path d="M5 5v5.5h2V5M9 5v5.5h2V5M13 5v5.5h2V5" fill="currentColor" strokeWidth={0} />
    </>
  ),
  /** Two signals merging into one bus. */
  aux: <path d="M2.5 5.5h4l4 4.5h7M2.5 14.5h4l4-4.5M15 7.5l2.5 2.5-2.5 2.5" />,
  folder: <path d="M2.5 6h5l1.5 1.8h8.5v8.7h-15z" />,
  summing: <path d="M2.5 6h5l1.5 1.8h8.5v8.7h-15zM7 12.2h6M10 9.2v6" />,

  // Drums.
  kick: (
    <>
      <circle cx="10" cy="9.3" r="6.5" />
      <circle cx="12.3" cy="11.6" r="1.6" />
      <path d="M5.4 14 3.8 17.5M14.6 14l1.6 3.5" />
    </>
  ),
  snare: (
    <>
      <ellipse cx="10" cy="9.5" rx="7" ry="2.3" />
      <path d="M3 9.5v4.4c0 1.3 3.1 2.3 7 2.3s7-1 7-2.3V9.5" />
      <path d="M5.5 2.5 9 8M14.5 2.5 11 8" />
    </>
  ),
  hihat: <path d="M3 7.5c2.5-2.2 11.5-2.2 14 0zM3 9.5c2.5 2.2 11.5 2.2 14 0zM10 2.5v2.4M10 11.2v6.3M7 17.5h6" />,
  toms: (
    <>
      <ellipse cx="5.8" cy="6.5" rx="3.5" ry="1.3" />
      <path d="M2.3 6.5v4.3c0 .8 1.6 1.4 3.5 1.4s3.5-.6 3.5-1.4V6.5" />
      <ellipse cx="14.2" cy="9.8" rx="3.5" ry="1.3" />
      <path d="M10.7 9.8v4.6c0 .8 1.6 1.4 3.5 1.4s3.5-.6 3.5-1.4V9.8M5.8 12.2v5" />
    </>
  ),
  overheads: (
    <>
      <ellipse cx="10" cy="6" rx="7.5" ry="1.7" transform="rotate(-10 10 6)" />
      <path d="M10 7.6v9.9M6.5 17.5 10 14l3.5 3.5" />
    </>
  ),
  room: (
    <>
      <path d="M2.5 2.5v15h15" />
      <path d="M6 11a3 3 0 0 1 3 3M6 7.5a6.5 6.5 0 0 1 6.5 6.5M6 4a10 10 0 0 1 10 10" />
      {dot(6, 14, 1)}
    </>
  ),
  kit: (
    <>
      <circle cx="10" cy="13" r="4.5" />
      <path d="M1.8 5h5.4M12.8 5h5.4M4.5 5v12.5M15.5 5v12.5M7.3 8.6c0-1 1.2-1.6 2.7-1.6s2.7.6 2.7 1.6" />
    </>
  ),
  "drum-machine": (
    <>
      <rect x="2.5" y="4.5" width="15" height="11" rx="1.5" />
      <path d="M5 7.6h4" />
      {dot(12.5, 7.6)}
      {dot(15, 7.6)}
      <path d="M4.8 10.4h2.2v2.2H4.8zM7.8 10.4H10v2.2H7.8zM10.8 10.4H13v2.2h-2.2zM13.8 10.4H16v2.2h-2.2z" fill="currentColor" strokeWidth={0.6} />
    </>
  ),

  // Percussion.
  congas: (
    <>
      <ellipse cx="6.2" cy="4.5" rx="2.9" ry="1.1" />
      <path d="M3.3 4.5c-.4 3.6.4 8.4 1.1 12.5h3.6c.7-4.1 1.5-8.9 1.1-12.5" />
      <ellipse cx="13.8" cy="6" rx="2.9" ry="1.1" />
      <path d="M10.9 6c-.4 3.2.4 7.4 1.1 11h3.6c.7-3.6 1.5-7.8 1.1-11" />
    </>
  ),
  shaker: (
    <>
      <ellipse cx="11.5" cy="11" rx="3.5" ry="5.6" transform="rotate(40 11.5 11)" />
      <path d="M2.5 9.5 4.3 10M3.3 6l1.6.9M5.8 3l1.1 1.4" />
    </>
  ),
  tambourine: (
    <>
      <circle cx="10" cy="10" r="6.5" />
      <circle cx="10" cy="10" r="4.3" />
      <path d="M10 3.5v2.2M10 14.3v2.2M3.5 10h2.2M14.3 10h2.2" strokeWidth={2.4} />
    </>
  ),
  cowbell: <path d="M6.5 5.5h7l2.5 11.5h-12zM8.5 5.5V2.8h3v2.7" />,

  // Bass.
  bass: (
    <Tilted>
      <path d="M8.6 12.6c-.4-1.2-2.1-1.1-2.1.3 0 1.1.6 1.6.6 2.5 0 1.3-.9 1.6-.9 2.5 0 .9.9 1.4 1.8 1.4h4c.9 0 1.8-.5 1.8-1.4 0-.9-.9-1.2-.9-2.5 0-.9.6-1.4.6-2.5 0-1.4-1.7-1.5-2.1-.3-.3.6-.8.9-1.4.9s-1.1-.3-1.4-.9z" />
      <path d="M10 13V1.8M9 15.6h2" />
      <path d="M8.7 1.6h2.6M8.7 3.3h2.6" />
    </Tilted>
  ),
  "upright-bass": (
    <>
      <path d="M7.4 10.6A3.2 3.2 0 1 1 12.6 10.6A4 4 0 1 1 7.4 10.6z" />
      <path d="M10 5.5V1.8M10 17.6v.9M8.6 12v2.4M11.4 12v2.4" />
    </>
  ),

  // Guitars.
  "electric-guitar": (
    <Tilted>
      <path d="M8.4 10.2c-.5-1.4-2.4-1.3-2.4.3 0 1.3.7 1.9.7 2.9 0 1.5-1 1.9-1 3 0 1 1 1.6 2 1.6h4.6c1 0 2-.6 2-1.6 0-1.1-1-1.5-1-3 0-1 .7-1.6.7-2.9 0-1.6-1.9-1.7-2.4-.3-.3.7-.9 1-1.6 1s-1.3-.3-1.6-1z" />
      <path d="M10 10.8V2.5M8.6 13.4h2.8M8.6 15.4h2.8M9 2.5h2l-.3-2h-1.4z" />
    </Tilted>
  ),
  "acoustic-guitar": (
    <Tilted>
      <path d="M8 11A2.7 2.7 0 1 1 12 11A3.6 3.6 0 1 1 8 11z" />
      <circle cx="10" cy="12" r="1.2" />
      <path d="M10 6.5V2.5M9 2.5h2l-.3-2h-1.4z" />
    </Tilted>
  ),
  amp: (
    <>
      <rect x="2.5" y="4" width="15" height="12.5" rx="1.5" />
      <path d="M2.5 7.8h15M5 10.5h10M5 12.5h10M5 14.5h10" />
      {dot(5.2, 5.9, 0.8)}
      {dot(7.6, 5.9, 0.8)}
      {dot(10, 5.9, 0.8)}
    </>
  ),

  // Keys.
  piano: (
    <>
      <path d="M2.5 9.5h12.3c1.6 0 2.7 1 2.7 2.3v.9h-15z" />
      <path d="M3.2 9.5 14 4.2M9.5 9.5 10 7.2M4 12.7v4.8M16 12.7v4.8M10 12.7v2.8" />
    </>
  ),
  organ: (
    <>
      <rect x="2.5" y="8.5" width="15" height="3.4" rx=".7" />
      <rect x="2.5" y="13.6" width="15" height="3.4" rx=".7" />
      <path d="M5 2.5v3.5M7.5 2.5v2.5M10 2.5v4M12.5 2.5v2M15 2.5v3" />
    </>
  ),
  synth: (
    <>
      <rect x="2" y="4" width="16" height="12" rx="1.5" />
      <path d="M2 10.5h16M6 10.5V16M10 10.5V16M14 10.5V16M12.8 6.2v2.4M15.3 6.2v2.4" />
      <circle cx="5.4" cy="7.3" r="1.1" />
      <circle cx="9" cy="7.3" r="1.1" />
    </>
  ),

  // Strings.
  violin: (
    <Tilted>
      <path d="M6.6 10.3A2.2 2.2 0 1 1 9.4 10.3A2.7 2.7 0 1 1 6.6 10.3z" />
      <path d="M8 6.4V2.6M14 2v16M14 18l-1.2-1" />
      <circle cx="8" cy="1.9" r=".8" />
    </Tilted>
  ),
  harp: <path d="M4.5 17.5V3c3.5-1 6.5 3.5 11.5 2.5L5.5 17.5M3 17.5h3.5M7.5 3.6v11.2M10.5 4.4v7.6M13.5 5.4v3.6" />,

  // Brass.
  trumpet: (
    <>
      <path d="M2.5 10h10M7 13h5.5M7 13a1.5 1.5 0 0 1 0-3M12.5 10l4.5-3v9l-4.5-3" />
      <path d="M7.3 10V7M9.3 10V7M11.3 10V7" />
    </>
  ),
  trombone: <path d="M2.5 6.5h9.5M12 6.5l5-3v7l-5-3M4.5 10h11.5M4.5 13.4h11.5M4.5 10a1.7 1.7 0 0 0 0 3.4M7 6.5V10" />,

  // Woodwinds.
  sax: (
    <>
      <path d="M8 5v8a3.2 3.2 0 0 0 6.4 0V9.5M12.2 6.8l2.2 2.7 2.4-2.7M8 5c0-1.4-1-2.4-2.6-2.2L3.5 3.4" />
      {dot(8, 7.6, 0.75)}
      {dot(8, 10.2, 0.75)}
    </>
  ),
  flute: (
    <>
      <rect x="1.5" y="8.8" width="17" height="2.4" rx="1.2" transform="rotate(-25 10 10)" />
      {dot(7.3, 11.3, 0.6)}
      {dot(9.6, 10.2, 0.6)}
      {dot(11.9, 9.1, 0.6)}
      {dot(14.2, 8, 0.6)}
    </>
  ),

  // Vocals.
  mic: (
    <>
      <rect x="7" y="2.5" width="6" height="9.5" rx="3" />
      <path d="M4.5 9.5a5.5 5.5 0 0 0 11 0M10 15v2.5M7 17.5h6" />
    </>
  ),
  choir: (
    <>
      <circle cx="4.8" cy="8.5" r="1.8" />
      <circle cx="15.2" cy="8.5" r="1.8" />
      <circle cx="10" cy="6" r="2" />
      <path d="M1.8 16.5a3 3 0 0 1 6 0M12.2 16.5a3 3 0 0 1 6 0M6.3 14.5a3.7 3.7 0 0 1 7.4 0" />
    </>
  ),

  // FX and buses.
  fx: <path d="M8.5 3.5l1.4 4.9 4.9 1.4-4.9 1.4-1.4 4.9-1.4-4.9-4.9-1.4 4.9-1.4zM15 2.5v3.5M13.25 4.25h3.5M14.5 13v3M13 14.5h3" />,
  reverb: (
    <>
      {dot(4, 10, 1.1)}
      <path d="M7 7a4.2 4.2 0 0 1 0 6" />
      <path d="M10 4.5a7.5 7.5 0 0 1 0 11" strokeDasharray="2 1.8" />
      <path d="M13 2a11 11 0 0 1 0 16" strokeDasharray="1.2 2.2" />
    </>
  ),
  delay: <path d="M3 4v12M7.5 6v8M11.5 7.6v4.8M15 8.8v2.4M18 9.7v.6" />,
  headphones: (
    <>
      <path d="M4 12v-1.5a6 6 0 0 1 12 0V12" />
      <rect x="2.5" y="11" width="3.5" height="6" rx="1.2" />
      <rect x="14" y="11" width="3.5" height="6" rx="1.2" />
    </>
  ),
  speaker: (
    <>
      <rect x="4.5" y="2.5" width="11" height="15" rx="1.5" />
      <circle cx="10" cy="12" r="3" />
      <circle cx="10" cy="6" r="1.3" />
    </>
  ),
};

/** One icon of the set, by id (an unknown id draws the audio icon). */
export function TrackIconArt({ id, size = 22 }: { id: string; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      {ART[id] ?? ART.audio}
    </svg>
  );
}

/** The icon a track shows: the one chosen for it, else its name's or its kind's. */
export function TrackIcon({ track, size }: { track: Pick<Track, "kind" | "name" | "icon">; size?: number }) {
  return <TrackIconArt id={shownIcon(track)} size={size} />;
}

