use serde_json::Value;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn openclaw_config_probe_env_port_wins_and_keeps_config_path_and_token() {
    let workspace = TempDir::new().expect("temp workspace");
    let openclaw_config = workspace.path().join("openclaw.json");
    std::fs::write(
        &openclaw_config,
        r#"{"gateway":{"port":25307},"hooks":{"path":"custom-hooks","token":"hook-token"}}"#,
    )
    .expect("write openclaw config");

    enable_openclaw_sink(&workspace);
    let show = awiki_cmd_with_workspace_env(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
        &[
            ("OPENCLAW_CONFIG_PATH", path_str(&openclaw_config)),
            ("OPENCLAW_GATEWAY_PORT", "25308"),
        ],
    );
    assert_success(&show);
    assert_not_contains(&String::from_utf8_lossy(&show.stdout), "hook-token");
    let envelope = success_json(&show);
    let openclaw = &envelope["data"]["host_notify"]["openclaw"];
    assert_eq!(
        openclaw["hook_url"],
        "http://127.0.0.1:25308/custom-hooks/agent"
    );
    assert_eq!(openclaw["detected_webhook_port"], 25308);
    assert_eq!(openclaw["detected_webhook_source"], "environment");
    assert_eq!(openclaw["detected_webhook_path"], "/custom-hooks/agent");
    assert_eq!(openclaw["detected_webhook_path_source"], "openclaw_config");
    assert_eq!(openclaw["token_configured"], true);
    assert_eq!(openclaw["token_source"], "openclaw_config");
}

#[test]
fn openclaw_config_probe_uses_home_fallback_path() {
    let workspace = TempDir::new().expect("temp workspace");
    let home = TempDir::new().expect("temp home");
    let openclaw_dir = home.path().join(".openclaw");
    std::fs::create_dir_all(&openclaw_dir).expect("openclaw dir");
    std::fs::write(
        openclaw_dir.join("openclaw.json"),
        r#"{"gateway":{"port":25309},"hooks":{"path":"/home-hooks","token":"home-token"}}"#,
    )
    .expect("write home openclaw config");

    enable_openclaw_sink(&workspace);
    let show = awiki_cmd_with_workspace_env(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
        &[("HOME", path_str(home.path()))],
    );
    assert_success(&show);
    assert_not_contains(&String::from_utf8_lossy(&show.stdout), "home-token");
    let envelope = success_json(&show);
    let openclaw = &envelope["data"]["host_notify"]["openclaw"];
    assert_eq!(
        openclaw["hook_url"],
        "http://127.0.0.1:25309/home-hooks/agent"
    );
    assert_eq!(openclaw["detected_webhook_source"], "openclaw_config");
    assert_eq!(openclaw["detected_webhook_path_source"], "openclaw_config");
    assert_eq!(openclaw["token_source"], "openclaw_config");
}

#[test]
fn openclaw_config_probe_silently_falls_back_for_missing_or_invalid_config() {
    let workspace = TempDir::new().expect("temp workspace");
    enable_openclaw_sink(&workspace);

    let missing_show = awiki_cmd_with_workspace_env(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
        &[(
            "OPENCLAW_CONFIG_PATH",
            path_str(&workspace.path().join("missing.json")),
        )],
    );
    assert_success(&missing_show);
    let envelope = success_json(&missing_show);
    let openclaw = &envelope["data"]["host_notify"]["openclaw"];
    assert_eq!(openclaw["hook_url"], "http://127.0.0.1:18789/hooks/agent");
    assert_eq!(openclaw["detected_webhook_source"], "default");
    assert_eq!(openclaw["detected_webhook_path_source"], "default");
    assert_eq!(openclaw["token_configured"], false);
    assert_eq!(openclaw["token_source"], "unset");

    let invalid_config = workspace.path().join("invalid-openclaw.json");
    std::fs::write(
        &invalid_config,
        r#"{"gateway":{"port":"25307"},"hooks":{"path":"/bad","token":"bad-token"}}"#,
    )
    .expect("write invalid openclaw config");
    let invalid_show = awiki_cmd_with_workspace_env(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
        &[("OPENCLAW_CONFIG_PATH", path_str(&invalid_config))],
    );
    assert_success(&invalid_show);
    assert_not_contains(&String::from_utf8_lossy(&invalid_show.stdout), "bad-token");
    let envelope = success_json(&invalid_show);
    let openclaw = &envelope["data"]["host_notify"]["openclaw"];
    assert_eq!(openclaw["hook_url"], "http://127.0.0.1:18789/hooks/agent");
    assert_eq!(openclaw["detected_webhook_source"], "default");
    assert_eq!(openclaw["detected_webhook_path"], "/hooks/agent");
    assert_eq!(openclaw["token_source"], "unset");
}

