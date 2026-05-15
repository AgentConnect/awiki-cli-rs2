use serde_json::Value;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn runtime_mode_and_listener_config_round_trip() {
    let workspace = TempDir::new().expect("temp workspace");

    let mode_before = awiki_cmd_with_workspace(&["runtime", "mode", "get"], workspace.path());
    assert_success(&mode_before);
    let envelope = success_json(&mode_before);
    assert_eq!(envelope["data"]["runtime"]["mode"], "websocket");

    let set_mode = awiki_cmd_with_workspace(&["runtime", "mode", "set", "http"], workspace.path());
    assert_success(&set_mode);
    let envelope = success_json(&set_mode);
    assert_eq!(envelope["data"]["mode"], "http");

    let listener_config = awiki_cmd_with_workspace(
        &[
            "runtime",
            "listener",
            "config",
            "set",
            "--enabled=false",
            "--auto-install=false",
            "--auto-start=false",
        ],
        workspace.path(),
    );
    assert_success(&listener_config);
    let envelope = success_json(&listener_config);
    assert_eq!(envelope["data"]["listener"]["enabled"], false);
    assert_eq!(envelope["data"]["listener"]["auto_install"], false);
    assert_eq!(envelope["data"]["listener"]["auto_start"], false);

    let show =
        awiki_cmd_with_workspace(&["runtime", "listener", "config", "show"], workspace.path());
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(envelope["data"]["listener"]["enabled"], false);
}

#[test]
fn runtime_validation_and_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("temp workspace");

    let invalid_mode =
        awiki_cmd_with_workspace(&["runtime", "mode", "set", "invalid"], workspace.path());
    assert_code(&invalid_mode, 2);
    let envelope = error_json(&invalid_mode);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(&envelope["error"]["message"], "unsupported runtime mode");

    let invalid_setup =
        awiki_cmd_with_workspace(&["runtime", "setup", "--mode", "invalid"], workspace.path());
    assert_code(&invalid_setup, 2);
    let envelope = error_json(&invalid_setup);
    assert_contains(
        &envelope["error"]["message"],
        "runtime setup requires --mode http|websocket",
    );

    let setup_plan = awiki_cmd_with_workspace(
        &["--dry-run", "runtime", "setup", "--mode", "websocket"],
        workspace.path(),
    );
    assert_success(&setup_plan);
    let envelope = success_json(&setup_plan);
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "runtime_setup");

    let apply_plan = awiki_cmd_with_workspace(&["--dry-run", "runtime", "apply"], workspace.path());
    assert_success(&apply_plan);
    let envelope = success_json(&apply_plan);
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "runtime_apply");
}

#[test]
fn listener_status_merges_saved_sessions_and_host_notify_state() {
    let workspace = TempDir::new().expect("temp workspace");
    let runtime_dir = workspace.path().join("runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let status_file = runtime_dir.join("listener.status.json");
    std::fs::write(
        &status_file,
        r#"{
  "mode": "websocket",
  "running": true,
  "pid": 42,
  "boot_id": "boot-new",
  "started_at": "2026-04-17T05:18:13Z",
  "sessions": [
    {"identity_name": "alice", "connected": true},
    {"identity_name": "bob", "connected": false, "last_error": "refresh websocket session JWT: unauthorized"}
  ],
  "host_notify": {
    "enabled": true,
    "sink": "log",
    "file_path": "/tmp/host-notify.events.jsonl",
    "last_error": "sink boom"
  }
}"#,
    )
    .expect("write listener status");

    let start =
        awiki_cmd_with_workspace(&["runtime", "mode", "set", "websocket"], workspace.path());
    assert_success(&start);

    let status = awiki_cmd_with_workspace(&["runtime", "listener", "status"], workspace.path());
    assert_success(&status);
    let envelope = success_json(&status);
    let listener = &envelope["data"]["listener"];
    assert_eq!(listener["running"], true);
    assert_eq!(listener["pid"], 42);
    assert_eq!(listener["boot_id"], "boot-new");
    assert_eq!(listener["started_at"], "2026-04-17T05:18:13Z");
    assert_eq!(listener["sessions"].as_array().expect("sessions").len(), 2);
    assert_eq!(listener["host_notify"]["sink"], "log");
    assert_eq!(
        listener["host_notify"]["file_path"],
        "/tmp/host-notify.events.jsonl"
    );
    assert_eq!(listener["host_notify"]["last_error"], "sink boom");
    assert!(listener["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|warning| warning.as_str().unwrap_or_default().contains(
            "websocket session for identity bob is disconnected: refresh websocket session JWT: unauthorized"
        )));
}

