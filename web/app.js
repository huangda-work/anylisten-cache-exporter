import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  createIcons, Settings2, RefreshCw, Search, CircleAlert, Music2,
  FolderPlus, FolderOpen, Download, X, CircleCheck, Circle,
  CircleHelp, LoaderCircle, CircleMinus, Play, Pause,
} from "lucide";
import { filterTracks, reconcileSelection, toggleVisibleSelection, selectionSummary, selectOnlyUnsaved, retryFailedIds } from "./state.js";
import { createPreviewController } from "./player.js";

const icons = {
  Settings2, RefreshCw, Search, CircleAlert, Music2, FolderPlus,
  FolderOpen, Download, X, CircleCheck, Circle, CircleHelp,
  LoaderCircle, CircleMinus, Play, Pause,
};
const $ = (id) => document.getElementById(id);
const model = {
  tracks: [], selected: new Set(), statuses: {}, source: "all", query: "",
  cachedCount: 0, remoteCount: 0, unavailableCount: 0,
  prefs: { dataRootOverride: null, lastExportDirectory: null },
  dataRoot: null, scanning: false, exporting: false, cancelRequested: false,
  status: "", progress: null, statusVersion: 0,
};
let activityTimer;
let activityVersion = 0;
let activityFocusReturn;
let preview;
let renderedPreviewTrackId = null;
const previewButtons = new Map();
let failedTrackIds = [];

function icon(name) {
  const element = document.createElement("i");
  element.dataset.lucide = name;
  return element;
}

function cell(className, text) {
  const element = document.createElement("span");
  element.className = className;
  element.textContent = text;
  return element;
}

