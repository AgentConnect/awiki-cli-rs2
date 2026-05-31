use awiki_cli::workspace_config::{self, Paths};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn config_writer_uses_go_style_tempfile_permissions_and_cleanup() {
    let temp = TempDir::new("config-writer-durable").expect("temp dir");
    let paths = test_paths(&temp.path().join("nested"));

    workspace_config::update_active_identity(&paths, "alice").expect("write config");

    let config_path = Path::new(&paths.config_file);
    assert!(config_path.exists(), "config file should exist");
    assert!(
        std::fs::read_dir(config_path.parent().unwrap())
            .expect("read config dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .all(|name| !name.starts_with(".config-") || !name.ends_with(".tmp")),
        "successful config writes must not leave Go-style temp files behind"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = std::fs::metadata(config_path)
            .expect("config metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600, "config file mode should match Go");

        let dir_mode = std::fs::metadata(config_path.parent().unwrap())
            .expect("config dir metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700, "config dir mode should match Go");
    }
}

#[test]
fn config_writer_updates_schema_version_and_preserves_existing_values() {
    let temp = TempDir::new("config-writer-schema").expect("temp dir");
    let paths = test_paths(temp.path());

    workspace_config::ensure_config_schema_version(&paths.config_file)
        .expect("missing config is a no-op");
    assert!(
        !Path::new(&paths.config_file).exists(),
        "ensure_config_schema_version must not create missing config"
    );

    std::fs::write(
        &paths.config_file,
        concat!(
            "schema_version: 0\n",
            "services:\n",
            "  service_base_url: https://platform.example\n",
            "  did_domain: old.example\n",
            "  anp_service_endpoint: https://rpc.example/anp\n",
            "  anp_service_did: did:wba:rpc.example\n",
        ),
    )
    .expect("seed config");

    workspace_config::ensure_config_schema_version(&paths.config_file).expect("update schema");
    let text = read_config(&paths);
    assert_contains(&text, "schema_version: 1\n");
    assert_contains(&text, "  service_base_url: https://platform.example\n");
    assert_contains(&text, "  did_domain: old.example\n");
    assert_contains(&text, "  anp_service_endpoint: https://rpc.example/anp\n");
    assert_contains(&text, "  anp_service_did: did:wba:rpc.example\n");
}

#[test]
fn config_writer_core_mutators_match_go_contract() {
    let temp = TempDir::new("config-writer-core").expect("temp dir");
    let paths = test_paths(temp.path());

    workspace_config::update_runtime_settings(&paths, "websocket", "/tmp/awiki.sock")
        .expect("runtime settings");
    workspace_config::update_active_identity(&paths, "  alice  ").expect("active identity");
    workspace_config::update_runtime_listener_settings(
        &paths,
        Some(false),
        Some(false),
        Some(true),
    )
    .expect("listener booleans");
    let did_domain =
        workspace_config::update_did_domain(&paths, " Tenant.Example. ").expect("did domain");
    workspace_config::update_host_notify_sink(&paths, "webhook").expect("host notify sink");
    workspace_config::update_host_notify_enabled(&paths, false).expect("host notify enabled");

    assert_eq!(did_domain, "tenant.example");
    let text = read_config(&paths);
    assert_contains(&text, "schema_version: 1\n");
    assert_contains(&text, "  active: alice\n");
    assert_contains(&text, "  mode: websocket\n");
    assert_contains(&text, "  socket_path: /tmp/awiki.sock\n");
    assert_contains(&text, "    enabled: false\n");
    assert_contains(&text, "    auto_install: false\n");
    assert_contains(&text, "    auto_start: true\n");
    assert_contains(&text, "    sink: hermes\n");
    assert_contains(&text, "  did_domain: tenant.example\n");
}

#[test]
fn config_writer_existing_helpers_keep_go_helper_boundaries() {
    let temp = TempDir::new("config-writer-helper-boundaries").expect("temp dir");
    let paths = test_paths(temp.path());

    workspace_config::update_runtime_settings(&paths, "WebSocket", "/tmp/Raw.sock")
        .expect("runtime settings");
    workspace_config::update_host_notify_sink(&paths, " unsupported-sink ")
        .expect("host notify helper does not validate direct values");

    let text = read_config(&paths);
    assert_contains(&text, "  mode: WebSocket\n");
    assert_contains(&text, "  socket_path: /tmp/Raw.sock\n");
    assert_contains(&text, "    sink: unsupported-sink\n");
}

#[test]
fn config_writer_openclaw_mutators_match_go_contract() {
    let temp = TempDir::new("config-writer-openclaw").expect("temp dir");
    let paths = test_paths(temp.path());

    workspace_config::update_host_notify_sink(&paths, "openclaw").expect("host notify sink");
    workspace_config::update_openclaw_settings(
        &paths,
        Some("  http://127.0.0.1:18789/hooks/agent  "),
    )
    .expect("openclaw hook");
    workspace_config::set_openclaw_token(&paths, "token-123").expect("set token");
    assert_eq!(
        workspace_config::read_openclaw_token(&paths),
        ("token-123".to_string(), "config_file".to_string())
    );
    workspace_config::clear_openclaw_token(&paths).expect("clear token");

    let text = read_config(&paths);
    assert_contains(&text, "    sink: openclaw\n");
    assert_contains(
        &text,
        "      hook_url: http://127.0.0.1:18789/hooks/agent\n",
    );
    assert_contains(&text, "      token: \n");
    assert_eq!(
        workspace_config::read_openclaw_token(&paths),
        (String::new(), "unset".to_string())
    );
}

#[test]
fn config_writer_hermes_mutators_double_write_legacy_webhook() {
    let temp = TempDir::new("config-writer-hermes").expect("temp dir");
    let paths = test_paths(temp.path());

    let notify_url = "  http://127.0.0.1:8765/notify/host-event  ";
    let deliver = " Telegram ";
    workspace_config::update_host_notify_sink(&paths, "hermes").expect("host notify sink");
    workspace_config::update_hermes_settings(&paths, Some(notify_url), Some(deliver))
        .expect("hermes settings");
    workspace_config::set_hermes_secret(&paths, "secret-123").expect("set hermes secret");

    let text = read_config(&paths);
    assert_contains(&text, "    sink: hermes\n");
    assert_contains(
        &text,
        "      notify_url: http://127.0.0.1:8765/notify/host-event\n",
    );
    assert_contains(&text, "      deliver: telegram\n");
    assert_contains(&text, "      secret: secret-123\n");
    assert_contains(&text, "    webhook:\n");
    assert_contains(
        &text,
        "      notify_url: http://127.0.0.1:8765/notify/host-event\n",
    );
    assert_contains(&text, "      secret: secret-123\n");

    workspace_config::clear_hermes_secret(&paths).expect("clear hermes secret");
    let text = read_config(&paths);
    assert_contains(&text, "      secret: \n");
}

#[test]
fn config_writer_configure_hermes_one_shot_matches_go_contract() {
    let temp = TempDir::new("config-writer-hermes-setup").expect("temp dir");
    let paths = test_paths(temp.path());

    workspace_config::configure_hermes_host_notify(
        &paths,
        "  http://127.0.0.1:8765/notify/host-event  ",
        Some(" secret-setup "),
        " Telegram ",
        false,
    )
    .expect("configure hermes");

    let text = read_config(&paths);
    assert_contains(&text, "    enabled: false\n");
    assert_contains(&text, "    sink: hermes\n");
    assert_contains(
        &text,
        "      notify_url: http://127.0.0.1:8765/notify/host-event\n",
    );
    assert_contains(&text, "      deliver: telegram\n");
    assert_contains(&text, "      secret: secret-setup\n");
}

#[test]
fn config_writer_quotes_scalars_that_would_break_yaml_round_trip() {
    let temp = TempDir::new("config-writer-yaml-quotes").expect("temp dir");
    let paths = test_paths(temp.path());

    workspace_config::update_active_identity(&paths, " alice # one ").expect("active identity");
    workspace_config::update_openclaw_settings(
        &paths,
        Some("http://127.0.0.1:18789/hooks/agent#local"),
    )
    .expect("openclaw hook");
    workspace_config::set_openclaw_token(&paths, "token # local").expect("set token");
    workspace_config::set_hermes_secret(&paths, "secret \"quoted\"").expect("set hermes secret");

    let text = read_config(&paths);
    assert_contains(&text, "  active: \"alice # one\"\n");
    assert_contains(
        &text,
        "      hook_url: \"http://127.0.0.1:18789/hooks/agent#local\"\n",
    );
    assert_contains(&text, "      token: \"token # local\"\n");
    assert_contains(&text, "      secret: \"secret \\\"quoted\\\"\"\n");

    let output = awiki_cmd_with_workspace(&["config", "show"], temp.path().to_str().unwrap());
    assert!(
        output.status.success(),
        "config show should parse quoted writer output; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn test_paths(root: &Path) -> Paths {
    let data_dir = root.join("data");
    let state_dir = root.join("runtime");
    Paths {
        workspace_home_dir: path_string(root),
        root_dir: path_string(root),
        config_dir: path_string(root),
        data_dir: path_string(&data_dir),
        state_dir: path_string(&state_dir),
        cache_dir: path_string(&root.join("cache")),
        logs_dir: path_string(&root.join("logs")),
        config_file: path_string(&root.join("config.yaml")),
        identity_dir: path_string(&root.join("identities")),
        database_file: path_string(&data_dir.join("awiki-cli.db")),
        legacy_credentials_dir: path_string(&root.join("legacy").join("credentials")),
        legacy_data_dir: path_string(&root.join("legacy").join("data")),
    }
}

fn read_config(paths: &Paths) -> String {
    std::fs::read_to_string(&paths.config_file).expect("read config")
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
    );
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &str) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", std::path::Path::new(workspace).join("home"))
        .env("USERPROFILE", std::path::Path::new(workspace).join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME");
    command.output().expect("run awiki-cli binary")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{prefix}-{}-{nonce}",
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
