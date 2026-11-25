use crate::blockfile::{self, Chunk};
use crate::cache::{parse_cache, ParsedCache};
use rusqlite::{Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    pub modified: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSnapshot {
    pub sparse: FileStamp,
    pub metadata: FileStamp,
}

#[derive(Debug, Clone)]
pub struct CachedTrack {
    pub id: String,
    pub music_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub extension: String,
    pub total_size: usize,
    pub url: String,
    pub source: CachedSource,
}

#[derive(Debug, Clone)]
pub enum CachedSource {
    Sparse {
        sparse_path: PathBuf,
        metadata_path: PathBuf,
        parsed: ParsedCache,
        snapshot: CacheSnapshot,
    },
    Blockfile {
        chunks: Vec<Chunk>,
    },
}

#[derive(Debug, Clone)]
pub struct RemoteTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub url: String,
    pub extension: String,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub cached: Vec<CachedTrack>,
    pub remote: Vec<RemoteTrack>,
    pub unavailable_count: usize,
}

impl FileStamp {
    pub fn capture(path: &Path) -> Result<Self, String> {
        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        Ok(Self {
            size: metadata.len(),
            modified: metadata.modified().map_err(|error| error.to_string())?,
        })
    }
}

impl CacheSnapshot {
    pub fn capture(sparse: &Path, metadata: &Path) -> Result<Self, String> {
        Ok(Self {
            sparse: FileStamp::capture(sparse)?,
            metadata: FileStamp::capture(metadata)?,
        })
    }
}

pub fn scan(data_root: &Path) -> Result<ScanResult, String> {
    let cache_directory = data_root.join("Partitions/view-main/Cache/Cache_Data");
    let names =
        fs::read_dir(&cache_directory).map_err(|error| format!("无法读取缓存目录：{error}"))?;
    let lookup = SongLookup::load(&data_root.join("AnyListenDatas/anylisten.data.db")).ok();
    let mut cached = Vec::new();
    let mut cached_urls = HashSet::new();
    let mut cached_ids = HashSet::new();
    let mut unavailable_count = 0;
    for entry in names {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix("_0") else {
            continue;
        };
        let metadata_path = entry.path();
        let metadata = match fs::read(&metadata_path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if !looks_like_audio(&metadata) {
            continue;
        }
        let sparse_path = cache_directory.join(format!("{stem}_s"));
        let track = (|| -> Result<CachedTrack, String> {
            let before = CacheSnapshot::capture(&sparse_path, &metadata_path)?;
            let sparse = fs::read(&sparse_path).map_err(|error| error.to_string())?;
            let parsed = parse_cache(&sparse, &metadata)?;
            if CacheSnapshot::capture(&sparse_path, &metadata_path)? != before {
                return Err("缓存已被改写".into());
            }
            let song = lookup
                .as_ref()
                .and_then(|lookup| lookup.song_for_url(&parsed.url));
            Ok(CachedTrack {
                id: stem.to_owned(),
                music_id: lookup
                    .as_ref()
                    .and_then(|lookup| lookup.ids_by_url.get(&parsed.url))
                    .cloned(),
                title: song.map_or_else(|| format!("缓存 {stem}"), |song| song.0.clone()),
                artist: song.map_or_else(String::new, |song| song.1.clone()),
                extension: parsed.extension.clone(),
                total_size: parsed.total_size,
                url: parsed.url.clone(),
                source: CachedSource::Sparse {
                    sparse_path,
                    metadata_path,
                    parsed,
                    snapshot: before,
                },
            })
        })();
        match track {
            Ok(track) => {
                cached_urls.insert(track.url.clone());
                if let Some(id) = lookup
                    .as_ref()
                    .and_then(|lookup| lookup.ids_by_url.get(&track.url))
                {
                    cached_ids.insert(id.clone());
                }
                cached.push(track);
            }
            Err(_) => unavailable_count += 1,
        }
    }
    let (blockfile_tracks, blockfile_unavailable) =
        blockfile::scan(&cache_directory).unwrap_or_else(|_| (Vec::new(), 1));
    unavailable_count += blockfile_unavailable;
    for track in blockfile_tracks {
        if !cached_urls.insert(track.url.clone()) {
            continue;
        }
        let song = lookup
            .as_ref()
            .and_then(|lookup| lookup.song_for_url(&track.url));
        if let Some(id) = lookup
            .as_ref()
            .and_then(|lookup| lookup.ids_by_url.get(&track.url))
        {
            cached_ids.insert(id.clone());
        }
        cached.push(CachedTrack {
            id: track.id.clone(),
            music_id: lookup
                .as_ref()
                .and_then(|lookup| lookup.ids_by_url.get(&track.url))
                .cloned(),
            title: song.map_or_else(|| format!("缓存 {}", track.id), |song| song.0.clone()),
            artist: song.map_or_else(String::new, |song| song.1.clone()),
            extension: track.extension,
            total_size: track.total_size,
            url: track.url,
            source: CachedSource::Blockfile {
                chunks: track.chunks,
            },
        });
    }
    cached.sort_by(|a, b| (&a.artist, &a.title).cmp(&(&b.artist, &b.title)));
    let mut remote = lookup.map_or_else(Vec::new, |lookup| {
        lookup.remote_tracks(&cached_urls, &cached_ids)
    });
    remote.sort_by(|a, b| (&a.artist, &a.title).cmp(&(&b.artist, &b.title)));
    Ok(ScanResult {
        cached,
        remote,
        unavailable_count,
    })
}

fn looks_like_audio(metadata: &[u8]) -> bool {
    let text = String::from_utf8_lossy(metadata).to_ascii_lowercase();
    text.split(['\0', '\r', '\n']).any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim() == "content-type" && value.trim().starts_with("audio/")
        })
    }) && (text.contains("http/1.1 200")
        || text.contains("http/1.1 206")
        || text.contains("http/2 200")
        || text.contains("http/2 206"))
}

