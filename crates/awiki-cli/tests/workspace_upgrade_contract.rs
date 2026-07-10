#![allow(clippy::bool_assert_comparison)]

use awiki_cli::{workspace_config, workspace_upgrade};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn workspace_upgrade_empty_detection_matches_go_contract() {
    let temp = TempDir::new("workspace-upgrade-empty").expect("temp dir");
    let resolved = test_resolved(temp.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    let inspection = workspace_upgrade::inspect(&resolved, "dev").expect("inspect empty workspace");

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
    assert_eq!(inspection.detection.current_version, 4);
    assert_eq!(inspection.detection.latest_version, 4);
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
    let db = open_local_state(&resolved.paths).expect("open db");
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

    let paths = workspace_upgrade::resolve_paths(&resolved);
    workspace_upgrade::save_meta(
        &paths.meta_path,
        &workspace_upgrade::Meta {
            workspace_schema_version: 2,
            app_version: "1.0.0".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: "upgrade-1".to_string(),
            last_backup_dir: "backup-1".to_string(),
            warnings: vec!["manual review".to_string()],
        },
    )
    .expect("save meta");
    workspace_upgrade::save_journal(
        &paths.journal_path,
        &workspace_upgrade::Journal {
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

    let inspection = workspace_upgrade::inspect(&resolved, "dev").expect("inspect state");

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
        im_core::compat::local_state::SCHEMA_VERSION
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

    let meta = workspace_upgrade::load_meta(&path_string(&meta_path))
        .expect("load meta")
        .expect("meta present");
    assert_eq!(meta.workspace_schema_version, 0);
    assert_eq!(meta.updated_at, "");
    assert!(meta.warnings.is_empty());

    let journal = workspace_upgrade::load_journal(&path_string(&journal_path))
        .expect("load journal")
        .expect("journal present");
    assert_eq!(journal.from_version, 0);
    assert_eq!(journal.to_version, 0);
    assert_eq!(journal.current_step, "");
    assert_eq!(journal.phase, "");
}

#[test]
fn workspace_upgrade_legacy_settings_parser_matches_go_contract() {
    let settings = workspace_upgrade::parse_legacy_settings(
        br#"{
          "user_service_url": " https://awiki.example/// ",
          "molt_message_url": "https://awiki.example",
          "did_domain": "tenant.example",
          "message_transport": {"receive_mode": "WebSocket"}
        }"#,
    )
    .expect("parse same-url settings");
    assert_eq!(settings.service_base_url, "https://awiki.example");
    assert_eq!(settings.did_domain, "tenant.example");
    assert_eq!(settings.runtime_mode, "websocket");

    let message_only = workspace_upgrade::parse_legacy_settings(
        br#"{
          "molt_message_url": "https://message.example/",
          "did_domain": "message.example",
          "message_transport": {"receive_mode": "poll"}
        }"#,
    )
    .expect("parse message-only settings");
    assert_eq!(message_only.service_base_url, "https://message.example");
    assert_eq!(message_only.runtime_mode, "http");

    let split = workspace_upgrade::parse_legacy_settings(
        br#"{
          "user_service_url": "https://auth.example",
          "molt_message_url": "https://message.example",
          "did_domain": "tenant.example",
          "message_transport": {"receive_mode": "websocket"}
        }"#,
    )
    .expect_err("split service URLs should be rejected");
    assert!(
        split
            .to_string()
            .contains("automatic migration to one service_base_url is not supported"),
        "unexpected split URL error: {split}"
    );
    assert!(split
        .to_string()
        .contains("user_service_url (https://auth.example)"));
    assert!(split
        .to_string()
        .contains("molt_message_url (https://message.example)"));
}

#[test]
fn workspace_upgrade_load_legacy_settings_wraps_io_and_parse_errors_like_go() {
    let temp = TempDir::new("workspace-upgrade-settings-errors").expect("temp dir");
    let missing =
        workspace_upgrade::load_legacy_settings(&path_string(&temp.path().join("missing.json")))
            .expect_err("missing settings should fail");
    assert!(missing.to_string().starts_with("read legacy settings:"));

    let invalid = temp.path().join("settings.json");
    std::fs::write(&invalid, "{not-json").expect("write invalid settings");
    let err = workspace_upgrade::load_legacy_settings(&path_string(&invalid))
        .expect_err("invalid settings should fail");
    assert!(err.to_string().starts_with("parse legacy settings:"));
}

