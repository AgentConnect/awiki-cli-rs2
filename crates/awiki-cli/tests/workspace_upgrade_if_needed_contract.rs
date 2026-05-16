use awiki_cli::{config, identity, upgrade};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
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
fn workspace_upgrade_if_needed_applies_local_v0_to_v3_without_current_k1_dids() {
    let workspace =
        TempDir::new("workspace-upgrade-if-needed-v0-v1-local").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");
    std::fs::write(&paths.legacy_config_file, "{\"legacy\":true}\n").expect("write legacy config");

    let mut context = upgrade::new_context(&resolved, "1.2.3");
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("no-k1 v2 to v3 migration should complete locally");
    assert!(
        Path::new(&paths.lock_path).exists(),
        "lock anchor should be created before migration execution"
    );
    let lock = read_upgrade_lock_metadata(Path::new(&paths.lock_path));
    assert_eq!(lock["lock_scheme"], "os_file_lock_v1");
    assert_eq!(lock["app_version"], "1.2.3");

    let guard = upgrade::acquire_file_lock(&paths.lock_path, "1.2.4")
        .expect("upgrade_if_needed should release the OS lock after completed v2 to v3");
    guard.release().expect("release lock");

    assert!(
        !context.backup_dir.is_empty(),
        "backup dir should be captured during migration execution"
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

    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta saved after v2 to v3");
    assert_eq!(meta.workspace_schema_version, 3);
    assert_eq!(meta.app_version, "1.2.3");
    assert_eq!(meta.last_backup_dir, context.backup_dir);
    assert_eq!(
        context
            .current_meta
            .as_ref()
            .expect("context meta after v2 to v3")
            .workspace_schema_version,
        3
    );
    assert!(
        upgrade::load_journal(&paths.journal_path)
            .expect("load cleared journal")
            .is_none(),
        "journal should be cleared after completed v2 to v3"
    );
}

#[test]
fn workspace_upgrade_if_needed_migrates_legacy_config_json_through_v0_to_v1_loop() {
    let workspace =
        TempDir::new("workspace-upgrade-if-needed-legacy-config").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(
        &paths.legacy_config_file,
        r#"{"schema_version":1,"services":{"service_base_url":"https://legacy.example","did_domain":"legacy.example"},"runtime":{"mode":"http"}}"#,
    )
    .expect("write legacy config");

    let mut context = upgrade::new_context(&resolved, "1.2.7");
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("legacy config migration should complete through no-k1 v2 to v3");

    assert!(
        !Path::new(&paths.legacy_config_file).exists(),
        "legacy config should be removed after v0 to v1 local apply"
    );
    let config = std::fs::read_to_string(&paths.config_file).expect("read migrated config");
    assert_contains(&config, "schema_version: 1\n");
    assert_contains(&config, "  mode: http\n");
    assert_contains(&config, "  service_base_url: https://legacy.example\n");
    assert_contains(&config, "  did_domain: legacy.example\n");
    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta after v2 to v3");
    assert_eq!(meta.workspace_schema_version, 3);
    assert!(
        upgrade::load_journal(&paths.journal_path)
            .expect("load cleared journal")
            .is_none(),
        "journal should be cleared after completed migration"
    );
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
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("no-k1 v2 to v3 migration should complete with reused backup");
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
    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta after reused backup migration");
    assert_eq!(meta.workspace_schema_version, 3);
    assert_eq!(meta.last_backup_dir, path_string(&existing_backup));
    assert!(
        upgrade::load_journal(&paths.journal_path)
            .expect("load cleared journal")
            .is_none(),
        "journal should be cleared after completed reused-backup migration"
    );
}

#[test]
fn workspace_upgrade_if_needed_defers_v0_to_v1_when_imported_k1_replacement_is_required() {
    let workspace =
        TempDir::new("workspace-upgrade-if-needed-k1-deferred").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    seed_flat_legacy_identity(&paths, "legacy", "did:wba:example.test:user:k1_legacy");

    let mut context = upgrade::new_context(&resolved, "1.2.5");
    let err = upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect_err("imported k1 replacement remains deferred");
    assert_eq!(
        err.to_string(),
        "workspace migration execution is not implemented: workspace_0_to_1_bootstrap_local_state_upgrade"
    );

    let journal = upgrade::load_journal(&paths.journal_path)
        .expect("load deferred journal")
        .expect("journal remains after deferred v0 to v1 apply");
    assert_eq!(journal.from_version, 0);
    assert_eq!(journal.to_version, 1);
    assert_eq!(
        journal.current_step,
        "workspace_0_to_1_bootstrap_local_state_upgrade"
    );
    assert_eq!(journal.phase, "applying");
    assert!(upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .is_none());
}

#[test]
fn workspace_upgrade_if_needed_applies_v1_to_v3_without_current_k1_dids() {
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
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("v1 to v2 cleanup should continue through no-k1 v2 to v3");

    assert!(!skill_dir.exists());
    let heartbeat_text = std::fs::read_to_string(&heartbeat).expect("read heartbeat");
    assert!(!heartbeat_text.contains("awiki-agent-id-message"));
    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta saved after v2 to v3");
    assert_eq!(meta.workspace_schema_version, 3);
    assert_eq!(meta.app_version, "1.2.6");
    assert_eq!(meta.last_backup_dir, context.backup_dir);
    assert_eq!(
        context
            .current_meta
            .as_ref()
            .expect("context meta after v2 to v3")
            .workspace_schema_version,
        3
    );
    assert!(
        upgrade::load_journal(&paths.journal_path)
            .expect("load cleared journal")
            .is_none(),
        "journal should be cleared after completed v2 to v3"
    );
}

#[test]
fn workspace_upgrade_if_needed_replaces_v2_to_v3_current_k1_dids_like_go() {
    let workspace = TempDir::new("workspace-upgrade-if-needed-v2-v3-k1").expect("temp workspace");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"handle":"legacy","full_handle":"legacy.example.test","access_token":"jwt-replaced"},"id":"req-1"}"#,
    )]);
    let mut resolved = test_resolved(workspace.path());
    resolved.service_base_url = server.base_url();
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(
        &paths.config_file,
        format!(
            "schema_version: 1\nservices:\n  service_base_url: {}\n  did_domain: example.test\n",
            server.base_url()
        ),
    )
    .expect("write config");
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 2,
            app_version: "1.0.0".to_string(),
            updated_at: "2026-05-15T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save v2 meta");
    seed_current_identity(
        &resolved,
        "legacy",
        "did:wba:example.test:legacy:k1_legacy",
        "jwt-legacy",
    );

    let mut context = upgrade::new_context(&resolved, "1.2.8");
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("current k1 identities should be replaced during v2 to v3");

    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta saved after replacement");
    assert_eq!(meta.workspace_schema_version, 3);
    assert_eq!(meta.app_version, "1.2.8");
    assert!(meta.warnings.is_empty());
    assert_eq!(
        context
            .current_meta
            .as_ref()
            .expect("context meta after v2 to v3")
            .workspace_schema_version,
        3
    );
    assert!(
        upgrade::load_journal(&paths.journal_path)
            .expect("load cleared journal")
            .is_none(),
        "journal should be cleared after successful v2 to v3"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert_contains(&requests[0], "Authorization: Bearer jwt-legacy\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "replace_did");
    let new_did = body["params"]["new_did_document"]["id"]
        .as_str()
        .expect("new did in request");
    assert!(
        new_did.starts_with("did:wba:example.test:legacy:e1_"),
        "new DID should preserve handle path and use e1 suffix: {new_did}"
    );

    let stored = read_stored_identity(&resolved, "legacy");
    assert_eq!(stored["did"], new_did);
    assert_eq!(stored["handle"], "legacy");
    assert_eq!(stored["full_handle"], "legacy.example.test");
    let auth = read_stored_auth(&resolved, "legacy");
    assert_eq!(auth["jwt_token"], "jwt-replaced");
    let backup_manifest = read_single_replace_did_backup_manifest(&resolved);
    assert_eq!(backup_manifest["identity_name"], "legacy");
    assert_eq!(
        backup_manifest["old_did"],
        "did:wba:example.test:legacy:k1_legacy"
    );
    assert_eq!(backup_manifest["planned_new_did"], new_did);

    let guard = upgrade::acquire_file_lock(&paths.lock_path, "1.2.9")
        .expect("upgrade_if_needed should release the OS lock after v2 to v3 replacement");
    guard.release().expect("release lock");
}

#[test]
fn workspace_upgrade_if_needed_records_v2_to_v3_k1_replacement_failures_as_warnings_like_go() {
    let workspace =
        TempDir::new("workspace-upgrade-if-needed-v2-v3-k1-warning").expect("temp workspace");
    let server = TestServer::new(vec![TestResponse {
        status: 500,
        body: "replace unavailable".to_string(),
    }]);
    let mut resolved = test_resolved(workspace.path());
    resolved.service_base_url = server.base_url();
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(
        &paths.config_file,
        format!(
            "schema_version: 1\nservices:\n  service_base_url: {}\n  did_domain: example.test\n",
            server.base_url()
        ),
    )
    .expect("write config");
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 2,
            app_version: "1.0.0".to_string(),
            updated_at: "2026-05-15T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save v2 meta");
    seed_current_identity(
        &resolved,
        "legacy",
        "did:wba:example.test:legacy:k1_legacy",
        "jwt-legacy",
    );

    let mut context = upgrade::new_context(&resolved, "1.2.11");
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("replacement failure should be captured as a warning");

    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta saved despite warning");
    assert_eq!(meta.workspace_schema_version, 3);
    assert_eq!(meta.warnings.len(), 1);
    assert_contains(
        &meta.warnings[0],
        "Automatic DID replacement failed for identity legacy (did:wba:example.test:legacy:k1_legacy):",
    );
    assert_contains(&meta.warnings[0], "service http error 500");
    assert_eq!(
        context
            .current_meta
            .as_ref()
            .expect("context meta after warning")
            .warnings,
        meta.warnings
    );
    let stored = read_stored_identity(&resolved, "legacy");
    assert_eq!(stored["did"], "did:wba:example.test:legacy:k1_legacy");
    assert!(Path::new(&paths.journal_path).exists() == false);
}

