use anylisten_core::records::Settings;
use std::fs;

#[test]
fn saved_remote_record_survives_restart_and_invalidates_on_change() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    let audio = output.join("song.mp3");
    fs::write(&audio, b"abcdef").unwrap();
    let settings_file = root.path().join("config/settings.json");
    let mut settings = Settings::default();
    assert!(!settings.is_remote_saved("remote:one", &output));
    settings
        .record_remote("remote:one", &output, &audio)
        .unwrap();
    settings.save(&settings_file).unwrap();
    let settings = Settings::load(&settings_file).unwrap();
    assert!(settings.is_remote_saved("remote:one", &output));
    assert_eq!(
        settings.saved_remote_path("remote:one", &output),
        Some(audio.canonicalize().unwrap())
    );
    fs::write(&audio, b"changed length").unwrap();
    assert!(!settings.is_remote_saved("remote:one", &output));
    assert_eq!(settings.saved_remote_path("remote:one", &output), None);
}

#[test]
fn corrupt_settings_are_backed_up_before_recovery() {
    let root = tempfile::tempdir().unwrap();
    let settings_file = root.path().join("settings.json");
    fs::write(&settings_file, b"{broken json").unwrap();

    let (settings, backup) = Settings::load_recovering(&settings_file).unwrap();
    let backup = backup.expect("损坏的设置必须备份");
    assert_eq!(fs::read(&backup).unwrap(), b"{broken json");
    assert!(settings.last_export_directory.is_none());
    settings.save(&settings_file).unwrap();
    assert!(Settings::load(&settings_file).is_ok());
    assert_eq!(fs::read(&backup).unwrap(), b"{broken json");
}

#[test]
fn older_settings_without_saved_records_remain_valid() {
    let root = tempfile::tempdir().unwrap();
    let settings_file = root.path().join("settings.json");
    fs::write(&settings_file, r#"{"last_export_directory":"/tmp/music"}"#).unwrap();

    let (settings, backup) = Settings::load_recovering(&settings_file).unwrap();
    assert!(backup.is_none());
    assert_eq!(
        settings.last_export_directory.unwrap().to_string_lossy(),
        "/tmp/music"
    );
    assert!(settings.saved_remote_files.is_empty());
}
