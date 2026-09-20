use super::*;
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    let spec: Value = serde_json::from_str(SPECIFICATION).unwrap();
    let manifest = json!({"schema_version":1,"platform":"darwin-arm64","available":true,
        "node_version":spec["node_version"],"adapters":spec["adapters"]});
    for adapter in spec["adapters"].as_object().unwrap().values() {
        let path = root.path().join(adapter["entry"].as_str().unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "// synthetic adapter").unwrap();
    }
    std::fs::write(root.path().join("node"), "synthetic runtime").unwrap();
    std::fs::write(root.path().join("manifest.json"), manifest.to_string()).unwrap();
    (root, manifest)
}

#[test]
fn uses_private_runtime_and_exact_host_cli_for_both_adapters() {
    let (root, _) = fixture();
    for (brand, env) in [
        (Brand::Codex, "CODEX_PATH"),
        (Brand::ClaudeCode, "CLAUDE_CODE_EXECUTABLE"),
    ] {
        let adapter = Adapter::from_directory(root.path(), brand, "darwin-arm64").unwrap();
        let native = Path::new("/host path/client");
        let launch = adapter.launch(native);
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
fn missing_or_mismatched_component_never_falls_back_to_npx_or_host_node() {
    let (root, mut manifest) = fixture();
    manifest["node_version"] = json!("0.0.0");
    std::fs::write(root.path().join("manifest.json"), manifest.to_string()).unwrap();
    assert!(Adapter::from_directory(root.path(), Brand::Codex, "darwin-arm64").is_err());
    let (root, _) = fixture();
    std::fs::remove_file(root.path().join("node")).unwrap();
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
fn rejects_runtime_symlinks_and_adapter_directory_escape() {
    use std::os::unix::fs::symlink;
    let (root, _) = fixture();
    let outside = tempfile::tempdir().unwrap();
    let node = root.path().join("node");
    std::fs::remove_file(&node).unwrap();
    std::fs::write(outside.path().join("node"), "other runtime").unwrap();
    symlink(outside.path().join("node"), &node).unwrap();
    assert!(Adapter::from_directory(root.path(), Brand::Codex, "darwin-arm64").is_err());
    std::fs::remove_file(&node).unwrap();
    std::fs::write(&node, "synthetic runtime").unwrap();
    let directory = root
        .path()
        .join("node_modules/@agentclientprotocol/codex-acp/dist");
    std::fs::remove_dir_all(&directory).unwrap();
    std::fs::write(outside.path().join("index.js"), "other adapter").unwrap();
    symlink(outside.path(), directory).unwrap();
    assert!(Adapter::from_directory(root.path(), Brand::Codex, "darwin-arm64").is_err());
}
