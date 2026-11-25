use anylisten_core::library::scan;
use anylisten_core::store::read_cached_audio;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

const CHUNK_SIZE: usize = 1024 * 1024;
const URL: &str = "https://example.test/cached.mp3";
const NONCE: u64 = 0x1122334455667788;

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn entry(bytes: &mut [u8], block: usize, key: &str, streams: &[(usize, u32)]) {
    let start = 8192 + block * 256;
    put_u32(bytes, start + 32, key.len() as u32);
    bytes[start + 96..start + 96 + key.len()].copy_from_slice(key.as_bytes());
    for (index, (size, address)) in streams.iter().enumerate() {
        put_u32(bytes, start + 40 + index * 4, *size as u32);
        put_u32(bytes, start + 56 + index * 4, *address);
    }
}

fn write_fixture(
    root: &Path,
    include_last_chunk: bool,
    content_type: &str,
    response_end: usize,
) -> Vec<u8> {
    let directory = root.join("Partitions/view-main/Cache/Cache_Data");
    fs::create_dir_all(&directory).unwrap();
    let total = CHUNK_SIZE + 3;
    let parent_key = format!("1/0/{URL}");
    let header = format!(
        "HTTP/1.1 206\0content-type: {content_type}\0content-range: bytes 0-{}/{}\0content-length: {}\0\0",
        response_end,
        total,
        total
    );
    let mut response = vec![0; 8192 + 4096];
    put_u32(&mut response, 0, 0xc104cac3);
    put_u32(&mut response, 12, 4096);
    put_u32(&mut response, 80, 1);
    put_u32(&mut response, 8192, header.len() as u32);
    response[8196..8196 + header.len()].copy_from_slice(header.as_bytes());
    fs::write(directory.join("data_3"), response).unwrap();

    let mut entries = vec![0; 8192 + 6 * 256];
    put_u32(&mut entries, 0, 0xc104cac3);
    put_u32(&mut entries, 12, 256);
    put_u32(
        &mut entries,
        80,
        if include_last_chunk {
            0b11_1111
        } else {
            0b10_0111
        },
    );
    entry(
        &mut entries,
        0,
        &parent_key,
        &[(header.len() + 4, 0xc0030000), (0, 0), (192, 0xa0010005)],
    );
    entries[8192 + 5 * 256..8192 + 5 * 256 + 8].copy_from_slice(&NONCE.to_le_bytes());
    put_u32(&mut entries, 8192 + 5 * 256 + 8, 0xc103cac3);
    let first_key = format!("Range_{parent_key}:{NONCE:016x}:0");
    entry(
        &mut entries,
        1,
        &first_key,
        &[(0, 0), (CHUNK_SIZE, 0x80000001), (192, 0xa0010002)],
    );
    entries[8192 + 2 * 256..8192 + 2 * 256 + 8].copy_from_slice(&NONCE.to_le_bytes());
    put_u32(&mut entries, 8192 + 2 * 256 + 8, 0xc103cac3);
    if include_last_chunk {
        let last_key = format!("Range_{parent_key}:{NONCE:016x}:1");
        entry(
            &mut entries,
            3,
            &last_key,
            &[(0, 0), (3, 0x80000002), (192, 0xa0010004)],
        );
        entries[8192 + 4 * 256..8192 + 4 * 256 + 8].copy_from_slice(&NONCE.to_le_bytes());
        put_u32(&mut entries, 8192 + 4 * 256 + 8, 0xc103cac3);
    }
    fs::write(directory.join("data_1"), entries).unwrap();

    let mut index = vec![0; 368 + 8 * 4];
    put_u32(&mut index, 0, 0xc103cac3);
    put_u32(&mut index, 28, 8);
    put_u32(&mut index, 368, 0xa0010000);
    put_u32(&mut index, 372, 0xa0010001);
    if include_last_chunk {
        put_u32(&mut index, 376, 0xa0010003);
    }
    fs::write(directory.join("index"), index).unwrap();

    let mut audio = vec![0x55; CHUNK_SIZE];
    audio[..3].copy_from_slice(b"ID3");
    fs::write(directory.join("f_000001"), &audio).unwrap();
    if include_last_chunk {
        fs::write(directory.join("f_000002"), b"end").unwrap();
    }
    audio.extend_from_slice(b"end");

    let database_dir = root.join("AnyListenDatas");
    fs::create_dir_all(&database_dir).unwrap();
    let database = Connection::open(database_dir.join("anylisten.data.db")).unwrap();
    database.execute_batch("CREATE TABLE music_url (id TEXT, url TEXT); CREATE TABLE play_list_music_info (id TEXT, name TEXT, singer TEXT); CREATE TABLE my_list_music_info (id TEXT, name TEXT, singer TEXT); CREATE TABLE music_info_other_source (id TEXT, name TEXT, singer TEXT); INSERT INTO music_url VALUES ('song', 'https://example.test/cached.mp3'); INSERT INTO play_list_music_info VALUES ('song', '已缓存', '歌手');").unwrap();
    audio
}

