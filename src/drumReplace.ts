// Drum Replacement and Doubling (Logic's Track › Drum Replacement/Doubling): the sheet's
// settings, the hits its threshold keeps, and the markers the Tracks area draws for them while
// the sheet is open. The engine finds every candidate hit once (crates/engine/src/drum_replace.rs);
// the threshold chooses among them here, so the markers follow the slider at once. Pure but for
// the small store of what's shown, so tests can import it.

/** The engine's DrumKind, with its General MIDI note and name. */
export type DrumKind = "kick" | "snare" | "tom_high" | "tom_mid" | "tom_low" | "hi_hat";
export type DrumMode = "replace" | "double";
export type DrumTiming = "attack" | "peak";

export const DRUM_KINDS: { id: DrumKind; name: string; note: number }[] = [
  { id: "kick", name: "Kick", note: 36 },
  { id: "snare", name: "Snare", note: 38 },
  { id: "tom_high", name: "Tom High", note: 50 },
  { id: "tom_mid", name: "Tom Mid", note: 47 },
  { id: "tom_low", name: "Tom Low", note: 43 },
  { id: "hi_hat", name: "Hi-Hat", note: 42 },
];

/** The engine's DrumSettings. */
export interface DrumSettings {
  instrument: DrumKind;
  mode: DrumMode;
  /** dBFS a hit's level must reach. */
  threshold_db: number;
  timing: DrumTiming;
  /** Moves every note, in ms. */
  offset_ms: number;
}

export const DRUM_DEFAULTS: DrumSettings = { instrument: "kick", mode: "replace", threshold_db: -30, timing: "attack", offset_ms: 0 };

/** A candidate hit: timeline frames of its attack and its peak, and its linear level. */
export interface DrumHit {
  pos: number;
  peak: number;
  level: number;
}

export const toDb = (v: number) => 20 * Math.log10(Math.max(v, 1e-6));

/** The hits at or above the threshold (the engine's `kept`). */
export function keptHits(hits: DrumHit[], thresholdDb: number): DrumHit[] {
  const thr = 10 ** (thresholdDb / 20);
  return hits.filter((h) => h.level >= thr);
}

/** "12 hits, the loudest at −3.2 dB", or why there are none. */
export function hitSummary(all: DrumHit[], kept: DrumHit[]): string {
  if (!all.length) return "No hits found on the track";
  if (!kept.length) return `No hits reach the threshold (${all.length} below it)`;
  const loudest = kept.reduce((m, h) => Math.max(m, h.level), 0);
  const left = all.length - kept.length;
  return `${kept.length} hit${kept.length === 1 ? "" : "s"}, the loudest at ${toDb(loudest).toFixed(1)} dB${left ? `; ${left} quieter left out` : ""}`;
}

/** Where a note goes for a hit, as the engine places it (before it's kept inside the region). */
export function notePos(h: DrumHit, s: DrumSettings, rate: number): number {
  return (s.timing === "attack" ? h.pos : h.peak) + Math.round((s.offset_ms * rate) / 1000);
}

// ---- the markers shown while the sheet is open ----------------------------------------------

/** The track the markers are on and where they are (timeline frames). */
export interface DrumMarks {
  track: number;
  at: number[];
}

let marks: DrumMarks | null = null;
const subs = new Set<() => void>();

export function drumMarks(): DrumMarks | null {
  return marks;
}

export function setDrumMarks(m: DrumMarks | null) {
  marks = m;
  subs.forEach((f) => f());
}

export function onDrumMarks(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}
