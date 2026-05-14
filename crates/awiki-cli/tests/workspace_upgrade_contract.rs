use awiki_cli::{config, store, upgrade};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn workspace_upgrade_empty_detection_matches_go_contract() {
    let temp = TempDir::new("workspace-upgrade-empty").expect("temp dir");
    let resolved = test_resolved(temp.path());
    let paths = upgrade::resolve_paths(&resolved);
    let inspection = upgrade::inspect(&resolved, "dev").expect("inspect empty workspace");

    assert_eq!(
        paths.meta_path,
        path_string(&temp.path().join("upgrade").join("meta.json"))
    );
    assert_eq!(
        paths.journal_path,
        path_string(&temp.path().join("upgrade").join("upgrade_journal.json"))
    );
    assert_eq!(
        paths.lock_path,
        path_string(&temp.path().join("upgrade").join("upgrade.lock"))
    );
    assert_eq!(
        paths.backup_root,
        path_string(&temp.path().join("upgrade").join("backups"))
    );
    assert_eq!(
        paths.legacy_config_file,
        path_string(&temp.path().join("config.json"))
    );
    assert_eq!(
        paths.legacy_settings_path,
        path_string(
            &temp
                .path()
                .join("legacy-data")
                .join("config")
                .join("settings.json")
        )
    );
    assert_eq!(inspection.meta, None);
    assert_eq!(inspection.journal, None);
    assert_eq!(inspection.detection.current_version, 3);
    assert_eq!(inspection.detection.latest_version, 3);
    assert_eq!(inspection.detection.current_version_source, "default_empty");
    assert_eq!(inspection.detection.empty, true);
    assert_eq!(inspection.detection.has_workspace, false);
    assert_eq!(inspection.detection.has_legacy, false);
    assert_eq!(inspection.detection.config_exists, false);
    assert_eq!(inspection.detection.legacy_config_exists, false);
    assert_eq!(inspection.detection.identity_index_exists, false);
    assert_eq!(inspection.detection.database_exists, false);
}

#[test]
fn workspace_upgrade_loads_meta_journal_and_detects_local_state() {
    let temp = TempDir::new("workspace-upgrade-state").expect("temp dir");
    let resolved = test_resolved(temp.path());
    std::fs::write(
        temp.path().join("config.yaml"),
        "schema_version: 1\nservices:\n  did_domain: tenant.example\n",
    )
    .expect("write config");
    std::fs::create_dir_all(temp.path().join("identities")).expect("create identities");
    std::fs::write(
        temp.path().join("identities").join("index.json"),
        r#"{"schema_version":3,"default_credential_name":"","credentials":{}}"#,
    )
    .expect("write index");
    let db = store::open(&resolved.paths).expect("open db");
    store::ensure_schema(&db).expect("ensure schema");
    drop(db);
    std::fs::create_dir_all(temp.path().join("legacy-data").join("config"))
        .expect("create legacy settings dir");
    std::fs::write(
        temp.path()
            .join("legacy-data")
            .join("config")
            .join("settings.json"),
        "{}\n",
    )
    .expect("write legacy settings");

    let paths = upgrade::resolve_paths(&resolved);
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 2,
            app_version: "1.0.0".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: "upgrade-1".to_string(),
            last_backup_dir: "backup-1".to_string(),
            warnings: vec!["manual review".to_string()],
        },
    )
    .expect("save meta");
    upgrade::save_journal(
        &paths.journal_path,
        &upgrade::Journal {
            upgrade_id: "upgrade-2".to_string(),
            from_version: 1,
            to_version: 2,
            current_step: "apply".to_string(),
            phase: "running".to_string(),
            backup_dir: "backup-2".to_string(),
            started_at: "2026-05-14T00:01:00Z".to_string(),
            app_version: "1.0.0".to_string(),
        },
    )
    .expect("save journal");

    let inspection = upgrade::inspect(&resolved, "dev").expect("inspect state");

    let meta = inspection.meta.expect("meta");
    assert_eq!(meta.workspace_schema_version, 2);
    assert_eq!(meta.app_version, "1.0.0");
    assert_eq!(meta.warnings, vec!["manual review"]);
    let journal = inspection.journal.expect("journal");
    assert_eq!(journal.upgrade_id, "upgrade-2");
    assert_eq!(journal.current_step, "apply");
    assert_eq!(inspection.detection.current_version, 2);
    assert_eq!(inspection.detection.current_version_source, "meta");
    assert_eq!(inspection.detection.config_exists, true);
    assert_eq!(inspection.detection.config_schema_version, 1);
    assert_eq!(inspection.detection.identity_index_exists, true);
    assert_eq!(inspection.detection.identity_index_schema_version, 3);
    assert_eq!(inspection.detection.database_exists, true);
    assert_eq!(
        inspection.detection.database_schema_version,
        store::SCHEMA_VERSION
    );
    assert_eq!(inspection.detection.legacy_settings_exists, true);
    assert_eq!(inspection.detection.has_workspace, true);
    assert_eq!(inspection.detection.has_legacy, true);
}

