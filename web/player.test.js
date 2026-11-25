import test from "node:test";
import assert from "node:assert/strict";
import { createPreviewController } from "./player.js";

class FakeAudio extends EventTarget {
  src = "";
  currentTime = 0;
  duration = 120;
  paused = true;

  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }

  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }

  load() { this.currentTime = 0; }
}

function setup(load = async () => new Uint8Array([1, 2, 3])) {
  const audio = new FakeAudio();
  const revoked = [];
  let nextUrl = 0;
  const player = createPreviewController({
    audio, load,
    createUrl: () => `blob:preview-${++nextUrl}`,
    revokeUrl: (url) => revoked.push(url),
    onChange: () => {},
  });
  return { player, audio, revoked };
}

const cached = (id, extension = "mp3") => ({ id: `cache:${id}`, title: id, artist: "歌手", extension, needsNetwork: false });

test("切歌和停止时释放之前的音频数据", async () => {
  const { player, audio, revoked } = setup();
  await player.play(cached("one"));
  assert.equal(audio.src, "blob:preview-1");
  assert.equal(player.state.playing, true);

  await player.play(cached("two"));
  assert.equal(audio.src, "blob:preview-2");
  assert.deepEqual(revoked, ["blob:preview-1"]);
  await player.toggle();
  assert.equal(player.state.playing, false);
  await player.toggle();
  assert.equal(player.state.playing, true);
  player.stop();
  assert.deepEqual(revoked, ["blob:preview-1", "blob:preview-2"]);
  assert.equal(audio.src, "");
});

test("较慢的旧歌曲加载完成后不会覆盖新歌曲", async () => {
  let finishFirst;
  const load = (id) => id === "cache:one"
    ? new Promise((resolve) => { finishFirst = resolve; })
    : Promise.resolve(new Uint8Array([4, 5, 6]));
  const { player, audio, revoked } = setup(load);
  const first = player.play(cached("one"));
  await player.play(cached("two"));
  finishFirst(new Uint8Array([1, 2, 3]));
  await first;
  assert.equal(player.state.track.id, "cache:two");
  assert.equal(audio.src, "blob:preview-1");
  assert.deepEqual(revoked, []);
});

test("需联网歌曲不会发出试听读取请求", async () => {
  let loads = 0;
  const { player } = setup(async () => { loads++; return new Uint8Array([1]); });
  await assert.rejects(player.play({ ...cached("remote"), needsNetwork: true }), /完整缓存/);
  assert.equal(loads, 0);
});

test("首次播放被拒绝后可以直接重试已读取的歌曲", async () => {
  let loads = 0;
  const { player, audio, revoked } = setup(async () => { loads++; return new Uint8Array([1, 2, 3]); });
  const playAudio = audio.play.bind(audio);
  let attempts = 0;
  audio.play = async () => {
    if (++attempts === 1) throw new Error("user gesture required");
    return playAudio();
  };

  await assert.rejects(player.play(cached("one")), /user gesture/);
  assert.equal(player.state.track?.id, "cache:one");
  assert.equal(audio.src, "blob:preview-1");
  await player.play(cached("one"));
  assert.equal(player.state.playing, true);
  assert.equal(loads, 1);
  assert.deepEqual(revoked, []);
});

test("进度与结束状态跟随音频元素", async () => {
  const { player, audio } = setup();
  await player.play(cached("one"));
  audio.currentTime = 42;
  audio.dispatchEvent(new Event("timeupdate"));
  assert.equal(player.state.currentTime, 42);
  assert.equal(player.state.duration, 120);
  player.seek(18);
  assert.equal(audio.currentTime, 18);
  audio.dispatchEvent(new Event("ended"));
  assert.equal(player.state.playing, false);
});
