use super::*;
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    let spec: Value = serde_json::from_str(SPECIFICATION).unwrap();
    let manifest = json!({"schema_version":1,"platform":"darwin-arm64","available":true,
        "runtime":spec["runtime"],"adapters":spec["adapters"]});
    for adapter in spec["adapters"].as_object().unwrap().values() {
        let path = root.path().join(adapter["entry"].as_str().unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "// synthetic adapter").unwrap();
    }
    node_fixture(root.path(), "v24.12.0");
    std::fs::write(root.path().join("manifest.json"), manifest.to_string()).unwrap();
    (root, manifest)
}

#[test]
fn uses_detected_host_node_and_exact_host_cli_for_both_adapters() {
    let (root, _) = fixture();
    for (brand, env) in [
        (Brand::Codex, "CODEX_PATH"),
        (Brand::ClaudeCode, "CLAUDE_CODE_EXECUTABLE"),
    ] {
        let adapter = Adapter::from_directory(root.path(), brand, "darwin-arm64").unwrap();
        let native = Path::new("/host path/client");
        let launch = adapter.launch(native).unwrap();
        assert_eq!(launch.command(), &root.path().join("node"));
        assert_eq!(launch.arguments().len(), 1);
        assert_eq!(
            launch.environment().get(env).unwrap(),
            native.to_str().unwrap()
        );
        assert!(!adapter.version.is_empty());
    }
}

#[test]
fn mismatched_component_never_falls_back_to_npx() {
    let (root, mut manifest) = fixture();
    manifest["runtime"] = json!({"kind":"bundled"});
    std::fs::write(root.path().join("manifest.json"), manifest.to_string()).unwrap();
    assert!(Adapter::from_directory(root.path(), Brand::Codex, "darwin-arm64").is_err());
}

#[test]
fn unsupported_platform_is_explicit_before_component_lookup() {
    assert_eq!(
        Adapter::from_directory(Path::new("/absent"), Brand::Codex, "linux-arm64")
            .err()
            .unwrap()
            .to_string(),
        "acp_adapter_platform_unsupported"
    );
}

#[cfg(unix)]
#[test]
fn rejects_adapter_directory_escape() {
    use std::os::unix::fs::symlink;
    let (root, _) = fixture();
    let outside = tempfile::tempdir().unwrap();
    let directory = root
        .path()
        .join("node_modules/@agentclientprotocol/codex-acp/dist");
    std::fs::remove_dir_all(&directory).unwrap();
    std::fs::write(outside.path().join("index.js"), "other adapter").unwrap();
    symlink(outside.path(), directory).unwrap();
    assert!(Adapter::from_directory(root.path(), Brand::Codex, "darwin-arm64").is_err());
}

fn node_fixture(root: &Path, version: &str) {
    use std::os::unix::fs::PermissionsExt;
    let node = root.join("node");
    std::fs::write(&node, format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n")).unwrap();
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o700)).unwrap();
    crate::cli_runtime_env::set_test_client("node", node);
}

#[test]
fn host_node_versions_missing_and_permissions_are_checked_on_every_launch() {
    let (root, _) = fixture();
    let adapter = Adapter::from_directory(root.path(), Brand::Codex, "darwin-arm64").unwrap();
    for version in ["v22.0.0", "v24.12.0", "v26.0.0"] {
        node_fixture(root.path(), version);
        adapter
            .validate_node(Instant::now() + Duration::from_secs(5))
            .unwrap();
    }
    for version in ["v20.20.0", "not-node", "v24.0.0-preview", "v24.0.0 extra"] {
        node_fixture(root.path(), version);
        assert_eq!(
            adapter.validate_node(Instant::now() + Duration::from_secs(5)),
            Err("node_incompatible")
        );
    }
    std::fs::remove_file(&adapter.node).unwrap();
    assert_eq!(
        adapter.validate_node(Instant::now() + Duration::from_secs(5)),
        Err("node_missing")
    );
    assert!(adapter.launch(Path::new("/host/codex")).is_err());
    node_fixture(root.path(), "v24.0.0");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&adapter.node, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        adapter.validate_node(Instant::now() + Duration::from_secs(5)),
        Err("node_unavailable")
    );
}

#[test]
fn host_node_supports_installation_symlinks_and_bounded_probe() {
    let (root, _) = fixture();
    let real = root.path().join("node-real");
    std::fs::rename(root.path().join("node"), &real).unwrap();
    std::os::unix::fs::symlink(&real, root.path().join("node")).unwrap();
    let adapter = Adapter::from_directory(root.path(), Brand::ClaudeCode, "darwin-arm64").unwrap();
    assert!(adapter.launch(Path::new("/host/claude")).is_ok());
    std::fs::write(real, "#!/bin/sh\nexec /bin/sleep 10\n").unwrap();
    let start = Instant::now();
    assert_eq!(
        adapter.validate_node(start + Duration::from_millis(100)),
        Err("node_timeout")
    );
    assert!(start.elapsed() < Duration::from_secs(2));
}
