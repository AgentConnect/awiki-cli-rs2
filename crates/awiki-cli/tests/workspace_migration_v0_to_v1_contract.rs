use anp::authentication::{create_did_wba_document, DidDocumentOptions};
use awiki_cli::{workspace_config, workspace_upgrade};
use rusqlite::OpenFlags;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = im_core::compat::local_state::SCHEMA_VERSION;

const LEGACY_V6_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS contacts (
    owner_did       TEXT NOT NULL DEFAULT '',
    did             TEXT NOT NULL,
    name            TEXT,
    handle          TEXT,
    first_seen_at   TEXT,
    last_seen_at    TEXT,
    metadata        TEXT,
    PRIMARY KEY (owner_did, did)
);

CREATE TABLE IF NOT EXISTS messages (
    msg_id          TEXT NOT NULL,
    owner_did       TEXT NOT NULL DEFAULT '',
    thread_id       TEXT NOT NULL,
    direction       INTEGER NOT NULL DEFAULT 0,
    sender_did      TEXT,
    receiver_did    TEXT,
    group_id        TEXT,
    group_did       TEXT,
    content_type    TEXT DEFAULT 'text',
    content         TEXT,
    title           TEXT,
    server_seq      INTEGER,
    sent_at         TEXT,
    stored_at       TEXT NOT NULL,
    is_e2ee         INTEGER DEFAULT 0,
    is_read         INTEGER DEFAULT 0,
    sender_name     TEXT,
    metadata        TEXT,
    credential_name TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (msg_id, owner_did)
);

CREATE TABLE IF NOT EXISTS e2ee_outbox (
    outbox_id            TEXT PRIMARY KEY,
    owner_did            TEXT NOT NULL DEFAULT '',
    peer_did             TEXT NOT NULL,
    session_id           TEXT,
    original_type        TEXT NOT NULL DEFAULT 'text',
    plaintext            TEXT NOT NULL,
    local_status         TEXT NOT NULL DEFAULT 'queued',
    attempt_count        INTEGER NOT NULL DEFAULT 0,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    credential_name      TEXT NOT NULL DEFAULT ''
);
"#;

const LEGACY_V11_EXTRA_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS e2ee_sessions (
    owner_did        TEXT NOT NULL DEFAULT '',
    peer_did         TEXT NOT NULL,
    session_id       TEXT NOT NULL,
    is_initiator     INTEGER NOT NULL DEFAULT 0,
    send_chain_key   TEXT NOT NULL,
    recv_chain_key   TEXT NOT NULL,
    send_seq         INTEGER NOT NULL DEFAULT 0,
    recv_seq         INTEGER NOT NULL DEFAULT 0,
    expires_at       REAL,
    created_at       TEXT NOT NULL,
    active_at        TEXT,
    peer_confirmed   INTEGER NOT NULL DEFAULT 0,
    credential_name  TEXT NOT NULL DEFAULT '',
    updated_at       TEXT NOT NULL,
    PRIMARY KEY (owner_did, peer_did),
    UNIQUE (owner_did, session_id)
);
"#;

#[test]
fn refresh_resolved_config_syncs_mail_service_url_from_config() {
    let workspace = TempDir::new("workspace-upgrade-refresh-mail").expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.service_base_url = "https://stale.example".to_string();
    resolved.mail_service_url = "https://stale-mail.example".to_string();
    resolved.anp_service_endpoint.clear();
    resolved.anp_service_did.clear();
    std::fs::write(
        &resolved.paths.config_file,
        concat!(
            "runtime:\n",
            "  mode: http\n",
            "  socket_path: /tmp/awiki.sock\n",
            "output:\n",
            "  format: table\n",
            "  no_color: true\n",
            "services:\n",
            "  service_base_url: https://api.example///\n",
            "  did_domain: tenant.example\n",
            "  mail_service_url: https://mail.example///\n",
            "  anp_service_endpoint: https://api.example/anp-im/rpc\n",
            "  anp_service_did: did:wba:api.example\n",
            "  ca_bundle: /tmp/ca.pem\n",
        ),
    )
    .expect("write config");

    let refreshed =
        workspace_upgrade::refresh_resolved_config(&resolved).expect("refresh resolved");

    assert!(refreshed.config_exists);
    assert_eq!(refreshed.config_schema_version, 0);
    assert_eq!(refreshed.runtime_mode, "http");
    assert_eq!(refreshed.runtime_socket_path, "/tmp/awiki.sock");
    assert_eq!(refreshed.output_format, "table");
    assert!(refreshed.no_color);
    assert_eq!(refreshed.service_base_url, "https://api.example");
    assert_eq!(refreshed.did_domain, "tenant.example");
    assert_eq!(refreshed.mail_service_url, "https://mail.example");
    assert_eq!(
        refreshed.anp_service_endpoint,
        "https://api.example/anp-im/rpc"
    );
    assert_eq!(refreshed.anp_service_did, "did:wba:api.example");
    assert_eq!(refreshed.ca_bundle, "/tmp/ca.pem");
}

