// The tempo map and time-signature changes (Logic's Tempo and Signature global tracks), for
// the UI. This mirrors crates/engine/src/model/tempo/; keep the two in step. lib/ holds the pure maths
// the whole app uses: this, musical time (time.ts) and decibels (decibels.ts).
//
// Timeline positions stay in samples. Musical time is counted in quarter notes: quarters(t) =
// ∫ bpm(t) / 60 dt, with each tempo event either holding until the next one or ramping
// linearly (in time) to its tempo. Signature changes start on bar lines, given by bar number,
// so bars are laid out in quarters whatever the tempo. Beats count in the signature's
// denominator, as on Logic's LCD.
//
// Bars are numbered as the user sees them (BarNumbering): the first full bar is `firstBar`, and
// a pickup before it is a partial bar numbered `firstBar - 1`, from timeline 0 to `firstBar`'s
// downbeat, holding the last beats of a bar in the first signature (its bar line is before 0).
// Every bar number in and out of the map is such a number; only `index` and `number` know the
// offset.
import type { Project } from "../engine/project";

export interface TempoEvent {
  /** Timeline position in samples. */
  pos: number;
  bpm: number;
  /** Ramps linearly to the next event's tempo instead of holding until it. */
  ramp: boolean;
}

export interface SigEvent {
  /** The bar the signature starts at, as numbered on the ruler. */
  bar: number;
  num: number;
  den: number;
}

export const MIN_BPM = 5;
export const MAX_BPM = 990;
export const SIG_DENOMINATORS = [2, 4, 8, 16];

/** How the bars are numbered: Project Settings › Pickup and First Bar Number. */
export interface BarNumbering {
  /** The pickup's length in quarter notes; 0 for none. At most a bar of the first signature. */
  pickup: number;
  /** The number of the first full bar (the pickup is the one before it). */
  firstBar: number;
}

export const MIN_FIRST_BAR = -999;
export const MAX_FIRST_BAR = 9999;

/** Slack for float noise when a position sits on a bar or grid line. */
const EPS = 1e-9;

function clampBpm(bpm: number): number {
  return Number.isFinite(bpm) ? Math.min(MAX_BPM, Math.max(MIN_BPM, bpm)) : 120;
}

/** A tempo for display: up to two decimals, none when whole. */
export function fmtBpm(bpm: number): string {
  const r = Math.round(bpm * 100) / 100;
  return Number.isInteger(r) ? String(r) : r.toFixed(2).replace(/0$/, "");
}

function clampSig(num: number, den: number): [number, number] {
  return [Math.min(32, Math.max(1, Math.round(num))), [1, 2, 4, 8, 16].includes(den) ? den : 4];
}

/** Sorted by position, one per position (the later wins), the first at 0 (`base` if missing). */
export function normalizeTempo(events: TempoEvent[], base: number): TempoEvent[] {
  const v = events.map((e, i) => ({ e: { pos: Math.max(0, Math.round(e.pos)), bpm: clampBpm(e.bpm), ramp: !!e.ramp }, i }));
  v.sort((a, b) => a.e.pos - b.e.pos || a.i - b.i);
  const out: TempoEvent[] = [];
  for (const { e } of v) {
    if (out.length && out[out.length - 1].pos === e.pos) out[out.length - 1] = e;
    else out.push(e);
  }
  if (!out.length || out[0].pos !== 0) out.unshift({ pos: 0, bpm: clampBpm(base), ramp: false });
  out[out.length - 1] = { ...out[out.length - 1], ramp: false };
  return out;
}

