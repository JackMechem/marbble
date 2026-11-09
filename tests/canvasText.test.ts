// Canvas text on the type scale: canvasText.ts's sizes and font match the stylesheet's, and every
// canvas that sets a font goes through it (the ruler, regions and notes were drawn at 8-12 px
// literals, off the scale and in three different font stacks).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { canvasFont, readTypeScale, TYPE_SCALE_PX, UI_FONT } from "../src/canvasText.ts";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const read = (p: string) => readFileSync(join(SRC, p), "utf8");

test("the scale is read from its tokens, and a missing or odd one keeps its default", () => {
  const tokens: Record<string, string> = { "--fs-xs": " 9px", "--fs-sm": "12.5px", "--fs-md": "1em" };
  assert.deepEqual(readTypeScale((t) => tokens[t] ?? ""), { ...TYPE_SCALE_PX, xs: 9, sm: 12.5 });
});

test("the defaults are tokens.css's scale, and the font is the body's", () => {
  const tokens = read("styles/tokens.css");
  assert.deepEqual(readTypeScale((t) => new RegExp(`${t}:\\s*([^;]+);`).exec(tokens)?.[1] ?? ""), TYPE_SCALE_PX);
  const body = /\bfont:\s*var\(--fs-md\)\/[\d.]+\s+([^;]+);/.exec(read("styles/base.css"))?.[1];
  assert.equal(body, UI_FONT);
});

test("a canvas font is a weight, a scale size and the interface's font", () => {
  assert.equal(canvasFont("sm", 600), `600 11px ${UI_FONT}`);
  assert.equal(canvasFont("xs"), `400 10px ${UI_FONT}`);
});

/** Files that may set a canvas font without canvasFont, with the reason. */
const FONT_EXCEPTIONS: Record<string, string> = {
  "components/PathText.tsx": "measures text with the font an element already has",
};

test("every canvas font comes from canvasFont", () => {
  const bad: string[] = [];
  for (const p of readdirSync(SRC, { recursive: true, encoding: "utf8" }).map((p) => p.replace(/\\/g, "/"))) {
    if (!/\.tsx?$/.test(p) || p === "canvasText.ts" || p in FONT_EXCEPTIONS) continue;
    for (const m of read(p).matchAll(/\.font\s*=\s*([^;]+);/g)) {
      if (!m[1].trim().startsWith("canvasFont(")) bad.push(`${p}: ${m[0]}`);
    }
  }
  assert.deepEqual(bad, [], "set canvas text with canvasFont (src/canvasText.ts)");
});
