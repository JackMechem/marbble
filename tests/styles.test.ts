// Guard for src/styles/: the stylesheet is many files whose @import order is the cascade, and a
// file left out of index.css or a brace left open in one shows up only as wrong looks in the app.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const DIR = fileURLToPath(new URL("../src/styles/", import.meta.url));
const read = (f: string) => readFileSync(join(DIR, f), "utf8");
const css = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, "");
const imports = [...css(read("index.css")).matchAll(/@import\s+"\.\/([^"]+)";/g)].map((m) => m[1]);
const files = readdirSync(DIR).filter((f) => f.endsWith(".css") && f !== "index.css");

test("index.css imports every stylesheet exactly once", () => {
  assert.deepEqual([...imports].sort(), [...files].sort());
  assert.equal(new Set(imports).size, imports.length);
});

test("the theme tokens come first", () => {
  assert.equal(imports[0], "tokens.css");
  assert.match(css(read("tokens.css")), /^\s*:root\s*\{/);
});

test("only index.css imports, and each file's braces balance", () => {
  for (const f of files) {
    const src = css(read(f)).replace(/"[^"]*"/g, '""');
    assert.ok(!src.includes("@import"), `${f} has an @import; index.css keeps the order`);
    assert.equal(src.split("{").length, src.split("}").length, `${f} has unbalanced braces`);
  }
});

test("main.tsx is what loads the stylesheet, and nothing else loads CSS", () => {
  const SRC = fileURLToPath(new URL("../src/", import.meta.url));
  const sources = readdirSync(SRC, { recursive: true, encoding: "utf8" })
    .map((p) => p.replace(/\\/g, "/"))
    .filter((p) => /\.tsx?$/.test(p));
  const loaders = sources.filter((p) => /^\s*import\s+["'][^"']+\.css["']/m.test(readFileSync(join(SRC, p), "utf8")));
  assert.deepEqual(loaders, ["main.tsx"]);
  assert.match(readFileSync(join(SRC, "main.tsx"), "utf8"), /import "\.\/styles\/index\.css";/);
});

// ---- the design rules: colours from tokens, the type scale, the focus ring ------------------------

/** Raw hex colours allowed outside tokens.css, as "file: colour" with the reason. None so far: a
 *  colour that's the same in every theme (a piano key, the VU meter's face) still gets a token. */
const HEX_EXCEPTIONS: Record<string, string> = {};

test("colours come from tokens: no raw hex colour outside tokens.css", () => {
  const found: string[] = [];
  for (const f of files.filter((f) => f !== "tokens.css")) {
    for (const m of css(read(f)).matchAll(/#[0-9a-fA-F]{3,8}\b/g)) {
      const at = `${f}: ${m[0]}`;
      if (!(at in HEX_EXCEPTIONS)) found.push(at);
    }
  }
  assert.deepEqual(found, [], "use a token from tokens.css (and themes.ts) instead");
});

/** The type scale in tokens.css. */
const FONT_TOKENS = ["--fs-xs", "--fs-sm", "--fs-md", "--fs-lg", "--fs-xl", "--fs-xxl"];
/** Display numerals sized to their box (the LCD's digits, the Tuner's note) may set px sizes from here up. */
const DISPLAY_PX = 18;

test("font sizes come from the type scale, smallest text 11 px", () => {
  const tokens = css(read("tokens.css"));
  const px = (name: string) => Number(new RegExp(`${name}:\\s*([\\d.]+)px;`).exec(tokens)?.[1]);
  assert.deepEqual(FONT_TOKENS.map(px), [10, 11, 12, 13, 15, 17]);
  const bad: string[] = [];
  for (const f of files) {
    const src = css(read(f));
    for (const m of src.matchAll(/font-size:\s*([^;]+);/g)) {
      const v = m[1].trim();
      const lit = /^([\d.]+)px$/.exec(v);
      if (lit ? Number(lit[1]) < DISPLAY_PX : !FONT_TOKENS.some((t) => v === `var(${t})`)) bad.push(`${f}: font-size: ${v}`);
    }
    for (const m of src.matchAll(/\bfont:\s*([^;]+);/g)) {
      const v = m[1].trim();
      if (v !== "inherit" && !FONT_TOKENS.some((t) => v.startsWith(`var(${t})`))) bad.push(`${f}: font: ${v}`);
    }
  }
  assert.deepEqual(bad, []);
});

test("tokens.css holds Studio's values from themes.ts, so the first paint matches", () => {
  const themes = readFileSync(fileURLToPath(new URL("../src/themes.ts", import.meta.url)), "utf8");
  const studio = themes.slice(themes.indexOf("const studioCss"), themes.indexOf("const studioCanvas"));
  const ts = Object.fromEntries([...studio.matchAll(/"(--[\w-]+)":\s*"([^"]+)"/g)].map((m) => [m[1], m[2]]));
  const tokens = css(read("tokens.css"));
  const first = tokens.slice(0, tokens.indexOf("}"));
  const root = Object.fromEntries([...first.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2]]));
  const norm = (v: string) => v.replace(/\s+/g, "");
  assert.deepEqual(Object.keys(root).sort(), Object.keys(ts).sort());
  for (const k of Object.keys(ts)) assert.equal(norm(root[k]), norm(ts[k]), k);
});

test("keyboard focus shows a ring from the token, and only for the keyboard", () => {
  const base = css(read("base.css"));
  assert.match(base, /:focus-visible\s*\{\s*outline:\s*var\(--focus-ring\);/);
  assert.match(base, /html:not\(\[data-keyboard-focus\]\) :focus-visible/);
  assert.match(css(read("tokens.css")), /--focus-ring:/);
});

// ---- the Inspector -----------------------------------------------------------------------------

test("Inspector rows grow to fit their controls (a fixed height made the Region rows overlap)", () => {
  // A number field is 24 px tall; rows of a fixed height smaller than that drew each one over the
  // next row's label.
  for (const f of ["inspector.css", "inspector-regions.css", "region-params.css"]) {
    for (const m of css(read(f)).matchAll(/([^{}]*\.insp-row[^{}]*)\{([^}]*)\}/g)) {
      assert.ok(!/(^|[;\s])height\s*:/.test(m[2]), `${f}: "${m[1].trim()}" sets a fixed height; use min-height`);
    }
  }
  const field = /\.numfield\s*\{([^}]*)\}/.exec(css(read("form-controls.css")))![1];
  const fieldH = Number(/height:\s*(\d+)px/.exec(field)![1]);
  const row = /\.insp-row\.rp-row\s*\{([^}]*)\}/.exec(css(read("region-params.css")))![1];
  assert.ok(Number(/min-height:\s*(\d+)px/.exec(row)![1]) >= fieldH, "a parameter row holds a number field");
});
