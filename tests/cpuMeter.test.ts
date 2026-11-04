import { test } from "node:test";
import assert from "node:assert/strict";
import { cpuLevel, cpuPercent, cpuTitle, shownPeak } from "../src/cpuMeter.ts";

test("the peak never shows under the average", () => {
  assert.equal(shownPeak(0.2, 0.9), 0.9);
  assert.equal(shownPeak(0.5, 0.3), 0.5);
});

test("the tooltip gives the average and the peak", () => {
  const title = cpuTitle(0.52, 1.304, 0, 1);
  assert.match(title, /Average 52%, recent peak 130%\./);
  assert.match(title, /system monitor/);
});

test("late blocks are counted, and only mentioned when there are some", () => {
  assert.doesNotMatch(cpuTitle(0.1, 0.1, 0, 1), /Click to clear/);
  assert.match(cpuTitle(0.1, 0.1, 1, 1), /1 block over 100%\. Click to clear\./);
  assert.match(cpuTitle(0.1, 0.1, 3, 1), /3 blocks over 100%\. Click to clear\./);
});

test("the tooltip says how many threads are playing", () => {
  assert.match(cpuTitle(0.1, 0.1, 0, 1), /audio thread alone/);
  assert.match(cpuTitle(0.1, 0.1, 0, 4), /Playing on 4 threads\./);
});

test("the figure is the average as a whole percentage, past 100% when late", () => {
  assert.equal(cpuPercent(0), "0%");
  assert.equal(cpuPercent(0.144), "14%");
  assert.equal(cpuPercent(1.126), "113%");
  assert.equal(cpuPercent(-0.01), "0%");
});

test("the bar turns amber past half and red past 80%", () => {
  assert.equal(cpuLevel(0.5), "low");
  assert.equal(cpuLevel(0.51), "mid");
  assert.equal(cpuLevel(0.8), "mid");
  assert.equal(cpuLevel(0.81), "high");
});
