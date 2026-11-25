use crate::cache::{parse_cache, reconstruct};
use crate::library::{CacheSnapshot, CachedSource, CachedTrack};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(PathBuf),
    Skipped(PathBuf),
}

pub struct SavedFileIndex {
    by_size: HashMap<u64, Vec<PathBuf>>,
}

impl SavedFileIndex {
    pub fn scan(directory: &Path) -> Result<Self, String> {
        let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    by_size
                        .entry(metadata.len())
                        .or_default()
                        .push(entry.path());
                }
            }
        }
        Ok(Self { by_size })
    }

    pub fn find(&self, audio: &[u8]) -> Result<Option<PathBuf>, String> {
        let Some(paths) = self.by_size.get(&(audio.len() as u64)) else {
            return Ok(None);
        };
        for path in paths {
            let mut reader = BufReader::new(File::open(path).map_err(|error| error.to_string())?);
            let mut equal = true;
            for chunk in audio.chunks(64 * 1024) {
                let mut buffer = vec![0; chunk.len()];
                if reader.read_exact(&mut buffer).is_err() || buffer != chunk {
                    equal = false;
                    break;
                }
            }
            if equal {
                return Ok(Some(path.clone()));
            }
        }
        Ok(None)
    }
}

pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .take(160)
        .map(|character| {
            if character.is_control() || "\\/:*?\"<>|".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']);
    let mut cleaned = if cleaned.is_empty() {
        "未命名".to_owned()
    } else {
        cleaned.to_owned()
    };
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        cleaned.insert(stem.len(), '_');
    }
    cleaned
}

pub fn save_audio(
    audio: &[u8],
    directory: &Path,
    name: &str,
    extension: &str,
) -> Result<SaveOutcome, String> {
    if let Some(path) = matching_file(audio, directory)? {
        return Ok(SaveOutcome::Skipped(path));
    }
    if !matches!(extension, "mp3" | "m4a" | "flac" | "ogg" | "wav" | "aac") {
        return Err("不支持的音频扩展名".into());
    }
    let safe = safe_name(name);
    let mut temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    temporary
        .write_all(audio)
        .map_err(|error| error.to_string())?;
    temporary.flush().map_err(|error| error.to_string())?;
    for index in 1..=10_000 {
        let suffix = if index == 1 {
            String::new()
        } else {
            format!(" ({index})")
        };
        let target = directory.join(format!("{safe}{suffix}.{extension}"));
        match temporary.persist_noclobber(&target) {
            Ok(_) => return Ok(SaveOutcome::Saved(target)),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temporary = error.file;
            }
            Err(error) => return Err(error.error.to_string()),
        }
    }
    Err("同名文件过多".into())
}

pub fn export_cached(track: &CachedTrack, directory: &Path) -> Result<SaveOutcome, String> {
    let audio = read_cached_audio(track)?;
    let name = track_name(&track.artist, &track.title);
    save_audio(&audio, directory, &name, &track.extension)
}

pub fn existing_cached_file(
    track: &CachedTrack,
    directory: &Path,
) -> Result<Option<PathBuf>, String> {
    matching_file(&read_cached_audio(track)?, directory)
}

pub fn track_name(artist: &str, title: &str) -> String {
    if artist.is_empty() {
        title.to_owned()
    } else {
        format!("{artist} - {title}")
    }
}

pub fn read_cached_audio(track: &CachedTrack) -> Result<Vec<u8>, String> {
    if let CachedSource::Blockfile { chunks } = &track.source {
        return crate::blockfile::read_chunks(chunks, track.total_size);
    }
    let CachedSource::Sparse {
        sparse_path,
        metadata_path,
        parsed: original,
        snapshot,
    } = &track.source
    else {
        unreachable!()
    };
    if CacheSnapshot::capture(sparse_path, metadata_path)? != *snapshot {
        return Err("缓存已被 Any Listen 改写，请刷新后重试".into());
    }
    let sparse = fs::read(sparse_path).map_err(|error| error.to_string())?;
    let metadata = fs::read(metadata_path).map_err(|error| error.to_string())?;
    if CacheSnapshot::capture(sparse_path, metadata_path)? != *snapshot {
        return Err("缓存已被 Any Listen 改写，请刷新后重试".into());
    }
    let parsed = parse_cache(&sparse, &metadata)?;
    if parsed.url != original.url || parsed.total_size != track.total_size {
        return Err("缓存已被 Any Listen 改写，请刷新后重试".into());
    }
    let audio = reconstruct(&sparse, &parsed)?;
    if CacheSnapshot::capture(sparse_path, metadata_path)? != *snapshot {
        return Err("缓存已被 Any Listen 改写，请刷新后重试".into());
    }
    Ok(audio)
}

fn matching_file(audio: &[u8], directory: &Path) -> Result<Option<PathBuf>, String> {
    SavedFileIndex::scan(directory)?.find(audio)
}
