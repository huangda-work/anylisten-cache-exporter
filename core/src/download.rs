use crate::cache::audio_extension;
use crate::library::RemoteTrack;
use crate::store::{save_audio, track_name, SaveOutcome};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

pub fn download_remote(
    track: &RemoteTrack,
    directory: &Path,
    client: &Client,
    is_cancelled: impl Fn() -> bool,
) -> Result<SaveOutcome, String> {
    if is_cancelled() {
        return Err("已取消导出".into());
    }
    let url = url::Url::parse(&track.url).map_err(|error| error.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("音乐地址无效".into());
    }
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(300))
        .send()
        .map_err(|error| error.to_string())?;
    if response.status().as_u16() != 200 {
        let status = response.status().as_u16();
        return Err(if status == 403 {
            "音乐地址返回 HTTP 403，可能已失效；可在 Any Listen 中播放后刷新重试".into()
        } else {
            format!("音乐地址返回 HTTP {status}")
        });
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !content_type.starts_with("audio/") && content_type != "application/octet-stream" {
        return Err("地址没有返回支持的音频文件".into());
    }
    let extension =
        audio_extension(&content_type, &track.url).ok_or("地址没有返回支持的音频文件")?;
    const MAX_SIZE: usize = 1_000_000_000;
    if response
        .content_length()
        .is_some_and(|size| size == 0 || size > MAX_SIZE as u64)
    {
        return Err("音频文件为空或过大".into());
    }
    let mut audio =
        Vec::with_capacity(response.content_length().unwrap_or(0).min(MAX_SIZE as u64) as usize);
    let mut buffer = [0; 64 * 1024];
    loop {
        if is_cancelled() {
            return Err("已取消导出".into());
        }
        let count = response
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        if audio.len() + count > MAX_SIZE {
            return Err("音频文件为空或过大".into());
        }
        audio.extend_from_slice(&buffer[..count]);
    }
    if audio.is_empty() || is_cancelled() {
        return Err("音频文件为空或过大或导出已取消".into());
    }
    save_audio(
        &audio,
        directory,
        &track_name(&track.artist, &track.title),
        extension,
    )
}
