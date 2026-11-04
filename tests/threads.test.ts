import { test } from "node:test";
import assert from "node:assert/strict";
import { AUTOMATIC_THREADS, threadNote, threadOptions } from "../src/threads.ts";

test("Automatic comes first and says what it means on this machine", () => {
  const o = threadOptions({ automatic: 6, max: 16 });
  assert.equal(o[0][0], AUTOMATIC_THREADS);
  assert.match(o[0][1], /up to 6 threads/);
  assert.equal(threadOptions({ automatic: 1, max: 2 })[0][1], "Automatic (up to 1 thread)");
});

test("1 is the off switch, then every number up to the machine's", () => {
  const o = threadOptions({ automatic: 3, max: 4 });
  assert.deepEqual(o.map((x) => x[0]), [0, 1, 2, 3, 4]);
  assert.match(o[1][1], /audio thread alone/);
  assert.equal(o[4][1], "4 threads");
});

test("a one-core machine offers only the two", () => {
  assert.deepEqual(threadOptions({ automatic: 1, max: 1 }).map((x) => x[0]), [0, 1]);
});

test("no note while every thread plays", () => {
  assert.equal(threadNote({ automatic: 6, max: 16 }), null);
  assert.equal(threadNote({ automatic: 6, max: 16, left_out: null }), null);
});

test("threads refused real-time priority are named, with what plays instead", () => {
  const all = threadNote({ automatic: 6, max: 16, left_out: { playing: 1, threads: 6, refused: 5, stuck: 0 } });
  assert.equal(all, "The system refused real-time priority to the extra threads, so tracks play on the audio thread alone.");
  const some = threadNote({ automatic: 6, max: 16, left_out: { playing: 4, threads: 6, refused: 2, stuck: 0 } });
  assert.equal(some, "The system refused real-time priority to 2 of the 5 extra threads, so tracks play on 4 threads.");
});

test("a thread stopped in a plug-in says how to get it back", () => {
  const one = threadNote({ automatic: 6, max: 16, left_out: { playing: 5, threads: 6, refused: 0, stuck: 1 } });
  assert.equal(one, "A thread was stopped after a plug-in on it stopped responding, so tracks play on 5 threads. Reset Audio Engine starts the threads afresh.");
  const both = threadNote({ automatic: 4, max: 16, left_out: { playing: 2, threads: 4, refused: 1, stuck: 1 } });
  assert.match(both!, /^The system refused real-time priority to 1 of the 3 extra threads, and a thread was stopped/);
});