function formatSize(bytes) {
  if (bytes == null) return "需联网";
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function formatTime(seconds) {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const whole = Math.floor(seconds);
  const minutes = Math.floor(whole / 60);
  const time = `${String(minutes % 60).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
  return minutes >= 60 ? `${Math.floor(minutes / 60)}:${time}` : `${minutes}:${String(whole % 60).padStart(2, "0")}`;
}

const statusLabels = {
  saved: ["已保存", "circle-check"],
  notSaved: ["未保存", "circle"],
  unverified: ["未确认", "circle-help"],
  checking: ["检查中", "loader-circle"],
  noDirectory: ["未设目录", "circle-minus"],
};

function createTrackRow(track) {
  const row = document.createElement("div");
  row.className = "track-row";
  const checkboxCell = document.createElement("label");
  checkboxCell.className = "track-checkbox";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = model.selected.has(track.id);
  checkbox.disabled = model.scanning || model.exporting;
  checkbox.setAttribute("aria-label", `选择${track.title}`);
  checkbox.addEventListener("change", () => {
    if (checkbox.checked) model.selected.add(track.id);
    else model.selected.delete(track.id);
    renderFooter();
  });
  checkboxCell.append(checkbox);
  row.append(checkboxCell);

  const info = document.createElement("div");
  info.className = "track-info";
  const title = cell("track-title", track.title);
  title.title = track.title;
  const artist = cell("track-artist", track.artist || "未识别歌手");
  artist.title = track.artist || "未识别歌手";
  info.append(title, artist);
  row.append(info);

  const size = cell(`track-size${track.needsNetwork ? " remote" : ""}`, formatSize(track.totalSize));
  const format = cell("track-format", (track.extension || "未知").toUpperCase());
  row.append(size, format);

  const statusKey = model.statuses[track.id] || (model.prefs.lastExportDirectory ? "checking" : "noDirectory");
  const [label, symbol] = statusLabels[statusKey] || statusLabels.checking;
  const status = document.createElement("span");
  status.className = `track-status ${statusKey}`;
  status.title = label;
  status.append(icon(symbol), document.createTextNode(label));
  row.append(status);

  const playButton = document.createElement("button");
  playButton.type = "button";
  playButton.className = "icon-button row-play";
  playButton.dataset.previewId = track.id;
  playButton.dataset.previewTitle = track.title;
  playButton.disabled = track.needsNetwork || model.scanning || model.exporting;
  updatePreviewButton(playButton, track);
  playButton.addEventListener("click", () => playPreview(track));
  previewButtons.set(track.id, { button: playButton, track });
  row.append(playButton);

  const saveButton = document.createElement("button");
  saveButton.type = "button";
  const isSaved = statusKey === "saved";
  saveButton.className = `icon-button row-save${isSaved ? " saved-file" : ""}`;
  saveButton.title = isSaved ? `打开已保存的${track.title}` : `保存${track.title}`;
  saveButton.setAttribute("aria-label", saveButton.title);
  saveButton.disabled = model.scanning || model.exporting;
  saveButton.append(icon(isSaved ? "folder-open" : "download"));
  saveButton.addEventListener("click", () => isSaved ? openSavedTrack(track) : saveOne(track));
  row.append(saveButton);
  return row;
}

function visibleTracks() { return filterTracks(model.tracks, model.source, model.query); }

function updatePreviewButton(button, track) {
  const active = preview?.state.track?.id === track.id;
  const playing = active && preview.state.playing;
  const loading = active && preview.state.loading;
  const symbol = loading ? "loader-circle" : playing ? "pause" : "play";
  if (button.dataset.icon !== symbol) {
    button.replaceChildren(icon(symbol));
    button.dataset.icon = symbol;
  }
  button.classList.toggle("active", active);
  const label = track.needsNetwork ? "需联网歌曲暂不支持试听" :
    loading ? `正在读取${track.title}` : playing ? `暂停${track.title}` : `试听${track.title}`;
  button.title = label;
  button.setAttribute("aria-label", label);
}

function renderPreview() {
  const state = preview.state;
  $("previewBar").hidden = !state.track;
  if (state.track) {
    $("previewTitle").textContent = state.track.title;
    $("previewTitle").title = state.track.title;
    $("previewArtist").textContent = state.track.artist || "未识别歌手";
    const seek = $("previewSeek");
    seek.disabled = state.loading || !state.duration;
    seek.max = state.duration || 1;
    seek.value = Math.min(state.currentTime, state.duration || 1);
    seek.setAttribute("aria-valuetext", `${formatTime(state.currentTime)}，共 ${formatTime(state.duration)}`);
    $("previewTime").textContent = state.loading ? "读取中" : `${formatTime(state.currentTime)} / ${formatTime(state.duration)}`;
  }
  const toggle = $("previewToggleButton");
  const symbol = state.loading ? "loader-circle" : state.playing ? "pause" : "play";
  if (toggle.dataset.icon !== symbol) {
    toggle.replaceChildren(icon(symbol));
    toggle.dataset.icon = symbol;
  }
  toggle.disabled = state.loading;
  toggle.title = state.playing ? "暂停试听" : "继续试听";
  toggle.setAttribute("aria-label", toggle.title);
  const currentId = state.track?.id || null;
  let changed = false;
  for (const id of new Set([renderedPreviewTrackId, currentId])) {
    const entry = previewButtons.get(id);
    if (!entry) continue;
    const { button, track } = entry;
    const old = button.dataset.icon;
    updatePreviewButton(button, track);
    changed ||= old !== button.dataset.icon;
  }
  renderedPreviewTrackId = currentId;
  if (changed || toggle.querySelector("i[data-lucide]")) createIcons({ icons });
}

function renderList() {
  const visible = visibleTracks();
  const list = $("trackList");
  list.replaceChildren();
  previewButtons.clear();
  const fragment = document.createDocumentFragment();
  for (const track of visible) fragment.append(createTrackRow(track));
  list.append(fragment);
  const empty = $("emptyState");
  empty.hidden = visible.length > 0;
  list.hidden = visible.length === 0;
  if (!empty.hidden) {
    $("emptyTitle").textContent = model.scanning ? "正在读取音乐" :
      model.tracks.length === 0 ? "目前没有可导出的音乐" :
      model.query.trim() ? "没有匹配的歌曲" :
      model.source === "remote" ? "没有需联网的歌曲" : "没有已缓存的歌曲";
  }
  createIcons({ icons });
}

function renderFooter() {
  const visible = visibleTracks();
  const allSelected = visible.length > 0 && visible.every((track) => model.selected.has(track.id));
  $("selectAllButton").textContent = allSelected ? "取消全选" : "全选";
  $("selectAllButton").disabled = !visible.length || model.scanning || model.exporting;
  $("selectUnsavedButton").disabled = model.scanning || model.exporting || !model.tracks.some((track) => model.statuses[track.id] === "notSaved");
  const summary = selectionSummary(model.selected, model.statuses);
  $("selectedCount").textContent = `已选 ${summary.total} 首`;
  const breakdown = $("selectionBreakdown");
  breakdown.hidden = !summary.total || !model.prefs.lastExportDirectory || !Object.keys(model.statuses).length;
  if (!breakdown.hidden) {
    const parts = [`待保存 ${summary.toSave} 首`];
    if (summary.saved) parts.push(`预计跳过 ${summary.saved} 首`);
    if (summary.uncertain) parts.push(`未确认 ${summary.uncertain} 首`);
    breakdown.textContent = parts.join(" · ");
  }
  const directory = model.prefs.lastExportDirectory;
  $("outputPath").textContent = directory || "首次保存时选择目录";
  $("outputPath").title = directory || "";
  $("openOutputButton").disabled = !directory || model.exporting;
  $("chooseOutputButton").disabled = model.exporting;
  $("exportButton").disabled = model.selected.size === 0 || model.scanning || model.exporting;
  $("cancelButton").hidden = !model.exporting;
  $("cancelButton").disabled = model.cancelRequested;
  $("progressArea").hidden = !model.exporting;
  if (model.progress) {
    $("progressText").textContent = model.progress.title;
    $("progressBar").max = model.progress.total || 1;
    $("progressBar").value = model.progress.completed;
    $("progressCount").textContent = `${model.progress.completed}/${model.progress.total}`;
  }
}

function renderActivity() {
  const toast = $("activityNotice");
  if (!model.status || document.querySelector("dialog[open]")) {
    if (toast.hidePopover && toast.matches(":popover-open")) toast.hidePopover();
    toast.hidden = true;
    return;
  }
  if (toast.hidden) activityFocusReturn = document.activeElement;
  $("activityText").textContent = model.status;
  toast.hidden = false;
  if (toast.showPopover && !toast.matches(":popover-open")) toast.showPopover();
}

function setActivity(message, duration = 0) {
  clearTimeout(activityTimer);
  const version = ++activityVersion;
  model.status = message;
  renderActivity();
  $("activityAnnouncement").textContent = message;
  if (duration) {
    activityTimer = setTimeout(() => {
      const toast = $("activityNotice");
      if (version !== activityVersion) return;
      if (toast.contains(document.activeElement)) {
        toast.addEventListener("focusout", () => {
          if (version === activityVersion) setActivity("");
        }, { once: true });
      } else setActivity("");
    }, duration);
  }
}

function dismissActivity() {
  const hadFocus = $("activityNotice").contains(document.activeElement);
  setActivity("");
  if (hadFocus) {
    const target = activityFocusReturn?.isConnected && !activityFocusReturn.disabled
      ? activityFocusReturn : $("searchInput");
    target.focus();
  }
}

function render() {
  $("cachedCount").textContent = model.cachedCount;
  $("remoteCount").textContent = model.remoteCount;
  $("unavailableCount").textContent = `${model.unavailableCount} 条缓存不可恢复`;
  $("unavailableCount").hidden = model.unavailableCount === 0;
  $("refreshButton").disabled = model.scanning || model.exporting;
  $("settingsButton").disabled = model.exporting;
  for (const button of document.querySelectorAll("[data-source]")) {
    button.setAttribute("aria-pressed", button.dataset.source === model.source ? "true" : "false");
  }
  renderList();
  renderFooter();
  renderActivity();
}

function showError(error) {
  $("noticeText").textContent = String(error);
  $("notice").hidden = false;
}

function clearError() { $("notice").hidden = true; }

async function refreshStatuses() {
  if (!model.tracks.length) return;
  const version = ++model.statusVersion;
  model.statuses = {};
  renderList();
  renderFooter();
  try {
    const statuses = await invoke("get_saved_statuses");
    if (version === model.statusVersion) {
      model.statuses = statuses;
      renderList();
      renderFooter();
    }
  } catch (error) {
    if (version === model.statusVersion) showError(error);
  }
}

async function refresh() {
  if (model.scanning || model.exporting) return;
  preview.stop();
  model.scanning = true;
  setActivity("正在读取音乐");
  clearError();
  render();
  let scanned = false;
  try {
    const result = await invoke("scan_music");
    const previous = new Set(model.tracks.map((track) => track.id));
    model.selected = reconcileSelection(model.selected, previous, result.tracks);
    model.tracks = result.tracks;
    model.cachedCount = result.cachedCount;
    model.remoteCount = result.remoteCount;
    model.unavailableCount = result.unavailableCount;
    model.dataRoot = result.dataRoot;
    scanned = true;
    setActivity("扫描完成", 4000);
  } catch (error) {
    setActivity("");
    showError(error);
  } finally {
    model.scanning = false;
    render();
  }
  if (scanned) await refreshStatuses();
}

async function chooseOutputDirectory() {
  try {
    const directory = await open({ directory: true, multiple: false, defaultPath: model.prefs.lastExportDirectory || undefined });
    if (!directory) return false;
    await invoke("set_export_directory", { path: directory });
    model.prefs.lastExportDirectory = directory;
    clearError();
    renderFooter();
    await refreshStatuses();
    return true;
  } catch (error) { showError(error); return false; }
}

async function runExport(ids) {
  if (model.exporting || !ids.length) return;
  if (!model.prefs.lastExportDirectory && !(await chooseOutputDirectory())) return;
  preview.stop();
  model.exporting = true;
  model.cancelRequested = false;
  model.progress = { completed: 0, total: ids.length, title: "准备导出" };
  setActivity("正在导出");
  clearError();
  render();
  try {
    const summary = await invoke("export_tracks", { ids });
    const message = `${summary.cancelled ? "已取消 · " : ""}已导出 ${summary.saved} 首 · 已跳过 ${summary.skipped} 首 · 失败 ${summary.failures.length} 首`;
    if (summary.failures.length) {
      setActivity("");
      showFailures(summary.failures, message);
    } else setActivity(message, 7000);
  } catch (error) {
    setActivity("");
    showError(error);
  } finally {
    model.exporting = false;
    model.progress = null;
    render();
    await refreshStatuses();
  }
}

async function saveOne(track) { await runExport([track.id]); }

async function playPreview(track) {
  clearError();
  try {
    await preview.play(track);
  } catch (error) {
    showError(`试听失败：${error}`);
  }
}

async function openSavedTrack(track) {
  try {
    await invoke("open_saved_track", { id: track.id });
  } catch (error) {
    showError(error);
    await refreshStatuses();
  }
}

function showFailures(failures, summary) {
  failedTrackIds = retryFailedIds(failures);
  const list = $("failureList");
  list.replaceChildren();
  for (const failure of failures) {
    const row = document.createElement("div");
    row.className = "failure-item";
    const title = document.createElement("strong");
    title.textContent = failure.title;
    const reason = document.createElement("span");
    reason.textContent = failure.reason;
    row.append(title, reason);
    list.append(row);
  }
  const dialog = $("failuresDialog");
  dialog.addEventListener("close", () => {
    const retrying = dialog.returnValue === "retry";
    dialog.returnValue = "";
    if (!retrying) setActivity(summary, 7000);
  }, { once: true });
  dialog.showModal();
}

async function selectDataRoot() {
  try {
    const path = await open({ directory: true, multiple: false, defaultPath: model.prefs.dataRootOverride || model.dataRoot || undefined });
    if (!path) return;
    await invoke("set_data_root", { root: path });
    model.prefs.dataRootOverride = path;
    $("settingsDialog").close();
    await refresh();
  } catch (error) { showSettingsError(error); }
}

function showSettingsError(error) {
  $("settingsError").textContent = String(error);
  $("settingsError").hidden = false;
}

function bindEvents() {
  $("previewToggleButton").addEventListener("click", async () => {
    try { await preview.toggle(); } catch (error) { showError(`试听失败：${error}`); }
  });
  $("previewSeek").addEventListener("input", (event) => preview.seek(Number(event.target.value)));
  $("previewCloseButton").addEventListener("click", () => {
    const id = preview.state.track?.id;
    preview.stop();
    const row = [...document.querySelectorAll("[data-preview-id]")].find((button) => button.dataset.previewId === id);
    (row || $("refreshButton")).focus();
  });
  $("dismissActivityButton").addEventListener("click", dismissActivity);
  $("refreshButton").addEventListener("click", refresh);
  $("searchInput").addEventListener("input", (event) => { model.query = event.target.value; renderList(); renderFooter(); });
  for (const button of document.querySelectorAll("[data-source]")) {
    button.addEventListener("click", () => { model.source = button.dataset.source; render(); });
  }
  $("selectAllButton").addEventListener("click", () => {
    model.selected = toggleVisibleSelection(model.selected, visibleTracks());
    renderList(); renderFooter();
  });
  $("selectUnsavedButton").addEventListener("click", () => {
    model.selected = selectOnlyUnsaved(model.tracks, model.statuses);
    renderList(); renderFooter();
  });
  $("exportButton").addEventListener("click", () => runExport([...model.selected]));
  $("chooseOutputButton").addEventListener("click", chooseOutputDirectory);
  $("openOutputButton").addEventListener("click", async () => {
    try { await invoke("open_output_directory"); } catch (error) { showError(error); }
  });
  $("cancelButton").addEventListener("click", async () => {
    model.cancelRequested = true;
    setActivity("正在取消");
    renderFooter();
    try { await invoke("cancel_export"); } catch (error) { showError(error); }
  });
  $("settingsButton").addEventListener("click", () => {
    $("dataRootPath").textContent = model.prefs.dataRootOverride || model.dataRoot || "自动查找";
    $("settingsError").hidden = true;
    $("settingsDialog").showModal();
  });
  $("closeSettingsButton").addEventListener("click", () => $("settingsDialog").close());
  $("chooseDataRootButton").addEventListener("click", selectDataRoot);
  $("autoDataRootButton").addEventListener("click", async () => {
    try {
      await invoke("set_data_root", { root: null });
      model.prefs.dataRootOverride = null;
      $("settingsDialog").close();
      await refresh();
    } catch (error) { showSettingsError(error); }
  });
  $("closeFailuresButton").addEventListener("click", () => $("failuresDialog").close());
  $("acknowledgeFailuresButton").addEventListener("click", () => $("failuresDialog").close());
  $("retryFailuresButton").addEventListener("click", () => {
    const ids = failedTrackIds;
    if (!ids.length) return;
    const dialog = $("failuresDialog");
    dialog.addEventListener("close", () => runExport(ids), { once: true });
    dialog.close("retry");
  });
}

async function main() {
  preview = createPreviewController({
    audio: $("previewAudio"),
    load: (id) => invoke("preview_cached_track", { id }),
    createUrl: (blob) => URL.createObjectURL(blob),
    revokeUrl: (url) => URL.revokeObjectURL(url),
    onChange: renderPreview,
  });
  bindEvents();
  createIcons({ icons });
  render();
  if (!window.__TAURI_INTERNALS__) {
    setActivity("请在桌面应用中打开");
    return;
  }
  getVersion().then((version) => {
    $("appVersion").textContent = `v${version}`;
    $("appVersion").hidden = false;
  }).catch((error) => console.warn("无法读取应用版本", error));
  await listen("export-progress", ({ payload }) => {
    model.progress = payload;
    renderFooter();
  });
  try {
    model.prefs = await invoke("get_preferences");
    renderFooter();
    await refresh();
    if (model.prefs.startupWarning) showError(model.prefs.startupWarning);
  } catch (error) { showError(error); }
}

main();
