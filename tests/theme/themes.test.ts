// Switching themes (src/theme/themes.ts): a theme lists only the CSS custom properties it changes, so
// applyTheme must clear what the last theme set and this one doesn't (the Light theme's darker
// record and input-monitor letters stayed in every other theme until a restart).
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";

// themes.ts imports the store, whose modules import each other without file extensions, as Vite
// allows. Node needs them spelled out, so this resolves "./store" to "./store.ts" (or .tsx).
const hook = `
export async function resolve(spec, ctx, next) {
  if (/^\\.\\.?\\//.test(spec) && !/\\.[cm]?[jt]sx?$/.test(spec) && ctx.parentURL?.includes("/src/")) {
    for (const ext of [".ts", ".tsx"]) {
      try {
        return await next(spec + ext, ctx);
      } catch {}
    }
  }
  return next(spec, ctx);
}`;
register("data:text/javascript," + encodeURIComponent(hook));

/** Just enough of `document.documentElement` for applyTheme: a style with custom properties. */
const props = new Map<string, string>();
const root = {
  style: {
    setProperty: (k: string, v: string) => props.set(k, v),
    removeProperty: (k: string) => props.delete(k),
    colorScheme: "",
  },
  dataset: {} as Record<string, string>,
};
Object.assign(globalThis, { document: { documentElement: root } });

const { THEMES, applyTheme, staleThemeKeys } = await import("../../src/theme/themes.ts");

test("after any theme, every other theme leaves exactly its own properties set", () => {
  for (const from of THEMES) {
    for (const to of THEMES) {
      props.clear();
      applyTheme(from.id, false);
      applyTheme(to.id, false);
      assert.deepEqual(new Map(props), new Map(Object.entries(to.css)), `${from.id} → ${to.id}`);
      assert.equal(root.dataset.theme, to.id);
    }
  }
});

test("Light's record and monitor letters don't stay in Studio", () => {
  props.clear();
  applyTheme("light", false);
  assert.ok(props.has("--rec-text") && props.has("--mon-text"), "Light sets them");
  applyTheme("studio", false);
  assert.ok(!props.has("--rec-text") && !props.has("--mon-text"));
  assert.deepEqual(staleThemeKeys("light"), []);
});
