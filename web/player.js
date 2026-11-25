const audioTypes = {
  mp3: "audio/mpeg",
  m4a: "audio/mp4",
  flac: "audio/flac",
  ogg: "audio/ogg",
  wav: "audio/wav",
  aac: "audio/aac",
};

export function createPreviewController({ audio, load, createUrl, revokeUrl, onChange }) {
  const state = { track: null, loading: false, playing: false, currentTime: 0, duration: 0 };
  let generation = 0;
  let objectUrl = null;
  const notify = () => onChange(state);

  audio.addEventListener("play", () => {
    state.playing = true;
    notify();
  });
  audio.addEventListener("pause", () => {
    state.playing = false;
    notify();
  });
  audio.addEventListener("timeupdate", () => {
    state.currentTime = audio.currentTime || 0;
    state.duration = Number.isFinite(audio.duration) ? audio.duration : 0;
    notify();
  });
  audio.addEventListener("loadedmetadata", () => {
    state.duration = Number.isFinite(audio.duration) ? audio.duration : 0;
    notify();
  });
  audio.addEventListener("ended", () => {
    state.playing = false;
    notify();
  });

  function stop() {
    generation++;
    audio.pause();
    audio.src = "";
    audio.load();
    if (objectUrl) revokeUrl(objectUrl);
    objectUrl = null;
    state.track = null;
    state.loading = false;
    state.playing = false;
    state.currentTime = 0;
    state.duration = 0;
    notify();
  }

  async function toggle() {
    if (!state.track || state.loading) return;
    if (!audio.paused) {
      audio.pause();
      return;
    }
    if (audio.ended) audio.currentTime = 0;
    await audio.play();
  }

  async function play(track) {
    if (track.needsNetwork || !track.id.startsWith("cache:")) {
      throw new Error("仅完整缓存歌曲可试听");
    }
    if (state.track?.id === track.id) return toggle();
    stop();
    const request = generation;
    state.track = track;
    state.loading = true;
    notify();
    try {
      const bytes = await load(track.id);
      if (request !== generation) return;
      const type = audioTypes[track.extension?.toLowerCase()] || "application/octet-stream";
      objectUrl = createUrl(new Blob([bytes], { type }));
      audio.src = objectUrl;
      state.loading = false;
      notify();
    } catch (error) {
      if (request !== generation) return;
      stop();
      throw error;
    }
    try {
      await audio.play();
    } catch (error) {
      if (request === generation) throw error;
    }
  }

  function seek(time) {
    if (!state.track || !Number.isFinite(state.duration) || state.duration <= 0) return;
    audio.currentTime = Math.max(0, Math.min(time, state.duration));
    state.currentTime = audio.currentTime;
    notify();
  }

  return { state, play, toggle, seek, stop };
}