#[test]
fn workspace_upgrade_if_needed_completes_v2_to_v3_when_current_identity_index_has_only_e1_dids() {
    let workspace = TempDir::new("workspace-upgrade-if-needed-v2-v3-e1").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");
    upgrade::save_meta(
        &paths.meta_path,
        &upgrade::Meta {
            workspace_schema_version: 2,
            app_version: "1.0.0".to_string(),
            updated_at: "2026-05-15T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save v2 meta");
    std::fs::create_dir_all(&paths.identity_dir).expect("create identity dir");
    std::fs::write(
        Path::new(&paths.identity_dir).join("index.json"),
        r#"{"schema_version":3,"default_credential_name":"current","credentials":{"current":{"credential_name":"current","dir_name":"current","did":"did:wba:example.test:user:e1_current","unique_id":"e1_current","name":"Current User","handle":"current","full_handle":"current.example.test","is_default":true}}}"#,
    )
    .expect("write e1 identity index");

    let mut context = upgrade::new_context(&resolved, "1.2.10");
    upgrade::new_default_upgrader()
        .upgrade_if_needed(&mut context)
        .expect("current non-k1 identities should complete v2 to v3 locally");

    let meta = upgrade::load_meta(&paths.meta_path)
        .expect("load meta")
        .expect("meta saved after v2 to v3");
    assert_eq!(meta.workspace_schema_version, 3);
    assert_eq!(meta.app_version, "1.2.10");
    assert!(meta.warnings.is_empty());
    assert_eq!(
        context
            .current_meta
            .as_ref()
            .expect("context meta after v2 to v3")
            .workspace_schema_version,
        3
    );
    assert!(
        upgrade::load_journal(&paths.journal_path)
            .expect("load cleared journal")
            .is_none(),
        "journal should be cleared after completed v2 to v3"
    );
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

fn seed_flat_legacy_identity(paths: &upgrade::Paths, name: &str, did: &str) {
    std::fs::create_dir_all(Path::new(&paths.legacy_credentials_dir))
        .expect("create legacy credentials dir");
    let generated = identity::generate_identity("example.test", "", "")
        .expect("generate legacy identity key material");
    std::fs::write(
        Path::new(&paths.legacy_credentials_dir).join(format!("{name}.json")),
        serde_json::to_vec_pretty(&json!({
            "did": did,
            "unique_id": did.rsplit(':').next().unwrap_or(did),
            "name": "Legacy User",
            "handle": name,
            "jwt_token": "legacy-token",
            "private_key_pem": generated.key1_private_pem,
            "public_key_pem": generated.key1_public_pem,
            "e2ee_signing_private_pem": generated.e2ee_signing_private_pem,
            "e2ee_agreement_private_pem": generated.e2ee_agreement_private_pem,
            "did_document": {"id": did}
        }))
        .expect("legacy identity json"),
    )
    .expect("write legacy identity");
}

fn seed_current_identity(resolved: &config::Resolved, name: &str, did: &str, jwt_token: &str) {
    let generated = identity::generate_identity("example.test", "", "")
        .expect("generate identity key material");
    let manager = identity::Manager::new(resolved.paths.clone());
    manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: name.to_string(),
            did: did.to_string(),
            unique_id: did.rsplit(':').next().unwrap_or(did).to_string(),
            user_id: format!("user-{name}"),
            display_name: "Legacy User".to_string(),
            handle: name.to_string(),
            full_handle: format!("{name}.example.test"),
            jwt_token: jwt_token.to_string(),
            did_document: Some(json!({ "id": did })),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            replace_existing: true,
        })
        .expect("save current identity");
}

