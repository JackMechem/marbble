// Unit tests for src/textFit.ts: long paths shortened at a separator. Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fitPath } from "../src/textFit.ts";

const upTo = (n: number) => (s: string) => s.length <= n;
const P = "/tmp/claude-1000/-home-jack-Projects-daw/319082f0-64c8-416d-9901-c47e55fdcafe/scratchpad/grader/gui-home/Music/Marbble";

test("a path that fits is shown whole", () => {
  assert.equal(fitPath("/home/jack/Music", upTo(40)), "/home/jack/Music");
  assert.equal(fitPath("", upTo(0)), "");
});

test("a long path loses its start at a separator, never mid-name", () => {
  assert.equal(fitPath(P, upTo(24)), "…/gui-home/Music/Marbble");
  assert.equal(fitPath(P, upTo(30)), "…/gui-home/Music/Marbble");
  assert.equal(fitPath(P, upTo(32)), "…/grader/gui-home/Music/Marbble");
  for (let n = 10; n < P.length; n += 7) {
    const s = fitPath(P, upTo(n));
    assert.ok(s.startsWith("…/"), s);
    assert.ok(P.endsWith("/" + s.slice(2)), `${s} is the end of the path, from a whole name`);
  }
});

test("when not even the last name fits, it's kept for the box's own ellipsis", () => {
  assert.equal(fitPath(P, upTo(5)), "…/Marbble");
  assert.equal(fitPath("/a/very-long-folder-name", upTo(5)), "…/very-long-folder-name");
});

test("Windows paths split at backslashes", () => {
  assert.equal(fitPath("C:\\Users\\jack\\Music\\Marbble", upTo(16)), "…\\Music\\Marbble");
});