struct SongLookup {
    ids_by_url: HashMap<String, String>,
    urls_by_id: HashMap<String, String>,
    songs_by_id: HashMap<String, (String, String)>,
}

impl SongLookup {
    fn load(path: &Path) -> Result<Self, String> {
        let database = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| error.to_string())?;
        let mut ids_by_url = HashMap::new();
        let mut urls_by_id = HashMap::new();
        {
            let mut statement = database
                .prepare("SELECT id, url FROM music_url")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (id, address) = row.map_err(|error| error.to_string())?;
                ids_by_url.insert(address.clone(), id.clone());
                urls_by_id.insert(id, address);
            }
        }
        let mut songs_by_id = HashMap::new();
        let sql = "SELECT id, name, singer FROM play_list_music_info UNION ALL SELECT id, name, singer FROM my_list_music_info UNION ALL SELECT id, name, singer FROM music_info_other_source";
        let mut statement = database.prepare(sql).map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (id, title, artist) = row.map_err(|error| error.to_string())?;
            songs_by_id.entry(id).or_insert((title, artist));
        }
        Ok(Self {
            ids_by_url,
            urls_by_id,
            songs_by_id,
        })
    }

    fn song_for_url(&self, url: &str) -> Option<&(String, String)> {
        let id = self.ids_by_url.get(url)?;
        self.song_for_id(id)
    }

    fn song_for_id(&self, id: &str) -> Option<&(String, String)> {
        self.songs_by_id.get(id).or_else(|| {
            let (base, suffix) = id.rsplit_once('_')?;
            (suffix.ends_with('k')
                && suffix[..suffix.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_digit()))
            .then(|| self.songs_by_id.get(base))
            .flatten()
        })
    }

    fn remote_tracks(
        &self,
        cached_urls: &HashSet<String>,
        cached_ids: &HashSet<String>,
    ) -> Vec<RemoteTrack> {
        self.urls_by_id
            .iter()
            .filter_map(|(id, address)| {
                if cached_urls.contains(address) || cached_ids.contains(id) {
                    return None;
                }
                let url = url::Url::parse(address).ok()?;
                if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
                    return None;
                }
                let song = self.song_for_id(id);
                let extension = url.path().rsplit_once('.').map_or("", |(_, suffix)| suffix);
                Some(RemoteTrack {
                    id: id.clone(),
                    title: song.map_or_else(|| format!("歌曲 {id}"), |song| song.0.clone()),
                    artist: song.map_or_else(String::new, |song| song.1.clone()),
                    url: address.clone(),
                    extension: extension.to_ascii_lowercase(),
                })
            })
            .collect()
    }
}
