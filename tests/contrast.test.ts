// Guard for the themes' secondary text: --dim and --faint label things (strip sections, empty
// slots, toolbar captions, a tab's track name), so in every theme they keep a WCAG contrast of at
// least 4.5:1 on each surface text sits on. themes.ts is read as text (it imports Tauri), each
// theme's map built as it is at run time: the theme it spreads, then its own values.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const src = readFileSync(fileURLToPath(new URL("../src/themes.ts", import.meta.url)), "utf8");
const themes: Record<string, Record<string, string>> = {};
for (const m of src.matchAll(/const (\w+)Css: Record<string, string> = \{([\s\S]*?)\n\};/g)) {
  const spread = /\.\.\.(\w+)Css/.exec(m[2]);
  const own = Object.fromEntries([...m[2].matchAll(/"(--[\w-]+)":\s*"([^"]+)"/g)].map((x) => [x[1], x[2]]));
  themes[m[1]] = { ...(spread ? themes[spread[1]] : {}), ...own };
}

function rgb(hex: string): number[] {
  const h = hex.replace("#", "");
  assert.match(h, /^[0-9a-f]{6}$/i, `${hex} is a plain #rrggbb colour`);
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
}

/** WCAG 2 relative luminance and contrast ratio. */
function contrast(a: string, b: string): number {
  const lum = (hex: string) => {
    const [r, g, b] = rgb(hex).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** The surfaces secondary text is drawn on: panes, bars, strips, sheets, fields and slots. (Button
 *  faces have their own text colour, --btn-text.) */
const SURFACES = ["--bg", "--panel", "--panel-2", "--panel-3", "--local-bar", "--strip", "--strip-sel", "--strip-name", "--sheet-bg", "--sheet-inner", "--header", "--headers-bg", "--headers-top", "--status-bg", "--field", "--slot-empty", "--readout"];

test("every theme is found, with its text tokens", () => {
  assert.deepEqual(Object.keys(themes), ["studio", "flat", "midnight", "light", "contrast"]);
  for (const [id, t] of Object.entries(themes)) for (const k of ["--text", "--dim", "--faint", ...SURFACES]) assert.ok(t[k], `${id} has ${k}`);
});

test("secondary text is at least 4.5:1 on every surface, in every theme", () => {
  const low: string[] = [];
  for (const [id, t] of Object.entries(themes)) {
    for (const fg of ["--text", "--dim", "--faint"]) {
      for (const bg of SURFACES) {
        const c = contrast(t[fg], t[bg]);
        if (c < 4.5) low.push(`${id}: ${fg} on ${bg} is ${c.toFixed(2)}:1`);
      }
    }
    const lcd = contrast(t["--lcd-label"], t["--lcd-bg"]);
    if (lcd < 4.5) low.push(`${id}: --lcd-label on --lcd-bg is ${lcd.toFixed(2)}:1`);
  }
  assert.deepEqual(low, []);
});

test("text, dim and faint stay in that order, so the hierarchy still reads", () => {
  for (const [id, t] of Object.entries(themes)) {
    for (const bg of SURFACES) {
      const [text, dim, faint] = ["--text", "--dim", "--faint"].map((k) => contrast(t[k], t[bg]));
      assert.ok(text > dim && dim > faint, `${id} on ${bg}: ${text.toFixed(2)} > ${dim.toFixed(2)} > ${faint.toFixed(2)}`);
    }
  }
});
