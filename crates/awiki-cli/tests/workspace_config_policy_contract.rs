use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn config_show_rejects_deprecated_service_url_fields_like_go() {
    for field in [
        "service_base_url",
        "user_service_endpoint",
        "message_service_endpoint",
        "did_domain",
        "user_service_url",
        "message_service_url",
        "message_service_ws_url",
    ] {
        let workspace = TempDir::new().expect("temp workspace");
        write_tenant_config(
            workspace.path(),
            &format!("services:\n  {field}: https://awiki.ai\n"),
        );

        let output =
            awiki_cmd_with_workspace(&["config", "show"], workspace.path().to_str().unwrap());
        assert_code(&output, 1);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], "internal_error");
        assert_contains(
            &envelope["error"]["message"],
            "deprecated config.yaml fields are no longer supported",
        );
        assert_contains(&envelope["error"]["message"], &format!("services.{field}"));
    }
}

#[test]
fn config_show_rejects_empty_deprecated_service_url_field_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    write_tenant_config(workspace.path(), "services:\n  user_service_url:\n");

    let output = awiki_cmd_with_workspace(&["config", "show"], workspace.path().to_str().unwrap());
    assert_code(&output, 1);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "deprecated config.yaml fields are no longer supported",
    );
    assert_contains(&envelope["error"]["message"], "services.user_service_url");
}

#[test]
fn config_show_preserves_hash_inside_quoted_yaml_scalars_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    write_tenant_config(
        workspace.path(),
        "services:\n  ca_bundle: \"/tmp/ca#bundle.pem\" # inline comment\n",
    );

    let output = awiki_cmd_with_workspace(&["config", "show"], workspace.path().to_str().unwrap());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["service_base_url"], "https://awiki.ai");
    assert_eq!(envelope["data"]["did_domain"], "awiki.ai");
    assert_eq!(envelope["data"]["ca_bundle"], "/tmp/ca#bundle.pem");
    assert_eq!(
        envelope["data"]["sources"]["service_base_url"]["value"],
        "https://awiki.ai"
    );
}

#[test]
fn config_show_decodes_common_quoted_yaml_scalars_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    write_tenant_config(
        workspace.path(),
        concat!(
            "runtime:\n",
            "  host_notify:\n",
            "    sink: null\n",
            "output:\n",
            "  format: \"false\"\n",
        ),
    );

    let output = awiki_cmd_with_workspace(&["config", "show"], workspace.path().to_str().unwrap());
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["service_base_url"], "https://awiki.ai");
    assert_eq!(envelope["data"]["did_domain"], "awiki.ai");
    assert_eq!(envelope["data"]["host_notify_sink"], "log");
    assert_eq!(envelope["data"]["output_format"], "false");
}

#[test]
fn config_show_reports_malformed_yaml_in_config_error_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    write_tenant_config(
        workspace.path(),
        "services:\n  service_base_url: \"https://platform.example\n",
    );

    let output = awiki_cmd_with_workspace(&["config", "show"], workspace.path().to_str().unwrap());
    assert_code(&output, 1);
    let envelope = error_json(&output);
    assert_contains(
        &envelope["error"]["message"],
        "deprecated config.yaml fields are no longer supported",
    );
    assert_contains(&envelope["error"]["message"], "services.service_base_url");
    assert!(!workspace.path().join("data").exists());
    assert!(!workspace.path().join("runtime").exists());
}

fn write_tenant_config(workspace: &Path, text: &str) {
    std::fs::write(
        workspace.join("global.json"),
        r#"{"schema_version":1,"active_tenant":"default"}"#,
    )
    .expect("write global tenant config");
    let registry = workspace.join("tenants").join("registry.json");
    std::fs::create_dir_all(registry.parent().expect("registry parent"))
        .expect("create registry parent");
    std::fs::write(
        &registry,
        r#"{"schema_version":1,"tenants":[{"name":"default","display_name":"AWiki","backend_base_url":"https://awiki.ai","did_host":"awiki.ai","dir_name":"default","created_at":"2026-05-25T00:00:00Z","updated_at":"2026-05-25T00:00:00Z"}]}"#,
    )
    .expect("write tenant registry");
    let config = workspace
        .join("tenants")
        .join("default")
        .join("config.yaml");
    std::fs::create_dir_all(config.parent().expect("tenant config parent"))
        .expect("create tenant config parent");
    std::fs::write(config, text).expect("write tenant config");
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", std::path::Path::new(workspace).join("home"))
        .env("USERPROFILE", std::path::Path::new(workspace).join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty, got {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true, "success envelope should set ok=true");
    envelope
}

fn error_json(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false, "error envelope should set ok=false");
    envelope
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value
        .as_str()
        .unwrap_or_else(|| panic!("expected string containing {needle:?}, got {value:?}"));
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-test-{}-{nanos}-{counter}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
