// Splits the repository's Markdown into Starlight pages, makes the sidebar and rewrites every
// link. Runs from astro.config.mjs, so the pages can't drift from their source and are never
// committed. A link that doesn't resolve stops the build, with a list of them all.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import GithubSlugger from "github-slugger";
import { BASE, BRANCH, DOCS, LONG_PAGE_LINES, REPO_URL } from "../docs.config.mjs";

const siteDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoDir = path.resolve(siteDir, "..");
const outDir = path.join(siteDir, "src/content/docs");
const genDir = path.join(siteDir, "src/generated");

/** A heading's text as a slugger sees it: rendered, without its Markdown. */
export function plainHeading(text) {
  return text
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]/g, "$1")
    .replace(/[`*]/g, "")
    .replace(/\s+$/, "");
}

/** Every heading in a Markdown source, skipping fenced code, with its GitHub anchor. */
function headingsOf(lines) {
  const slugger = new GithubSlugger();
  const out = [];
  let fence = null;
  lines.forEach((line, i) => {
    const f = line.match(/^(\s*)(```+|~~~+)/);
    if (f) {
      if (!fence) fence = f[2][0];
      else if (f[2][0] === fence) fence = null;
      return;
    }
    if (fence) return;
    const m = line.match(/^(#{1,6})\s+(.*)$/);
    if (!m) return;
    const text = plainHeading(m[2]);
    out.push({ line: i, level: m[1].length, text, raw: m[2], anchor: slugger.slug(text) });
  });
  return out;
}

const stripNumber = (text) => text.replace(/^\d+\.\s+/, "");
const keyOf = (text) => new GithubSlugger().slug(stripNumber(text));

/** Drops a hand-kept "Contents:" list from a document's opening: the sidebar and overview replace it. */
function dropContents(lines) {
  const start = lines.findIndex((l) => /^Contents:\s*$/.test(l));
  if (start < 0) return lines;
  let end = start + 1;
  while (end < lines.length) {
    const l = lines[end];
    const next = lines[end + 1] ?? "";
    if (l.trim() === "" && !/^(\s+|\d+\.\s|-\s)/.test(next)) break;
    end++;
  }
  return lines.slice(0, start).concat(lines.slice(end));
}

const trimBlank = (lines) => {
  let a = 0;
  let b = lines.length;
  const blank = (s) => s.trim() === "" || /^---+\s*$/.test(s);
  while (a < b && blank(lines[a])) a++;
  while (b > a && blank(lines[b - 1])) b--;
  return lines.slice(a, b);
};

/** The first sentence or two of a page, for its description in search results and link previews. */
function describe(lines, fallback) {
  let fence = false;
  for (const line of lines) {
    if (/^\s*(```|~~~)/.test(line)) fence = !fence;
    if (fence || !line.trim() || /^[#|<>-]/.test(line.trim()) || /^!\[/.test(line.trim()) || /^\d+\./.test(line)) continue;
    const text = line
      .replace(/\]\([^)]*\)/g, "]")
      .replace(/[\[\]*`]/g, "")
      .trim();
    if (text.length > 30) return text.length > 155 ? `${text.slice(0, 152).replace(/\s+\S*$/, "")}…` : text;
  }
  return fallback;
}

const yaml = (s) => JSON.stringify(s);

