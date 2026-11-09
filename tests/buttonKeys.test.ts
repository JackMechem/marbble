// Many of the app's buttons act as the mouse goes down (pop-ups, local menus, track switches), and
// a button pressed with Return or Space only gets a click. Such a button spreads `onPress`
// (keyNav.ts), which runs the same handler on a click with no pointer behind it. This reads every
// component and fails on a <button> with onMouseDown and no onClick (one taken out of the Tab
// order with tabIndex -1 is left alone), and checks onPress itself.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { onPress } from "../src/keyNav.ts";

// fileURLToPath, not .pathname: on Windows that would be "/D:/…", which fs can't open.
const SRC = fileURLToPath(new URL("../src/", import.meta.url));

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return sources(p);
    return f.endsWith(".tsx") ? [p] : [];
  });
}

/** Where the JSX tag opened at `start` ends: its `>`, past attribute strings and `{…}` expressions. */
function tagEnd(src: string, start: number): number {
  let depth = 0;
  let quote = "";
  for (let i = start; i < src.length; i++) {
    const c = src[i];
    if (quote) {
      if (c === "\\") i++;
      else if (c === quote) quote = "";
    } else if (c === '"' || c === "'" || (c === "`" && depth > 0)) quote = c;
    else if (c === "{") depth++;
    else if (c === "}") depth--;
    else if (c === ">" && depth === 0) return i;
  }
  return src.length;
}

/** The lines of buttons that only listen for the mouse going down. */
function mouseOnly(src: string): number[] {
  const out: number[] = [];
  for (const m of src.matchAll(/<button\b/g)) {
    const tag = src.slice(m.index, tagEnd(src, m.index + 7) + 1);
    if (/\bonMouseDown=/.test(tag) && !/\bonClick=/.test(tag) && !/tabIndex=\{-1\}/.test(tag)) out.push(src.slice(0, m.index).split("\n").length);
  }
  return out;
}

test("the checker finds buttons that only listen for the mouse", () => {
  assert.deepEqual(mouseOnly(`<button className="popup" onMouseDown={(e) => open(e.currentTarget)}>`), [1]);
  assert.deepEqual(mouseOnly(`<button className="popup" {...onPress((e) => open(e.currentTarget))}>`), []);
  assert.deepEqual(mouseOnly(`<button onMouseDown={(e) => e.preventDefault()} onClick={go}>`), []);
  assert.deepEqual(mouseOnly(`<button className="slot-arrow" tabIndex={-1} onMouseDown={menu}>`), []);
  assert.deepEqual(mouseOnly(`<div onMouseDown={drag}>`), []);
});

test("onPress runs the handler on the mouse going down and on Return or Space, not on a mouse click", () => {
  const seen: string[] = [];
  const p = onPress((e: { detail: number; why: string }) => seen.push(e.why));
  p.onMouseDown({ detail: 1, why: "down" });
  p.onClick({ detail: 1, why: "click" });
  p.onClick({ detail: 0, why: "key" });
  assert.deepEqual(seen, ["down", "key"]);
});

test("every button that acts as the mouse goes down also works from the keyboard", () => {
  const bad = sources(SRC).flatMap((p) => mouseOnly(readFileSync(p, "utf8")).map((line) => `${p.slice(SRC.length).replaceAll("\\", "/")}:${line}`));
  assert.deepEqual(bad, [], "spread {...onPress(handler)} (keyNav.ts) in place of onMouseDown={handler}");
});
