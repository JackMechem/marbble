// Checks the built site: every internal link and asset, under the base path, resolves to a file,
// every #fragment to an id on that page, and the landing page keeps to the naming rule. The docs
// generator catches broken links in the Markdown with their source line; this catches the rest.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { BASE } from "../docs.config.mjs";

const dist = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../dist");
const htmlFiles = fs.readdirSync(dist, { recursive: true }).filter((f) => f.endsWith(".html"));
const errors = [];
const idCache = new Map();

function idsOf(file) {
  if (!idCache.has(file)) {
    const html = fs.readFileSync(file, "utf8");
    idCache.set(file, new Set([...html.matchAll(/\s(?:id|name)=["']?([^"'\s>]+)/g)].map((m) => m[1])));
  }
  return idCache.get(file);
}

function targetFile(pathname) {
  const local = path.join(dist, decodeURIComponent(pathname.slice(BASE.length)));
  if (fs.existsSync(local) && fs.statSync(local).isFile()) return local;
  const index = path.join(local, "index.html");
  return fs.existsSync(index) ? index : null;
}

let checked = 0;
for (const rel of htmlFiles) {
  const file = path.join(dist, rel);
  const html = fs.readFileSync(file, "utf8").replace(/<script\b[\s\S]*?<\/script>/g, "");
  const url = `/${rel.replace(/index\.html$/, "")}`;
  for (const m of html.matchAll(/\s(?:href|src|poster)=["']([^"']+)["']/g)) {
    const raw = m[1].replace(/&amp;/g, "&");
    if (/^(https?:|mailto:|tel:|data:|javascript:|\/\/)/.test(raw)) continue;
    checked++;
    const resolved = new URL(raw, `http://x${BASE}${url}`);
    const where = `${rel}: ${raw}`;
    if (resolved.pathname !== BASE && !resolved.pathname.startsWith(`${BASE}/`)) {
      errors.push(`${where}: outside the base path ${BASE}`);
      continue;
    }
    const target = targetFile(resolved.pathname);
    if (!target) {
      errors.push(`${where}: no such file`);
      continue;
    }
    const fragment = decodeURIComponent(resolved.hash.slice(1));
    if (fragment && fragment !== "_top" && target.endsWith(".html") && !idsOf(target).has(fragment)) {
      errors.push(`${where}: no #${fragment} on that page`);
    }
  }
}

// Instruments' names stay our own on the landing page; the docs may say what a model follows.
const landing = fs.readFileSync(path.join(dist, "index.html"), "utf8").replace(/<script\b[\s\S]*?<\/script>/g, "").replace(/<style\b[\s\S]*?<\/style>/g, "");
const banned = /\b(minimoog|moog|juno|roland|prophet|sequential circuits|tb-?303|dx-?7|yamaha|serum|xfer)\b/i;
const hit = landing.replace(/<[^>]+>/g, " ").match(banned);
if (hit) errors.push(`index.html: "${hit[0]}" is a trademarked synth name; it belongs in the docs, not the landing page`);

if (errors.length) {
  console.error(`Link check failed, ${errors.length} problem(s):`);
  for (const e of errors) console.error(`  ${e}`);
  process.exit(1);
}
console.log(`Link check passed: ${checked} internal links and assets across ${htmlFiles.length} pages.`);
