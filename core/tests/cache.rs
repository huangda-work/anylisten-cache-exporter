use anylisten_core::cache::{parse_cache, reconstruct};

fn fixture(blocks: &[(u64, &[u8])], total: usize, status: u16) -> (Vec<u8>, Vec<u8>) {
    let key = b"1/0/https://example.test/a.mp3";
    let mut sparse = vec![0; 24];
    sparse[..8].copy_from_slice(&[0x30, 0x5c, 0x72, 0xa7, 0x1b, 0x6d, 0xfb, 0xfc]);
    sparse[12..16].copy_from_slice(&(key.len() as u32).to_le_bytes());
    sparse.extend_from_slice(key);
    for (offset, bytes) in blocks {
        sparse.extend_from_slice(&[0x6b, 0x67, 0x53, 0x65, 0x01, 0xbf, 0x97, 0xeb]);
        sparse.extend_from_slice(&offset.to_le_bytes());
        sparse.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        sparse.extend_from_slice(&[0; 12]);
        sparse.extend_from_slice(bytes);
    }
    let mut metadata = vec![0; 24];
    metadata[..8].copy_from_slice(&[0x30, 0x5c, 0x72, 0xa7, 0x1b, 0x6d, 0xfb, 0xfc]);
    metadata[12..16].copy_from_slice(&(key.len() as u32).to_le_bytes());
    metadata.extend_from_slice(key);
    metadata.extend_from_slice(
        format!(
            "HTTP/1.1 {status}\0content-type:audio/mpeg\0content-range:bytes 0-{}/{}\0",
            total - 1,
            total
        )
        .as_bytes(),
    );
    (sparse, metadata)
}

#[test]
fn reconstructs_out_of_order_blocks() {
    let (sparse, metadata) = fixture(&[(3, b"def"), (0, b"abc")], 6, 206);
    let parsed = parse_cache(&sparse, &metadata).unwrap();
    assert_eq!(parsed.url, "https://example.test/a.mp3");
    assert_eq!(parsed.extension, "mp3");
    assert_eq!(parsed.total_size, 6);
    assert_eq!(reconstruct(&sparse, &parsed).unwrap(), b"abcdef");
}

#[test]
fn rejects_gap_and_bad_metadata() {
    let (sparse, metadata) = fixture(&[(0, b"ab"), (3, b"def")], 6, 206);
    assert!(parse_cache(&sparse, &metadata).is_err());
    let (sparse, mut metadata) = fixture(&[(0, b"abcdef")], 6, 206);
    metadata[24] = b'2';
    assert!(parse_cache(&sparse, &metadata).is_err());
}

#[test]
fn rejects_failed_http_response_and_bad_block_magic() {
    let (sparse, metadata) = fixture(&[(0, b"abcdef")], 6, 403);
    assert!(parse_cache(&sparse, &metadata).is_err());
    let (mut sparse, metadata) = fixture(&[(0, b"abcdef")], 6, 206);
    sparse[24 + b"1/0/https://example.test/a.mp3".len()] = 0;
    assert!(parse_cache(&sparse, &metadata).is_err());
}

#[test]
fn rejects_malformed_content_range() {
    let (sparse, mut metadata) = fixture(&[(0, b"abcdef")], 6, 206);
    let start = metadata
        .windows(b"content-range:".len())
        .position(|part| part == b"content-range:")
        .unwrap();
    metadata.truncate(start);
    metadata.extend_from_slice(b"content-range:banana/6\0");
    assert!(parse_cache(&sparse, &metadata).is_err());
}