/** Rewrites what's outside inline code and fences; `fn` receives the text between them. */
function mapText(line, fn) {
  return line
    .split(/(`+[^`]*`+)/)
    .map((part, i) => (i % 2 ? part : fn(part)))
    .join("");
}

function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function generate() {
  const warnings = [];
  const errors = [];
  fs.rmSync(outDir, { recursive: true, force: true });
  fs.mkdirSync(outDir, { recursive: true });
  fs.mkdirSync(genDir, { recursive: true });

  // First pass: decide every page, and where every source heading lands, before any link is
  // rewritten, since links run between documents.
  const docs = DOCS.map((doc) => {
    const source = fs.readFileSync(path.join(repoDir, doc.file), "utf8");
    const lines = source.split("\n");
    const headings = headingsOf(lines);
    const pages = doc.releases
      ? releasePages(doc, lines, headings)
      : doc.single
        ? singlePage(doc, lines, headings)
        : splitPages(doc, lines, headings, warnings);
    placeAnchors(doc, pages, headings);
    return { ...doc, lines, headings, pages };
  });
  const byFile = new Map(docs.map((d) => [d.file, d]));

  // Second pass: rewrite and write.
  const pageSizes = [];
  for (const doc of docs) {
    for (const page of doc.pages) {
      const output = renderBody(doc, page, page.body, byFile, errors);
      const editUrl = `${REPO_URL}/edit/${BRANCH}/${doc.file}`;
      const front = [
        "---",
        `title: ${yaml(page.title)}`,
        `description: ${yaml(describe(page.body, `${page.title}, from the Marbble ${doc.title}.`))}`,
        `editUrl: ${yaml(editUrl)}`,
        "---",
        "",
      ];
      const file = path.join(outDir, `${page.slug}/index.md`);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, `${front.join("\n")}${output.join("\n")}\n`);
      pageSizes.push([page.slug, page.body.length]);
      if (page.body.length > LONG_PAGE_LINES) {
        warnings.push(`${page.slug}: ${page.body.length} source lines; docs.config.mjs could split it`);
      }
    }
  }

  if (errors.length) {
    const list = errors.map((e) => `  ${e}`).join("\n");
    throw new Error(`Broken links in the docs (${errors.length}):\n${list}`);
  }

  const sidebar = docs.map(sidebarOf);
  const anchors = {};
  for (const d of docs) anchors[d.file] = { "": `/${d.pages[0].slug}/`, ...Object.fromEntries(d.headings.filter((h) => h.target).map((h) => [h.anchor, h.target])) };
  const version = latestVersion(byFile.get("CHANGELOG.md"));
  fs.writeFileSync(path.join(genDir, "anchors.json"), JSON.stringify(anchors, null, 1));
  fs.writeFileSync(path.join(genDir, "meta.json"), JSON.stringify(version, null, 1));
  fs.writeFileSync(path.join(genDir, "pages.json"), JSON.stringify(pageSizes, null, 0));
  for (const w of warnings) console.warn(`[docs] ${w}`);
  console.log(`[docs] ${pageSizes.length} pages from ${docs.length} documents`);
  return { sidebar, pages: pageSizes, watch: docs.map((d) => path.join(repoDir, d.file)) };
}

/** The ordinary case: the opening becomes an overview, `##` sections pages, the long ones cut. */
function splitPages(doc, lines, headings, warnings) {
  const h2s = headings.filter((h) => h.level === 2);
  const firstH2 = h2s[0]?.line ?? lines.length;
  const titleLine = headings.find((h) => h.level === 1)?.line ?? -1;
  const pages = [
    {
      slug: doc.area,
      title: doc.title,
      from: titleLine + 1,
      to: firstH2,
      kind: "overview",
      section: null,
    },
  ];
  const known = new Set((doc.parts ?? []).flatMap(([, keys]) => keys));
  const sections = [];
  for (let i = 0; i < h2s.length; i++) {
    const h = h2s[i];
    const key = keyOf(h.text);
    const end = h2s[i + 1]?.line ?? lines.length;
    if (!known.has(key)) warnings.push(`${doc.file}: section "${h.text}" isn't in any part of docs.config.mjs; it goes last`);
    const title = stripNumber(h.text);
    const slug = `${doc.area}/${key}`;
    const inner = headings.filter((x) => x.line > h.line && x.line < end);
    const cuts = [];
    for (const [cutTitle, anchor] of doc.split?.[key] ?? []) {
      const at = inner.find((x) => x.anchor === anchor);
      if (!at) {
        warnings.push(`${doc.file}: split "${anchor}" in "${key}" names no heading; its pages merge`);
        continue;
      }
      cuts.push({ title: cutTitle ?? stripNumber(at.text), at });
    }
    const section = { key, title, slug, pages: [] };
    sections.push(section);
    if (!cuts.length) {
      const page = { slug, title, from: h.line + 1, to: end, kind: "section", section, heading: h };
      pages.push(page);
      section.pages.push(page);
      continue;
    }
    const introEnd = cuts[0].at.line;
    const intro = trimBlank(lines.slice(h.line + 1, introEnd));
    if (intro.length) {
      const page = { slug, title, sidebarLabel: "Introduction", from: h.line + 1, to: introEnd, kind: "intro", section, heading: h };
      pages.push(page);
      section.pages.push(page);
    }
    const seen = new Set();
    cuts.forEach((cut, n) => {
      let sub = new GithubSlugger().slug(cut.title);
      if (seen.has(sub)) sub = cut.at.anchor;
      seen.add(sub);
      const page = {
        slug: `${slug}/${sub}`,
        title: cut.title,
        from: cut.at.line + 1,
        to: cuts[n + 1]?.at.line ?? end,
        kind: "sub",
        section,
        heading: cut.at,
        topLevel: cut.at.level,
      };
      pages.push(page);
      section.pages.push(page);
    });
    if (!intro.length) section.headingGoesTo = section.pages[0];
  }
  for (const p of pages) p.body = lines.slice(p.from, p.to);
  pages[0].body = dropContents(pages[0].body);
  doc.sections = sections;
  pages[0].sections = sections;
  return pages;
}

/** A short document kept whole: one page, its sections as headings on it. */
function singlePage(doc, lines, headings) {
  const titleLine = headings.find((h) => h.level === 1)?.line ?? -1;
  const page = { slug: doc.area, title: doc.title, from: titleLine + 1, to: lines.length, kind: "single", section: null };
  page.body = lines.slice(page.from, page.to);
  doc.sections = [];
  return [page];
}

/** The changelog: one page per `## [version]` heading. */
function releasePages(doc, lines, headings) {
  const h2s = headings.filter((h) => h.level === 2);
  const titleLine = headings.find((h) => h.level === 1)?.line ?? -1;
  const pages = [{ slug: doc.area, title: doc.title, from: titleLine + 1, to: h2s[0]?.line ?? lines.length, kind: "overview", section: null }];
  const isDefinition = (l) => /^\[[^\]]+\]:\s/.test(l);
  const definitions = Object.fromEntries(
    lines.filter(isDefinition).map((l) => {
      const m = l.match(/^\[([^\]]+)\]:\s*(\S+)/);
      return [m[1], m[2]];
    }),
  );
  const sections = [];
  h2s.forEach((h, i) => {
    const m = h.raw.match(/^\[([^\]]+)\](?:\s*-\s*(.*))?/);
    const name = m?.[1] ?? h.text;
    const date = m?.[2];
    const key = new GithubSlugger().slug(name.replace(/\./g, "-"));
    const page = {
      slug: `${doc.area}/${key}`,
      title: name,
      from: h.line + 1,
      to: h2s[i + 1]?.line ?? lines.length,
      kind: "section",
      heading: h,
      release: { name, date, url: definitions[name] },
      section: null,
    };
    const section = { key, title: page.title, slug: page.slug, pages: [page], date };
    page.section = section;
    sections.push(section);
    pages.push(page);
  });
  for (const p of pages) p.body = lines.slice(p.from, p.to).filter((l) => !isDefinition(l));
  doc.sections = sections;
  pages[0].sections = sections;
  return pages;
}