#[test]
fn workspace_upgrade_default_upgrader_plan_matches_go_migration_chain() {
    let upgrader = workspace_upgrade::new_default_upgrader();
    assert_eq!(upgrader.latest_version(), 4);

    let plan = upgrader.plan(0, 4).expect("default 0 to latest plan");
    let steps: Vec<(i64, i64, &str)> = plan
        .iter()
        .map(|migration| (migration.from(), migration.to(), migration.name()))
        .collect();
    assert_eq!(
        steps,
        vec![
            (0, 1, "workspace_0_to_1_bootstrap_local_state_upgrade"),
            (1, 2, "workspace_1_to_2_remove_legacy_skill_and_listener"),
            (2, 3, "workspace_2_to_3_replace_existing_k1_handle_dids"),
            (3, 4, "workspace_3_to_4_owner_identity_local_state"),
        ]
    );

    let partial = upgrader.plan(1, 4).expect("partial plan");
    assert_eq!(
        partial
            .iter()
            .map(|migration| migration.name())
            .collect::<Vec<_>>(),
        vec![
            "workspace_1_to_2_remove_legacy_skill_and_listener",
            "workspace_2_to_3_replace_existing_k1_handle_dids",
            "workspace_3_to_4_owner_identity_local_state",
        ]
    );
    assert!(upgrader.plan(4, 4).expect("no-op current plan").is_empty());
}

#[test]
fn workspace_upgrade_plan_errors_match_go_messages() {
    let upgrader = workspace_upgrade::new_default_upgrader();
    let newer = upgrader
        .plan(5, 4)
        .expect_err("newer source version should fail");
    assert_eq!(
        newer.to_string(),
        "workspace schema version 5 is newer than target 4"
    );

    let missing = upgrader
        .plan(4, 5)
        .expect_err("missing migration should fail");
    assert_eq!(missing.to_string(), "missing workspace migration 4 -> 5");
}