#[test]
fn refresh_resolved_config_derives_mail_service_url_from_service_base_url() {
    let workspace = TempDir::new("workspace-upgrade-refresh-mail-derived").expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.mail_service_url.clear();
    resolved.anp_service_endpoint.clear();
    resolved.anp_service_did.clear();
    std::fs::write(
        &resolved.paths.config_file,
        "services:\n  service_base_url: https://awiki.info///\n",
    )
    .expect("write config");

    let refreshed =
        workspace_upgrade::refresh_resolved_config(&resolved).expect("refresh resolved");

    assert_eq!(refreshed.service_base_url, "https://awiki.info");
    assert_eq!(refreshed.mail_service_url, "https://awiki.info");
    assert_eq!(
        refreshed.anp_service_endpoint,
        "https://awiki.info/anp-im/rpc"
    );
    assert_eq!(refreshed.anp_service_did, "did:wba:awiki.info");
}

#[test]
fn refresh_resolved_config_preserves_current_mail_when_config_omits_mail() {
    let workspace =
        TempDir::new("workspace-upgrade-refresh-mail-preserve").expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.mail_service_url = "https://mail.current.example".to_string();
    std::fs::write(
        &resolved.paths.config_file,
        "services:\n  service_base_url: https://api.changed.example///\n",
    )
    .expect("write config");

    let refreshed =
        workspace_upgrade::refresh_resolved_config(&resolved).expect("refresh resolved");

    assert_eq!(refreshed.service_base_url, "https://api.changed.example");
    assert_eq!(refreshed.mail_service_url, "https://mail.current.example");
}

#[test]
fn refresh_resolved_config_keeps_go_required_and_missing_config_boundaries() {
    let workspace = TempDir::new("workspace-upgrade-refresh-missing").expect("temp workspace");
    let resolved = test_resolved(workspace.path());

    let required = workspace_upgrade::refresh_resolved_config_optional(None)
        .expect_err("missing resolved config should fail");
    assert_eq!(required.to_string(), "resolved config is required");

    let refreshed =
        workspace_upgrade::refresh_resolved_config(&resolved).expect("missing config is ok");
    assert!(!refreshed.config_exists);
    assert_eq!(refreshed.config_schema_version, 0);
    assert_eq!(refreshed.service_base_url, resolved.service_base_url);
    assert_eq!(refreshed.mail_service_url, resolved.mail_service_url);
}

#[test]
fn ensure_target_store_schema_matches_go_helper_boundary() {
    let workspace = TempDir::new("workspace-upgrade-ensure-store-schema").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::create_dir_all(Path::new(&resolved.paths.database_file).parent().unwrap())
        .expect("create data dir");
    drop(rusqlite::Connection::open(&resolved.paths.database_file).expect("create empty db"));

    workspace_upgrade::ensure_target_store_schema(&resolved.paths).expect("ensure target schema");

    let verify = open_read_only(&resolved.paths.database_file).expect("open verify db");
    assert_eq!(
        current_schema_version(&verify).expect("schema version"),
        SCHEMA_VERSION
    );
    assert_table_exists(&verify, "messages");
    assert_table_exists(&verify, "contact_handle_bindings");
    assert_table_exists(&verify, "direct_e2ee_sessions");
    assert_table_exists(&verify, "identity_did_history");
}

#[test]
fn ensure_target_store_schema_reuses_store_version_errors() {
    let newer = TempDir::new("workspace-upgrade-store-newer").expect("temp workspace");
    let newer_resolved = test_resolved(newer.path());
    std::fs::create_dir_all(
        Path::new(&newer_resolved.paths.database_file)
            .parent()
            .unwrap(),
    )
    .expect("create data dir");
    {
        let db =
            rusqlite::Connection::open(&newer_resolved.paths.database_file).expect("open newer db");
        db.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("set newer version");
    }
    let err = workspace_upgrade::ensure_target_store_schema(&newer_resolved.paths)
        .expect_err("newer schema should fail");
    assert_eq!(
        err.to_string(),
        format!(
            "sqlite schema version {} is newer than supported {}",
            SCHEMA_VERSION + 1,
            SCHEMA_VERSION
        )
    );

    let old = TempDir::new("workspace-upgrade-store-old").expect("temp workspace");
    let old_resolved = test_resolved(old.path());
    std::fs::create_dir_all(
        Path::new(&old_resolved.paths.database_file)
            .parent()
            .unwrap(),
    )
    .expect("create data dir");
    {
        let db =
            rusqlite::Connection::open(&old_resolved.paths.database_file).expect("open old db");
        db.pragma_update(None, "user_version", 5)
            .expect("set old version");
    }
    let err = workspace_upgrade::ensure_target_store_schema(&old_resolved.paths)
        .expect_err("old schema should fail");
    assert_eq!(
        err.to_string(),
        "sqlite schema version 5 requires owner-identity migration before schema 20"
    );
}

