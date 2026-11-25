use anylisten_core::download::download_remote;
use anylisten_core::library::{scan, CachedTrack, RemoteTrack, ScanResult};
use anylisten_core::lyrics::read_lyrics;
use anylisten_core::platform::{discover_data_root, settings_path, valid_data_root};
use anylisten_core::records::Settings;
use anylisten_core::store::{
    existing_cached_file, export_cached, read_cached_audio, SaveOutcome, SavedFileIndex,
};
use reqwest::blocking::Client;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    scan: Mutex<Option<ScanResult>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    startup_warning: Option<String>,
    client: Client,
    exporting: AtomicBool,
    cancel: AtomicBool,
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let settings_path = settings_path()?;
        let (settings, backup) = Settings::load_recovering(&settings_path)?;
        let startup_warning = backup.map(|path| {
            format!(
                "设置文件已损坏，已恢复默认设置；原文件备份于 {}",
                path.display()
            )
        });
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            scan: Mutex::new(None),
            settings: Mutex::new(settings),
            settings_path,
            startup_warning,
            client,
            exporting: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        })
    }

    fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings.save(&self.settings_path)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackView {
    id: String,
    title: String,
    artist: String,
    extension: String,
    total_size: Option<usize>,
    needs_network: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanView {
    data_root: String,
    cached_count: usize,
    remote_count: usize,
    unavailable_count: usize,
    tracks: Vec<TrackView>,
}

impl ScanView {
    fn from_result(data_root: &Path, result: &ScanResult) -> Self {
        let tracks = result
            .cached
            .iter()
            .map(|track| TrackView {
                id: format!("cache:{}", track.id),
                title: track.title.clone(),
                artist: track.artist.clone(),
                extension: track.extension.clone(),
                total_size: Some(track.total_size),
                needs_network: false,
            })
            .chain(result.remote.iter().map(|track| TrackView {
                id: format!("remote:{}", track.id),
                title: track.title.clone(),
                artist: track.artist.clone(),
                extension: track.extension.clone(),
                total_size: None,
                needs_network: true,
            }))
            .collect();
        Self {
            data_root: data_root.to_string_lossy().into_owned(),
            cached_count: result.cached.len(),
            remote_count: result.remote.len(),
            unavailable_count: result.unavailable_count,
            tracks,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesView {
    data_root_override: Option<String>,
    last_export_directory: Option<String>,
    startup_warning: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportProgress {
    completed: usize,
    total: usize,
    title: String,
}

#[derive(Serialize)]
pub struct TrackFailure {
    id: String,
    title: String,
    reason: String,
}

#[derive(Serialize)]
pub struct ExportSummary {
    saved: usize,
    skipped: usize,
    cancelled: bool,
    failures: Vec<TrackFailure>,
}

#[tauri::command]
pub fn get_preferences(state: State<'_, Arc<AppState>>) -> Result<PreferencesView, String> {
    let settings = state.settings.lock().map_err(|error| error.to_string())?;
    Ok(PreferencesView {
        data_root_override: settings
            .data_root_override
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        last_export_directory: settings
            .last_export_directory
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        startup_warning: state.startup_warning.clone(),
    })
}

#[tauri::command]
pub fn set_data_root(state: State<'_, Arc<AppState>>, root: Option<String>) -> Result<(), String> {
    let path = root.map(PathBuf::from);
    if let Some(path) = &path {
        if !valid_data_root(path) {
            return Err("所选目录不是 Any Listen 数据目录".into());
        }
    }
    let mut settings = state.settings.lock().map_err(|error| error.to_string())?;
    settings.data_root_override = path;
    state.save_settings(&settings)?;
    *state.scan.lock().map_err(|error| error.to_string())? = None;
    Ok(())
}

#[tauri::command]
pub fn set_export_directory(state: State<'_, Arc<AppState>>, path: String) -> Result<(), String> {
    let directory = PathBuf::from(path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !directory.is_dir() {
        return Err("保存路径不是文件夹".into());
    }
    let mut settings = state.settings.lock().map_err(|error| error.to_string())?;
    settings.last_export_directory = Some(directory);
    state.save_settings(&settings)
}

#[tauri::command]
pub fn open_output_directory(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let directory = state
        .settings
        .lock()
        .map_err(|error| error.to_string())?
        .last_export_directory
        .clone()
        .ok_or("尚未选择保存目录")?;
    if !directory.is_dir() {
        return Err("保存目录已不存在".into());
    }
    open::that(directory).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_saved_track(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let scan = state
            .scan
            .lock()
            .map_err(|error| error.to_string())?
            .clone()
            .ok_or("请先刷新列表")?;
        let settings = state
            .settings
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        let directory = settings
            .last_export_directory
            .clone()
            .ok_or("尚未选择保存目录")?;
        if !directory.is_dir() {
            return Err("保存目录已不存在".into());
        }
        let path = if let Some(track_id) = id.strip_prefix("cache:") {
            let track = scan
                .cached
                .iter()
                .find(|track| track.id == track_id)
                .ok_or("列表已变化，请刷新后重试")?;
            existing_cached_file(track, &directory)?
        } else if let Some(track_id) = id.strip_prefix("remote:") {
            if !scan.remote.iter().any(|track| track.id == track_id) {
                return Err("列表已变化，请刷新后重试".into());
            }
            settings.saved_remote_path(&id, &directory)
        } else {
            return Err("列表已变化，请刷新后重试".into());
        };
        let path = path.ok_or("保存文件已变化，请刷新列表")?;
        open::that(path).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn scan_music(state: State<'_, Arc<AppState>>) -> Result<ScanView, String> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let override_root = state
            .settings
            .lock()
            .map_err(|error| error.to_string())?
            .data_root_override
            .clone();
        let data_root = discover_data_root(override_root.as_deref())?;
        let result = scan(&data_root)?;
        let view = ScanView::from_result(&data_root, &result);
        *state.scan.lock().map_err(|error| error.to_string())? = Some(result);
        Ok(view)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn preview_cached_track(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<tauri::ipc::Response, String> {
    let track_id = id
        .strip_prefix("cache:")
        .ok_or("仅完整缓存歌曲可试听")?
        .to_owned();
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let track = state
            .scan
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .ok_or("请先刷新列表")?
            .cached
            .iter()
            .find(|track| track.id == track_id)
            .cloned()
            .ok_or("列表已变化，请刷新后重试")?;
        read_cached_audio(&track).map(tauri::ipc::Response::new)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_cached_lyrics(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<Option<String>, String> {
    let track_id = id
        .strip_prefix("cache:")
        .ok_or("仅完整缓存歌曲可查看歌词")?
        .to_owned();
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let music_id = state
            .scan
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .ok_or("请先刷新列表")?
            .cached
            .iter()
            .find(|track| track.id == track_id)
            .ok_or("列表已变化，请刷新后重试")?
            .music_id
            .clone();
        let Some(music_id) = music_id else {
            return Ok(None);
        };
        let override_root = state
            .settings
            .lock()
            .map_err(|error| error.to_string())?
            .data_root_override
            .clone();
        let data_root = discover_data_root(override_root.as_deref())?;
        read_lyrics(
            &data_root.join("AnyListenDatas/anylisten.data.db"),
            &music_id,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_saved_statuses(
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, String>, String> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let scan = state
            .scan
            .lock()
            .map_err(|error| error.to_string())?
            .clone()
            .ok_or("请先刷新列表")?;
        let settings = state
            .settings
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        let mut statuses = HashMap::new();
        let Some(directory) = settings
            .last_export_directory
            .clone()
            .filter(|path| path.is_dir())
        else {
            for track in scan.cached {
                statuses.insert(format!("cache:{}", track.id), "noDirectory".into());
            }
            for track in scan.remote {
                statuses.insert(format!("remote:{}", track.id), "noDirectory".into());
            }
            return Ok(statuses);
        };
        let saved_files = SavedFileIndex::scan(&directory);
        for track in scan.cached {
            let status = match &saved_files {
                Ok(index) => match read_cached_audio(&track).and_then(|audio| index.find(&audio)) {
                    Ok(Some(_)) => "saved",
                    Ok(None) => "notSaved",
                    Err(_) => "unverified",
                },
                Err(_) => "unverified",
            };
            statuses.insert(format!("cache:{}", track.id), status.into());
        }
        for track in scan.remote {
            let id = format!("remote:{}", track.id);
            let status = if settings.is_remote_saved(&id, &directory) {
                "saved"
            } else {
                "unverified"
            };
            statuses.insert(id, status.into());
        }
        Ok(statuses)
    })
    .await
    .map_err(|error| error.to_string())?
}

enum SelectedTrack {
    Cached(CachedTrack),
    Remote(RemoteTrack),
}

#[tauri::command]
pub async fn export_tracks(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
) -> Result<ExportSummary, String> {
    let state = Arc::clone(state.inner());
    if state
        .exporting
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("已有导出任务正在进行".into());
    }
    state.cancel.store(false, Ordering::Release);
    let result = tauri::async_runtime::spawn_blocking({
        let state = Arc::clone(&state);
        move || export_selected(&app, &state, ids)
    })
    .await
    .map_err(|error| error.to_string());
    state.exporting.store(false, Ordering::Release);
    result?
}

fn export_selected(
    app: &AppHandle,
    state: &AppState,
    ids: Vec<String>,
) -> Result<ExportSummary, String> {
    let scan = state
        .scan
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or("请先刷新列表")?;
    let directory = state
        .settings
        .lock()
        .map_err(|error| error.to_string())?
        .last_export_directory
        .clone()
        .ok_or("请先选择保存目录")?;
    if !directory.is_dir() {
        return Err("保存目录已不存在".into());
    }
    let cached: HashMap<_, _> = scan
        .cached
        .into_iter()
        .map(|track| (format!("cache:{}", track.id), track))
        .collect();
    let remote: HashMap<_, _> = scan
        .remote
        .into_iter()
        .map(|track| (format!("remote:{}", track.id), track))
        .collect();
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for id in ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(track) = cached.get(&id) {
            selected.push(SelectedTrack::Cached(track.clone()));
        } else if let Some(track) = remote.get(&id) {
            selected.push(SelectedTrack::Remote(track.clone()));
        } else {
            return Err("列表已变化，请刷新后重试".into());
        }
    }
    if selected.is_empty() {
        return Err("请先选择音乐".into());
    }
    let total = selected.len();
    let mut summary = ExportSummary {
        saved: 0,
        skipped: 0,
        cancelled: false,
        failures: Vec::new(),
    };
    for (index, track) in selected.into_iter().enumerate() {
        if state.cancel.load(Ordering::Acquire) {
            summary.cancelled = true;
            break;
        }
        let (track_id, title) = match &track {
            SelectedTrack::Cached(track) => (format!("cache:{}", track.id), &track.title),
            SelectedTrack::Remote(track) => (format!("remote:{}", track.id), &track.title),
        };
        let _ = app.emit(
            "export-progress",
            ExportProgress {
                completed: index,
                total,
                title: title.clone(),
            },
        );
        let outcome = match &track {
            SelectedTrack::Cached(track) => export_cached(track, &directory),
            SelectedTrack::Remote(track) => {
                download_remote(track, &directory, &state.client, || {
                    state.cancel.load(Ordering::Acquire)
                })
            }
        };
        match outcome {
            Ok(outcome) => {
                let path = match &outcome {
                    SaveOutcome::Saved(path) => {
                        summary.saved += 1;
                        path
                    }
                    SaveOutcome::Skipped(path) => {
                        summary.skipped += 1;
                        path
                    }
                };
                if let SelectedTrack::Remote(track) = &track {
                    let mut settings = state.settings.lock().map_err(|error| error.to_string())?;
                    if let Err(reason) = settings
                        .record_remote(&format!("remote:{}", track.id), &directory, path)
                        .and_then(|_| state.save_settings(&settings))
                    {
                        summary.failures.push(TrackFailure {
                            id: track_id.clone(),
                            title: track.title.clone(),
                            reason: format!("文件已保存，但状态记录失败：{reason}"),
                        });
                    }
                }
            }
            Err(_) if state.cancel.load(Ordering::Acquire) => {
                summary.cancelled = true;
                break;
            }
            Err(reason) => summary.failures.push(TrackFailure {
                id: track_id,
                title: title.clone(),
                reason,
            }),
        }
        let _ = app.emit(
            "export-progress",
            ExportProgress {
                completed: index + 1,
                total,
                title: title.clone(),
            },
        );
    }
    Ok(summary)
}

#[tauri::command]
pub fn cancel_export(state: State<'_, Arc<AppState>>) {
    state.cancel.store(true, Ordering::Release);
}
