// The Step Sequencer's pure part: pattern edits, step values, names, where the playhead is in
// each row, and the miniature a pattern region shows in the Tracks area. The pattern itself
// (and the notes it plays) lives in crates/engine/src/pattern.rs; StepSequencer.tsx is the
// editor, which sends each edited pattern with `set_pattern`.
import type { Track } from "../../engine/project";
import type { Note, Pattern, PatternRow, PatternStep, Region } from "../../engine/regions";
import { scaleOf } from "../midiEdit.ts";
import type { TempoMap } from "../../lib/tempo";
import { regionEnd } from "../../tracks/regions/loops.ts";

/** Step rates as steps per whole note: 1/4 to 1/32, then their triplets (as pattern.rs). */
export const RATES = [4, 8, 16, 32, 6, 12, 24, 48] as const;
export const MAX_STEPS = 64;
export const MAX_ROWS = 64;
const DEFAULT_VELOCITY = 100;

export const isTriplet = (rate: number) => rate % 3 === 0;

/** "1/16", or "1/16T" for a triplet rate. */
export function rateLabel(rate: number): string {
  return isTriplet(rate) ? `1/${(rate * 2) / 3}T` : `1/${rate}`;
}

/** Quarter notes per step. */
export const stepQuarters = (rate: number) => 4 / Math.max(1, rate);

// ---- names ----------------------------------------------------------------------------------

const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** Logic's octave numbering: MIDI note 60 is C3, 36 is C1. */
export const noteName = (n: number) => `${NAMES[n % 12]}${Math.floor(n / 12) - 2}`;

/** The General MIDI drum map, keys 35–81. */
export const GM_DRUMS: Record<number, string> = {
  35: "Acoustic Bass Drum",
  36: "Kick",
  37: "Side Stick",
  38: "Snare",
  39: "Clap",
  40: "Electric Snare",
  41: "Low Floor Tom",
  42: "Closed Hi-Hat",
  43: "High Floor Tom",
  44: "Pedal Hi-Hat",
  45: "Low Tom",
  46: "Open Hi-Hat",
  47: "Low-Mid Tom",
  48: "Hi-Mid Tom",
  49: "Crash Cymbal 1",
  50: "High Tom",
  51: "Ride Cymbal 1",
  52: "Chinese Cymbal",
  53: "Ride Bell",
  54: "Tambourine",
  55: "Splash Cymbal",
  56: "Cowbell",
  57: "Crash Cymbal 2",
  58: "Vibraslap",
  59: "Ride Cymbal 2",
  60: "Hi Bongo",
  61: "Low Bongo",
  62: "Mute Hi Conga",
  63: "Open Hi Conga",
  64: "Low Conga",
  65: "High Timbale",
  66: "Low Timbale",
  67: "High Agogo",
  68: "Low Agogo",
  69: "Cabasa",
  70: "Maracas",
  71: "Short Whistle",
  72: "Long Whistle",
  73: "Short Guiro",
  74: "Long Guiro",
  75: "Claves",
  76: "Hi Wood Block",
  77: "Low Wood Block",
  78: "Mute Cuica",
  79: "Open Cuica",
  80: "Mute Triangle",
  81: "Open Triangle",
};

/** A row's name: on a kit, the drum it plays in the General MIDI map, else its note name. */
export const rowName = (pitch: number, kit = true) => (kit ? GM_DRUMS[pitch] : undefined) ?? noteName(pitch);

// ---- the rows of a new pattern --------------------------------------------------------------

/** The engine's kit for a new pattern (pattern.rs DEFAULT_KIT), from the kick up. */
export const DEFAULT_KIT = [36, 38, 39, 42, 46, 45, 48, 49];
/** The most rows a new pattern takes from the notes its track already plays. */
export const TEMPLATE_ROWS_MAX = 16;
/** Where a pitched instrument's scale rows start: C3, Logic's middle C. */
const SCALE_ROOT_OCTAVE = 60;

type KitInfo = Pick<Track, "name" | "regions"> & { instrument: { plugin_id: string; name: string } | null };