/** Sorted by bar, one per bar (the later wins), the first at `firstBar` (`base` if missing). */
export function normalizeSigs(events: SigEvent[], base: [number, number], firstBar = 1): SigEvent[] {
  const v = events.map((e, i) => {
    const [num, den] = clampSig(e.num, e.den);
    return { e: { bar: Math.max(firstBar, Math.round(e.bar)), num, den }, i };
  });
  v.sort((a, b) => a.e.bar - b.e.bar || a.i - b.i);
  const out: SigEvent[] = [];
  for (const { e } of v) {
    if (out.length && out[out.length - 1].bar === e.bar) out[out.length - 1] = e;
    else out.push(e);
  }
  if (!out.length || out[0].bar !== firstBar) {
    const [num, den] = clampSig(base[0], base[1]);
    out.unshift({ bar: firstBar, num, den });
  }
  return out;
}

/** Bar, beat, division (a quarter of a beat) and tick (1/240 division), all 1-based. */
export interface Bbt {
  bar: number;
  beat: number;
  div: number;
  tick: number;
}

/** A point of the beat grid: a bar line, a beat or a subdivision of one. */
export interface GridPoint {
  /** Timeline position in samples (not rounded). */
  pos: number;
  /** Position in quarters. */
  q: number;
  /** The bar, as numbered on the ruler. */
  bar: number;
  /** Index in the bar in steps of 1/div beat; 0 is the bar line. */
  sub: number;
  div: number;
}

/** One bar of the grid. */
export interface BarSpan {
  /** The bar, as numbered on the ruler. */
  bar: number;
  /** Where it starts on the timeline: the pickup bar from 0, where the timeline starts. */
  start: number;
  /** Its whole length in samples, the pickup's part before 0 included (for spacing labels and
   *  lines as in the bars after it). */
  len: number;
  end: number;
  startQ: number;
  num: number;
  den: number;
}

interface Seg {
  pos: number;
  /** Quarters elapsed at `pos`. */
  q: number;
  bpm: number;
  /** BPM gained per sample (0 for a step). */
  slope: number;
}

interface Meter {
  /** Index of its first bar, counted from the first full bar (0); the pickup's is -1. */
  bar: number;
  /** Quarters at its first bar line. */
  q: number;
  num: number;
  den: number;
  barQ: number;
  beatQ: number;
}

/** Index of the last item with `key(item) <= x` (0 when there is none). */
function lastAtOrBefore<T>(items: T[], x: number, key: (t: T) => number): number {
  let lo = 0;
  let hi = items.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (key(items[mid]) <= x) lo = mid + 1;
    else hi = mid;
  }
  return Math.max(0, lo - 1);
}

export class TempoMap {
  readonly sr: number;
  /** Normalized events: sorted, the first at 0 (or the first full bar). */
  readonly events: TempoEvent[];
  readonly sigs: SigEvent[];
  /** The number of the first full bar. */
  readonly firstBar: number;
  /** The pickup's length in quarters (0 without one). */
  readonly pickup: number;
  private segs: Seg[] = [];
  private meters: Meter[] = [];

  constructor(sampleRate: number, tempo: TempoEvent[], sigs: SigEvent[], baseBpm: number, baseSig: [number, number], bars?: BarNumbering) {
    this.sr = Math.max(1, sampleRate);
    this.events = normalizeTempo(tempo, baseBpm);
    let q = 0;
    this.events.forEach((e, i) => {
      const next = this.events[i + 1];
      const slope = next && e.ramp ? (next.bpm - e.bpm) / (next.pos - e.pos) : 0;
      this.segs.push({ pos: e.pos, q, bpm: e.bpm, slope });
      if (next) {
        const dt = next.pos - e.pos;
        q += (e.bpm * dt + 0.5 * slope * dt * dt) / (60 * this.sr);
      }
    });
    this.firstBar = Math.min(MAX_FIRST_BAR, Math.max(MIN_FIRST_BAR, Math.round(bars?.firstBar ?? 1)));
    this.sigs = normalizeSigs(sigs, baseSig, this.firstBar);
    const firstBarQ = (this.sigs[0].num * 4) / this.sigs[0].den;
    const pickup = bars?.pickup ?? 0;
    this.pickup = Number.isFinite(pickup) ? Math.min(firstBarQ, Math.max(0, pickup)) : 0;
    for (const s of this.sigs) {
      const bar = s.bar - this.firstBar;
      const prev = this.meters[this.meters.length - 1];
      // The first full bar starts after the pickup.
      const mq = prev ? prev.q + (bar - prev.bar) * prev.barQ : this.pickup;
      this.meters.push({ bar, q: mq, num: s.num, den: s.den, barQ: (s.num * 4) / s.den, beatQ: 4 / s.den });
    }
  }

