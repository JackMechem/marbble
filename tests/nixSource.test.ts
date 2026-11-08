// Guard for the Nix package: nix/package.nix builds from a fileset of what the build reads, so a
// file or folder the frontend build needs must be listed there, or the Nix build leaves it out
// (public/ once was, and the project chooser's logo was a broken image).
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = (p: string) => fileURLToPath(new URL(`../${p}`, import.meta.url));
const nix = readFileSync(root("nix/package.nix"), "utf8");
const listed = new Set([...nix.matchAll(/^\s*\.\.\/([\w.-]+)\s*$/gm)].map((m) => m[1]));

test("the Nix package's source has everything the frontend build reads", () => {
  // Vite's root (index.html, src/), its public/ folder copied into the build, and its config.
  const read = ["index.html", "src", "public", "vite.config.ts", "tsconfig.json", "package.json", "package-lock.json"];
  for (const p of read.filter((p) => existsSync(root(p)))) assert.ok(listed.has(p), `nix/package.nix's fileset lists ../${p}`);
});