#[test]
fn validate_sqlite_health_matches_go_pragmas() {
    let healthy = rusqlite::Connection::open_in_memory().expect("open healthy db");
    healthy
        .pragma_update(None, "foreign_keys", "ON")
        .expect("enable fk");
    workspace_upgrade::validate_sqlite_health(&healthy).expect("healthy sqlite");

    let fk = rusqlite::Connection::open_in_memory().expect("open fk db");
    fk.pragma_update(None, "foreign_keys", "OFF")
        .expect("disable fk");
    fk.execute_batch(
        r#"
CREATE TABLE parent (id INTEGER PRIMARY KEY);
CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id));
INSERT INTO child (id, parent_id) VALUES (1, 42);
"#,
    )
    .expect("seed fk violation");
    fk.pragma_update(None, "foreign_keys", "ON")
        .expect("enable fk");

    let err = workspace_upgrade::validate_sqlite_health(&fk)
        .expect_err("foreign key violation should fail");
    assert_eq!(
        err.to_string(),
        "PRAGMA foreign_key_check returned foreign key violations"
    );
}

#[test]
fn workspace_v0_to_v1_validate_accepts_current_config_and_healthy_sqlite() {
    let workspace = TempDir::new("workspace-v0-v1-validate-current").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::write(&resolved.paths.config_file, "schema_version: 1\n").expect("write config");
    std::fs::create_dir_all(Path::new(&resolved.paths.database_file).parent().unwrap())
        .expect("create data dir");
    {
        let db = open_local_state(&resolved.paths).expect("open writable db");
        ensure_local_state_schema(&db).expect("ensure schema");
    }
    let context = workspace_upgrade::new_context(&resolved, "1.2.3");

    validate_first_migration(&context).expect("current config and healthy sqlite validate");
}

#[test]
fn workspace_v0_to_v1_validate_rejects_wrong_config_schema() {
    let workspace = TempDir::new("workspace-v0-v1-validate-config").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::write(&resolved.paths.config_file, "schema_version: 0\n").expect("write config");
    let context = workspace_upgrade::new_context(&resolved, "1.2.3");

    let err = validate_first_migration(&context).expect_err("wrong schema should fail");

    assert_eq!(err.to_string(), "config schema version = 0, want 1");
}

#[test]
fn workspace_v0_to_v1_validate_rejects_wrong_sqlite_schema() {
    let workspace =
        TempDir::new("workspace-v0-v1-validate-sqlite-version").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::create_dir_all(Path::new(&resolved.paths.database_file).parent().unwrap())
        .expect("create data dir");
    {
        let db = rusqlite::Connection::open(&resolved.paths.database_file).expect("open db");
        db.pragma_update(None, "user_version", SCHEMA_VERSION - 1)
            .expect("set wrong schema");
    }
    let context = workspace_upgrade::new_context(&resolved, "1.2.3");

    let err = validate_first_migration(&context).expect_err("wrong sqlite schema should fail");

    assert_eq!(
        err.to_string(),
        format!(
            "sqlite schema version = {}, want {}",
            SCHEMA_VERSION - 1,
            SCHEMA_VERSION
        )
    );
}

#[test]
fn workspace_v0_to_v1_validate_reuses_sqlite_health_errors() {
    let workspace = TempDir::new("workspace-v0-v1-validate-sqlite-health").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::create_dir_all(Path::new(&resolved.paths.database_file).parent().unwrap())
        .expect("create data dir");
    {
        let db = rusqlite::Connection::open(&resolved.paths.database_file).expect("open db");
        db.pragma_update(None, "foreign_keys", "OFF")
            .expect("disable fk");
        db.execute_batch(
            r#"
CREATE TABLE parent (id INTEGER PRIMARY KEY);
CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id));
INSERT INTO child (id, parent_id) VALUES (1, 42);
"#,
        )
        .expect("seed fk violation");
        db.pragma_update(None, "user_version", SCHEMA_VERSION)
            .expect("set current schema");
    }
    let context = workspace_upgrade::new_context(&resolved, "1.2.3");

    let err = validate_first_migration(&context).expect_err("sqlite health should fail");

    assert_eq!(
        err.to_string(),
        "PRAGMA foreign_key_check returned foreign key violations"
    );
}

#[test]
fn workspace_v0_to_v1_validate_requires_imported_identity_after_legacy_detection() {
    let workspace = TempDir::new("workspace-v0-v1-validate-identity").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let mut context = workspace_upgrade::new_context(&resolved, "1.2.3");
    let detection = workspace_upgrade::Detection {
        has_workspace: false,
        legacy_identity_exists: true,
        ..Default::default()
    };
    context.inspection = Some(workspace_upgrade::Inspection {
        paths: context.paths.clone(),
        detection,
        ..Default::default()
    });

    let err =
        validate_first_migration(&context).expect_err("missing imported identity should fail");

    assert_eq!(
        err.to_string(),
        "expected at least one imported identity after legacy upgrade"
    );
}

