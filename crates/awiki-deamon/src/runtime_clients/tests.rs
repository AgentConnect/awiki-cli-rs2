use super::*;
#[cfg(unix)]
fn executable(dir: &Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn report_never_leaks_output_and_handles_stderr_versions() {
    for kind in KINDS {
        let item =
            ClientInstallation::result(kind, Ok("secret-string /private/path 1.2.3-rc.1".into()));
        assert_eq!(item.status, "ready");
        assert_eq!(item.version.as_deref(), Some("1.2.3-rc.1"));
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("/private"));
    }
    assert_eq!(
        ClientInstallation::result("hermes", Ok(String::new())).status,
        "ready"
    );
    assert_eq!(
        ClientInstallation::result("kimi", Err("not_found")).status,
        "missing"
    );
    assert_eq!(
        ClientInstallation::result("kimi", Err("timeout")).status,
        "unknown"
    );
    assert_eq!(
        ClientInstallation::result("kimi", Err("launch_failed")).status,
        "unavailable"
    );
}

#[cfg(unix)]
#[test]
fn version_probe_only_uses_version_and_distinguishes_failure_modes() {
    let dir = tempfile::tempdir().unwrap();
    let good = executable(
        dir.path(),
        "good",
        "[ \"$1\" = --version ] || exit 8; echo 2.3.4 >&2",
    );
    assert_eq!(
        version(&probe_version(&good, Instant::now() + ITEM_TIMEOUT).unwrap()).as_deref(),
        Some("2.3.4")
    );
    let bad = executable(dir.path(), "bad", "exit 2");
    assert_eq!(
        probe_version(&bad, Instant::now() + ITEM_TIMEOUT),
        Err("version_failed")
    );
    assert_eq!(
        probe_version(&dir.path().join("missing"), Instant::now() + ITEM_TIMEOUT),
        Err("not_found")
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&good, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        probe_version(&good, Instant::now() + ITEM_TIMEOUT),
        Err("not_executable")
    );
}

#[cfg(unix)]
#[test]
fn timeout_and_exited_launcher_cleanup_descendants_and_bound_output() {
    let dir = tempfile::tempdir().unwrap();
    let child = executable(dir.path(), "child", "while :; do echo output; done");
    let start = Instant::now();
    assert_eq!(
        probe_version(&child, start + Duration::from_millis(80)),
        Err("timeout")
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    let exited = executable(
        dir.path(),
        "exited",
        "(while :; do :; done) &\necho 1.0.0\nexit 0",
    );
    assert!(probe_version(&exited, Instant::now() + ITEM_TIMEOUT).is_ok());
}

#[test]
fn cache_refresh_and_simultaneous_requests_share_one_probe() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cache = InstallationCache::default();
    let calls = AtomicUsize::new(0);
    let make = || {
        calls.fetch_add(1, Ordering::SeqCst);
        snapshot(vec![])
    };
    cache.inspect(false, make);
    cache.inspect(false, make);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    cache.inspect(true, make);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    cache.state.lock().unwrap().value.as_mut().unwrap().0 = Instant::now() - TTL;
    cache.inspect(false, make);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let (started, waiting) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(|| {
            cache.inspect(true, || {
                started.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(100));
                make()
            })
        });
        waiting.recv().unwrap();
        s.spawn(|| cache.inspect(true, || panic!("joined request must not probe")));
    });
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}
