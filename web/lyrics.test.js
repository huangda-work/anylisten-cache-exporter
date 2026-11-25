import test from "node:test";
import assert from "node:assert/strict";
import { activeLyricIndex, parseLyrics } from "./lyrics.js";

test("解析重复时间戳、毫秒偏移与元数据", () => {
  const parsed = parseLyrics("[ar:歌手]\n[offset:500]\n[00:01.00][00:03.00]第一句\n[00:02.50]第二句");
  assert.equal(parsed.timed, true);
  assert.deepEqual(parsed.lines, [
    { time: 1.5, text: "第一句" },
    { time: 3, text: "第二句" },
    { time: 3.5, text: "第一句" },
  ]);
  assert.equal(activeLyricIndex(parsed.lines, 2), 0);
  assert.equal(activeLyricIndex(parsed.lines, 3.2), 1);
  assert.equal(activeLyricIndex(parsed.lines, 0), -1);
});

test("没有时间戳时展示普通文本", () => {
  assert.deepEqual(parseLyrics("[ti:歌名]\n第一句\n第二句"), {
    timed: false,
    lines: [{ text: "第一句" }, { text: "第二句" }],
  });
  assert.deepEqual(parseLyrics("  "), { timed: false, lines: [] });
});
