use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub data_root_override: Option<PathBuf>,
    pub last_export_directory: Option<PathBuf>,
    pub saved_remote_files: HashMap<PathBuf, HashMap<String, SavedRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedRecord {
    pub track_id: String,
    pub directory: PathBuf,
    pub file: PathBuf,
    pub size: u64,
    pub modified_nanos: u128,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(data) => serde_json::from_slice(&data).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn load_recovering(path: &Path) -> Result<(Self, Option<PathBuf>), String> {
        match fs::read(path) {
            Ok(data) => match serde_json::from_slice(&data) {
                Ok(settings) => Ok((settings, None)),
                Err(_) => {
                    let directory = path.parent().ok_or("配置文件路径无效")?;
                    let mut backup = tempfile::Builder::new()
                        .prefix("settings-corrupt-")
                        .suffix(".json")
                        .tempfile_in(directory)
                        .map_err(|error| error.to_string())?;
                    backup.write_all(&data).map_err(|error| error.to_string())?;
                    let (_, backup_path) =
                        backup.keep().map_err(|error| error.error.to_string())?;
                    Ok((Self::default(), Some(backup_path)))
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok((Self::default(), None))
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let directory = path.parent().ok_or("配置文件路径无效")?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        let data = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        temporary
            .write_all(&data)
            .map_err(|error| error.to_string())?;
        temporary
            .persist(path)
            .map_err(|error| error.error.to_string())?;
        Ok(())
    }

    pub fn record_remote(
        &mut self,
        track_id: &str,
        directory: &Path,
        file: &Path,
    ) -> Result<(), String> {
        let directory = directory
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let file = file.canonicalize().map_err(|error| error.to_string())?;
        if file.parent() != Some(directory.as_path()) {
            return Err("保存文件不在所选目录".into());
        }
        let metadata = fs::metadata(&file).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("保存路径不是文件".into());
        }
        let record = SavedRecord {
            track_id: track_id.to_owned(),
            directory: directory.clone(),
            file,
            size: metadata.len(),
            modified_nanos: modified_nanos(&metadata)?,
        };
        self.saved_remote_files
            .entry(directory)
            .or_default()
            .insert(track_id.to_owned(), record);
        Ok(())
    }

    pub fn is_remote_saved(&self, track_id: &str, directory: &Path) -> bool {
        self.saved_remote_path(track_id, directory).is_some()
    }

    pub fn saved_remote_path(&self, track_id: &str, directory: &Path) -> Option<PathBuf> {
        let Ok(directory) = directory.canonicalize() else {
            return None;
        };
        let Some(record) = self
            .saved_remote_files
            .get(&directory)
            .and_then(|records| records.get(track_id))
        else {
            return None;
        };
        if record.track_id != track_id
            || record.directory != directory
            || record.file.parent() != Some(directory.as_path())
        {
            return None;
        }
        fs::metadata(&record.file).ok().and_then(|metadata| {
            (metadata.is_file()
                && metadata.len() == record.size
                && modified_nanos(&metadata).ok() == Some(record.modified_nanos))
            .then(|| record.file.clone())
        })
    }
}

fn modified_nanos(metadata: &fs::Metadata) -> Result<u128, String> {
    metadata
        .modified()
        .map_err(|error| error.to_string())?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| error.to_string())
}