  /** The bar number of bar `index` (counted from the first full bar, 0). */
  private number(index: number): number {
    return index + this.firstBar;
  }

  /** The index (counted from the first full bar, 0) of bar number `bar`. */
  private index(bar: number): number {
    return bar - this.firstBar;
  }

  static constant(sampleRate: number, bpm: number, num: number, den: number): TempoMap {
    return new TempoMap(sampleRate, [], [], bpm, [num, den]);
  }

  /** One tempo and one signature throughout. */
  get isConstant(): boolean {
    return this.segs.length === 1 && this.meters.length === 1;
  }

  private segAtSample(s: number): Seg {
    return this.segs[lastAtOrBefore(this.segs, s, (g) => g.pos)];
  }

  private segAtQ(q: number): Seg {
    return this.segs[lastAtOrBefore(this.segs, q, (g) => g.q)];
  }

  private meterAtQ(q: number): Meter {
    return this.meters[lastAtOrBefore(this.meters, q + EPS, (m) => m.q)];
  }

  private meterAtBar(index: number): Meter {
    return this.meters[lastAtOrBefore(this.meters, index, (m) => m.bar)];
  }

  /** Quarters elapsed at timeline sample `s`. Before 0 the first tempo carries on. */
  beatsAt(s: number): number {
    const g = this.segAtSample(s);
    const dt = s - g.pos;
    if (dt <= 0) return g.q + (g.bpm * dt) / (60 * this.sr);
    return g.q + (g.bpm * dt + 0.5 * g.slope * dt * dt) / (60 * this.sr);
  }

  /** The timeline sample (not rounded) where `q` quarters have elapsed. */
  sampleAt(q: number): number {
    const g = this.segAtQ(q);
    const c = (q - g.q) * 60 * this.sr;
    if (c <= 0 || g.slope === 0) return g.pos + c / g.bpm;
    // bpm·dt + slope·dt²/2 = c, solved in the form that stays accurate as the slope goes to 0.
    const d = Math.max(0, g.bpm * g.bpm + 2 * g.slope * c);
    return g.pos + (2 * c) / (g.bpm + Math.sqrt(d));
  }

  bpmAt(s: number): number {
    const g = this.segAtSample(s);
    return g.bpm + g.slope * Math.max(0, s - g.pos);
  }

  /** Index of the tempo event in effect at `s`. */
  eventIndexAt(s: number): number {
    return lastAtOrBefore(this.events, s, (e) => e.pos);
  }

  samplesPerQuarterAt(s: number): number {
    return (60 * this.sr) / this.bpmAt(s);
  }

  /** Samples per beat (of the signature's denominator) at `s`. */
  samplesPerBeatAt(s: number): number {
    return (this.samplesPerQuarterAt(s) * 4) / this.sigAt(s)[1];
  }

  /** Samples per bar at the tempo and signature at `s`, as if the tempo held. */
  samplesPerBarAt(s: number): number {
    const [num, den] = this.sigAt(s);
    return (this.samplesPerQuarterAt(s) * num * 4) / den;
  }

  /** The signature of `bar`. */
  sigAtBar(bar: number): [number, number] {
    const m = this.meterAtBar(this.index(bar));
    return [m.num, m.den];
  }

  /** Index of the signature change in effect at `bar`. */
  sigIndexAtBar(bar: number): number {
    return lastAtOrBefore(this.sigs, bar, (s) => s.bar);
  }