fn read_stored_identity(resolved: &config::Resolved, name: &str) -> Value {
    let manager = identity::Manager::new(resolved.paths.clone());
    let record = manager.load(name).expect("load stored identity");
    json!({
        "did": record.did,
        "handle": record.handle,
        "full_handle": record.full_handle,
    })
}

fn read_stored_auth(resolved: &config::Resolved, name: &str) -> Value {
    let manager = identity::Manager::new(resolved.paths.clone());
    let record = manager.load(name).expect("load stored identity");
    let auth_path = Path::new(&resolved.paths.identity_dir)
        .join(record.dir_name)
        .join("auth.json");
    serde_json::from_slice(&std::fs::read(auth_path).expect("read auth")).expect("parse auth")
}

fn read_single_replace_did_backup_manifest(resolved: &config::Resolved) -> Value {
    let backup_root = Path::new(&resolved.paths.identity_dir)
        .join(".legacy-backup")
        .join("replace-did");
    let entries = std::fs::read_dir(&backup_root)
        .unwrap_or_else(|err| panic!("read backup root {backup_root:?}: {err}"));
    let manifests = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
        })
        .map(|entry| entry.path().join("backup_manifest.json"))
        .collect::<Vec<_>>();
    assert_eq!(manifests.len(), 1, "expected one replacement backup");
    serde_json::from_slice(&std::fs::read(&manifests[0]).expect("read backup manifest"))
        .expect("parse backup manifest")
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected config to contain {needle:?}, got:\n{haystack}"
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn read_upgrade_lock_metadata(path: &Path) -> Value {
    let raw = std::fs::read(path).expect("read lock metadata");
    serde_json::from_slice(&raw).expect("parse lock metadata")
}

