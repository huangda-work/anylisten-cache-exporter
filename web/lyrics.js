const timeTag = /\[(\d{1,3}):(\d{1,2})(?:\.(\d{1,3}))?\]/g;
const timedLine = /^\s*((?:\[\d{1,3}:\d{1,2}(?:\.\d{1,3})?\])+)(.*)$/;

export function parseLyrics(lyrics) {
  if (!lyrics?.trim()) return { timed: false, lines: [] };
  const source = lyrics.split(/\r\n|\n|\r/);
  const offset = Number(source.find((line) => /^\[offset:[+-]?\d+\]$/i.test(line.trim()))?.trim().match(/[+-]?\d+/)?.[0] || 0) / 1000;
  const lines = [];
  for (const line of source) {
    const match = timedLine.exec(line);
    if (!match) continue;
    const text = match[2].trim();
    if (!text) continue;
    for (const tag of match[1].matchAll(timeTag)) {
      const fraction = tag[3] ? Number(`0.${tag[3]}`) : 0;
      lines.push({ time: Number(tag[1]) * 60 + Number(tag[2]) + fraction + offset, text });
    }
  }
  if (lines.length) {
    lines.sort((a, b) => a.time - b.time);
    return { timed: true, lines };
  }
  return {
    timed: false,
    lines: source.map((line) => line.trim()).filter((line) => line && !/^\[[a-z][\w-]*:/i.test(line)).map((text) => ({ text })),
  };
}

export function activeLyricIndex(lines, time) {
  let low = 0;
  let high = lines.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (lines[middle].time <= time) low = middle + 1;
    else high = middle;
  }
  return low - 1;
}
