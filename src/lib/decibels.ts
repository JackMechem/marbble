// Levels in decibels: the fader's taper (like Logic's: +6 dB at the top, -∞ at the bottom), gain
// to dB, and dB as the strips show it.

export function dbToFader(db: number): number {
  // Fader travel: 0..1 maps roughly like Logic's taper (+6 dB at top, -inf at bottom).
  if (db <= -96) return 0;
  if (db >= 6) return 1;
  if (db >= -12) return 0.62 + ((db + 12) / 18) * 0.38;
  if (db >= -40) return 0.22 + ((db + 40) / 28) * 0.4;
  return ((db + 96) / 56) * 0.22;
}

export function faderToDb(f: number): number {
  if (f <= 0.001) return -96;
  if (f >= 0.62) return -12 + ((f - 0.62) / 0.38) * 18;
  if (f >= 0.22) return -40 + ((f - 0.22) / 0.4) * 28;
  return -96 + (f / 0.22) * 56;
}

export function formatDb(db: number): string {
  if (db <= -96) return "-∞";
  const r = Math.round(db * 10) / 10;
  return (r > 0 ? "+" : "") + r.toFixed(1);
}

export function gainToDb(g: number): number {
  return g <= 0.0000158 ? -96 : 20 * Math.log10(g);
}
