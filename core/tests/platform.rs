use anylisten_core::platform::{candidate_roots, discover_data_root, Platform};
use std::fs;
use std::path::Path;

#[test]
fn produces_native_data_locations() {
    let home = Path::new("/users/alice");
    let config = Path::new("/users/alice/config");
    assert_eq!(
        candidate_roots(Platform::MacOS, home, config)[0],
        home.join("Library/Application Support/any-listen")
    );
    assert_eq!(
        candidate_roots(Platform::Windows, home, config)[0],
        config.join("any-listen")
    );
    assert_eq!(
        candidate_roots(Platform::Linux, home, config)[0],
        config.join("any-listen")
    );
}

#[test]
fn accepts_explicit_anylisten_data_root() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("Partitions/view-main/Cache/Cache_Data")).unwrap();
    assert_eq!(discover_data_root(Some(root.path())).unwrap(), root.path());
    assert!(discover_data_root(Some(Path::new("/missing/anylisten/root"))).is_err());
}
