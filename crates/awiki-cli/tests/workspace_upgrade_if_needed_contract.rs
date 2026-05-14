use awiki_cli::{config, upgrade};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn workspace_upgrade_if_needed_skips_empty_workspace_and_captures_inspection_like_go() {
    let workspace = TempDir::new("workspace-upgrade-if-needed-empty").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let mut context = upgrade::new_context(&resolved, "1.0.0");
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("empty workspace upgrade should be a no-op");

    let paths = upgrade::resolve_paths(&resolved);
    assert!(!Path::new(&paths.meta_path).exists());
    assert!(!Path::new(&paths.journal_path).exists());
    assert_eq!(context.current_meta, None);
    let inspection = context.inspection.expect("inspection captured");
    assert_eq!(inspection.detection.current_version, 3);
    assert_eq!(inspection.detection.current_version_source, "default_empty");
    assert!(inspection.detection.empty);
}

#[test]
fn workspace_upgrade_if_needed_clears_journal_for_empty_or_latest_workspace_like_go() {
    let empty_workspace =
        TempDir::new("workspace-upgrade-if-needed-empty-journal").expect("temp workspace");
    let empty_resolved = test_resolved(empty_workspace.path());
    let empty_paths = upgrade::resolve_paths(&empty_resolved);
    upgrade::save_journal(
        &empty_paths.journal_path,
        &upgrade::Journal {
            upgrade_id: "upgrade-empty".to_string(),
            from_version: 0,
            to_version: 1,
            current_step: "workspace_0_to_1_bootstrap_local_state_upgrade".to_string(),
            phase: "checking".to_string(),
            backup_dir: "backup-empty".to_string(),
            started_at: "2026-05-14T00:00:00Z".to_string(),
            app_version: "1.0.0".to_string(),
        },
    )
    .expect("save empty journal");
    upgrade::upgrade_if_needed(&empty_resolved, "1.0.0").expect("empty workspace clears journal");
    assert!(!Path::new(&empty_paths.journal_path).exists());
    assert!(!Path::new(&empty_paths.meta_path).exists());

    let latest_workspace =
        TempDir::new("workspace-upgrade-if-needed-latest-journal").expect("temp workspace");
    let latest_resolved = test_resolved(latest_workspace.path());
    std::fs::create_dir_all(latest_workspace.path().join("identities")).expect("create ids");
    std::fs::write(
        latest_workspace.path().join("config.yaml"),
        "schema_version: 1\n",
    )
    .expect("write config");
    let latest_paths = upgrade::resolve_paths(&latest_resolved);
    upgrade::save_meta(
        &latest_paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 3,
            app_version: "1.2.3".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save latest meta");
    upgrade::save_journal(
        &latest_paths.journal_path,
        &upgrade::Journal {
            upgrade_id: "upgrade-latest".to_string(),
            from_version: 2,
            to_version: 3,
            current_step: "workspace_2_to_3_replace_existing_k1_handle_dids".to_string(),
            phase: "validating".to_string(),
            backup_dir: "backup-latest".to_string(),
            started_at: "2026-05-14T00:01:00Z".to_string(),
            app_version: "1.2.3".to_string(),
        },
    )
    .expect("save latest journal");
    let mut latest_context = upgrade::new_context(&latest_resolved, "1.2.4");
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut latest_context)
        .expect("latest workspace clears journal");
    assert!(!Path::new(&latest_paths.journal_path).exists());
    let meta = upgrade::load_meta(&latest_paths.meta_path)
        .expect("load latest meta")
        .expect("meta remains");
    assert_eq!(meta.workspace_schema_version, 3);
    assert_eq!(
        latest_context
            .current_meta
            .expect("current meta captured before no-op")
            .workspace_schema_version,
        3
    );
}

#[test]
fn workspace_upgrade_if_needed_reports_newer_workspace_like_go() {
    let workspace = TempDir::new("workspace-upgrade-if-needed-newer").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 4,
            app_version: "9.9.9".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save newer meta");

    let err = upgrade::upgrade_if_needed(&resolved, "1.2.3")
        .expect_err("newer schema should be rejected");
    assert_eq!(
        err.to_string(),
        "workspace schema version 4 is newer than supported 3"
    );
}

