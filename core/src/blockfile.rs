use crate::cache::audio_extension;
use crate::library::FileStamp;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

// Chromium blockfile cache: index buckets point to entries in data_N;
// sparse audio bodies live in Range_ entries, usually in f_ files.
const INDEX_MAGIC: u32 = 0xc103cac3;
const BLOCK_MAGIC: u32 = 0xc104cac3;
const INDEX_HEADER_SIZE: usize = 368;
const BLOCK_HEADER_SIZE: usize = 8192;
const CHUNK_SIZE: usize = 1024 * 1024;
const MAX_AUDIO_SIZE: usize = 1_000_000_000;
const MAX_METADATA_SIZE: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub path: PathBuf,
    pub offset: u64,
    pub length: usize,
    pub digest: u64,
}

#[derive(Debug)]
pub struct BlockfileTrack {
    pub id: String,
    pub url: String,
    pub extension: String,
    pub total_size: usize,
    pub chunks: Vec<Chunk>,
}

#[derive(Clone, Copy)]
struct Stream {
    size: usize,
    address: u32,
}

struct Entry {
    address: u32,
    key: String,
    streams: [Stream; 4],
}

struct BlockFile {
    path: PathBuf,
    bytes: Vec<u8>,
    entry_size: usize,
}

struct Reader {
    directory: PathBuf,
    files: HashMap<u8, BlockFile>,
}

struct AudioResponse {
    url: String,
    extension: String,
    total_size: usize,
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn valid_size(value: u32) -> Option<usize> {
    let size = value as usize;
    (size <= MAX_AUDIO_SIZE).then_some(size)
}

impl Reader {
    fn open(directory: &Path) -> Result<Self, String> {
        let mut files = HashMap::new();
        for item in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let item = item.map_err(|error| error.to_string())?;
            let name = item.file_name().to_string_lossy().into_owned();
            let Some(number) = name
                .strip_prefix("data_")
                .and_then(|n| n.parse::<u8>().ok())
            else {
                continue;
            };
            let bytes = fs::read(item.path())
                .map_err(|error| format!("无法读取 Windows 缓存块文件 {name}：{error}"))?;
            if bytes.len() < BLOCK_HEADER_SIZE || u32_at(&bytes, 0) != Some(BLOCK_MAGIC) {
                return Err(format!("Windows 缓存块文件 {name} 格式无效"));
            }
            let entry_size = u32_at(&bytes, 12).unwrap_or(0) as usize;
            if entry_size == 0 || entry_size > 4096 {
                return Err(format!("Windows 缓存块文件 {name} 块大小无效"));
            }
            files.insert(
                number,
                BlockFile {
                    path: item.path(),
                    bytes,
                    entry_size,
                },
            );
        }
        Ok(Self {
            directory: directory.to_path_buf(),
            files,
        })
    }

    fn location(&self, address: u32, length: usize) -> Option<Chunk> {
        if address & 0x8000_0000 == 0 || length > MAX_AUDIO_SIZE {
            return None;
        }
        let kind = (address >> 28) & 7;
        if kind == 0 {
            let number = address & 0x0fff_ffff;
            let path = self.directory.join(format!("f_{number:06x}"));
            if FileStamp::capture(&path).ok()?.size < length as u64 {
                return None;
            }
            return Some(Chunk {
                path,
                offset: 0,
                length,
                digest: 0,
            });
        }
        if kind > 4 || address & 0x0c00_0000 != 0 {
            return None;
        }
        let file = self.files.get(&((address >> 16) as u8))?;
        let blocks = ((address >> 24) & 3) as usize + 1;
        let first = (address & 0xffff) as usize;
        let capacity = blocks.checked_mul(file.entry_size)?;
        let start = BLOCK_HEADER_SIZE.checked_add(first.checked_mul(file.entry_size)?)?;
        if first + blocks > (BLOCK_HEADER_SIZE - 80) * 8
            || length > capacity
            || start.checked_add(capacity)? > file.bytes.len()
        {
            return None;
        }
        for block in first..first + blocks {
            let word = u32_at(&file.bytes, 80 + (block / 32) * 4)?;
            if word & (1 << (block % 32)) == 0 {
                return None;
            }
        }
        Some(Chunk {
            path: file.path.clone(),
            offset: start as u64,
            length,
            digest: 0,
        })
    }

    fn read(&self, address: u32, length: usize) -> Option<Vec<u8>> {
        let location = self.location(address, length)?;
        if (address >> 28) & 7 != 0 {
            let file = self.files.get(&((address >> 16) as u8))?;
            let start = location.offset as usize;
            return Some(file.bytes.get(start..start + length)?.to_vec());
        }
        let mut file = File::open(location.path).ok()?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }

    fn entry(&self, address: u32) -> Option<(Option<Entry>, u32)> {
        if (address >> 28) & 7 != 2 {
            return None;
        }
        let file = self.files.get(&((address >> 16) as u8))?;
        let blocks = ((address >> 24) & 3) as usize + 1;
        let bytes = self.read(address, blocks * file.entry_size)?;
        if bytes.len() < 256 {
            return None;
        }
        let next = u32_at(&bytes, 4)?;
        if u32_at(&bytes, 20)? != 0 {
            return Some((None, next));
        }
        let key_length = u32_at(&bytes, 32)? as usize;
        if key_length == 0 || key_length > 256 * 1024 {
            return Some((None, next));
        }
        let long_key = u32_at(&bytes, 36)?;
        let key_bytes = if long_key == 0 {
            bytes.get(96..96 + key_length)?.to_vec()
        } else {
            self.read(long_key, key_length)?
        };
        if key_bytes.contains(&0) {
            return Some((None, next));
        }
        let key = String::from_utf8(key_bytes).ok()?;
        let mut streams = [Stream {
            size: 0,
            address: 0,
        }; 4];
        for (index, stream) in streams.iter_mut().enumerate() {
            stream.size = valid_size(u32_at(&bytes, 40 + index * 4)?)?;
            if index != 1 && stream.size > MAX_METADATA_SIZE {
                return Some((None, next));
            }
            stream.address = u32_at(&bytes, 56 + index * 4)?;
        }
        Some((
            Some(Entry {
                address,
                key,
                streams,
            }),
            next,
        ))
    }
}

fn entries(index: &[u8], reader: &Reader) -> Result<Vec<Entry>, String> {
    if index.len() < INDEX_HEADER_SIZE || u32_at(index, 0) != Some(INDEX_MAGIC) {
        return Err("Windows 缓存索引格式无效".into());
    }
    let table_len = match u32_at(index, 28).unwrap_or(0) as usize {
        0 => 65_536,
        length => length,
    };
    if !table_len.is_power_of_two()
        || table_len > 1_048_576
        || index.len() < INDEX_HEADER_SIZE + table_len * 4
    {
        return Err("Windows 缓存索引表不完整".into());
    }
    let mut found = Vec::new();
    let mut visited = HashSet::new();
    for bucket in 0..table_len {
        let mut address = u32_at(index, INDEX_HEADER_SIZE + bucket * 4).unwrap_or(0);
        while address != 0 && visited.insert(address) && visited.len() <= 100_000 {
            let Some((entry, next)) = reader.entry(address) else {
                break;
            };
            if let Some(entry) = entry {
                found.push(entry);
            }
            address = next;
        }
    }
    Ok(found)
}

fn url_from_key(key: &str) -> Option<&str> {
    let last = key.rsplit(' ').next()?;
    let start = last.find("https://").or_else(|| last.find("http://"))?;
    Some(&last[start..])
}

fn audio_response(entry: &Entry, reader: &Reader) -> Option<AudioResponse> {
    if entry.key.starts_with("Range_") {
        return None;
    }
    let url = url_from_key(&entry.key)?;
    let stream = entry.streams[0];
    let bytes = reader.read(stream.address, stream.size)?;
    let start = bytes.windows(5).position(|part| part == b"HTTP/")?;
    let header_end = bytes[start..]
        .windows(2)
        .position(|part| part == [0, 0])
        .map_or(bytes.len(), |offset| start + offset);
    let headers = String::from_utf8_lossy(&bytes[start..header_end]);
    let mut lines = headers.split('\0');
    let status = lines
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse::<u16>()
        .ok()?;
    if status != 200 && status != 206 {
        return None;
    }
    let mut content_type = None;
    let mut content_range = None;
    let mut content_length = None;
    let mut encoded = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-type" => content_type = Some(value.trim().to_ascii_lowercase()),
            "content-range" => content_range = Some(value.trim().to_owned()),
            "content-length" => content_length = value.trim().parse::<usize>().ok(),
            "content-encoding" => encoded = !value.trim().eq_ignore_ascii_case("identity"),
            _ => {}
        }
    }
    if encoded {
        return None;
    }
    let mime = content_type?;
    let mime = mime.split(';').next()?.trim();
    if !mime.starts_with("audio/") {
        return None;
    }
    let extension = audio_extension(mime, url)?;
    let total_size = if let Some(range) = content_range {
        let (span, total) = range.rsplit_once('/')?;
        let (first, last) = span.strip_prefix("bytes ")?.split_once('-')?;
        let total = total.parse::<usize>().ok()?;
        let first = first.parse::<usize>().ok()?;
        let last = last.parse::<usize>().ok()?;
        if first > last || last >= total {
            return None;
        }
        total
    } else if status == 200 {
        content_length?
    } else {
        return None;
    };
    if total_size == 0 || total_size > MAX_AUDIO_SIZE {
        return None;
    }
    Some(AudioResponse {
        url: url.to_owned(),
        extension: extension.to_owned(),
        total_size,
    })
}