#[test]
fn workspace_upgrade_context_and_is_done_use_go_paths_and_meta_version() {
    let workspace = TempDir::new("workspace-upgrade-context").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let mut context = workspace_upgrade::new_context(&resolved, "1.2.3");
    let paths = workspace_upgrade::resolve_paths(&resolved);
    assert_eq!(context.paths, paths);
    assert_eq!(context.app_version, "1.2.3");
    assert!(context.inspection.is_none());
    assert_eq!(context.backup_dir, "");
    assert_eq!(context.current_meta, None);
    assert!(context.warnings.is_empty());

    let upgrader = workspace_upgrade::new_default_upgrader();
    let plan = upgrader.plan(0, 4).expect("default plan");
    assert_eq!(
        plan[0].is_done(&context).expect("missing meta is not done"),
        false
    );

    workspace_upgrade::save_meta(
        &paths.meta_path,
        &workspace_upgrade::Meta {
            workspace_schema_version: 1,
            app_version: "1.2.3".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save meta");
    assert_eq!(
        plan[0]
            .is_done(&context)
            .expect("meta version 1 completes first migration"),
        true
    );
    assert_eq!(
        plan[1]
            .is_done(&context)
            .expect("meta version 1 does not complete second migration"),
        false
    );

    let apply_err = plan[0]
        .apply(&mut context)
        .expect_err("v0 to v1 apply still requires captured inspection");
    assert_eq!(
        apply_err.to_string(),
        "workspace upgrade inspection is required"
    );
    context.inspection = Some(workspace_upgrade::Inspection {
        paths: context.paths.clone(),
        detection: workspace_upgrade::Detection::default(),
        ..Default::default()
    });
    plan[0]
        .apply(&mut context)
        .expect("v0 to v1 local apply is wired for non-legacy local state");
    plan[0]
        .validate(&context)
        .expect("first migration validation is wired");
    plan[1]
        .validate(&context)
        .expect("second migration validation is Go no-op");
    plan[2]
        .validate(&context)
        .expect("third migration validation is Go no-op");
}

#[test]
fn workspace_upgrade_file_lock_leaves_persistent_metadata() {
    let temp = TempDir::new("workspace-upgrade-lock-metadata").expect("temp dir");
    let lock_path = temp.path().join("upgrade").join("upgrade.lock");
    let guard = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.3")
        .expect("acquire lock");
    let metadata = read_upgrade_lock_metadata(&lock_path);
    assert_eq!(metadata["lock_scheme"], "os_file_lock_v1");
    assert_eq!(metadata["pid"], std::process::id());
    assert_eq!(metadata["app_version"], "1.2.3");
    assert!(metadata["started_at"]
        .as_str()
        .unwrap_or_default()
        .ends_with('Z'));
    assert!(
        !metadata["executable"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "executable should be populated"
    );

    guard.release().expect("release lock");
    assert!(
        lock_path.exists(),
        "upgrade.lock should remain as persistent OS lock anchor"
    );

    let guard = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.4")
        .expect("reacquire lock after release");
    guard.release().expect("release second lock");
}

#[test]
fn workspace_upgrade_file_lock_rejects_concurrent_os_lock() {
    let temp = TempDir::new("workspace-upgrade-lock-concurrent").expect("temp dir");
    let lock_path = temp.path().join("upgrade").join("upgrade.lock");
    let guard = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.3")
        .expect("acquire first lock");

    let err = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.3")
        .expect_err("second lock should fail");
    assert!(err.is_locked(), "unexpected lock error: {err}");
    assert_eq!(
        err.to_string(),
        format!(
            "workspace upgrade is already running: {}",
            path_string(&lock_path)
        )
    );

    guard.release().expect("release first lock");
}

#[test]
fn workspace_upgrade_file_lock_ignores_residual_os_lock_metadata() {
    let temp = TempDir::new("workspace-upgrade-lock-residual").expect("temp dir");
    let lock_path = temp.path().join("upgrade").join("upgrade.lock");
    write_upgrade_lock_metadata(
        &lock_path,
        json!({
            "lock_scheme": "os_file_lock_v1",
            "pid": missing_pid(),
            "app_version": "old",
            "started_at": "20000101T000000Z"
        }),
    );

    let guard = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "new")
        .expect("acquire lock");
    let metadata = read_upgrade_lock_metadata(&lock_path);
    assert_eq!(metadata["lock_scheme"], "os_file_lock_v1");
    assert_eq!(metadata["app_version"], "new");
    assert_eq!(metadata["pid"], std::process::id());
    guard.release().expect("release lock");
}

#[test]
fn workspace_upgrade_file_lock_ignores_corrupt_legacy_lock() {
    let temp = TempDir::new("workspace-upgrade-lock-corrupt").expect("temp dir");
    let lock_path = temp.path().join("upgrade").join("upgrade.lock");
    std::fs::create_dir_all(lock_path.parent().expect("lock parent")).expect("create lock dir");
    std::fs::write(&lock_path, "not-json\n").expect("write corrupt lock");

    let guard = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.3")
        .expect("acquire lock");
    let metadata = read_upgrade_lock_metadata(&lock_path);
    assert_eq!(metadata["lock_scheme"], "os_file_lock_v1");
    guard.release().expect("release lock");
}

#[test]
fn workspace_upgrade_file_lock_ignores_dead_legacy_pid() {
    let temp = TempDir::new("workspace-upgrade-lock-dead-pid").expect("temp dir");
    let lock_path = temp.path().join("upgrade").join("upgrade.lock");
    write_upgrade_lock_metadata(
        &lock_path,
        json!({
            "pid": missing_pid(),
            "app_version": "legacy",
            "started_at": "29991231T235959Z"
        }),
    );

    let guard = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.3")
        .expect("dead legacy pid should be ignored");
    let metadata = read_upgrade_lock_metadata(&lock_path);
    assert_eq!(metadata["lock_scheme"], "os_file_lock_v1");
    guard.release().expect("release lock");
}

#[test]
fn workspace_upgrade_file_lock_ignores_old_legacy_live_pid() {
    let temp = TempDir::new("workspace-upgrade-lock-old-live").expect("temp dir");
    let lock_path = temp.path().join("upgrade").join("upgrade.lock");
    write_upgrade_lock_metadata(
        &lock_path,
        json!({
            "pid": std::process::id(),
            "app_version": "legacy",
            "started_at": "20000101T000000Z"
        }),
    );

    let guard = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.3")
        .expect("old live legacy lock should be ignored");
    let metadata = read_upgrade_lock_metadata(&lock_path);
    assert_eq!(metadata["lock_scheme"], "os_file_lock_v1");
    guard.release().expect("release lock");
}

#[test]
fn workspace_upgrade_file_lock_rejects_recent_legacy_live_pid() {
    let temp = TempDir::new("workspace-upgrade-lock-recent-live").expect("temp dir");
    let lock_path = temp.path().join("upgrade").join("upgrade.lock");
    write_upgrade_lock_metadata(
        &lock_path,
        json!({
            "pid": std::process::id(),
            "app_version": "legacy",
            "started_at": "29991231T235959Z"
        }),
    );

    let err = workspace_upgrade::acquire_file_lock(&path_string(&lock_path), "1.2.3")
        .expect_err("recent live legacy lock should fail");
    assert!(err.is_locked(), "unexpected lock error: {err}");
    let metadata = read_upgrade_lock_metadata(&lock_path);
    assert_eq!(metadata["lock_scheme"], Value::Null);
    assert_eq!(metadata["app_version"], "legacy");
}

#[test]
fn doctor_workspace_upgrade_uses_meta_journal_and_go_warning_rules() {
    let workspace = TempDir::new("workspace-upgrade-doctor").expect("temp workspace");
    success_json(&awiki_cmd(&["status"], workspace.path()));
    let resolved = tenant_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    workspace_upgrade::save_meta(
        &paths.meta_path,
        &workspace_upgrade::Meta {
            workspace_schema_version: 4,
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
        4
    );
    assert_eq!(
        workspace_check["details"]["detection"]["current_version_source"],
        "meta"
    );

    workspace_upgrade::save_journal(
        &paths.journal_path,
        &workspace_upgrade::Journal {
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
    success_json(&awiki_cmd(&["status"], workspace.path()));
    let resolved = tenant_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    workspace_upgrade::save_meta(
        &paths.meta_path,
        &workspace_upgrade::Meta {
            workspace_schema_version: 4,
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
    assert_eq!(upgrade["meta"]["workspace_schema_version"], 4);
    assert_eq!(upgrade["journal"], Value::Null);
    assert_eq!(upgrade["detection"]["current_version"], 4);
    assert_eq!(upgrade["detection"]["latest_version"], 4);
    assert_eq!(upgrade["detection"]["current_version_source"], "meta");
    assert!(upgrade.get("actions").is_none());
}

#[test]
fn workspace_upgrade_create_backup_copies_go_named_inputs_and_sqlite_backup() {
    let workspace = TempDir::new("workspace-upgrade-backup").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");
    std::fs::write(&paths.legacy_config_file, "{\"legacy\":true}\n").expect("write legacy config");
    std::fs::create_dir_all(Path::new(&paths.identity_dir).join("alice"))
        .expect("create identity dir");
    std::fs::write(
        Path::new(&paths.identity_dir)
            .join("alice")
            .join("identity.json"),
        "{\"name\":\"alice\"}\n",
    )
    .expect("write identity");
    workspace_upgrade::save_meta(
        &paths.meta_path,
        &workspace_upgrade::Meta {
            workspace_schema_version: 3,
            app_version: "1.2.3".to_string(),
            updated_at: "2026-05-14T00:00:00Z".to_string(),
            last_upgrade_id: String::new(),
            last_backup_dir: String::new(),
            warnings: Vec::new(),
        },
    )
    .expect("save meta");
    workspace_upgrade::save_journal(
        &paths.journal_path,
        &workspace_upgrade::Journal {
            upgrade_id: "upgrade-1".to_string(),
            from_version: 0,
            to_version: 3,
            current_step: "backup".to_string(),
            phase: "running".to_string(),
            backup_dir: String::new(),
            started_at: "2026-05-14T00:01:00Z".to_string(),
            app_version: "1.2.3".to_string(),
        },
    )
    .expect("save journal");
    let db = open_local_state(&resolved.paths).expect("open db");
    db.execute(
        "INSERT INTO messages(msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, content, stored_at, credential_name) VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?6, ?7)",
        rusqlite::params![
            "msg-1",
            "alice-id",
            "did:owner:alice",
            "dm:alice-peer",
            "hello",
            "2026-05-14T00:00:00Z",
            "alice"
        ],
    )
    .expect("insert db row");
    drop(db);

    let backup_dir =
        workspace_upgrade::create_backup(&paths, "fixed'backup").expect("create backup");
    let backup = PathBuf::from(&backup_dir);
    assert_eq!(backup, Path::new(&paths.backup_root).join("fixed'backup"));
    assert_eq!(
        std::fs::read_to_string(backup.join("config.yaml.bak")).unwrap(),
        "schema_version: 1\n"
    );
    assert_eq!(
        std::fs::read_to_string(backup.join("config.json.bak")).unwrap(),
        "{\"legacy\":true}\n"
    );
    assert_eq!(
        std::fs::read_to_string(
            backup
                .join("identities")
                .join("alice")
                .join("identity.json")
        )
        .unwrap(),
        "{\"name\":\"alice\"}\n"
    );
    assert!(backup.join("meta.json.bak").is_file());
    assert!(backup.join("upgrade_journal.json.bak").is_file());
    let backup_db =
        rusqlite::Connection::open(backup.join("awiki-cli.db.bak")).expect("open sqlite backup");
    let count: i64 = backup_db
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE msg_id = 'msg-1'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite backup");
    assert_eq!(count, 1);
}

#[test]
fn workspace_upgrade_create_backup_skips_absent_inputs_without_placeholders() {
    let workspace = TempDir::new("workspace-upgrade-backup-sparse").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    std::fs::write(&paths.config_file, "schema_version: 1\n").expect("write config");

    let backup_dir =
        workspace_upgrade::create_backup(&paths, "sparse").expect("create sparse backup");
    let backup = PathBuf::from(&backup_dir);
    assert!(backup.join("config.yaml.bak").is_file());
    assert!(!backup.join("config.json.bak").exists());
    assert!(!backup.join("identities").exists());
    assert!(!backup.join("awiki-cli.db.bak").exists());
    assert!(!backup.join("meta.json.bak").exists());
    assert!(!backup.join("upgrade_journal.json.bak").exists());
}

#[test]
fn workspace_upgrade_backup_sqlite_replaces_existing_destination_and_escapes_path() {
    let temp = TempDir::new("workspace-upgrade-sqlite-backup").expect("temp dir");
    let db_path = temp.path().join("source.db");
    let paths = workspace_config::Paths {
        database_file: path_string(&db_path),
        ..test_resolved(temp.path()).paths
    };
    let db = open_local_state(&paths).expect("open source db");
    db.execute(
        "INSERT INTO messages(msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, content, stored_at, credential_name) VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?6, ?7)",
        rusqlite::params![
            "msg-2",
            "bob-id",
            "did:owner:bob",
            "dm:bob-peer",
            "hello",
            "2026-05-14T00:00:00Z",
            "bob"
        ],
    )
    .expect("insert source row");
    drop(db);
    let backup_path = temp.path().join("out").join("backup's.db");
    std::fs::create_dir_all(backup_path.parent().expect("backup parent")).expect("create parent");
    std::fs::write(&backup_path, "stale").expect("write stale destination");

    workspace_upgrade::backup_sqlite_database(&path_string(&db_path), &path_string(&backup_path))
        .expect("backup sqlite database");
    let backup = rusqlite::Connection::open(&backup_path).expect("open backup db");
    let count: i64 = backup
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE msg_id = 'msg-2'",
            [],
            |row| row.get(0),
        )
        .expect("query backup row");
    assert_eq!(count, 1);
}

fn test_resolved(root: &Path) -> workspace_config::Resolved {
    workspace_config::Resolved {
        paths: workspace_config::Paths {
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
        user_service_endpoint: "https://awiki.ai".to_string(),
        message_service_endpoint: "https://awiki.ai".to_string(),
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

fn tenant_resolved(product_home: &Path) -> workspace_config::Resolved {
    test_resolved(&product_home.join("tenants").join("default"))
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_awiki-cli"))
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
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

fn open_local_state(paths: &workspace_config::Paths) -> rusqlite::Result<rusqlite::Connection> {
    if let Some(parent) = Path::new(&paths.database_file).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
    }
    let connection = rusqlite::Connection::open(&paths.database_file)?;
    im_core::compat::local_state::ensure_schema(&connection)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
    Ok(connection)
}

fn write_upgrade_lock_metadata(path: &Path, metadata: Value) {
    std::fs::create_dir_all(path.parent().expect("lock parent")).expect("create lock dir");
    let mut raw = serde_json::to_vec_pretty(&metadata).expect("serialize lock metadata");
    raw.push(b'\n');
    std::fs::write(path, raw).expect("write lock metadata");
}

fn read_upgrade_lock_metadata(path: &Path) -> Value {
    let raw = std::fs::read(path).expect("read lock metadata");
    serde_json::from_slice(&raw).expect("parse lock metadata")
}

fn missing_pid() -> i64 {
    for pid in (2..=999_999_i64).rev() {
        if !process_alive_for_test(pid) {
            return pid;
        }
    }
    -1
}

#[cfg(not(windows))]
fn process_alive_for_test(pid: i64) -> bool {
    if pid <= 0 || pid > i32::MAX as i64 {
        return false;
    }
    extern "C" {
        fn kill(pid: std::os::raw::c_int, sig: std::os::raw::c_int) -> std::os::raw::c_int;
    }
    let result = unsafe { kill(pid as std::os::raw::c_int, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(windows)]
fn process_alive_for_test(pid: i64) -> bool {
    if pid <= 0 || pid > u32::MAX as i64 {
        return false;
    }
    false
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