#[test]
fn workspace_v0_to_v1_config_apply_stamps_existing_config_schema() {
    let workspace = TempDir::new("workspace-v0-v1-config-existing").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    std::fs::write(
        &resolved.paths.config_file,
        concat!(
            "schema_version: 0\n",
            "runtime:\n",
            "  mode: http\n",
            "services:\n",
            "  service_base_url: https://platform.example\n",
            "  did_domain: old.example\n",
        ),
    )
    .expect("write old config");
    std::fs::write(
        &paths.legacy_config_file,
        r#"{"services":{"service_base_url":"https://legacy.example"}}"#,
    )
    .expect("write lower-priority legacy config");
    std::fs::create_dir_all(Path::new(&paths.legacy_settings_path).parent().unwrap())
        .expect("create lower-priority settings dir");
    std::fs::write(
        &paths.legacy_settings_path,
        r#"{"user_service_url":"https://settings.example","molt_message_url":"https://settings.example","did_domain":"settings.example"}"#,
    )
    .expect("write lower-priority settings");
    let context = context_with_detection(&resolved, |detection| {
        detection.config_exists = true;
        detection.legacy_config_exists = true;
        detection.legacy_settings_exists = true;
        detection.has_workspace = true;
    });

    workspace_upgrade::apply_workspace_v0_to_v1_config(&context).expect("apply config branch");

    let text = std::fs::read_to_string(&resolved.paths.config_file).expect("read config");
    assert_contains(&text, "schema_version: 1\n");
    assert_contains(&text, "  mode: http\n");
    assert_contains(&text, "  service_base_url: https://platform.example\n");
    assert_contains(&text, "  did_domain: old.example\n");
    assert!(
        Path::new(&paths.legacy_config_file).exists(),
        "legacy config should remain when canonical config wins precedence"
    );
}

#[test]
fn workspace_v0_to_v1_config_apply_migrates_legacy_config_json_and_removes_it() {
    let workspace = TempDir::new("workspace-v0-v1-config-json").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    std::fs::write(
        &paths.legacy_config_file,
        r#"{"schema_version":1,"services":{"service_base_url":"https://legacy.example","did_domain":"legacy.example"},"runtime":{"mode":"http"}}"#,
    )
    .expect("write legacy config json");
    let context = context_with_detection(&resolved, |detection| {
        detection.legacy_config_exists = true;
        detection.has_legacy = true;
    });

    workspace_upgrade::apply_workspace_v0_to_v1_config(&context).expect("migrate legacy config");

    assert!(
        !Path::new(&paths.legacy_config_file).exists(),
        "legacy config should be removed after migration"
    );
    let text = std::fs::read_to_string(&resolved.paths.config_file).expect("read config");
    assert_contains(&text, "schema_version: 1\n");
    assert_contains(&text, "  mode: http\n");
    assert_contains(&text, "  service_base_url: https://legacy.example\n");
    assert_contains(&text, "  did_domain: legacy.example\n");
}

#[test]
fn workspace_v0_to_v1_config_apply_imports_legacy_settings_when_no_workspace() {
    let workspace = TempDir::new("workspace-v0-v1-config-settings").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    std::fs::create_dir_all(Path::new(&paths.legacy_settings_path).parent().unwrap())
        .expect("create legacy settings dir");
    std::fs::write(
        &paths.legacy_settings_path,
        r#"{"user_service_url":"https://settings.example/","molt_message_url":"https://settings.example/","did_domain":"tenant.example","message_transport":{"receive_mode":"websocket"}}"#,
    )
    .expect("write legacy settings");
    let context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = true;
        detection.legacy_settings_exists = true;
    });

    workspace_upgrade::apply_workspace_v0_to_v1_config(&context).expect("migrate legacy settings");

    let text = std::fs::read_to_string(&resolved.paths.config_file).expect("read config");
    assert_contains(&text, "schema_version: 1\n");
    assert_contains(&text, "  mode: websocket\n");
    assert_contains(&text, "  service_base_url: https://settings.example\n");
    assert_contains(&text, "  did_domain: tenant.example\n");
    assert!(
        !Path::new(&resolved.paths.database_file).exists(),
        "config migration must not create sqlite database"
    );
    assert!(
        !Path::new(&resolved.paths.identity_dir)
            .join("index.json")
            .exists(),
        "config migration must not create identity index"
    );
}

#[test]
fn workspace_v0_to_v1_config_apply_keeps_go_guard_and_split_settings_error() {
    let missing = workspace_upgrade::apply_workspace_v0_to_v1_config_optional(None)
        .expect_err("missing context should match Go guard");
    assert_eq!(
        missing.to_string(),
        "workspace upgrade requires a resolved config"
    );

    let workspace = TempDir::new("workspace-v0-v1-config-settings-split").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let paths = workspace_upgrade::resolve_paths(&resolved);
    std::fs::create_dir_all(Path::new(&paths.legacy_settings_path).parent().unwrap())
        .expect("create legacy settings dir");
    std::fs::write(
        &paths.legacy_settings_path,
        r#"{"user_service_url":"https://auth.example","molt_message_url":"https://message.example","did_domain":"tenant.example"}"#,
    )
    .expect("write split legacy settings");
    let context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.legacy_settings_exists = true;
    });

    let err = workspace_upgrade::apply_workspace_v0_to_v1_config(&context)
        .expect_err("split settings should fail");

    assert_eq!(
        err.to_string(),
        "legacy settings use different user_service_url (https://auth.example) and molt_message_url (https://message.example); automatic migration to one service_base_url is not supported"
    );
}

