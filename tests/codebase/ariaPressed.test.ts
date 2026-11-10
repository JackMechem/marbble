// A toggle button is one the app lights with an "on" class while its setting is on (Catch, Snap,
// Mute, a power button, one of a row of choices). Assistive technology can't see the class, so
// each must also say it with aria-pressed, as the track switches do. This reads every component
// and fails on a <button> whose class turns "on" with no aria-pressed (or another state: the role
// and aria-selected of a tab, aria-checked, or a pop-up button's aria-haspopup or aria-expanded).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// fileURLToPath, not .pathname: on Windows that would be "/D:/…", which fs can't open.
const SRC = fileURLToPath(new URL("../../src/", import.meta.url));

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return sources(p);
    return f.endsWith(".tsx") ? [p] : [];
  });
}

/** Where the JSX tag opened at `start` ends (its `>`, past attribute strings and `{…}` expressions),
 *  or with `close`, where the expression whose `{` is at `start` does. */
function scanTo(src: string, start: number, close = false): number {
  let depth = 0;
  let quote = "";
  for (let i = start; i < src.length; i++) {
    const c = src[i];
    if (quote) {
      if (c === "\\") i++;
      else if (c === quote) quote = "";
    } else if (c === '"' || c === "'" || (c === "`" && depth > 0)) quote = c;
    else if (c === "{") depth++;
    else if (c === "}" && --depth === 0 && close) return i;
    else if (c === ">" && depth === 0 && !close) return i;
  }
  return src.length;
}

/** A string literal that is, or ends with, the class "on": `" on"`, `"on"`, `"tbtn on"`. */
const ON_CLASS = /\?\s*(["'])[^"'\n]*\bon\s*\1|:\s*(["'])[^"'\n]*\bon\s*\2|\$\{[^}]*\?\s*" on"/;

/** Same-file helpers that build a class with "on" in it (`const toggle = (on) => "x" + (on ? " on" : "")`). */
function onHelpers(src: string): string[] {
  return [...src.matchAll(/const\s+(\w+)\s*=\s*\([^)]*\)\s*(?::\s*\w+\s*)?=>\s*([^;\n]*)/g)].filter((m) => ON_CLASS.test(m[2])).map((m) => m[1]);
}

const STATED = /\baria-(pressed|checked|selected|haspopup|expanded)=|\brole=/;

/** The toggle buttons in a source that don't state whether they're on, by line. */
function unstated(src: string): number[] {
  const helpers = onHelpers(src);
  const out: number[] = [];
  for (const m of src.matchAll(/<button\b/g)) {
    const tag = src.slice(m.index, scanTo(src, m.index + 7) + 1);
    const at = tag.indexOf("className={") + 10;
    const cls = at < 10 ? "" : tag.slice(at, scanTo(tag, at, true));
    const toggles = ON_CLASS.test(cls) || helpers.some((h) => new RegExp(`\\b${h}\\(`).test(cls));
    if (toggles && !STATED.test(tag)) out.push(src.slice(0, m.index).split("\n").length);
  }
  return out;
}

test("the checker finds toggle buttons that don't say they're pressed", () => {
  assert.deepEqual(unstated(`<button className={"lb-btn" + (snap ? " on" : "")} onClick={flip}>`), [1]);
  assert.deepEqual(unstated(`<button className={"lb-btn" + (snap ? " on" : "")} aria-pressed={snap} onClick={flip}>`), []);
  assert.deepEqual(unstated(`<button\n  key={v}\n  className={v === cur ? "on" : ""}\n  onClick={() => set(v)}\n>`), [1]);
  assert.deepEqual(unstated(`<button className={bypass ? "" : "on"} title="x">`), [1]);
  assert.deepEqual(unstated(`<button className={\`tuner-mute\${muted ? " on" : ""}\`} onClick={go}>`), [1]);
  assert.deepEqual(unstated(`const toggle = (on: boolean) => "si-btn" + (on ? " on" : "");\n<button className={toggle(x)} onClick={go}>`), [2]);
  assert.deepEqual(unstated(`const toggle = (on: boolean) => "si-btn" + (on ? " on" : "");\n<button className={toggle(x)} aria-pressed={x} onClick={go}>`), []);
  // Tabs, menu items and pop-up buttons state it their own way; buttons that never light don't need it.
  assert.deepEqual(unstated(`<button role="tab" aria-selected={on} className={"fx-tab" + (on ? " on" : "")}>`), []);
  assert.deepEqual(unstated(`<button className={"cb-more" + (any ? " on" : "")} aria-haspopup="menu">`), []);
  assert.deepEqual(unstated(`<button className="tp-btn" onClick={() => go(a ? "on" : "off")}>`), []);
  assert.deepEqual(unstated(`<button className={"slot" + (open ? " editing" : "")}>`), []);
});

test("every toggle button says whether it's on with aria-pressed", () => {
  const bad = sources(SRC).flatMap((p) => unstated(readFileSync(p, "utf8")).map((line) => `${p.slice(SRC.length).replaceAll("\\", "/")}:${line}`));
  assert.deepEqual(bad, [], "add aria-pressed={…} with the same condition that adds the on class");
});