  sigAt(s: number): [number, number] {
    const m = this.meterAtQ(this.beatsAt(s));
    return [m.num, m.den];
  }

  /** The index of the bar holding `q` quarters, its start in quarters and its meter. */
  private barOfQ(q: number): { i: number; start: number; m: Meter } {
    const m = this.meterAtQ(q);
    const n = Math.floor((q - m.q) / m.barQ + EPS);
    return { i: m.bar + n, start: m.q + n * m.barQ, m };
  }

  /** Quarters at the bar line of `bar` (before 0 for the pickup: its downbeat). */
  barStartQ(bar: number): number {
    const i = this.index(bar);
    const m = this.meterAtBar(i);
    return m.q + (i - m.bar) * m.barQ;
  }

  /** Timeline sample (not rounded) of the bar line of `bar` (before 0 for the pickup). */
  barStart(bar: number): number {
    return this.sampleAt(this.barStartQ(bar));
  }

  /** Where `bar` starts on the timeline: its bar line, or 0 for the pickup. */
  barStartOnTimeline(bar: number): number {
    return Math.max(0, this.barStart(bar));
  }

  /** The bar holding `s` on the timeline in whole samples, as [start, length]: what a new
   *  bar-long region there covers (the pickup from 0). */
  barSpanAt(s: number): [number, number] {
    const bar = this.barAt(s);
    const start = Math.round(this.barStartOnTimeline(bar));
    return [start, Math.round(this.barStart(bar + 1)) - start];
  }

  /** The bar playing at `s`. */
  barAt(s: number): number {
    return this.number(this.barOfQ(this.beatsAt(s)).i);
  }

  /** Whether `bar` is one of every `n` bars counted from the first full bar (which rulers label
   *  when they can't label them all). */
  everyNthBar(bar: number, n: number): boolean {
    return (((this.index(bar) % n) + n) % n) === 0;
  }

  /** The bar playing at `s` with the fraction of it gone by (the first full bar's downbeat is
   *  `firstBar`, halfway through it `firstBar + 0.5`). */
  barNumberAt(s: number): number {
    const q = this.beatsAt(s);
    const { i, start, m } = this.barOfQ(q);
    return this.number(i) + Math.max(0, (q - start) / m.barQ);
  }

  /** Bar, beat, division and tick at `s`, counting signature changes. */
  bbtAt(s: number): Bbt {
    const q = this.beatsAt(s);
    const { i, start, m } = this.barOfQ(q);
    const beats = Math.max(0, (q - start) / m.beatQ);
    const beat = Math.min(Math.floor(beats), m.num - 1);
    const frac = Math.min(Math.max(beats - beat, 0), 1 - 1e-12);
    const div = Math.floor(frac * 4);
    const tick = Math.floor((frac * 4 - div) * 240);
    return { bar: this.number(i), beat: beat + 1, div: div + 1, tick: tick + 1 };
  }

  /** Grid point `sub` (of `div` a beat) of bar `index`. */
  private gridPoint(index: number, sub: number, div: number): GridPoint {
    const m = this.meterAtBar(index);
    const start = m.q + (index - m.bar) * m.barQ;
    const q = start + (sub * m.beatQ) / div;
    return { pos: this.sampleAt(q), q, bar: this.number(index), sub, div };
  }

  /** The first grid point (bar lines, beats, `div` steps a beat) at or after `q`. */
  gridAtOrAfter(q: number, div: number): GridPoint {
    div = Math.max(1, div);
    const { i, start, m } = this.barOfQ(q);
    const step = m.beatQ / div;
    const j = Math.max(0, Math.ceil((q - start) / step - EPS));
    return j >= m.num * div ? this.gridPoint(i + 1, 0, div) : this.gridPoint(i, j, div);
  }

  gridAfter(g: GridPoint): GridPoint {
    const i = this.index(g.bar);
    const { num } = this.meterAtBar(i);
    return g.sub + 1 >= num * g.div ? this.gridPoint(i + 1, 0, g.div) : this.gridPoint(i, g.sub + 1, g.div);
  }

