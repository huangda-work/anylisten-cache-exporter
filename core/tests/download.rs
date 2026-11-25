use anylisten_core::download::download_remote;
use anylisten_core::library::RemoteTrack;
use anylisten_core::store::SaveOutcome;
use reqwest::blocking::Client;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn response(status: &str, content_type: &str, body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let content_type = content_type.to_owned();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 2048];
        let _ = stream.read(&mut request);
        let header = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });
    format!("http://{address}/song.mp3")
}

fn track(url: String) -> RemoteTrack {
    RemoteTrack {
        id: "one".into(),
        title: "歌曲".into(),
        artist: "歌手".into(),
        url,
        extension: "mp3".into(),
    }
}

#[test]
fn saves_audio_and_skips_identical_download() {
    let root = tempfile::tempdir().unwrap();
    let client = Client::new();
    let first = download_remote(
        &track(response("200 OK", "audio/mpeg", b"abcdef")),
        root.path(),
        &client,
        || false,
    )
    .unwrap();
    assert!(matches!(first, SaveOutcome::Saved(_)));
    let second = download_remote(
        &track(response("200 OK", "audio/mpeg", b"abcdef")),
        root.path(),
        &client,
        || false,
    )
    .unwrap();
    assert!(matches!(second, SaveOutcome::Skipped(_)));
}

#[test]
fn rejects_403_and_html() {
    let root = tempfile::tempdir().unwrap();
    let client = Client::new();
    assert!(download_remote(
        &track(response("403 Forbidden", "audio/mpeg", b"denied")),
        root.path(),
        &client,
        || false
    )
    .is_err());
    assert!(download_remote(
        &track(response("200 OK", "text/html", b"<html>")),
        root.path(),
        &client,
        || false
    )
    .is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn cancellation_does_not_create_a_file() {
    let root = tempfile::tempdir().unwrap();
    let client = Client::new();
    assert!(download_remote(
        &track("https://example.test/song.mp3".into()),
        root.path(),
        &client,
        || true
    )
    .is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