#[test]
fn workspace_v0_to_v1_legacy_imports_identity_and_sqlite_when_no_workspace() {
    let workspace = TempDir::new("workspace-v0-v1-legacy-import").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    seed_flat_legacy_identity(&resolved, "legacy", "did:wba:example.test:user:e1_legacy");
    seed_current_legacy_db_message(
        &resolved,
        "legacy-msg",
        "did:wba:example.test:user:e1_legacy",
        "legacy",
    );
    let context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = true;
        detection.legacy_identity_exists = true;
        detection.legacy_database_exists = true;
    });

    let imported = workspace_upgrade::apply_workspace_v0_to_v1_legacy_imports(&context)
        .expect("import legacy state");

    assert_eq!(imported.imported.len(), 1);
    assert_eq!(imported.imported[0].identity_name, "legacy");
    let index = read_identity_index(&resolved);
    let credentials = index["credentials"]
        .as_object()
        .expect("identity credentials object");
    assert_eq!(credentials.len(), 1);
    assert_eq!(
        credentials["legacy"]["did"],
        "did:wba:example.test:user:e1_legacy"
    );
    let target = open_read_only(&resolved.paths.database_file).expect("open target db");
    assert_eq!(
        current_schema_version(&target).expect("schema version"),
        SCHEMA_VERSION
    );
    let (owner_did, content): (String, String) = target
        .query_row(
            "SELECT owner_did, content FROM messages WHERE msg_id = ?1",
            ["legacy-msg"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("imported legacy message");
    assert_eq!(owner_did, "did:wba:example.test:user:e1_legacy");
    assert_eq!(content, "legacy hello");
    assert!(
        !Path::new(&resolved.paths.config_file).exists(),
        "legacy import helper must not refresh or write config"
    );
}

#[test]
fn workspace_v0_to_v1_legacy_imports_skip_when_workspace_exists() {
    let workspace = TempDir::new("workspace-v0-v1-legacy-skip-workspace").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    seed_flat_legacy_identity(&resolved, "legacy", "did:wba:example.test:user:e1_legacy");
    seed_current_legacy_db_message(
        &resolved,
        "legacy-msg",
        "did:wba:example.test:user:e1_legacy",
        "legacy",
    );
    let context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = true;
        detection.has_legacy = true;
        detection.legacy_identity_exists = true;
        detection.legacy_database_exists = true;
    });

    let imported = workspace_upgrade::apply_workspace_v0_to_v1_legacy_imports(&context)
        .expect("skip legacy import");

    assert!(imported.imported.is_empty());
    assert!(
        !Path::new(&resolved.paths.identity_dir)
            .join("index.json")
            .exists(),
        "workspace guard must skip identity import"
    );
    assert!(
        !Path::new(&resolved.paths.database_file).exists(),
        "workspace guard must skip sqlite import"
    );
}

#[test]
fn workspace_v0_to_v1_legacy_imports_skip_when_no_legacy_detected() {
    let workspace = TempDir::new("workspace-v0-v1-legacy-skip-empty").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    seed_flat_legacy_identity(&resolved, "legacy", "did:wba:example.test:user:e1_legacy");
    seed_current_legacy_db_message(
        &resolved,
        "legacy-msg",
        "did:wba:example.test:user:e1_legacy",
        "legacy",
    );
    let context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = false;
        detection.legacy_identity_exists = true;
        detection.legacy_database_exists = true;
    });

    let imported = workspace_upgrade::apply_workspace_v0_to_v1_legacy_imports(&context)
        .expect("skip legacy import");

    assert!(imported.imported.is_empty());
    assert!(
        !Path::new(&resolved.paths.identity_dir)
            .join("index.json")
            .exists(),
        "legacy guard must skip identity import"
    );
    assert!(
        !Path::new(&resolved.paths.database_file).exists(),
        "legacy guard must skip sqlite import"
    );
}

