// The landing page's drawings, computed at build time so they work without JavaScript. Nothing here
// is a capture of the app: they're pictures of the domain, made from ordinary audio arithmetic.

/** Small seeded generator: the same page on every build. */
function mulberry(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export const BARS = 10;
/** The bar (0-based) where the last note starts and rings out. */
export const LAST_BAR = 8;

/**
 * The hero's phrase: eight bars of a bass line, then one long note left to ring.
 * Returns peak values per column in 0..1.
 */
export function phrase(columns = 720) {
  const rand = mulberry(1877);
  const pattern = [
    [0, 1], [0.5, 0.45], [0.75, 0.6], [1.5, 0.55], [2, 0.85], [2.75, 0.5], [3.25, 0.65],
  ];
  const notes = [];
  for (let bar = 0; bar < LAST_BAR; bar++) {
    for (const [beat, vel] of pattern) {
      const v = vel * (0.72 + 0.28 * Math.min(1, (bar + 1) / 5)) * (0.9 + rand() * 0.15);
      if (bar % 4 === 3 && beat > 2.5 && rand() < 0.5) continue;
      notes.push({ at: bar + beat / 4, v, decay: 0.05 + rand() * 0.05 });
    }
  }
  const peaks = new Array(columns);
  const texture = [];
  let smooth = 0.5;
  for (let i = 0; i < columns; i++) {
    smooth += (rand() - 0.5) * 0.35;
    smooth = Math.min(1, Math.max(0, smooth));
    texture.push(0.55 + 0.3 * smooth + 0.3 * rand());
  }
  for (let i = 0; i < columns; i++) {
    const t = (i / columns) * BARS;
    let level = 0;
    for (const n of notes) {
      if (t < n.at) continue;
      level = Math.max(level, n.v * Math.exp(-(t - n.at) / n.decay));
    }
    if (t >= LAST_BAR) {
      // The last note: a quick swell, then a long sustain with a slow beat in it, then away.
      const d = t - LAST_BAR;
      const swell = Math.min(1, d / 0.05);
      const fade = t > 9.55 ? Math.max(0, 1 - (t - 9.55) / 0.42) ** 1.6 : 1;
      level = (0.82 + 0.1 * Math.sin(d * 13)) * swell * fade * (1 - d * 0.1);
    }
    peaks[i] = Math.min(1, level * texture[i]);
  }
  return peaks;
}

/** A path of vertical strokes for the peaks, mirrored about the centre of a viewBox. */
export function wavePath(peaks, width, height) {
  const mid = height / 2;
  const dx = width / peaks.length;
  let d = "";
  peaks.forEach((p, i) => {
    const h = Math.max(0.6, p * mid * 0.96);
    d += `M${(i * dx).toFixed(1)} ${(mid - h).toFixed(1)}V${(mid + h).toFixed(1)}`;
  });
  return d;
}

/** A coarse copy of the peaks for the meters to read as the playhead passes. */
export function coarse(peaks, n = 200) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const a = Math.floor((i / n) * peaks.length);
    const b = Math.floor(((i + 1) / n) * peaks.length);
    out.push(Math.round(Math.max(...peaks.slice(a, b)) * 99));
  }
  return out;
}

const trace = (fn, w, h, n = 180) => {
  const pts = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const y = Math.max(-1, Math.min(1, fn(t)));
    pts.push(`${(t * w).toFixed(1)} ${(h / 2 - (y * h * 0.42)).toFixed(1)}`);
  }
  return `M${pts.join("L")}`;
};

const saw = (x) => 2 * (x - Math.floor(x + 0.5));
const pulse = (x, w = 0.5) => (x - Math.floor(x) < w ? 1 : -1);
const TAU = Math.PI * 2;

/**
 * One oscilloscope trace per synth, from the textbook waveform of its kind of engine. Each is a
 * small sum or function, not a recording: they show what kind of sound each makes, not its sound.
 */
export function scopes(w = 220, h = 72) {
  return {
    // Three saws slightly apart, beating.
    tre: [trace((t) => (saw(t * 3) + saw(t * 3.03 + 0.2) + saw(t * 2.97 + 0.5)) / 2.4, w, h, 300)],
    // A pulse and its sub an octave down, with the chorus's slow offset.
    sei: [trace((t) => 0.6 * pulse(t * 4, 0.42) * (0.92 + 0.08 * Math.sin(t * 9)) + 0.35 * pulse(t * 2), w, h, 320)],
    // Hard sync: oscillator A restarting on B's period.
    cinque: [trace((t) => saw(((t * 2.5) % 1) * (1.6 + 2.4 * t)) * 0.9, w, h, 320)],
    // A saw through a resonant low-pass: ringing after each edge, dying as the envelope closes.
    uno: [
      trace((t) => {
        const phase = (t * 3) % 1;
        const env = Math.exp(-phase * 3.2);
        return (saw(t * 3) * 0.45 + Math.sin(phase * TAU * (5 + 8 * env)) * env * 0.7) * (0.55 + 0.45 * env);
      }, w, h, 320),
    ],
    // Two-operator frequency modulation with a falling index.
    sedici: [trace((t) => Math.sin(TAU * t * 4 + (5.5 * (1 - t) + 0.4) * Math.sin(TAU * t * 12)), w, h, 360)],
    // A table's frames, stacked: harmonics rising as the position moves.
    onda: [0, 1, 2, 3, 4].map((k) =>
      trace((t) => {
        const f = k / 4;
        let y = 0;
        for (let n = 1; n <= 1 + k * 3; n++) y += Math.sin(TAU * t * 2 * n) / n ** (1.6 - f * 0.7);
        return y * (0.72 - f * 0.08) - 0.55 + k * 0.28;
      }, w, h, 200),
    ),
  };
}

/** An EQ curve for the signal path: a low shelf, a mid dip, an air lift. */
export function eqCurve(w = 240, h = 64) {
  const bell = (x, c, g, q) => g * Math.exp(-(((x - c) * q) ** 2));
  const pts = [];
  for (let i = 0; i <= 120; i++) {
    const x = i / 120;
    const y = 0.5 / (1 + Math.exp((x - 0.14) * 28)) + bell(x, 0.42, -0.42, 7) + bell(x, 0.66, 0.2, 9) + 0.52 / (1 + Math.exp((0.9 - x) * 22));
    pts.push(`${(x * w).toFixed(1)} ${(h / 2 - y * h * 0.46).toFixed(1)}`);
  }
  return `M${pts.join("L")}`;
}

/** Sampler zones across a keyboard: start key, end key (inclusive), velocity low and high (0..1), held. */
export const ZONES = [
  { a: 0, b: 6, lo: 0, hi: 0.5 },
  { a: 0, b: 6, lo: 0.5, hi: 1 },
  { a: 7, b: 13, lo: 0, hi: 0.5 },
  { a: 7, b: 13, lo: 0.5, hi: 1, on: true },
  { a: 14, b: 21, lo: 0, hi: 0.33 },
  { a: 14, b: 21, lo: 0.33, hi: 0.66 },
  { a: 14, b: 21, lo: 0.66, hi: 1 },
  { a: 22, b: 28, lo: 0, hi: 1 },
  { a: 29, b: 36, lo: 0, hi: 0.5 },
  { a: 29, b: 36, lo: 0.5, hi: 1 },
];
