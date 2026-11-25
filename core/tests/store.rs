use anylisten_core::store::{safe_name, save_audio, SaveOutcome, SavedFileIndex};
use std::fs;

#[test]
fn repeated_content_is_skipped_even_under_another_name() {
    let root = tempfile::tempdir().unwrap();
    let first = save_audio(b"abcdef", root.path(), "歌手 - 歌曲", "mp3").unwrap();
    let first_path = match first {
        SaveOutcome::Saved(path) => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(fs::read(&first_path).unwrap(), b"abcdef");
    assert_eq!(
        save_audio(b"abcdef", root.path(), "另一个名字", "mp3").unwrap(),
        SaveOutcome::Skipped(first_path)
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn different_content_with_same_name_gets_a_suffix() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("歌手 - 歌曲.mp3");
    fs::write(&original, b"UVWXYZ").unwrap();
    let result = save_audio(b"abcdef", root.path(), "歌手 - 歌曲", "mp3").unwrap();
    let path = match result {
        SaveOutcome::Saved(path) => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(path.file_name().unwrap(), "歌手 - 歌曲 (2).mp3");
    assert_eq!(fs::read(original).unwrap(), b"UVWXYZ");
}

#[test]
fn sanitizes_windows_reserved_names_and_punctuation() {
    assert_eq!(safe_name("CON"), "CON_");
    assert_eq!(safe_name("CON.txt"), "CON_.txt");
    assert_eq!(safe_name("a\\b/c:*?\"<>|."), "a_b_c_______");
    assert_eq!(safe_name("  歌曲.  "), "歌曲");
}

#[test]
fn indexed_lookup_checks_content_after_matching_file_size() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("same-size.mp3"), b"ABCDEF").unwrap();
    let matching = root.path().join("matching.mp3");
    fs::write(&matching, b"abcdef").unwrap();
    fs::write(root.path().join("short.mp3"), b"abc").unwrap();

    let index = SavedFileIndex::scan(root.path()).unwrap();
    assert_eq!(index.find(b"abcdef").unwrap(), Some(matching));
    assert_eq!(index.find(b"UVWXYZ").unwrap(), None);
}
