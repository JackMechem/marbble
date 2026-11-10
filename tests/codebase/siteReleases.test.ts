// Unit tests for site/src/lib/releases.ts (the website's newest release on each channel). Run with
// `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { compareVersions, newestReleases, type Release } from "../../site/src/lib/releases.ts";

const release = (tag: string, extra: Partial<Release> = {}): Release => ({
  tag_name: tag,
  html_url: `https://github.com/JackMechem/marbble/releases/tag/${tag}`,
  draft: false,
  prerelease: tag.includes("-"),
  assets: [{ name: "x.AppImage", browser_download_url: "https://example.invalid/x" }],
  ...extra,
});

test("versions compare as semantic versions", () => {
  const ordered = ["v0.1.0-alpha.2", "v0.1.0-alpha.9", "v0.1.0-alpha.10", "v0.1.0-beta.1", "v0.1.0-rc.1", "v0.1.0", "v0.1.1", "v0.2.0", "v0.10.0", "v1.0.0"];
  for (let i = 0; i < ordered.length; i++) {
    assert.equal(compareVersions(ordered[i], ordered[i]), 0, ordered[i]);
    for (let j = i + 1; j < ordered.length; j++) {
      assert.ok(compareVersions(ordered[i], ordered[j]) < 0, `${ordered[i]} < ${ordered[j]}`);
      assert.ok(compareVersions(ordered[j], ordered[i]) > 0, `${ordered[j]} > ${ordered[i]}`);
    }
  }
});

test("the newest release doesn't depend on GitHub's order", () => {
  // As GitHub listed them on 2026-11-08: alpha.9 first.
  const list = [release("v0.1.0-alpha.9"), release("v0.1.0-alpha.10"), release("v0.1.0-alpha.8")];
  assert.equal(newestReleases(list).unstable?.tag_name, "v0.1.0-alpha.10");
  assert.equal(newestReleases(list).stable, undefined);
});

test("stable is the newest plain version, unstable the newest of all", () => {
  const list = [release("v1.0.0"), release("v1.1.0-rc.1"), release("v0.9.0"), release("v1.0.1")];
  const newest = newestReleases(list);
  assert.equal(newest.stable?.tag_name, "v1.0.1");
  assert.equal(newest.unstable?.tag_name, "v1.1.0-rc.1");
});

test("drafts and releases still without files are left out", () => {
  const list = [release("v0.1.0-alpha.11", { draft: true }), release("v0.1.0-alpha.12", { assets: [] }), release("v0.1.0-alpha.10")];
  assert.equal(newestReleases(list).unstable?.tag_name, "v0.1.0-alpha.10");
});