struct TestResponse {
    status: u16,
    body: String,
}

impl TestResponse {
    fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
        }
    }
}

struct TestServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(responses: Vec<TestResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        listener
            .set_nonblocking(true)
            .expect("set test server nonblocking");
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let join = thread::spawn(move || {
            for response in responses {
                let Some(stream) = accept_with_timeout(&listener) else {
                    break;
                };
                handle_connection(stream, &server_requests, response);
            }
        });
        Self {
            address,
            requests,
            join: Some(join),
        }
    }

    fn base_url(&self) -> String {
        self.address.clone()
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn accept_with_timeout(listener: &TcpListener) -> Option<TcpStream> {
    let start = std::time::Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() > std::time::Duration::from_secs(5) {
                    return None;
                }
                thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<String>>>,
    response: TestResponse,
) {
    let mut buffer = [0u8; 16384];
    let mut raw = Vec::new();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .expect("set read timeout");
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if request_complete(&raw) {
            break;
        }
    }
    let request = String::from_utf8_lossy(&raw).into_owned();
    requests.lock().expect("requests lock").push(request);
    let body = response.body;
    let status_text = if response.status == 200 {
        "OK"
    } else {
        "Internal Server Error"
    };
    let raw_response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        status_text,
        body.len(),
        body
    );
    stream
        .write_all(raw_response.as_bytes())
        .expect("write response");
}

fn request_complete(raw: &[u8]) -> bool {
    let Some(header_end) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&raw[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    raw.len() >= header_end + 4 + content_length
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