/** Works out where each source heading ends up: its page and its anchor there. */
function placeAnchors(doc, pages, headings) {
  for (const page of pages) {
    // A page that starts on a heading promotes the ones below it, so that its first level is `##`;
    // a whole document on one page keeps its levels.
    page.demote = page.kind === "single" ? 0 : (page.kind === "sub" ? page.topLevel : 2) - 1;
    const slugger = new GithubSlugger();
    for (const h of headings) {
      if (h.line < page.from || h.line >= page.to) continue;
      h.pageAnchor = slugger.slug(h.text);
      h.page = page;
      h.target = `/${page.slug}/#${h.pageAnchor}`;
    }
    if (page.heading) {
      page.heading.page = page;
      page.heading.target = `/${page.slug}/`;
    }
  }
  for (const section of doc.sections ?? []) {
    if (!section.headingGoesTo) continue;
    const h2 = headings.find((h) => h.level === 2 && keyOf(h.text) === section.key);
    if (h2) h2.target = `/${section.headingGoesTo.slug}/`;
  }
}

function sidebarOf(doc) {
  const item = (p, label) => ({ label: label ?? p.title, link: `/${p.slug}/` });
  const sectionItem = (section) => {
    if (section.pages.length === 1) return item(section.pages[0], section.title);
    return {
      label: section.title,
      collapsed: true,
      items: section.pages.map((p) => item(p, p.sidebarLabel ?? p.title)),
    };
  };
  const overview = doc.pages[0];
  if (doc.single) return { label: doc.title, items: [item(overview, "Overview")] };
  if (doc.releases) {
    return { label: doc.title, items: [item(overview, "About the changelog"), ...doc.sections.map(sectionItem)] };
  }
  const byKey = new Map(doc.sections.map((s) => [s.key, s]));
  const placed = new Set();
  const groups = doc.parts.map(([label, keys]) => {
    const items = keys
      .map((k) => {
        placed.add(k);
        return byKey.get(k);
      })
      .filter(Boolean)
      .map(sectionItem);
    return { label, items };
  });
  const last = groups[groups.length - 1];
  for (const s of doc.sections) if (!placed.has(s.key)) last.items.push(sectionItem(s));
  const flat = [];
  for (const g of groups) {
    if (g.label) flat.push({ label: g.label, items: g.items });
    else flat.push(...g.items);
  }
  return { label: doc.title, items: [item(overview, doc.overview), ...flat] };
}