#[test]
fn complete_windows_range_cache_is_local_and_exportable() {
    let root = tempfile::tempdir().unwrap();
    let expected = write_fixture(root.path(), true, "audio/mpeg", CHUNK_SIZE + 2);
    let result = scan(root.path()).unwrap();
    assert_eq!(result.cached.len(), 1);
    assert_eq!(result.remote.len(), 0);
    assert_eq!(result.cached[0].title, "已缓存");
    assert_eq!(read_cached_audio(&result.cached[0]).unwrap(), expected);
}

#[test]
fn incomplete_windows_range_cache_stays_remote() {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), false, "audio/mpeg", CHUNK_SIZE + 2);
    let result = scan(root.path()).unwrap();
    assert!(result.cached.is_empty());
    assert_eq!(result.remote.len(), 1);
}

#[test]
fn html_response_with_mp3_url_stays_remote() {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), true, "text/html", CHUNK_SIZE + 2);
    let result = scan(root.path()).unwrap();
    assert!(result.cached.is_empty());
    assert_eq!(result.remote.len(), 1);
}

#[test]
fn stale_range_generation_does_not_count_as_cached() {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), true, "audio/mpeg", CHUNK_SIZE + 2);
    let path = root
        .path()
        .join("Partitions/view-main/Cache/Cache_Data/data_1");
    let mut entries = fs::read(&path).unwrap();
    entries[8192 + 5 * 256..8192 + 5 * 256 + 8]
        .copy_from_slice(&0x9988776655443322u64.to_le_bytes());
    fs::write(path, entries).unwrap();
    let result = scan(root.path()).unwrap();
    assert!(result.cached.is_empty());
    assert_eq!(result.remote.len(), 1);
}

#[test]
fn complete_chunks_with_partial_initial_response_are_cached() {
    let root = tempfile::tempdir().unwrap();
    let expected = write_fixture(root.path(), true, "audio/mpeg", CHUNK_SIZE - 1);
    let result = scan(root.path()).unwrap();
    assert_eq!(result.cached.len(), 1);
    assert_eq!(read_cached_audio(&result.cached[0]).unwrap(), expected);
}

#[test]
fn corrupt_windows_index_does_not_hide_database_tracks() {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), true, "audio/mpeg", CHUNK_SIZE + 2);
    fs::write(
        root.path()
            .join("Partitions/view-main/Cache/Cache_Data/index"),
        b"broken",
    )
    .unwrap();
    let result = scan(root.path()).unwrap();
    assert!(result.cached.is_empty());
    assert_eq!(result.remote.len(), 1);
}

#[test]
fn unrelated_blockfile_write_does_not_invalidate_cached_audio() {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), true, "audio/mpeg", CHUNK_SIZE + 2);
    let cache = root.path().join("Partitions/view-main/Cache/Cache_Data");
    let mut index = fs::read(cache.join("index")).unwrap();
    let mut entries = fs::read(cache.join("data_1")).unwrap();
    let mut data = fs::read(cache.join("data_3")).unwrap();
    let header = format!("HTTP/1.1 200\0content-type: audio/mpeg\0content-length: 3\0\0");
    put_u32(&mut data, 8192, header.len() as u32);
    data[8196..8196 + header.len()].copy_from_slice(header.as_bytes());
    data.extend_from_slice(&[0; 4096]);
    data[8192 + 4096..8192 + 4099].copy_from_slice(b"ID3");
    put_u32(&mut data, 80, 0b11);
    let parent_key = format!("1/0/{URL}");
    entry(
        &mut entries,
        0,
        &parent_key,
        &[(header.len() + 4, 0xc0030000), (3, 0xc0030001)],
    );
    put_u32(&mut index, 372, 0);
    put_u32(&mut index, 376, 0);
    fs::write(cache.join("index"), index).unwrap();
    fs::write(cache.join("data_1"), entries).unwrap();
    fs::write(cache.join("data_3"), data).unwrap();
    let result = scan(root.path()).unwrap();
    assert_eq!(result.cached.len(), 1);
    assert_eq!(read_cached_audio(&result.cached[0]).unwrap(), b"ID3");
    let path = cache.join("data_3");
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    use std::io::Write;
    file.write_all(b"unrelated cache data").unwrap();
    assert_eq!(read_cached_audio(&result.cached[0]).unwrap(), b"ID3");
}