fn range_child(entry: &Entry, reader: &Reader) -> Option<(String, u64, usize, Stream)> {
    let key = entry.key.strip_prefix("Range_")?;
    let (key, index) = key.rsplit_once(':')?;
    let (parent, nonce) = key.rsplit_once(':')?;
    let index = index.parse::<usize>().ok()?;
    let nonce = u64::from_str_radix(nonce, 16).ok()?;
    let metadata = entry.streams[2];
    if metadata.size < 12 || metadata.size > MAX_METADATA_SIZE {
        return None;
    }
    let bytes = reader.read(metadata.address, metadata.size)?;
    if u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?) != nonce
        || u32_at(&bytes, 8) != Some(INDEX_MAGIC)
    {
        return None;
    }
    Some((parent.to_owned(), nonce, index, entry.streams[1]))
}

pub fn scan(directory: &Path) -> Result<(Vec<BlockfileTrack>, usize), String> {
    let index_path = directory.join("index");
    if !index_path.is_file() {
        return Ok((Vec::new(), 0));
    }
    let index = fs::read(&index_path)
        .map_err(|error| format!("无法读取 Windows 缓存索引，请关闭 Any Listen 后重试：{error}"))?;
    let reader = Reader::open(directory)?;
    let entries = entries(&index, &reader)?;
    let mut ranges: HashMap<String, HashMap<u64, Vec<(usize, Stream)>>> = HashMap::new();
    for entry in &entries {
        if let Some((parent, nonce, index, stream)) = range_child(entry, &reader) {
            ranges
                .entry(parent)
                .or_default()
                .entry(nonce)
                .or_default()
                .push((index, stream));
        }
    }
    let mut tracks = Vec::new();
    let mut urls = HashSet::new();
    let mut incomplete_urls = HashSet::new();
    for entry in &entries {
        let Some(response) = audio_response(entry, &reader) else {
            continue;
        };
        if urls.contains(&response.url) {
            continue;
        }
        let chunks = if entry.streams[1].size == response.total_size {
            reader
                .location(entry.streams[1].address, response.total_size)
                .map(|chunk| vec![chunk])
        } else {
            let count = response.total_size.div_ceil(CHUNK_SIZE);
            let parent = entry.streams[2];
            let nonce = (parent.size >= 12)
                .then(|| reader.read(parent.address, parent.size))
                .flatten()
                .and_then(|bytes| {
                    if u32_at(&bytes, 8) != Some(INDEX_MAGIC) {
                        return None;
                    }
                    Some(u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?))
                });
            nonce
                .and_then(|nonce| ranges.get(&entry.key)?.get(&nonce))
                .and_then(|group| {
                    if group.len() != count {
                        return None;
                    }
                    let by_index: HashMap<_, _> = group
                        .iter()
                        .map(|(index, stream)| (*index, stream))
                        .collect();
                    if by_index.len() != count {
                        return None;
                    }
                    (0..count)
                        .map(|index| {
                            let expected =
                                (response.total_size - index * CHUNK_SIZE).min(CHUNK_SIZE);
                            let stream = by_index.get(&index)?;
                            (stream.size == expected)
                                .then(|| reader.location(stream.address, expected))
                                .flatten()
                        })
                        .collect::<Option<Vec<_>>>()
                })
        };
        let chunks = chunks.and_then(|mut chunks| {
            for chunk in &mut chunks {
                chunk.digest = digest(&read_region(chunk).ok()?);
            }
            Some(chunks)
        });
        if let Some(chunks) = chunks {
            urls.insert(response.url.clone());
            tracks.push(BlockfileTrack {
                id: format!("blockfile-{:08x}", entry.address),
                url: response.url,
                extension: response.extension,
                total_size: response.total_size,
                chunks,
            });
        } else {
            incomplete_urls.insert(response.url);
        }
    }
    Ok((tracks, incomplete_urls.difference(&urls).count()))
}

pub fn read_chunks(chunks: &[Chunk], total_size: usize) -> Result<Vec<u8>, String> {
    let changed = "缓存已被 Any Listen 改写，请刷新后重试";
    let mut audio = Vec::with_capacity(total_size);
    for chunk in chunks {
        let bytes = read_region(chunk).map_err(|_| changed)?;
        if digest(&bytes) != chunk.digest {
            return Err(changed.into());
        }
        audio.extend_from_slice(&bytes);
    }
    if audio.len() != total_size {
        return Err("音频缓存不完整".into());
    }
    Ok(audio)
}

fn read_region(chunk: &Chunk) -> Result<Vec<u8>, String> {
    let mut file = File::open(&chunk.path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(chunk.offset))
        .map_err(|error| error.to_string())?;
    let mut bytes = vec![0; chunk.length];
    file.read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