#[test]
fn workspace_upgrade_if_needed_defers_real_migration_execution_boundary() {
    let workspace = TempDir::new("workspace-upgrade-if-needed-deferred").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");
    std::fs::write(&paths.legacy_config_file, "{\"legacy\":true}\n").expect("write legacy config");

    let mut context = upgrade::new_context(&resolved, "1.2.3");
    let err = upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect_err("legacy/current-version-zero migration remains deferred");
    assert_eq!(
        err.to_string(),
        "workspace migration execution is not implemented: workspace_0_to_1_bootstrap_local_state_upgrade"
    );
    assert!(
        Path::new(&paths.lock_path).exists(),
        "lock anchor should be created before deferring real migration execution"
    );
    let lock = read_upgrade_lock_metadata(Path::new(&paths.lock_path));
    assert_eq!(lock["lock_scheme"], "os_file_lock_v1");
    assert_eq!(lock["app_version"], "1.2.3");

    let guard = upgrade::acquire_file_lock(&paths.lock_path, "1.2.4")
        .expect("upgrade_if_needed should release the OS lock on return");
    guard.release().expect("release lock");

    assert!(
        !context.backup_dir.is_empty(),
        "backup dir should be captured before migration phase deferral"
    );
    let backup = PathBuf::from(&context.backup_dir);
    assert_eq!(
        backup.parent(),
        Some(Path::new(&paths.backup_root)),
        "backup should be created under the Go backup root"
    );
    assert_eq!(
        std::fs::read_to_string(backup.join("config.yaml.bak")).unwrap(),
        "schema_version: 1\n"
    );
    assert_eq!(
        std::fs::read_to_string(backup.join("config.json.bak")).unwrap(),
        "{\"legacy\":true}\n"
    );

    let journal = upgrade::load_journal(&paths.journal_path)
        .expect("load deferred journal")
        .expect("journal remains after failed migration apply");
    assert_eq!(journal.from_version, 0);
    assert_eq!(journal.to_version, 1);
    assert_eq!(
        journal.current_step,
        "workspace_0_to_1_bootstrap_local_state_upgrade"
    );
    assert_eq!(journal.phase, "applying");
    assert_eq!(journal.backup_dir, context.backup_dir);
    assert_eq!(journal.app_version, "1.2.3");
    assert!(upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .is_none());
}

#[test]
fn workspace_upgrade_if_needed_reuses_journal_backup_before_migration_like_go() {
    let workspace =
        TempDir::new("workspace-upgrade-if-needed-reuse-backup").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");
    let existing_backup = Path::new(&paths.backup_root).join("existing-backup");
    std::fs::create_dir_all(&existing_backup).expect("create existing backup");
    std::fs::write(existing_backup.join("sentinel.txt"), "keep\n").expect("write sentinel");
    upgrade::save_journal(
        &paths.journal_path,
        &upgrade::Journal {
            upgrade_id: "upgrade-existing".to_string(),
            from_version: 0,
            to_version: 1,
            current_step: "workspace_0_to_1_bootstrap_local_state_upgrade".to_string(),
            phase: "checking".to_string(),
            backup_dir: path_string(&existing_backup),
            started_at: "2026-05-15T00:02:00Z".to_string(),
            app_version: "1.2.3".to_string(),
        },
    )
    .expect("save journal with backup dir");

    let mut context = upgrade::new_context(&resolved, "1.2.4");
    let err = upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect_err("real migration still deferred");
    assert_eq!(
        err.to_string(),
        "workspace migration execution is not implemented: workspace_0_to_1_bootstrap_local_state_upgrade"
    );
    assert_eq!(context.backup_dir, path_string(&existing_backup));
    assert_eq!(
        std::fs::read_to_string(existing_backup.join("sentinel.txt")).unwrap(),
        "keep\n"
    );
    let backup_entries = std::fs::read_dir(&paths.backup_root)
        .expect("read backup root")
        .collect::<Result<Vec<_>, _>>()
        .expect("backup entries");
    assert_eq!(
        backup_entries.len(),
        1,
        "journal backup dir should be reused without creating a new backup"
    );
    let journal = upgrade::load_journal(&paths.journal_path)
        .expect("load journal")
        .expect("journal remains after deferred migration apply");
    assert_eq!(journal.upgrade_id, "upgrade-existing");
    assert_eq!(journal.phase, "applying");
    assert_eq!(journal.backup_dir, path_string(&existing_backup));
}