  /** `q` snapped to a multiple of `step` quarters counted from its bar line, or to the next
   *  bar line when that is nearer (so odd meters line up, as in Logic). Never before the
   *  timeline's start (the pickup's bar line is before it) when `q` isn't. */
  snapQ(q: number, step: number): number {
    const { start, m } = this.barOfQ(q);
    const end = start + m.barQ;
    if (!(step > 0)) return q;
    const at = Math.min(end, start + Math.round((q - start) / step) * step);
    const snapped = end - q < Math.abs(q - at) ? end : at;
    return q >= 0 ? Math.max(0, snapped) : snapped;
  }

  /** The grid line at or before `q` (steps of `step` quarters from its bar line), never before
   *  the timeline's start when `q` isn't. */
  floorQ(q: number, step: number): number {
    const { start } = this.barOfQ(q);
    if (!(step > 0)) return q;
    const at = start + Math.floor((q - start) / step + 1e-6) * step;
    return q >= 0 ? Math.max(0, at) : at;
  }

  /** The first beat (or bar line) strictly after `s`. */
  nextBeat(s: number): number {
    const g = this.gridAtOrAfter(this.beatsAt(s), 1);
    return g.pos > s + 1e-6 ? g.pos : this.gridAfter(g).pos;
  }

  /** The last beat (or bar line) strictly before `s`. */
  prevBeat(s: number): number {
    const q = this.beatsAt(s);
    const { i, start, m } = this.barOfQ(q);
    const j = Math.ceil((q - start) / m.beatQ - EPS) - 1;
    if (j >= 0) return this.gridPoint(i, j, 1).pos;
    return this.gridPoint(i - 1, this.meterAtBar(i - 1).num - 1, 1).pos;
  }

  /** The first bar line strictly after `s`. */
  nextBar(s: number): number {
    return this.barStart(this.barAt(s) + 1);
  }

  /** The last bar line strictly before `s`. */
  prevBar(s: number): number {
    const bar = this.barAt(s);
    const start = this.barStart(bar);
    return start < s - 1e-6 ? start : this.barStart(bar - 1);
  }

  /** The bars that overlap `[from, to)`. */
  *bars(from: number, to: number): Generator<BarSpan> {
    for (let bar = this.barAt(from); ; bar++) {
      const startQ = this.barStartQ(bar);
      const start = this.sampleAt(startQ);
      if (start >= to) return;
      const [num, den] = this.sigAtBar(bar);
      const end = this.barStart(bar + 1);
      // The pickup bar shows from the timeline's start.
      yield { bar, start: start < 0 && end > 0 ? 0 : start, end, len: end - start, startQ, num, den };
    }
  }

  /** The grid points (`div` a beat) in `[from, to)`. */
  *grid(from: number, to: number, div: number): Generator<GridPoint> {
    let g = this.gridAtOrAfter(this.beatsAt(from), div);
    if (g.pos < from) g = this.gridAfter(g);
    while (g.pos < to) {
      yield g;
      g = this.gridAfter(g);
    }
  }

  /** Where `s` under this map lands under `other`, keeping its bar and beat. */
  remap(other: TempoMap, s: number): number {
    return other.sampleAt(this.beatsAt(s));
  }
}

/** Whether one tempo and signature hold throughout and the bars start at 0 (no pickup), so
 *  plain multiples of a bar's length find the bar lines. */
export function plainBars(map: TempoMap): boolean {
  return map.isConstant && map.pickup === 0;
}

/** How many bars apart a ruler labels bars `barPx` wide, so the labels don't collide. */
export function labelEvery(barPx: number): number {
  return barPx >= 34 ? 1 : barPx >= 16 ? 2 : barPx >= 8 ? 4 : 8;
}

/** The narrowest pickup (px) a ruler labels, so its number doesn't run into the next bar's. */
const PICKUP_LABEL_PX = 14;