#[test]
fn workspace_v0_to_v1_legacy_imports_pre_v6_sqlite_after_identity_import() {
    let workspace = TempDir::new("workspace-v0-v1-legacy-pre-v6-success").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    seed_flat_legacy_identity(&resolved, "legacy", "did:wba:example.test:user:e1_legacy");
    seed_v5_legacy_db_message(&resolved, "legacy-v5-msg", "legacy");
    let context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = true;
        detection.legacy_identity_exists = true;
        detection.legacy_database_exists = true;
    });

    let imported = workspace_upgrade::apply_workspace_v0_to_v1_legacy_imports(&context)
        .expect("import legacy v5 state");

    assert_eq!(imported.imported.len(), 1);
    let target = open_read_only(&resolved.paths.database_file).expect("open target db");
    let (owner_did, content): (String, String) = target
        .query_row(
            "SELECT owner_did, content FROM messages WHERE msg_id = ?1",
            ["legacy-v5-msg"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("imported legacy v5 message");
    assert_eq!(owner_did, "did:wba:example.test:user:e1_legacy");
    assert_eq!(content, "legacy v5 hello");
}

#[test]
fn workspace_v0_to_v1_legacy_imports_propagate_pre_v6_owner_error() {
    let workspace = TempDir::new("workspace-v0-v1-legacy-pre-v6").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    seed_empty_legacy_db_with_version(&resolved, 5);
    let context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = true;
        detection.legacy_database_exists = true;
    });

    let err = workspace_upgrade::apply_workspace_v0_to_v1_legacy_imports(&context)
        .expect_err("pre-v6 import without identity should fail");

    assert_eq!(
        err.to_string(),
        "unsupported legacy sqlite schema version: legacy schema < 6 requires at least one imported identity so owner_did can be inferred"
    );
}

#[test]
fn workspace_v0_to_v1_legacy_imports_keep_go_guards() {
    let missing = workspace_upgrade::apply_workspace_v0_to_v1_legacy_imports_optional(None)
        .expect_err("missing context should match Go guard");
    assert_eq!(
        missing.to_string(),
        "workspace upgrade requires a resolved config"
    );

    let workspace =
        TempDir::new("workspace-v0-v1-legacy-missing-inspection").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let context = workspace_upgrade::new_context(&resolved, "1.2.3");

    let missing_inspection = workspace_upgrade::apply_workspace_v0_to_v1_legacy_imports(&context)
        .expect_err("missing inspection should fail");
    assert_eq!(
        missing_inspection.to_string(),
        "workspace upgrade inspection is required"
    );
}

#[test]
fn workspace_v0_to_v1_local_state_applies_config_imports_and_refreshes_context() {
    let workspace = TempDir::new("workspace-v0-v1-local-state").expect("temp workspace");
    let mut resolved = test_resolved(workspace.path());
    resolved.anp_service_endpoint.clear();
    resolved.anp_service_did.clear();
    let paths = workspace_upgrade::resolve_paths(&resolved);
    std::fs::create_dir_all(Path::new(&paths.legacy_settings_path).parent().unwrap())
        .expect("create legacy settings dir");
    std::fs::write(
        &paths.legacy_settings_path,
        r#"{"user_service_url":"https://local.example/","molt_message_url":"https://local.example/","did_domain":"tenant.local","message_transport":{"receive_mode":"websocket"}}"#,
    )
    .expect("write legacy settings");
    seed_flat_legacy_identity(&resolved, "legacy", "did:wba:example.test:user:e1_legacy");
    seed_current_legacy_db_message(
        &resolved,
        "legacy-msg",
        "did:wba:example.test:user:e1_legacy",
        "legacy",
    );
    let mut context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = true;
        detection.legacy_identity_exists = true;
        detection.legacy_database_exists = true;
        detection.legacy_settings_exists = true;
    });

    let imported = workspace_upgrade::apply_workspace_v0_to_v1_local_state(&mut context)
        .expect("apply local state");

    assert_eq!(imported.imported.len(), 1);
    assert_eq!(context.resolved.runtime_mode, "websocket");
    assert_eq!(context.resolved.service_base_url, "https://local.example");
    assert_eq!(context.resolved.did_domain, "tenant.local");
    assert_eq!(
        context.resolved.anp_service_endpoint,
        "https://local.example/anp-im/rpc"
    );
    assert_eq!(context.resolved.anp_service_did, "did:wba:local.example");
    assert_eq!(
        context.paths.config_file,
        workspace_upgrade::resolve_paths(&context.resolved).config_file
    );
    assert!(
        context.warnings.is_empty(),
        "local helper must not run DID replacement warnings"
    );
    let text = std::fs::read_to_string(&resolved.paths.config_file).expect("read config");
    assert_contains(&text, "schema_version: 1\n");
    assert_contains(&text, "  service_base_url: https://local.example\n");
    let target = open_read_only(&resolved.paths.database_file).expect("open target db");
    assert_eq!(
        current_schema_version(&target).expect("schema version"),
        SCHEMA_VERSION
    );
    let content: String = target
        .query_row(
            "SELECT content FROM messages WHERE msg_id = ?1 AND owner_did = ?2",
            rusqlite::params!["legacy-msg", "did:wba:example.test:user:e1_legacy"],
            |row| row.get(0),
        )
        .expect("imported legacy message");
    assert_eq!(content, "legacy hello");
}