#[test]
fn workspace_upgrade_if_needed_applies_v1_to_v2_then_defers_v2_to_v3_like_go() {
    let workspace = TempDir::new("workspace-upgrade-if-needed-v1-v2").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 1,
            app_version: "1.0.0".to_string(),
            updated_at: "2026-05-15T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save v1 meta");

    let home = workspace.path().join("home");
    let skill_dir = home
        .join(".openclaw")
        .join("skills")
        .join("awiki-agent-id-message");
    std::fs::create_dir_all(&skill_dir).expect("create legacy skill");
    std::fs::write(skill_dir.join("SKILL.md"), "# legacy\n").expect("write skill");
    let heartbeat = home
        .join(".openclaw")
        .join("workspace")
        .join("HEARTBEAT.md");
    std::fs::create_dir_all(heartbeat.parent().expect("heartbeat parent"))
        .expect("create heartbeat parent");
    std::fs::write(
        &heartbeat,
        "# Heartbeat checklist\n\n<!-- awiki-heartbeat-start -->\nRun awiki-agent-id-message\n<!-- awiki-heartbeat-end -->\n\n## Other checks\n",
    )
    .expect("write heartbeat");
    let _home_guard = EnvGuard::set("HOME", Some(path_string(&home)));

    let mut context = upgrade::new_context(&resolved, "1.2.6");
    let err = upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect_err("v2 to v3 replacement remains deferred");
    assert_eq!(
        err.to_string(),
        "workspace migration execution is not implemented: workspace_2_to_3_replace_existing_k1_handle_dids"
    );

    assert!(!skill_dir.exists());
    let heartbeat_text = std::fs::read_to_string(&heartbeat).expect("read heartbeat");
    assert!(!heartbeat_text.contains("awiki-agent-id-message"));
    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta saved after v1 to v2");
    assert_eq!(meta.workspace_schema_version, 2);
    assert_eq!(meta.app_version, "1.2.6");
    assert_eq!(meta.last_backup_dir, context.backup_dir);
    assert_eq!(
        context
            .current_meta
            .as_ref()
            .expect("context meta after v1 to v2")
            .workspace_schema_version,
        2
    );
    let journal = upgrade::load_journal(&paths.journal_path)
        .expect("load deferred journal")
        .expect("journal remains for deferred v2 to v3");
    assert_eq!(journal.from_version, 2);
    assert_eq!(journal.to_version, 3);
    assert_eq!(
        journal.current_step,
        "workspace_2_to_3_replace_existing_k1_handle_dids"
    );
    assert_eq!(journal.phase, "applying");
    assert_eq!(journal.backup_dir, context.backup_dir);
}

#[test]
fn workspace_upgrade_if_needed_rejects_concurrent_lock_before_migration_like_go() {
    let workspace = TempDir::new("workspace-upgrade-if-needed-lock-held").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");
    upgrade::save_journal(
        &paths.journal_path,
        &upgrade::Journal {
            upgrade_id: "upgrade-race".to_string(),
            from_version: 1,
            to_version: 2,
            current_step: "workspace_1_to_2_remove_legacy_skill_and_listener".to_string(),
            phase: "checking".to_string(),
            backup_dir: "backup-race".to_string(),
            started_at: "2026-05-15T00:00:00Z".to_string(),
            app_version: "1.2.3".to_string(),
        },
    )
    .expect("save stale journal");

    let guard =
        upgrade::acquire_file_lock(&paths.lock_path, "preflight").expect("pre-acquire lock");
    let err = upgrade::upgrade_if_needed(&resolved, "1.2.5")
        .expect_err("held upgrade lock should reject migration execution");
    assert_eq!(
        err.to_string(),
        format!("workspace upgrade is already running: {}", paths.lock_path)
    );
    assert!(
        Path::new(&paths.journal_path).exists(),
        "journal should not be cleared when migration execution is blocked on the lock"
    );
    guard.release().expect("release preflight lock");
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

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn read_upgrade_lock_metadata(path: &Path) -> Value {
    let raw = std::fs::read(path).expect("read lock metadata");
    serde_json::from_slice(&raw).expect("parse lock metadata")
}

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(key: &'static str, value: Option<String>) -> Self {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = ENV_LOCK.lock().expect("env lock");
        let previous = std::env::var_os(key);
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        Self {
            key,
            previous,
            _guard: guard,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
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