/** Whether a ruler labels bar `b` when it labels every `every`th one (counted from the first
 *  full bar), at `pps` pixels a sample: the pickup whenever its label fits. */
export function labelsBar(map: TempoMap, b: BarSpan, every: number, pps: number): boolean {
  return b.bar < map.firstBar ? (b.end - b.start) * pps >= PICKUP_LABEL_PX : map.everyNthBar(b.bar, every);
}

/** Bar lines at and around a time. */
export interface BarLines {
  /** The bar line at or before `t`. */
  floor(t: number): number;
  /** The bar line at or after `t`. */
  ceil(t: number): number;
}

/** The map's bar lines on the timeline (samples). */
export function barLines(map: TempoMap): BarLines {
  return {
    floor: (t) => map.barStart(map.barAt(t)),
    ceil: (t) => {
      const b = map.barAt(t);
      const s = map.barStart(b);
      return s >= t - 1e-6 ? s : map.barStart(b + 1);
    },
  };
}

/** Musical time laid out at a fixed tempo: `toV` turns a timeline position into samples at
 *  `spq` samples a quarter (keeping its bar and beat), `fromV` turns it back. Grid maths that
 *  assumes one tempo (snapping, quantize, note lengths) runs on these positions. */
export interface Warp {
  toV(t: number): number;
  fromV(v: number): number;
  /** Bar lines in warped positions. */
  bars: BarLines;
}

export function musicalWarp(map: TempoMap, spq: number): Warp {
  const toV = (t: number) => map.beatsAt(t) * spq;
  const fromV = (v: number) => map.sampleAt(v / spq);
  return {
    toV,
    fromV,
    bars: {
      floor: (v) => map.barStartQ(map.barAt(fromV(v))) * spq,
      ceil: (v) => {
        const b = map.barAt(fromV(v));
        const s = map.barStartQ(b) * spq;
        return s >= v - 1e-6 ? s : map.barStartQ(b + 1) * spq;
      },
    },
  };
}

/** The project's bar numbering (projects from before it have neither field). */
export function barNumbering(p: Pick<Project, "pickup" | "first_bar">): BarNumbering {
  return { pickup: p.pickup ?? 0, firstBar: p.first_bar ?? 1 };
}

const cache = new WeakMap<Project, TempoMap>();

/** The project's tempo map, cached per snapshot (every edit brings a new project object). */
export function tempoMap(p: Project): TempoMap {
  let m = cache.get(p);
  if (!m) {
    m = new TempoMap(p.sample_rate, p.tempo_map ?? [], p.signatures ?? [], p.tempo, [p.sig_num, p.sig_den], barNumbering(p));
    cache.set(p, m);
  }
  return m;
}

/** Bar lines and beat lines between two timeline positions, for drawing grids. `beatsMinPx`
 *  is the beat spacing (pixels) below which beats are skipped; `subdiv` adds 1/n-beat lines
 *  when they are at least `subMinPx` apart. */
export function forEachGridLine(
  map: TempoMap,
  from: number,
  to: number,
  pps: number,
  opts: { beatsMinPx: number; subdiv?: number; subMinPx?: number },
  line: (pos: number, kind: "bar" | "beat" | "sub", bar: BarSpan) => void,
) {
  for (const b of map.bars(from, to)) {
    line(b.start, "bar", b);
    const beatQ = 4 / b.den;
    const beatPx = (b.len / b.num) * pps;
    if (beatPx < opts.beatsMinPx) continue;
    const sub = opts.subdiv && beatPx / opts.subdiv >= (opts.subMinPx ?? 0) ? opts.subdiv : 1;
    for (let k = 1; k < b.num * sub; k++) {
      const pos = map.sampleAt(b.startQ + (k * beatQ) / sub);
      if (pos >= to) break;
      line(pos, k % sub === 0 ? "beat" : "sub", b);
    }
  }
}
