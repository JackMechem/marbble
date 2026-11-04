// Unit tests for src/sendsModel.ts: the buses a strip's other send slots already use.
import { test } from "node:test";
import assert from "node:assert/strict";
import { busesInUse } from "../src/sendsModel.ts";
import type { Send } from "../src/api.ts";

const send = (bus: number) => ({ bus, level_db: 0, pre_fader: false, muted: false }) as Send;

test("a send slot's menu greys out the buses the other slots send to, not its own", () => {
  const sends = [send(7), null, send(9)];
  assert.deepEqual(busesInUse(sends, 1), [7, 9]);
  // The slot's own bus stays choosable (it is ticked).
  assert.deepEqual(busesInUse(sends, 0), [9]);
  assert.deepEqual(busesInUse(undefined, 0), []);
});
