// Unit tests for src/settings/panes/memoryLock.ts: Settings › Audio › Lock audio memory.
import { test } from "node:test";
import assert from "node:assert/strict";
import { memoryLockNote, offersMemoryLock } from "../../../src/settings/panes/memoryLock.ts";

test("the switch is offered only where memory can be locked", () => {
  assert.equal(offersMemoryLock({ state: "unsupported" }), false);
  assert.equal(offersMemoryLock(null), false);
  for (const state of ["off", "locked"] as const) assert.equal(offersMemoryLock({ state }), true);
  assert.equal(offersMemoryLock({ state: "limit_too_small", limit: 8 << 20 }), true);
});

test("a finite limit is a warning that says what the limit is and how to raise it", () => {
  const n = memoryLockNote({ state: "limit_too_small", limit: 8 << 20 });
  assert.equal(n.warn, true);
  assert.match(n.text, /RLIMIT_MEMLOCK\) is 8 MB/);
  assert.match(n.text, /audio group/);
  assert.match(memoryLockNote({ state: "limit_too_small", limit: 65536 }).text, /is 64 kB/);
});

test("locked and off are notes, a refusal is a warning", () => {
  assert.equal(memoryLockNote({ state: "locked" }).warn, false);
  assert.match(memoryLockNote({ state: "locked" }).text, /locked/);
  assert.equal(memoryLockNote({ state: "off" }).warn, false);
  const f = memoryLockNote({ state: "failed", error: "mlockall failed: Cannot allocate memory" });
  assert.equal(f.warn, true);
  assert.equal(f.text, "Not locked: mlockall failed: Cannot allocate memory.");
});
