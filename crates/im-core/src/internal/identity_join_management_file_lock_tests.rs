use super::*;
use std::fs::OpenOptions;

fn open(path: &std::path::Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap()
}

#[test]
fn one_owner_defers_contenders_until_the_handle_is_dropped() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("worker.lock");
    let owner = try_acquire(open(&path)).unwrap().expect("first owner");
    assert!(try_acquire(open(&path))
        .expect("contention must defer, not fail")
        .is_none());
    assert!(try_acquire(open(&path))
        .expect("repeated contender")
        .is_none());
    drop(owner);
    assert!(try_acquire(open(&path)).unwrap().is_some());
}

#[test]
fn recognizes_the_platform_fs2_contention_error() {
    assert!(is_contention(&fs2::lock_contended_error()));
    assert!(is_contention(&io::Error::from(io::ErrorKind::WouldBlock)));
}

#[test]
fn unrelated_io_errors_remain_failures() {
    assert!(!is_contention(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
    assert!(!is_contention(&io::Error::from(io::ErrorKind::NotFound)));
    assert!(!is_contention(&io::Error::other(
        "synthetic failure without an OS code"
    )));
}
