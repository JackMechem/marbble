import { test } from "node:test";
import assert from "node:assert/strict";
import { compareVersions, effectiveChannel, failedText, httpFailure, newestOnChannel, offerText, parseVersion, updateOffer, upToDateText, type Release } from "../src/updateCheck.ts";

const v = (s: string) => {
  const p = parseVersion(s);
  assert.ok(p, s);
  return p;
};
const release = (tag: string, more: Partial<Release> = {}): Release => ({
  tag_name: tag,
  html_url: `https://github.com/JackMechem/marbble/releases/tag/${tag}`,
  draft: false,
  prerelease: /-/.test(tag),
  ...more,
});

test("tags and versions read as semver", () => {
  assert.deepEqual(parseVersion("v0.1.0-alpha.7"), { major: 0, minor: 1, patch: 0, pre: ["alpha", "7"] });
  assert.deepEqual(parseVersion("1.2.3"), { major: 1, minor: 2, patch: 3, pre: [] });
  assert.deepEqual(parseVersion(" 1.2.3+build.5 "), { major: 1, minor: 2, patch: 3, pre: [] });
  for (const bad of ["", "nightly", "v1.2", "1.2.3.4", "1.2.3-", "1.2.3-alpha..1", "va.b.c"]) assert.equal(parseVersion(bad), null, bad);
});

test("semver precedence, prereleases included", () => {
  // semver.org's own example, then this project's tags.
  const order = [
    "1.0.0-alpha",
    "1.0.0-alpha.1",
    "1.0.0-alpha.beta",
    "1.0.0-beta",
    "1.0.0-beta.2",
    "1.0.0-beta.11",
    "1.0.0-rc.1",
    "1.0.0",
    "1.0.1",
    "1.1.0",
    "2.0.0",
  ];
  const ours = ["0.1.0-alpha.7", "0.1.0-alpha.10", "0.1.0-beta.1", "0.1.0-rc.1", "0.1.0", "0.1.1-alpha.1", "0.2.0"];
  for (const list of [order, ours]) {
    for (let i = 0; i < list.length; i++) {
      for (let j = 0; j < list.length; j++) {
        assert.equal(compareVersions(v(list[i]), v(list[j])), Math.sign(i - j), `${list[i]} vs ${list[j]}`);
      }
    }
  }
  // Build metadata doesn't count.
  assert.equal(compareVersions(v("1.0.0+a"), v("1.0.0+b")), 0);
});

test("an unchosen channel follows the build", () => {
  assert.equal(effectiveChannel(undefined, "0.1.0-alpha.7"), "unstable");
  assert.equal(effectiveChannel(null, "0.1.0"), "stable");
  assert.equal(effectiveChannel(null, "not a version"), "stable");
  // A choice is kept, whatever is running; a value from elsewhere is ignored.
  assert.equal(effectiveChannel("stable", "0.1.0-alpha.7"), "stable");
  assert.equal(effectiveChannel("unstable", "1.0.0"), "unstable");
  assert.equal(effectiveChannel("nightly", "1.0.0"), "stable");
});

test("each channel's newest release", () => {
  const list = [release("v0.1.0-alpha.7"), release("v0.1.0-alpha.10"), release("v0.0.9"), release("v0.0.10"), release("v0.1.0-beta.1")];
  assert.equal(newestOnChannel(list, "unstable")?.version, "0.1.0-beta.1");
  assert.equal(newestOnChannel(list, "stable")?.version, "0.0.10");
  // A final release outranks its prereleases on Unstable too, so Unstable is never behind Stable.
  assert.equal(newestOnChannel([...list, release("v0.1.0")], "unstable")?.version, "0.1.0");
  assert.equal(newestOnChannel([...list, release("v0.1.0")], "stable")?.url, "https://github.com/JackMechem/marbble/releases/tag/v0.1.0");
});

test("drafts, prereleases on Stable and tags that aren't versions never count", () => {
  const list = [release("v0.0.1"), release("v9.0.0", { draft: true }), release("v8.0.0", { prerelease: true }), release("v7.0.0-rc.1", { prerelease: false }), release("latest")];
  assert.equal(newestOnChannel(list, "stable")?.version, "0.0.1");
  assert.equal(newestOnChannel(list, "unstable")?.version, "8.0.0");
  // Junk in the answer is skipped, not thrown on.
  assert.equal(newestOnChannel([null, 3, { tag_name: 5 }, { tag_name: "v1.0.0" }], "stable"), null);
  assert.equal(newestOnChannel({ message: "Not Found" }, "stable"), null);
  assert.equal(newestOnChannel([], "unstable"), null);
});

test("an update is offered only when it's newer than what runs", () => {
  const list = [release("v0.1.0-alpha.7"), release("v0.1.0-alpha.8")];
  assert.deepEqual(updateOffer(list, "unstable", "0.1.0-alpha.7"), { version: "0.1.0-alpha.8", url: "https://github.com/JackMechem/marbble/releases/tag/v0.1.0-alpha.8" });
  assert.equal(updateOffer(list, "unstable", "0.1.0-alpha.8"), null);
  // A build ahead of the releases (a dev build) is offered nothing.
  assert.equal(updateOffer(list, "unstable", "0.1.0-alpha.9"), null);
  assert.equal(updateOffer(list, "unstable", "0.1.0"), null);
  // Stable has nothing yet.
  assert.equal(updateOffer(list, "stable", "0.1.0-alpha.7"), null);
  // Stable offers a final release to someone on a prerelease of it.
  assert.equal(updateOffer([...list, release("v0.1.0")], "stable", "0.1.0-alpha.8")?.version, "0.1.0");
  assert.equal(updateOffer(list, "unstable", "garbage"), null);
});

test("what the check says", () => {
  const list = [release("v0.1.0-alpha.7")];
  assert.equal(offerText({ version: "0.1.0-alpha.8", url: "" }), "Marbble 0.1.0-alpha.8 is out");
  assert.equal(upToDateText(list, "unstable", "0.1.0-alpha.7"), "Marbble 0.1.0-alpha.7 is the newest version on the Unstable channel.");
  assert.equal(upToDateText(list, "stable", "0.1.0-alpha.7"), "There's no release on the Stable channel yet. You're running Marbble 0.1.0-alpha.7.");
  assert.equal(failedText("GitHub answered with HTTP 500"), "Couldn't check for updates: GitHub answered with HTTP 500.");
  assert.equal(failedText("Load failed."), "Couldn't check for updates: Load failed.");
  assert.match(httpFailure(403, "0"), /limit/);
  assert.equal(httpFailure(403, "12"), "GitHub answered with HTTP 403");
  assert.equal(httpFailure(502, null), "GitHub answered with HTTP 502");
});