#[test]
fn openclaw_config_probe_matches_go_positive_int_port_behavior() {
    let workspace = TempDir::new().expect("temp workspace");
    let openclaw_config = workspace.path().join("openclaw-large-port.json");
    std::fs::write(
        &openclaw_config,
        r#"{"gateway":{"port":70000},"hooks":{"path":"/large"}}"#,
    )
    .expect("write openclaw config");

    enable_openclaw_sink(&workspace);
    let config_show = awiki_cmd_with_workspace_env(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
        &[("OPENCLAW_CONFIG_PATH", path_str(&openclaw_config))],
    );
    assert_success(&config_show);
    let envelope = success_json(&config_show);
    let openclaw = &envelope["data"]["host_notify"]["openclaw"];
    assert_eq!(openclaw["hook_url"], "http://127.0.0.1:70000/large/agent");
    assert_eq!(openclaw["detected_webhook_port"], 70000);
    assert_eq!(openclaw["detected_webhook_source"], "openclaw_config");

    let env_show = awiki_cmd_with_workspace_env(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
        &[
            ("OPENCLAW_CONFIG_PATH", path_str(&openclaw_config)),
            ("OPENCLAW_GATEWAY_PORT", "70001"),
        ],
    );
    assert_success(&env_show);
    let envelope = success_json(&env_show);
    let openclaw = &envelope["data"]["host_notify"]["openclaw"];
    assert_eq!(openclaw["hook_url"], "http://127.0.0.1:70001/large/agent");
    assert_eq!(openclaw["detected_webhook_port"], 70001);
    assert_eq!(openclaw["detected_webhook_source"], "environment");
}

#[test]
fn openclaw_config_probe_normalizes_hook_paths_like_go_path_clean() {
    let cases = [
        ("custom-hooks", "/custom-hooks/agent"),
        ("/custom-hooks/", "/custom-hooks/agent"),
        ("/hooks/agent", "/hooks/agent"),
        ("/", "/agent"),
        (".", "/agent"),
        ("..", "/agent"),
        ("/a/../b/./", "/b/agent"),
    ];
    for (raw_path, want_endpoint) in cases {
        let workspace = TempDir::new().expect("temp workspace");
        let openclaw_config = workspace.path().join("openclaw-path.json");
        std::fs::write(
            &openclaw_config,
            format!(r#"{{"gateway":{{"port":25307}},"hooks":{{"path":{raw_path:?}}}}}"#),
        )
        .expect("write openclaw config");
        enable_openclaw_sink(&workspace);

        let show = awiki_cmd_with_workspace_env(
            &["runtime", "host-notify", "config", "show"],
            workspace.path(),
            &[("OPENCLAW_CONFIG_PATH", path_str(&openclaw_config))],
        );
        assert_success(&show);
        let envelope = success_json(&show);
        let openclaw = &envelope["data"]["host_notify"]["openclaw"];
        assert_eq!(
            openclaw["detected_webhook_path"], want_endpoint,
            "raw hooks.path={raw_path:?}"
        );
        assert_eq!(
            openclaw["hook_url"],
            format!("http://127.0.0.1:25307{want_endpoint}"),
            "raw hooks.path={raw_path:?}"
        );
    }
}

fn enable_openclaw_sink(workspace: &TempDir) {
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
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &std::path::Path) -> Output {
    awiki_cmd_with_workspace_env(args, workspace, &[])
}

fn awiki_cmd_with_workspace_env(
    args: &[&str],
    workspace: &std::path::Path,
    envs: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("OPENCLAW_CONFIG_PATH")
        .env_remove("OPENCLAW_GATEWAY_PORT")
        .env_remove("OPENCLAW_HOOK_TOKEN")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn assert_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "{haystack:?} should not contain {needle:?}"
    );
}

fn path_str(path: &std::path::Path) -> &str {
    path.to_str().expect("utf-8 test path")
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
            "awiki-cli-rs2-openclaw-config-test-{}-{nonce}",
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
