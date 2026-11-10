// Every file dialog goes through src/project/fileDialog.ts, which checks first that GTK's file chooser
// can open (on Linux it ends the app when its settings schema is missing).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SRC = fileURLToPath(new URL("../../src/", import.meta.url));

test("only fileDialog.ts takes open and save from the dialog plug-in", () => {
  const files = readdirSync(SRC, { recursive: true, encoding: "utf8" }).filter((p) => /\.tsx?$/.test(p));
  assert.ok(files.length > 100);
  const direct = files.filter((f) => {
    const src = readFileSync(join(SRC, f), "utf8");
    return [...src.matchAll(/import\s*\{([^}]*)\}\s*from\s*["']@tauri-apps\/plugin-dialog["']/g)].some((m) =>
      m[1].split(",").some((p) => /^(open|save)\b/.test(p.trim())),
    );
  });
  assert.deepEqual(direct.map((f) => f.replace(/\\/g, "/")), ["project/fileDialog.ts"]);
});
