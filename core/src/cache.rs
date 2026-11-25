#[derive(Debug, Clone)]
pub struct CacheBlock {
    pub offset: usize,
    pub data_offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone)]
pub struct ParsedCache {
    pub url: String,
    pub total_size: usize,
    pub extension: String,
    pub blocks: Vec<CacheBlock>,
}

const FILE_MAGIC: [u8; 8] = [0x30, 0x5c, 0x72, 0xa7, 0x1b, 0x6d, 0xfb, 0xfc];
const BLOCK_MAGIC: [u8; 8] = [0x6b, 0x67, 0x53, 0x65, 0x01, 0xbf, 0x97, 0xeb];
const MAX_AUDIO_SIZE: usize = 1_000_000_000;

pub fn parse_cache(sparse: &[u8], metadata: &[u8]) -> Result<ParsedCache, String> {
    let (sparse_key, mut position) = cache_key(sparse)?;
    let (metadata_key, header_start) = cache_key(metadata)?;
    if sparse_key != metadata_key {
        return Err("缓存记录不匹配".into());
    }
    let url_start = sparse_key
        .find("https://")
        .or_else(|| sparse_key.find("http://"))
        .ok_or("缺少音频地址")?;
    let url = sparse_key[url_start..].to_owned();
    let headers = String::from_utf8_lossy(&metadata[header_start..]);
    let mut status = None;
    let mut content_type = None;
    let mut content_range = None;
    let mut content_length = None;
    for line in headers.split(['\0', '\r', '\n']) {
        let line = line.trim();
        if line.starts_with("HTTP/") {
            status = line
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse::<u16>().ok());
        } else if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-type" => content_type = Some(value.trim().to_ascii_lowercase()),
                "content-range" => content_range = Some(value.trim().to_owned()),
                "content-length" => content_length = value.trim().parse::<usize>().ok(),
                _ => {}
            }
        }
    }
    let status = status.ok_or("缺少 HTTP 状态")?;
    if status != 200 && status != 206 {
        return Err("音频请求未成功".into());
    }
    let content_type = content_type.ok_or("缺少响应类型")?;
    let content_type = content_type.split(';').next().unwrap_or("").trim();
    let extension = audio_extension(content_type, &url).ok_or("不是支持的音频响应")?;
    let total_size = if let Some(range) = content_range {
        let (range, total) = range.rsplit_once('/').ok_or("分段响应格式无效")?;
        let byte_range = range.strip_prefix("bytes ").ok_or("分段响应格式无效")?;
        let (start, end) = byte_range.split_once('-').ok_or("分段响应格式无效")?;
        let start = start.parse::<usize>().map_err(|_| "分段响应格式无效")?;
        let end = end.parse::<usize>().map_err(|_| "分段响应格式无效")?;
        let total = total.parse::<usize>().map_err(|_| "文件长度异常")?;
        if start > end || end >= total {
            return Err("分段响应格式无效".into());
        }
        total
    } else if status == 206 {
        return Err("分段响应缺少完整文件长度".into());
    } else {
        content_length.ok_or("缺少文件长度")?
    };
    if total_size == 0 || total_size > MAX_AUDIO_SIZE {
        return Err("文件长度异常".into());
    }

    let mut blocks = Vec::new();
    while position < sparse.len() {
        let header = sparse.get(position..position + 32).ok_or("音频块头损坏")?;
        if header[..8] != BLOCK_MAGIC {
            return Err("音频块头损坏".into());
        }
        let offset = usize::try_from(u64::from_le_bytes(header[8..16].try_into().unwrap()))
            .map_err(|_| "音频块范围异常")?;
        let length = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        position += 32;
        if length == 0
            || offset
                .checked_add(length)
                .is_none_or(|end| end > total_size)
            || sparse.get(position..position + length).is_none()
        {
            return Err("音频块范围异常".into());
        }
        blocks.push(CacheBlock {
            offset,
            data_offset: position,
            length,
        });
        position += length;
    }
    if blocks.is_empty() {
        return Err("没有音频块".into());
    }
    let mut ordered = blocks.clone();
    ordered.sort_unstable_by_key(|block| block.offset);
    let mut covered = 0;
    for block in &ordered {
        if block.offset > covered {
            return Err("音频块有缺口".into());
        }
        covered = covered.max(block.offset + block.length);
    }
    if covered != total_size {
        return Err("音频未缓存完整".into());
    }
    Ok(ParsedCache {
        url,
        total_size,
        extension: extension.to_owned(),
        blocks,
    })
}

pub fn reconstruct(sparse: &[u8], parsed: &ParsedCache) -> Result<Vec<u8>, String> {
    let mut result = vec![0; parsed.total_size];
    for block in &parsed.blocks {
        let source = sparse
            .get(block.data_offset..block.data_offset + block.length)
            .ok_or("音频块在读取时发生变化")?;
        let target = result
            .get_mut(block.offset..block.offset + block.length)
            .ok_or("音频块范围异常")?;
        target.copy_from_slice(source);
    }
    Ok(result)
}

fn cache_key(data: &[u8]) -> Result<(String, usize), String> {
    if data.get(..8) != Some(&FILE_MAGIC) || data.len() < 24 {
        return Err("文件头损坏".into());
    }
    let length = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    if length == 0 || length > 16_384 {
        return Err("缓存地址损坏".into());
    }
    let end = 24usize.checked_add(length).ok_or("缓存地址损坏")?;
    let key = std::str::from_utf8(data.get(24..end).ok_or("缓存地址损坏")?)
        .map_err(|_| "缓存地址损坏")?;
    Ok((key.to_owned(), end))
}

pub fn audio_extension(content_type: &str, url: &str) -> Option<&'static str> {
    match content_type {
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => Some("m4a"),
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "audio/ogg" => Some("ogg"),
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
        "audio/aac" => Some("aac"),
        _ => {
            let path = url.split('?').next().unwrap_or(url);
            match path.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
                "mp3" => Some("mp3"),
                "m4a" => Some("m4a"),
                "flac" => Some("flac"),
                "ogg" => Some("ogg"),
                "wav" => Some("wav"),
                "aac" => Some("aac"),
                _ => None,
            }
        }
    }
}