/** Whether a track plays a kit, so its rows are named for the drums of the General MIDI map: a
 *  sampler, or an instrument or track named like a kit. Anything else plays pitches. */
export function isKitTrack(track: Pick<KitInfo, "name" | "instrument">): boolean {
  const words = /drum|kit|perc|beat|groove|808|909/i;
  return track.instrument?.plugin_id === "builtin:sampler" || words.test(track.name) || words.test(track.instrument?.name ?? "");
}

/** The pitch class (0 = C) of a key's tonic, such as "E♭ major"; C when it can't be read. */
function keyRoot(key: string): number {
  const m = /^([A-G])([#♯b♭]?)/i.exec(key.trim());
  if (!m) return 0;
  const base = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 }[m[1].toUpperCase() as "C"]!;
  const acc = m[2] === "#" || m[2] === "♯" ? 1 : m[2] === "b" || m[2] === "♭" ? -1 : 0;
  return (base + acc + 12) % 12;
}

/**
 * The rows of a new pattern region on `track`, as Logic picks a template by instrument: the
 * pitches its regions already play (the most played, up to TEMPLATE_ROWS_MAX), else the default
 * kit for a kit, else an octave of the project's `key` from C3. A kit lists its rows from the
 * lowest pitch up, as the kit does; pitched rows go from the highest down, as in the Piano Roll.
 */
export function templateRows(track: KitInfo, key: string): number[] {
  const kit = isKitTrack(track);
  const uses = new Map<number, number>();
  const use = (pitch: number, n = 1) => uses.set(pitch, (uses.get(pitch) ?? 0) + n);
  for (const r of track.regions) {
    if (r.pattern) for (const row of r.pattern.rows) use(row.pitch, 1 + row.steps.filter((s) => s.on).length);
    else if (r.content.type === "midi") for (const n of r.content.notes) use(n.pitch);
  }
  let rows: number[];
  if (uses.size) {
    rows = [...uses.entries()]
      .sort((a, b) => b[1] - a[1] || a[0] - b[0])
      .slice(0, TEMPLATE_ROWS_MAX)
      .map(([p]) => p);
  } else if (kit) {
    return DEFAULT_KIT.slice();
  } else {
    const scale = scaleOf(key);
    const root = SCALE_ROOT_OCTAVE + keyRoot(key);
    rows = Array.from({ length: 13 }, (_, k) => root + k).filter((p) => scale.has(p % 12));
  }
  return rows.sort((a, b) => (kit ? a - b : b - a));
}

// ---- steps and rows -------------------------------------------------------------------------

/** A step with every value filled in. */
export interface FullStep {
  on: boolean;
  velocity: number;
  gate: number;
  tie: boolean;
}

export function stepAt(row: PatternRow, i: number): FullStep {
  const s = row.steps[i] ?? {};
  return { on: !!s.on, velocity: s.velocity ?? DEFAULT_VELOCITY, gate: s.gate ?? 100, tie: !!s.tie };
}

/** A step stored compactly: only the values that differ from the defaults. */
function compact(s: FullStep): PatternStep {
  const out: PatternStep = {};
  if (s.on) out.on = true;
  if (s.velocity !== DEFAULT_VELOCITY) out.velocity = s.velocity;
  if (s.gate !== 100) out.gate = s.gate;
  if (s.tie) out.tie = true;
  return out;
}

export const rowRate = (p: Pattern, row: PatternRow) => row.rate ?? p.rate;
export const rowLength = (p: Pattern, row: PatternRow) => Math.max(1, row.length ?? p.length);

/** How many step columns the editor shows: the longest row. */
export const columns = (p: Pattern) => Math.max(p.length, ...p.rows.map((r) => rowLength(p, r)));

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The pattern with step `i` of row `r` changed. */
export function setStep(p: Pattern, r: number, i: number, patch: Partial<FullStep>): Pattern {
  const row = p.rows[r];
  if (!row || i < 0 || i >= MAX_STEPS) return p;
  const s = { ...stepAt(row, i), ...patch };
  s.velocity = clamp(Math.round(s.velocity), 1, 127);
  s.gate = clamp(Math.round(s.gate), 1, 100);
  const steps = row.steps.slice();
  while (steps.length <= i) steps.push({});
  steps[i] = compact(s);
  return setRow(p, r, { steps });
}

export function setRow(p: Pattern, r: number, patch: Partial<PatternRow>): Pattern {
  if (!p.rows[r]) return p;
  return { ...p, rows: p.rows.map((row, k) => (k === r ? { ...row, ...patch } : row)) };
}

/** A pitch for a new row: on a kit, the first of the kit's usual pieces not in use, else the one
 *  above the last row's; on a pitched instrument, the first free note below the last row's. */
export function newRowPitch(p: Pattern, kit = true): number {
  const used = new Set(p.rows.map((r) => r.pitch));
  if (!kit) {
    const last = p.rows[p.rows.length - 1]?.pitch ?? SCALE_ROOT_OCTAVE + 1;
    for (let n = last - 1; n >= 0; n--) if (!used.has(n)) return n;
    for (let n = last + 1; n <= 127; n++) if (!used.has(n)) return n;
    return last;
  }
  const pieces = [36, 38, 39, 42, 46, 45, 48, 49, 51, 37, 44, 50, 41, 43, 47, 54, 56, 70];
  const free = pieces.find((k) => !used.has(k));
  if (free !== undefined) return free;
  return Math.min(127, (p.rows[p.rows.length - 1]?.pitch ?? 35) + 1);
}

/** Adds a row below row `after` (at the end when it is absent). */
export function addRow(p: Pattern, after?: number, pitch = newRowPitch(p, true)): Pattern {
  if (p.rows.length >= MAX_ROWS) return p;
  const rows = p.rows.slice();
  rows.splice(after === undefined ? rows.length : after + 1, 0, { pitch, steps: [] });
  return { ...p, rows };
}

export function removeRow(p: Pattern, r: number): Pattern {
  return { ...p, rows: p.rows.filter((_, k) => k !== r) };
}

/** Turns every step of a row (or of every row) off, keeping the values. */
export function clearSteps(p: Pattern, r?: number): Pattern {
  const off = ({ on: _on, tie: _tie, ...kept }: PatternStep): PatternStep => kept;
  return { ...p, rows: p.rows.map((row, k) => (r === undefined || k === r ? { ...row, steps: row.steps.map(off) } : row)) };
}

// ---- edit modes -----------------------------------------------------------------------------

/** What clicking and dragging over steps changes (Logic's edit mode selector). */
export type EditMode = "on" | "velocity" | "gate" | "tie";

export const EDIT_MODES: { id: EditMode; label: string; title: string }[] = [
  { id: "on", label: "Note On", title: "Click or drag over steps to turn them on or off" },
  { id: "velocity", label: "Velocity", title: "Drag the bars to set each step's velocity" },
  { id: "gate", label: "Gate", title: "Drag the bars to set each step's note length" },
  { id: "tie", label: "Tie", title: "Click or drag over steps to tie them to the step before" },
];

/** Whether a mode sets values with bars (the vertical position) or switches steps. */
export const isBarMode = (m: EditMode) => m === "velocity" || m === "gate";

/** A bar mode's value at height `frac` (0 = bottom, 1 = top) of a step. */
function barValue(m: EditMode, frac: number): number {
  const f = clamp(frac, 0, 1);
  return m === "velocity" ? Math.round(1 + f * 126) : Math.round(1 + f * 99);
}

/** How full a step's bar is drawn (0–1) in a bar mode. */
export function barFraction(m: EditMode, s: FullStep): number {
  return m === "velocity" ? (s.velocity - 1) / 126 : (s.gate - 1) / 99;
}

/**
 * A press on step `i` of row `r` starts a paint: what every step the drag passes gets. A
 * switch mode sets them all to the opposite of the pressed step (so a drag turns a run on or
 * off); a bar mode sets the value at the pointer's height and turns the step on.
 */
export function paintFor(m: EditMode, p: Pattern, r: number, i: number, frac: number): Partial<FullStep> {
  const s = stepAt(p.rows[r], i);
  if (m === "on") return { on: !s.on };
  if (m === "tie") return { tie: !s.tie };
  return m === "velocity" ? { on: true, velocity: barValue(m, frac) } : { on: true, gate: barValue(m, frac) };
}

// ---- the playhead ---------------------------------------------------------------------------

/**
 * The step of row `row` playing at timeline sample `pos`, or null outside the region (and
 * after an unlooped pattern's end). Every pass of a looping region starts the pattern over,
 * from the pattern's offset.
 */
export function playingStep(map: TempoMap, region: Region, p: Pattern, row: PatternRow, pos: number, regionEnd: number): number | null {
  if (pos < region.start || pos >= regionEnd || region.length <= 0) return null;
  const pass = region.start + Math.floor((pos - region.start) / region.length) * region.length;
  const q = map.beatsAt(pos) - map.beatsAt(pass) + Math.max(0, p.offset ?? 0);
  if (p.looped === false && q >= p.length * stepQuarters(p.rate)) return null;
  const k = Math.floor(q / stepQuarters(rowRate(p, row)) + 1e-9);
  return k % rowLength(p, row);
}

// ---- the miniature in the Tracks area --------------------------------------------------------

/**
 * Draws a pattern region's body as rows of dots, one row per pattern row from the top, a dot
 * where each note it plays starts. `notes` are the played notes (relative to the region
 * start), so swing, ties and the region's playback parameters show as they sound; transposed
 * notes still find their row.
 */
export function drawPatternMini(
  ctx: CanvasRenderingContext2D,
  p: Pattern,
  notes: Note[],
  transpose: number,
  x: number,
  top: number,
  h: number,
  pps: number,
  color: string,
  visX0: number,
  visX1: number,
) {
  const n = p.rows.length;
  if (!n) return;
  const rowH = h / n;
  const r = clamp(rowH * 0.32, 0.8, 2.6);
  const rowOf = new Map<number, number[]>();
  p.rows.forEach((row, k) => rowOf.set(row.pitch, [...(rowOf.get(row.pitch) ?? []), k]));
  ctx.fillStyle = color;
  // Faint lines along the rows, when there is room for them.
  const alpha = ctx.globalAlpha;
  ctx.globalAlpha = alpha * 0.18;
  for (let k = 0; k < n && rowH >= 4; k++) ctx.fillRect(visX0, Math.round(top + (k + 0.5) * rowH), visX1 - visX0, 1);
  ctx.globalAlpha = alpha;
  for (const note of notes) {
    const nx = x + note.start * pps;
    if (nx + r < visX0 || nx - r > visX1) continue;
    for (const k of rowOf.get(note.pitch - transpose) ?? []) {
      const cy = top + (k + 0.5) * rowH;
      if (r < 1.2) ctx.fillRect(nx, cy - r, Math.max(1, r * 2), r * 2);
      else {
        ctx.beginPath();
        ctx.arc(nx + r, cy, r, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }
}

/**
 * Where a new bar-long pattern region goes when it's made at the playhead: the bar holding `pos`,
 * or the first bar after it that no region on the track covers (a looped region counts to its
 * loop end), so it never lands on top of one. Returns [start, length] in samples.
 */
export function freePatternBar(map: Pick<TempoMap, "barAt" | "barStart">, regions: readonly Pick<Region, "start" | "length" | "loop_end">[], pos: number): [number, number] {
  const span = (bar: number): [number, number] => {
    const start = Math.round(map.barStart(bar));
    return [start, Math.round(map.barStart(bar + 1)) - start];
  };
  let bar = map.barAt(Math.max(0, pos));
  // Each step moves past a region, so this ends within regions.length + 1 steps.
  for (let guard = 0; guard <= regions.length; guard++) {
    const [start, length] = span(bar);
    const hit = regions.filter((r) => r.start < start + length && regionEnd(r) > start);
    if (!hit.length) return [start, length];
    const end = Math.max(...hit.map(regionEnd));
    // The bar holding the end of what's in the way; the next one if it ends on a bar line.
    const next = map.barAt(end);
    bar = Math.round(map.barStart(next)) >= end ? next : next + 1;
  }
  return span(bar);
}