#[test]
fn host_notify_openclaw_config_redacts_token() {
    let workspace = TempDir::new().expect("temp workspace");

    let sink = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "config",
            "set",
            "--sink",
            "openclaw",
        ],
        workspace.path(),
    );
    assert_success(&sink);
    let envelope = success_json(&sink);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "openclaw");

    let hook = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "openclaw",
            "set",
            "--hook-url",
            "http://127.0.0.1:18789/hooks/agent",
        ],
        workspace.path(),
    );
    assert_success(&hook);
    let envelope = success_json(&hook);
    assert_eq!(
        envelope["data"]["openclaw"]["hook_url"],
        "http://127.0.0.1:18789/hooks/agent"
    );

    let set_token = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "openclaw",
            "set-token",
            "--value",
            "super-secret-token",
        ],
        workspace.path(),
    );
    assert_success(&set_token);
    assert_not_contains(
        &String::from_utf8_lossy(&set_token.stdout),
        "super-secret-token",
    );
    assert_not_contains(
        &String::from_utf8_lossy(&set_token.stderr),
        "super-secret-token",
    );

    let show = awiki_cmd_with_workspace(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    assert_not_contains(&String::from_utf8_lossy(&show.stdout), "super-secret-token");
    let envelope = success_json(&show);
    assert_eq!(
        envelope["data"]["host_notify"]["openclaw"]["token_configured"],
        true
    );
    assert_eq!(
        envelope["data"]["host_notify"]["openclaw"]["token_source"],
        "config_file"
    );
    assert!(envelope["data"]["host_notify"]["openclaw"]
        .as_object()
        .expect("openclaw object")
        .get("token")
        .is_none());
}

#[test]
fn host_notify_openclaw_routes_round_trip_and_config_view() {
    let workspace = TempDir::new().expect("temp workspace");

    let dry_add = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "openclaw",
            "route",
            "add",
            "--channel",
            " FeiShu ",
            "--to",
            " chat-1 ",
        ],
        workspace.path(),
    );
    assert_success(&dry_add);
    let envelope = success_json(&dry_add);
    assert_eq!(envelope["summary"], "Dry run: OpenClaw route add planned");
    assert_eq!(
        envelope["data"]["plan"]["action"],
        "host_notify_openclaw_route_add"
    );
    assert_eq!(envelope["data"]["plan"]["route"]["channel"], "feishu");
    assert_eq!(envelope["data"]["plan"]["route"]["to"], "chat-1");
    assert!(envelope["data"]["plan"]["route_registry_path"]
        .as_str()
        .expect("route registry path")
        .ends_with("runtime/openclaw.host-notify.routes.json"));

    let add = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "openclaw",
            "route",
            "add",
            "--session-key",
            "agent:main:telegram:direct:123456",
        ],
        workspace.path(),
    );
    assert_success(&add);
    let envelope = success_json(&add);
    assert_eq!(envelope["summary"], "OpenClaw route added");
    assert_eq!(envelope["data"]["route"]["channel"], "telegram");
    assert_eq!(envelope["data"]["route"]["to"], "123456");
    assert_eq!(
        envelope["data"]["routes"].as_array().expect("routes").len(),
        1
    );
    assert!(envelope["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap_or_default()
            .contains("confirmation webhook is deferred")));

    let duplicate = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "openclaw",
            "route",
            "add",
            "--channel",
            "Telegram",
            "--to",
            "123456",
        ],
        workspace.path(),
    );
    assert_success(&duplicate);
    let envelope = success_json(&duplicate);
    assert_eq!(envelope["summary"], "OpenClaw route already exists");
    assert_eq!(
        envelope["data"]["routes"].as_array().expect("routes").len(),
        1
    );

    let list = awiki_cmd_with_workspace(
        &["runtime", "host-notify", "openclaw", "route", "list"],
        workspace.path(),
    );
    assert_success(&list);
    let envelope = success_json(&list);
    assert_eq!(envelope["summary"], "OpenClaw routes loaded");
    assert_eq!(envelope["data"]["routes"][0]["channel"], "telegram");
    assert_eq!(envelope["data"]["routes"][0]["to"], "123456");

    let show = awiki_cmd_with_workspace(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(
        envelope["data"]["host_notify"]["routes"][0]["channel"],
        "telegram"
    );

    let dry_remove = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "openclaw",
            "route",
            "remove",
            "--channel",
            "telegram",
            "--to",
            "123456",
        ],
        workspace.path(),
    );
    assert_success(&dry_remove);
    let envelope = success_json(&dry_remove);
    assert_eq!(
        envelope["summary"],
        "Dry run: OpenClaw route remove planned"
    );
    assert_eq!(
        envelope["data"]["plan"]["action"],
        "host_notify_openclaw_route_remove"
    );

    let remove = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "openclaw",
            "route",
            "remove",
            "--channel",
            "telegram",
            "--to",
            "123456",
        ],
        workspace.path(),
    );
    assert_success(&remove);
    let envelope = success_json(&remove);
    assert_eq!(envelope["summary"], "OpenClaw route removed");
    assert!(envelope["data"]["routes"]
        .as_array()
        .expect("routes")
        .is_empty());
}