#[test]
fn workspace_v0_to_v1_local_state_ensures_existing_target_schema_without_legacy_db() {
    let workspace = TempDir::new("workspace-v0-v1-local-state-ensure").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::create_dir_all(Path::new(&resolved.paths.database_file).parent().unwrap())
        .expect("create data dir");
    drop(rusqlite::Connection::open(&resolved.paths.database_file).expect("create empty db"));
    let mut context = context_with_detection(&resolved, |detection| {
        detection.config_exists = false;
        detection.has_workspace = false;
        detection.has_legacy = false;
    });

    let imported = workspace_upgrade::apply_workspace_v0_to_v1_local_state(&mut context)
        .expect("ensure existing target schema");

    assert!(imported.imported.is_empty());
    let target = open_read_only(&resolved.paths.database_file).expect("open target db");
    assert_eq!(
        current_schema_version(&target).expect("schema version"),
        SCHEMA_VERSION
    );
    assert_table_exists(&target, "messages");
}

#[test]
fn workspace_v0_to_v1_local_state_keeps_guards_and_warns_on_non_handle_k1_replacement() {
    let missing = workspace_upgrade::apply_workspace_v0_to_v1_local_state_optional(None)
        .expect_err("missing context should match Go guard");
    assert_eq!(
        missing.to_string(),
        "workspace upgrade requires a resolved config"
    );

    let workspace =
        TempDir::new("workspace-v0-v1-local-state-missing-inspection").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let mut missing_inspection_context = workspace_upgrade::new_context(&resolved, "1.2.3");
    let missing_inspection =
        workspace_upgrade::apply_workspace_v0_to_v1_local_state(&mut missing_inspection_context)
            .expect_err("missing inspection should fail");
    assert_eq!(
        missing_inspection.to_string(),
        "workspace upgrade inspection is required"
    );

    let workspace =
        TempDir::new("workspace-v0-v1-local-state-apply-boundary").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    let mut context = context_with_detection(&resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = false;
    });
    workspace_upgrade::apply_workspace_v0_to_v1_local_state(&mut context)
        .expect("empty local state apply");
    let upgrader = workspace_upgrade::new_default_upgrader();
    let plan = upgrader.plan(0, 1).expect("v0 to v1 plan");
    plan[0]
        .apply(&mut context)
        .expect("empty local v0 to v1 migration should now be wired");

    let k1_workspace =
        TempDir::new("workspace-v0-v1-local-state-k1-deferred").expect("temp workspace");
    let k1_resolved = test_resolved(k1_workspace.path());
    seed_flat_legacy_identity(
        &k1_resolved,
        "legacy",
        "did:wba:example.test:user:k1_legacy",
    );
    let mut k1_context = context_with_detection(&k1_resolved, |detection| {
        detection.has_workspace = false;
        detection.has_legacy = true;
        detection.legacy_identity_exists = true;
    });
    plan[0]
        .apply(&mut k1_context)
        .expect("imported non-handle k1 replacement should warn and continue like Go");
    assert_eq!(k1_context.warnings.len(), 1);
    assert_contains(
        &k1_context.warnings[0],
        "Automatic DID replacement skipped for identity legacy (did:wba:example.test:user:k1_legacy):",
    );
    assert_contains(
        &k1_context.warnings[0],
        "invalid input: current did is not a handle did",
    );
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

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn open_read_only(path: &str) -> rusqlite::Result<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn open_local_state(
    paths: &workspace_config::Paths,
) -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    if let Some(parent) = Path::new(&paths.database_file).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = rusqlite::Connection::open(&paths.database_file)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "busy_timeout", 5000)?;
    Ok(connection)
}

fn ensure_local_state_schema(connection: &rusqlite::Connection) -> im_core::ImResult<()> {
    im_core::compat::local_state::ensure_schema(connection)
}

fn current_schema_version(connection: &rusqlite::Connection) -> im_core::ImResult<i64> {
    im_core::compat::local_state::current_schema_version(connection)
}

fn assert_table_exists(connection: &rusqlite::Connection, name: &str) {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1",
            [name],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert_eq!(count, 1, "expected sqlite object {name} to exist");
}

fn validate_first_migration(
    context: &workspace_upgrade::Context,
) -> Result<(), workspace_upgrade::MigrationError> {
    let upgrader = workspace_upgrade::new_default_upgrader();
    let plan = upgrader.plan(0, 1).expect("v0 to v1 plan");
    plan[0].validate(context)
}

fn context_with_detection(
    resolved: &workspace_config::Resolved,
    mutate: impl FnOnce(&mut workspace_upgrade::Detection),
) -> workspace_upgrade::Context {
    let mut context = workspace_upgrade::new_context(resolved, "1.2.3");
    let mut detection = workspace_upgrade::Detection::default();
    mutate(&mut detection);
    context.inspection = Some(workspace_upgrade::Inspection {
        paths: context.paths.clone(),
        detection,
        ..Default::default()
    });
    context
}

fn seed_flat_legacy_identity(resolved: &workspace_config::Resolved, name: &str, did: &str) {
    std::fs::create_dir_all(&resolved.paths.legacy_credentials_dir)
        .expect("create legacy credentials dir");
    let generated = generate_legacy_key_material(did);
    std::fs::write(
        Path::new(&resolved.paths.legacy_credentials_dir).join(format!("{name}.json")),
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
            "did_document": generated.did_document
        }))
        .expect("legacy identity json"),
    )
    .expect("write legacy identity");
}