#[test]
fn workspace_upgrade_meta_and_journal_accept_go_zero_value_json() {
    let temp = TempDir::new("workspace-upgrade-zero").expect("temp dir");
    let meta_path = temp.path().join("meta.json");
    let journal_path = temp.path().join("upgrade_journal.json");
    std::fs::write(&meta_path, "{}\n").expect("write empty meta object");
    std::fs::write(&journal_path, "{}\n").expect("write empty journal object");

    let meta = upgrade::load_meta(&path_string(&meta_path))
        .expect("load meta")
        .expect("meta present");
    assert_eq!(meta.workspace_schema_version, 0);
    assert_eq!(meta.updated_at, "");
    assert!(meta.warnings.is_empty());

    let journal = upgrade::load_journal(&path_string(&journal_path))
        .expect("load journal")
        .expect("journal present");
    assert_eq!(journal.from_version, 0);
    assert_eq!(journal.to_version, 0);
    assert_eq!(journal.current_step, "");
    assert_eq!(journal.phase, "");
}

#[test]
fn doctor_workspace_upgrade_uses_meta_journal_and_go_warning_rules() {
    let workspace = TempDir::new("workspace-upgrade-doctor").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::write(workspace.path().join("config.yaml"), "schema_version: 1\n")
        .expect("write config");
    let paths = upgrade::resolve_paths(&resolved);
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 3,
            app_version: "1.0.0".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: vec!["kept legacy backup".to_string()],
        },
    )
    .expect("save meta");

    let meta_warn = success_json(&awiki_cmd(&["doctor"], workspace.path()));
    let workspace_check = check_by_name(&meta_warn, "workspace_upgrade");
    assert_eq!(workspace_check["status"], "warn");
    assert_eq!(
        workspace_check["summary"],
        "Workspace upgrade completed with migration warnings"
    );
    assert_eq!(
        workspace_check["details"]["meta"]["workspace_schema_version"],
        3
    );
    assert_eq!(
        workspace_check["details"]["detection"]["current_version_source"],
        "meta"
    );

    upgrade::save_journal(
        &paths.journal_path,
        &upgrade::Journal {
            upgrade_id: "upgrade-3".to_string(),
            from_version: 2,
            to_version: 3,
            current_step: "validate".to_string(),
            phase: "running".to_string(),
            backup_dir: String::new(),
            started_at: "2026-05-14T00:02:00Z".to_string(),
            app_version: "1.0.0".to_string(),
        },
    )
    .expect("save journal");
    let journal_warn = success_json(&awiki_cmd(&["doctor"], workspace.path()));
    let workspace_check = check_by_name(&journal_warn, "workspace_upgrade");
    assert_eq!(workspace_check["status"], "warn");
    assert_eq!(
        workspace_check["summary"],
        "Workspace upgrade journal indicates an interrupted upgrade"
    );
    assert_eq!(
        workspace_check["details"]["journal"]["upgrade_id"],
        "upgrade-3"
    );
}

#[test]
fn config_show_embeds_upgrade_inspection_instead_of_stub_snapshot() {
    let workspace = TempDir::new("workspace-upgrade-config-show").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 3,
            app_version: "dev".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save meta");

    let envelope = success_json(&awiki_cmd(&["config", "show"], workspace.path()));
    let upgrade = &envelope["data"]["workspace_upgrade"];
    assert_eq!(upgrade["paths"]["meta_path"], paths.meta_path);
    assert_eq!(upgrade["paths"]["journal_path"], paths.journal_path);
    assert_eq!(upgrade["meta"]["workspace_schema_version"], 3);
    assert_eq!(upgrade["journal"], Value::Null);
    assert_eq!(upgrade["detection"]["current_version"], 3);
    assert_eq!(upgrade["detection"]["latest_version"], 3);
    assert_eq!(upgrade["detection"]["current_version_source"], "meta");
    assert!(upgrade.get("actions").is_none());
}

fn test_resolved(root: &Path) -> config::Resolved {
    config::Resolved {
        paths: config::Paths {
            workspace_home_dir: path_string(root),
            root_dir: path_string(root),
            config_dir: path_string(root),
            data_dir: path_string(&root.join("data")),
            state_dir: path_string(&root.join("runtime")),
            cache_dir: path_string(&root.join("cache")),
            logs_dir: path_string(&root.join("logs")),
            config_file: path_string(&root.join("config.yaml")),
            identity_dir: path_string(&root.join("identities")),
            database_file: path_string(&root.join("data").join("awiki-cli.db")),
            legacy_credentials_dir: path_string(&root.join("legacy-credentials")),
            legacy_data_dir: path_string(&root.join("legacy-data")),
        },
        config_schema_version: 1,
        active_identity: String::new(),
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:awiki.ai".to_string(),
        mail_service_url: "https://awiki.ai".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 86400,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: Default::default(),
    }
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_awiki-cli"))
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_ANP_MLS_BINARY")
        .output()
        .expect("run awiki-cli binary")
}

fn check_by_name<'a>(envelope: &'a Value, name: &str) -> &'a Value {
    envelope["data"]["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| panic!("missing check {name}"))
}

fn success_json(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty, got {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be JSON envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
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
