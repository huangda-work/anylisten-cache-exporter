import test from "node:test";
import assert from "node:assert/strict";
import { reconcileSelection, toggleVisibleSelection, filterTracks, selectionSummary, selectOnlyUnsaved, retryFailedIds } from "./state.js";

const cached = (id, title = id) => ({ id, title, artist: "甲", needsNetwork: false });
const remote = (id, title = id) => ({ id, title, artist: "乙", needsNetwork: true });

test("刷新保留已有选择，只自动选中新缓存", () => {
  const selected = new Set(["cache:old", "cache:gone"]);
  const previous = new Set(["cache:old", "cache:gone", "remote:old"]);
  const current = [cached("cache:old"), cached("cache:new"), remote("remote:new")];
  assert.deepEqual(reconcileSelection(selected, previous, current), new Set(["cache:old", "cache:new"]));
});

test("全选只切换当前可见音乐", () => {
  const visible = [cached("cache:a"), cached("cache:b")];
  assert.deepEqual(toggleVisibleSelection(new Set(["remote:c"]), visible), new Set(["remote:c", "cache:a", "cache:b"]));
  assert.deepEqual(toggleVisibleSelection(new Set(["cache:a", "cache:b", "remote:c"]), visible), new Set(["remote:c"]));
});

test("筛选与搜索同时生效", () => {
  const tracks = [cached("cache:a", "夜航"), remote("remote:b", "晨光")];
  assert.deepEqual(filterTracks(tracks, "cached", "夜").map((track) => track.id), ["cache:a"]);
  assert.deepEqual(filterTracks(tracks, "remote", "夜"), []);
});

test("批量保存统计区分已保存和未确认", () => {
  const selected = new Set(["cache:a", "cache:b", "remote:c", "cache:d"]);
  const statuses = { "cache:a": "saved", "cache:b": "notSaved", "remote:c": "unverified" };
  assert.deepEqual(selectionSummary(selected, statuses), { total: 4, toSave: 1, saved: 1, uncertain: 2 });
});

test("只选未保存排除已保存和未确认的歌曲", () => {
  const tracks = [cached("cache:a"), cached("cache:b"), remote("remote:c")];
  const statuses = { "cache:a": "saved", "cache:b": "notSaved", "remote:c": "unverified" };
  assert.deepEqual(selectOnlyUnsaved(tracks, statuses), new Set(["cache:b"]));
});

test("重试只提交失败歌曲且不重复提交", () => {
  const failures = [
    { id: "cache:a", title: "第一首", reason: "缓存已变化" },
    { id: "remote:b", title: "第二首", reason: "请求失败" },
    { id: "cache:a", title: "第一首", reason: "状态记录失败" },
  ];
  assert.deepEqual(retryFailedIds(failures), ["cache:a", "remote:b"]);
});