fn read_identity_index(resolved: &workspace_config::Resolved) -> Value {
    let path = Path::new(&resolved.paths.identity_dir).join("index.json");
    serde_json::from_slice(&std::fs::read(path).expect("read identity index"))
        .expect("parse identity index")
}

struct GeneratedLegacyKeyMaterial {
    did_document: Value,
    key1_private_pem: String,
    key1_public_pem: String,
    e2ee_signing_private_pem: String,
    e2ee_agreement_private_pem: String,
}

fn generate_legacy_key_material(did: &str) -> GeneratedLegacyKeyMaterial {
    let bundle = create_did_wba_document(
        "example.test",
        DidDocumentOptions {
            path_segments: vec!["user".to_string(), "legacy-fixture".to_string()],
            domain: Some("example.test".to_string()),
            challenge: Some("legacy-fixture".to_string()),
            ..DidDocumentOptions::default()
        },
    )
    .expect("generate legacy identity key material");
    GeneratedLegacyKeyMaterial {
        did_document: json!({ "id": did }),
        key1_private_pem: bundle
            .private_key_pem("key-1")
            .expect("key-1 private")
            .to_string(),
        key1_public_pem: bundle
            .public_key_pem("key-1")
            .expect("key-1 public")
            .to_string(),
        e2ee_signing_private_pem: bundle
            .private_key_pem("key-2")
            .unwrap_or_default()
            .to_string(),
        e2ee_agreement_private_pem: bundle
            .private_key_pem("key-3")
            .unwrap_or_default()
            .to_string(),
    }
}

fn seed_current_legacy_db_message(
    resolved: &workspace_config::Resolved,
    msg_id: &str,
    owner_did: &str,
    credential_name: &str,
) {
    let legacy_db = legacy_db_path(resolved);
    std::fs::create_dir_all(legacy_db.parent().unwrap()).expect("create legacy db dir");
    let mut legacy_paths = resolved.paths.clone();
    legacy_paths.database_file = path_string(&legacy_db);
    {
        let db = open_local_state(&legacy_paths).expect("open legacy db");
        db.execute_batch(LEGACY_V6_TABLES_SQL)
            .expect("create legacy v6 schema");
        db.execute_batch(LEGACY_V11_EXTRA_TABLES_SQL)
            .expect("create legacy v11 schema");
        db.pragma_update(None, "user_version", 11)
            .expect("set legacy schema version");
        db.execute(
            r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did,
    content_type, content, stored_at, credential_name
) VALUES (?1, ?2, ?3, 0, ?4, ?2, 'text', ?5, ?6, ?7)
"#,
            rusqlite::params![
                msg_id,
                owner_did,
                format!("dm:{owner_did}:did:wba:example.test:user:e1_peer"),
                "did:wba:example.test:user:e1_peer",
                "legacy hello",
                "2026-01-01T00:00:00Z",
                credential_name,
            ],
        )
        .expect("insert legacy message");
    }
}

fn seed_empty_legacy_db_with_version(resolved: &workspace_config::Resolved, schema_version: i64) {
    let legacy_db = legacy_db_path(resolved);
    std::fs::create_dir_all(legacy_db.parent().unwrap()).expect("create legacy db dir");
    let db = rusqlite::Connection::open(legacy_db).expect("open legacy db");
    db.pragma_update(None, "user_version", schema_version)
        .expect("set legacy schema version");
}

fn seed_v5_legacy_db_message(
    resolved: &workspace_config::Resolved,
    msg_id: &str,
    credential_name: &str,
) {
    let legacy_db = legacy_db_path(resolved);
    std::fs::create_dir_all(legacy_db.parent().unwrap()).expect("create legacy db dir");
    let db = rusqlite::Connection::open(legacy_db).expect("open legacy db");
    db.execute_batch(
        r#"
CREATE TABLE messages (
    msg_id TEXT NOT NULL,
    owner_did TEXT NOT NULL DEFAULT '',
    thread_id TEXT NOT NULL DEFAULT '',
    sender_did TEXT,
    receiver_did TEXT,
    content TEXT,
    stored_at TEXT NOT NULL,
    credential_name TEXT NOT NULL DEFAULT ''
);
"#,
    )
    .expect("create v5 messages table");
    db.pragma_update(None, "user_version", 5)
        .expect("set legacy schema version");
    db.execute(
        r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, sender_did, receiver_did, content, stored_at, credential_name)
VALUES (?1, '', '', ?2, '', ?3, ?4, ?5)
"#,
        rusqlite::params![
            msg_id,
            "did:wba:example.test:user:e1_peer",
            "legacy v5 hello",
            "2026-01-01T00:00:00Z",
            credential_name,
        ],
    )
    .expect("insert v5 legacy message");
}

fn legacy_db_path(resolved: &workspace_config::Resolved) -> PathBuf {
    Path::new(&resolved.paths.legacy_data_dir)
        .join("database")
        .join("awiki.db")
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected config to contain {needle:?}, got:\n{haystack}"
    );
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
