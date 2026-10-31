// The Compressor panel's model, mirroring crates/engine/src/fx/compressor.rs (keep them in step):
// parameter ids, the circuits and their fixed settings, and the steady-state curve the transfer graph
// draws. Pure and tested in tests/compressorModel.test.ts.

export const P = {
  circuit: 0,
  threshold: 1,
  ratio: 2,
  knee: 3,
  attack: 4,
  release: 5,
  autoRelease: 6,
  makeup: 7,
  autoGain: 8,
  detector: 9,
  distortion: 10,
  mix: 11,
  output: 12,
  link: 13,
  scMode: 14,
  scFreq: 15,
  listen: 16,
  lookahead: 17,
  fetRatio: 18,
  optoMode: 19,
  timeConstant: 20,
} as const;

// Not a TypeScript enum: Node can't load those without a transform, and the tests load this file.
export const Circuit = {
  Platinum: 0,
  Vca: 1,
  Fet: 2,
  Opto: 3,
  VariMu: 4,
} as const;
export type Circuit = (typeof Circuit)[keyof typeof Circuit];

export const CIRCUITS: { name: string; short: string; key: string; about: string }[] = [
  { name: "Platinum Digital", short: "Platinum", key: "platinum", about: "Clean, exact feedforward compression" },
  { name: "Studio VCA", short: "Studio VCA", key: "vca", about: "SSL / dbx-style VCA: punchy, stepped attack and release, Auto release" },
  { name: "Studio FET", short: "Studio FET", key: "fet", about: "1176-style FET feedback compressor: very fast, ratio buttons, All mode" },
  { name: "Classic Opto", short: "Classic Opto", key: "opto", about: "LA-2A-style optical cell: soft, program-dependent two-stage release" },
  { name: "Vintage Vari-Mu", short: "Vari-Mu", key: "varimu", about: "Fairchild-style variable-mu tube: very soft knee, time-constant positions" },
];

export const VCA_ATTACKS = [0.1, 0.3, 1, 3, 10, 30];
export const VCA_RELEASES = [100, 300, 600, 1200];
export const FET_ATTACK: [number, number] = [0.02, 0.8];
export const FET_RELEASE: [number, number] = [50, 1100];
/** (feedback slope S = R - 1, input-side knee dB) per FET ratio button. */
export const FET_CURVES: [number, number][] = [
  [3, 8],
  [7, 6],
  [11, 5],
  [19, 4],
  [19, 4],
];
export const ABI_SHIFT = 3;
const ABI_KNEE = 4;
const ABI_FOLD = 1.15;
const VARI_MU_WIDTH = 8;
export const VARI_MU_TC = [
  { attack: 0.2, release: "0.3 s" },
  { attack: 0.2, release: "0.8 s" },
  { attack: 0.4, release: "2 s" },
  { attack: 0.4, release: "5 s" },
  { attack: 0.4, release: "2–10 s" },
  { attack: 0.2, release: "0.3–25 s" },
];

function kneeRed(over: number, slope: number, knee: number) {
  if (2 * over <= -knee) return 0;
  if (knee > 0 && 2 * over < knee) {
    const t = over + knee / 2;
    return (slope * t * t) / (2 * knee);
  }
  return slope * over;
}

function kneeSlope(over: number, slope: number, knee: number) {
  if (2 * over <= -knee) return 0;
  if (knee > 0 && 2 * over < knee) return (slope * (over + knee / 2)) / knee;
  return slope;
}

function programRed(over: number, slope: number, w: number) {
  const z = over / w;
  const sp = z > 20 ? z : z < -20 ? Math.exp(z) : Math.log1p(Math.exp(z));
  return slope * w * sp;
}

/** Steady-state gain reduction (dB) for a detector level `x` dB — `static_reduction` in Rust. */
export function staticReduction(v: (id: number) => number, x: number): number {
  const circuit = Math.round(v(P.circuit));
  const thr = v(P.threshold);
  const ratio = Math.max(1, v(P.ratio));
  switch (circuit) {
    case Circuit.Fet: {
      const mode = Math.round(v(P.fetRatio));
      const [s, kIn] = FET_CURVES[mode] ?? FET_CURVES[0];
      const kn = (2 * kIn) / (s + 2);
      const abi = mode >= 4;
      const xo = x - (abi ? thr - ABI_SHIFT : thr);
      const extra = abi ? ABI_FOLD * Math.max(0, xo - ABI_KNEE) : 0;
      let c = 0;
      for (let i = 0; i < 30; i++) {
        const f = c - kneeRed(xo - c, s, kn) - extra;
        c -= f / (1 + kneeSlope(xo - c, s, kn));
      }
      return Math.max(0, c);
    }
    case Circuit.Opto: {
      const limit = v(P.optoMode) >= 0.5;
      return programRed(x - thr, limit ? 1 - 1 / 12 : 1 - 1 / 3, limit ? 3 : 5);
    }
    case Circuit.VariMu:
      return programRed(x - thr, 1 - 1 / ratio, VARI_MU_WIDTH);
    default:
      return kneeRed(x - thr, 1 - 1 / ratio, Math.max(0, v(P.knee)));
  }
}