#[test]
fn host_notify_openclaw_route_validation_matches_go_contract() {
    let workspace = TempDir::new().expect("temp workspace");

    let missing = awiki_cmd_with_workspace(
        &["runtime", "host-notify", "openclaw", "route", "add"],
        workspace.path(),
    );
    assert_code(&missing, 2);
    let envelope = error_json(&missing);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "route requires either --session-key or both --channel and --to",
    );

    let conflict = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "openclaw",
            "route",
            "add",
            "--channel",
            "feishu",
            "--to",
            "chat-1",
            "--session-key",
            "agent:main:telegram:direct:123456",
        ],
        workspace.path(),
    );
    assert_code(&conflict, 2);
    let envelope = error_json(&conflict);
    assert_contains(
        &envelope["error"]["message"],
        "provide either --session-key or --channel/--to, not both",
    );
}

#[test]
fn host_notify_validation_and_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("temp workspace");

    let missing_sink = awiki_cmd_with_workspace(
        &["runtime", "host-notify", "config", "set"],
        workspace.path(),
    );
    assert_code(&missing_sink, 2);
    let envelope = error_json(&missing_sink);
    assert_contains(&envelope["error"]["message"], "requires --sink");

    let invalid_sink = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "config",
            "set",
            "--sink",
            "invalid",
        ],
        workspace.path(),
    );
    assert_code(&invalid_sink, 2);
    let envelope = error_json(&invalid_sink);
    assert_contains(
        &envelope["error"]["message"],
        "unsupported host notify sink",
    );

    let dry_run = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "config",
            "set",
            "--sink",
            "openclaw",
        ],
        workspace.path(),
    );
    assert_success(&dry_run);
    let envelope = success_json(&dry_run);
    assert_eq!(envelope["data"]["plan"]["action"], "host_notify_config_set");

    let missing_openclaw = awiki_cmd_with_workspace(
        &["runtime", "host-notify", "openclaw", "set"],
        workspace.path(),
    );
    assert_code(&missing_openclaw, 2);
    let envelope = error_json(&missing_openclaw);
    assert_contains(
        &envelope["error"]["message"],
        "requires at least one changed flag",
    );
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &std::path::Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
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
    assert_stderr_empty(output);
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn error_json(output: &Output) -> Value {
    assert_stdout_empty(output);
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn assert_stdout_empty(output: &Output) {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_stderr_empty(output: &Output) {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value.as_str().expect("value should be a string");
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
    );
}

fn assert_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "{haystack:?} should not contain {needle:?}"
    );
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-runtime-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
