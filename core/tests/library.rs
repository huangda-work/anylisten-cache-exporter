use anylisten_core::library::{scan, CachedSource};
use anylisten_core::store::read_cached_audio;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

fn write_cache(root: &Path, stem: &str, data: &[u8], total: usize) {
    let cache = root.join("Partitions/view-main/Cache/Cache_Data");
    fs::create_dir_all(&cache).unwrap();
    let key = b"1/0/https://example.test/cached.mp3";
    let mut sparse = vec![0; 24];
    sparse[..8].copy_from_slice(&[0x30, 0x5c, 0x72, 0xa7, 0x1b, 0x6d, 0xfb, 0xfc]);
    sparse[12..16].copy_from_slice(&(key.len() as u32).to_le_bytes());
    sparse.extend_from_slice(key);
    sparse.extend_from_slice(&[0x6b, 0x67, 0x53, 0x65, 0x01, 0xbf, 0x97, 0xeb]);
    sparse.extend_from_slice(&0u64.to_le_bytes());
    sparse.extend_from_slice(&(data.len() as u32).to_le_bytes());
    sparse.extend_from_slice(&[0; 12]);
    sparse.extend_from_slice(data);
    let mut metadata = vec![0; 24];
    metadata[..8].copy_from_slice(&[0x30, 0x5c, 0x72, 0xa7, 0x1b, 0x6d, 0xfb, 0xfc]);
    metadata[12..16].copy_from_slice(&(key.len() as u32).to_le_bytes());
    metadata.extend_from_slice(key);
    metadata.extend_from_slice(
        format!(
            "HTTP/1.1 206\0content-type:audio/mpeg\0content-range:bytes 0-{}/{}\0",
            total - 1,
            total
        )
        .as_bytes(),
    );
    fs::write(cache.join(format!("{stem}_s")), sparse).unwrap();
    fs::write(cache.join(format!("{stem}_0")), metadata).unwrap();
}

fn write_database(root: &Path) {
    let directory = root.join("AnyListenDatas");
    fs::create_dir_all(&directory).unwrap();
    let database = Connection::open(directory.join("anylisten.data.db")).unwrap();
    database.execute_batch("CREATE TABLE music_url (id TEXT, url TEXT); CREATE TABLE play_list_music_info (id TEXT, name TEXT, singer TEXT); CREATE TABLE my_list_music_info (id TEXT, name TEXT, singer TEXT); CREATE TABLE music_info_other_source (id TEXT, name TEXT, singer TEXT); INSERT INTO music_url VALUES ('one', 'https://example.test/cached.mp3'), ('two', 'https://example.test/remote.mp3'); INSERT INTO play_list_music_info VALUES ('one', '缓存歌', '甲'), ('two', '联网歌', '乙');").unwrap();
}

#[test]
fn finds_cached_and_remote_songs_without_duplicates() {
    let root = tempfile::tempdir().unwrap();
    write_database(root.path());
    write_cache(root.path(), "record", b"abcdef", 6);
    let result = scan(root.path()).unwrap();
    assert_eq!(result.cached.len(), 1);
    assert_eq!(result.cached[0].title, "缓存歌");
    assert_eq!(result.cached[0].artist, "甲");
    assert_eq!(result.remote.len(), 1);
    assert_eq!(result.remote[0].title, "联网歌");
    assert_eq!(result.unavailable_count, 0);
}

#[test]
fn keeps_unnamed_cache_when_database_is_missing() {
    let root = tempfile::tempdir().unwrap();
    write_cache(root.path(), "record", b"abcdef", 6);
    let result = scan(root.path()).unwrap();
    assert_eq!(result.cached.len(), 1);
    assert_eq!(result.cached[0].title, "缓存 record");
    assert!(result.remote.is_empty());
}

#[test]
fn counts_incomplete_audio_without_listing_it() {
    let root = tempfile::tempdir().unwrap();
    write_cache(root.path(), "record", b"abc", 6);
    let result = scan(root.path()).unwrap();
    assert!(result.cached.is_empty());
    assert_eq!(result.unavailable_count, 1);
}

#[test]
fn preview_reads_validated_cache_and_rejects_changed_files() {
    let root = tempfile::tempdir().unwrap();
    write_cache(root.path(), "record", b"audio bytes", 11);
    let result = scan(root.path()).unwrap();
    let track = &result.cached[0];
    assert_eq!(read_cached_audio(track).unwrap(), b"audio bytes");

    let CachedSource::Sparse { sparse_path, .. } = &track.source else {
        panic!("expected sparse cache")
    };
    fs::write(sparse_path, b"changed cache").unwrap();
    assert!(read_cached_audio(track).unwrap_err().contains("缓存已被"));
}