function latestVersion(changelog) {
  const release = changelog.pages.find((p) => p.release && p.release.name !== "Unreleased");
  return { version: release?.release.name ?? "", date: release?.release.date ?? "", page: release ? `/${release.slug}/` : "/changelog/" };
}

/** Turns a page's source lines into its final Markdown. */
function renderBody(doc, page, body, byFile, errors) {
  const out = [];
  let fence = null;
  let mermaid = null;
  const where = (i) => `${doc.file}:${page.from + i + 1}`;
  body.forEach((line, i) => {
    const f = line.match(/^(\s*)(```+|~~~+)\s*(\S*)/);
    if (mermaid) {
      if (f) {
        out.push(`<pre class="mermaid">${escapeHtml(mermaid.join("\n"))}</pre>`);
        mermaid = null;
      } else mermaid.push(line);
      return;
    }
    if (f) {
      if (!fence && f[3] === "mermaid") {
        mermaid = [];
        return;
      }
      if (!fence) fence = f[2][0];
      else if (f[2][0] === fence) fence = null;
      out.push(line);
      return;
    }
    if (fence) {
      out.push(line);
      return;
    }
    const h = line.match(/^(#{2,6})(\s+.*)$/);
    if (h) {
      const level = Math.max(2, h[1].length - (page.demote ?? 1));
      out.push(`${"#".repeat(level)}${h[2]}`);
      return;
    }
    out.push(
      mapText(line, (text) =>
        text.replace(/\]\(([^()\s]+)\)/g, (whole, target) => {
          const resolved = resolveLink(doc, target, byFile);
          if (resolved.error) {
            errors.push(`${where(i)}: ${target} (${resolved.error})`);
            return whole;
          }
          return `](${resolved.url})`;
        }),
      ),
    );
  });
  let result = trimBlank(out);
  if (page.release) {
    const { date, url } = page.release;
    const bits = [];
    if (date) bits.push(`Released ${date}.`);
    if (url) bits.push(`[${url.includes("/compare/") ? "Compare on GitHub" : "See on GitHub"}](${url})`);
    if (bits.length) result = [bits.join(" "), "", ...result];
  }
  if (page.kind === "overview") result = result.concat(overviewList(doc, page));
  if (page.kind === "intro") {
    result = result.concat(["", "## In this section", "", ...page.section.pages.filter((p) => p !== page).map((p) => `- [${p.title}](${BASE}/${p.slug}/)`)]);
  }
  return result;
}

function overviewList(doc, page) {
  const out = ["", doc.releases ? "## Releases" : "## Contents", ""];
  const sections = page.sections ?? [];
  if (doc.releases) {
    for (const s of sections) out.push(`- [${s.title}](${BASE}/${s.slug}/)${s.date ? ` (${s.date})` : ""}`);
    return out;
  }
  const byKey = new Map(sections.map((s) => [s.key, s]));
  const placed = new Set();
  const line = (s) => {
    const first = s.pages[0];
    const rest = s.pages.length > 1 ? `: ${s.pages.filter((p) => p.kind !== "intro").map((p) => `[${p.title}](${BASE}/${p.slug}/)`).join(" · ")}` : "";
    return `- ${s.pages[0].kind === "sub" ? `**${s.title}**` : `[${s.title}](${BASE}/${first.slug}/)`}${rest}`;
  };
  for (const [label, keys] of doc.parts) {
    if (label) out.push(`### ${label}`, "");
    for (const k of keys) {
      placed.add(k);
      if (byKey.has(k)) out.push(line(byKey.get(k)));
    }
    out.push("");
  }
  for (const s of sections) if (!placed.has(s.key)) out.push(line(s));
  return out;
}

/** Where a link in `doc` goes: a page of the site, a repository file on GitHub, or nowhere. */
function resolveLink(doc, target, byFile) {
  const blob = `${REPO_URL}/blob/`;
  if (/^(mailto:|tel:)/.test(target)) return { url: target };
  if (/^https?:/.test(target)) {
    // The docs linking to themselves on GitHub stay on the site.
    if (target.startsWith(blob)) {
      const rest = target.slice(blob.length).replace(/^[^/]+\//, "");
      const [file, anchor] = rest.split("#");
      if (byFile.has(file)) return inSite(byFile.get(file), anchor);
    }
    return { url: target };
  }
  if (target.startsWith("#")) return inSite(doc, target.slice(1));
  const [rel, anchor] = target.split("#");
  const file = path.posix.normalize(path.posix.join(path.posix.dirname(doc.file), rel));
  if (byFile.has(file)) return inSite(byFile.get(file), anchor);
  if (file.startsWith("..")) return { error: "leaves the repository" };
  const full = path.join(repoDir, file);
  if (!fs.existsSync(full)) return { error: `no file ${file}` };
  // Pictures the docs show (screenshots) are the site's own files, so they display on both.
  if (file.startsWith("site/public/")) return { url: `${BASE}/${file.slice("site/public/".length)}` };
  if (anchor && /\.md$/.test(file)) {
    const known = headingsOf(fs.readFileSync(full, "utf8").split("\n")).map((h) => h.anchor);
    if (!known.includes(anchor)) return { error: `no heading #${anchor} in ${file}` };
  }
  const kind = fs.statSync(full).isDirectory() ? "tree" : "blob";
  return { url: `${REPO_URL}/${kind}/${BRANCH}/${file}${anchor ? `#${anchor}` : ""}` };
}

function inSite(doc, anchor) {
  if (!anchor) return { url: `${BASE}/${doc.pages[0].slug}/` };
  const h = doc.headings.find((x) => x.anchor === anchor);
  if (!h) return { error: `no heading #${anchor} in ${doc.file}` };
  if (!h.target) return { url: `${BASE}/${doc.pages[0].slug}/` };
  return { url: `${BASE}${h.target}` };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { pages } = generate();
  for (const [slug, size] of pages) console.log(String(size).padStart(5), slug);
}
