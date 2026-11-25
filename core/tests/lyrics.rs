use anylisten_core::lyrics::read_lyrics;
use rusqlite::Connection;

#[test]
fn prefers_edited_lyrics_and_falls_back_to_raw() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("music.db");
    let database = Connection::open(&path).unwrap();
    database
        .execute_batch("CREATE TABLE lyric (id TEXT, lyric TEXT, type TEXT)")
        .unwrap();
    for (id, text, kind) in [
        ("one", "[00:01.00]原词", "raw"),
        ("one", "[00:01.00]修改词", "edited"),
        ("two", "[00:02.00]原词", "raw"),
    ] {
        database
            .execute(
                "INSERT INTO lyric VALUES (?1, ?2, ?3)",
                (id, serde_json::to_string(text).unwrap(), kind),
            )
            .unwrap();
    }

    assert_eq!(
        read_lyrics(&path, "one").unwrap().as_deref(),
        Some("[00:01.00]修改词")
    );
    assert_eq!(
        read_lyrics(&path, "one_320k").unwrap().as_deref(),
        Some("[00:01.00]修改词")
    );
    assert_eq!(
        read_lyrics(&path, "two").unwrap().as_deref(),
        Some("[00:02.00]原词")
    );
    assert_eq!(read_lyrics(&path, "missing").unwrap(), None);
}

#[test]
fn reads_primary_lyrics_before_translation_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("music.db");
    let database = Connection::open(&path).unwrap();
    database
        .execute_batch("CREATE TABLE lyric (id TEXT, lyric TEXT, type TEXT)")
        .unwrap();
    let stored = format!(
        "{}\ntlyric__{}",
        serde_json::to_string("[00:01.00]原词").unwrap(),
        serde_json::to_string("[00:01.00]译文").unwrap()
    );
    database
        .execute(
            "INSERT INTO lyric VALUES (?1, ?2, ?3)",
            ("one", stored, "raw"),
        )
        .unwrap();

    assert_eq!(
        read_lyrics(&path, "one_320k").unwrap().as_deref(),
        Some("[00:01.00]原词")
    );
}

#[test]
fn ignores_empty_or_invalid_edited_lyrics() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("music.db");
    let database = Connection::open(&path).unwrap();
    database
        .execute_batch("CREATE TABLE lyric (id TEXT, lyric TEXT, type TEXT)")
        .unwrap();
    database
        .execute(
            "INSERT INTO lyric VALUES (?1, ?2, ?3)",
            (
                "one",
                serde_json::to_string("[00:01.00]原词").unwrap(),
                "raw",
            ),
        )
        .unwrap();
    database
        .execute(
            "INSERT INTO lyric VALUES (?1, ?2, ?3)",
            ("one", "broken JSON", "edited"),
        )
        .unwrap();

    assert_eq!(
        read_lyrics(&path, "one").unwrap().as_deref(),
        Some("[00:01.00]原词")
    );
}
