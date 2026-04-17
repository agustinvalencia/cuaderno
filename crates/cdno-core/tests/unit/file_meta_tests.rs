use cdno_core::file_meta::FileMeta;
use std::fs;
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

#[test]
fn new_stores_mtime_and_size() {
    let now = SystemTime::now();
    let meta = FileMeta::new(now, 1234);
    assert_eq!(meta.mtime, now);
    assert_eq!(meta.size, 1234);
}

#[test]
fn from_std_metadata_reads_real_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sample.md");
    fs::write(&path, b"hello world").unwrap();

    let std_meta = fs::metadata(&path).unwrap();
    let expected_size = std_meta.len();
    let expected_mtime = std_meta.modified().unwrap();

    let meta: FileMeta = std_meta.try_into().unwrap();

    assert_eq!(meta.size, expected_size);
    assert_eq!(meta.size, 11);
    assert_eq!(meta.mtime, expected_mtime);
}

#[test]
fn equality_and_clone() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let a = FileMeta::new(now, 42);
    let b = a.clone();
    assert_eq!(a, b);
}
