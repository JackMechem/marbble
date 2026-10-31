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
