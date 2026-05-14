use awiki_cli::{config, store, upgrade};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

    let refreshed = upgrade::refresh_resolved_config(&resolved).expect("refresh resolved");

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

    let refreshed = upgrade::refresh_resolved_config(&resolved).expect("refresh resolved");

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

    let refreshed = upgrade::refresh_resolved_config(&resolved).expect("refresh resolved");

    assert_eq!(refreshed.service_base_url, "https://api.changed.example");
    assert_eq!(refreshed.mail_service_url, "https://mail.current.example");
}

#[test]
fn refresh_resolved_config_keeps_go_required_and_missing_config_boundaries() {
    let workspace = TempDir::new("workspace-upgrade-refresh-missing").expect("temp workspace");
    let resolved = test_resolved(workspace.path());

    let required = upgrade::refresh_resolved_config_optional(None)
        .expect_err("missing resolved config should fail");
    assert_eq!(required.to_string(), "resolved config is required");

    let refreshed = upgrade::refresh_resolved_config(&resolved).expect("missing config is ok");
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

    upgrade::ensure_target_store_schema(&resolved.paths).expect("ensure target schema");

    let verify = store::open_read_only(&resolved.paths.database_file).expect("open verify db");
    assert_eq!(
        store::current_schema_version(&verify).expect("schema version"),
        store::SCHEMA_VERSION
    );
    assert_table_exists(&verify, "messages");
    assert_table_exists(&verify, "contact_handle_bindings");
    assert_table_exists(&verify, "e2ee_sessions");
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
        db.pragma_update(None, "user_version", store::SCHEMA_VERSION + 1)
            .expect("set newer version");
    }
    let err = upgrade::ensure_target_store_schema(&newer_resolved.paths)
        .expect_err("newer schema should fail");
    assert_eq!(
        err.to_string(),
        format!(
            "sqlite schema version {} is newer than supported {}",
            store::SCHEMA_VERSION + 1,
            store::SCHEMA_VERSION
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
    let err = upgrade::ensure_target_store_schema(&old_resolved.paths)
        .expect_err("old schema should fail");
    assert_eq!(
        err.to_string(),
        "sqlite schema version 5 is too old for in-place upgrade"
    );
}

#[test]
fn validate_sqlite_health_matches_go_pragmas() {
    let healthy = rusqlite::Connection::open_in_memory().expect("open healthy db");
    healthy
        .pragma_update(None, "foreign_keys", "ON")
        .expect("enable fk");
    upgrade::validate_sqlite_health(&healthy).expect("healthy sqlite");

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

    let err = upgrade::validate_sqlite_health(&fk).expect_err("foreign key violation should fail");
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
        let db = store::open(&resolved.paths).expect("open writable db");
        store::ensure_schema(&db).expect("ensure schema");
    }
    let context = upgrade::new_context(&resolved, "1.2.3");

    validate_first_migration(&context).expect("current config and healthy sqlite validate");
}

#[test]
fn workspace_v0_to_v1_validate_rejects_wrong_config_schema() {
    let workspace = TempDir::new("workspace-v0-v1-validate-config").expect("temp workspace");
    let resolved = test_resolved(workspace.path());
    std::fs::write(&resolved.paths.config_file, "schema_version: 0\n").expect("write config");
    let context = upgrade::new_context(&resolved, "1.2.3");

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
        db.pragma_update(None, "user_version", store::SCHEMA_VERSION - 1)
            .expect("set wrong schema");
    }
    let context = upgrade::new_context(&resolved, "1.2.3");

    let err = validate_first_migration(&context).expect_err("wrong sqlite schema should fail");

    assert_eq!(
        err.to_string(),
        format!(
            "sqlite schema version = {}, want {}",
            store::SCHEMA_VERSION - 1,
            store::SCHEMA_VERSION
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
        db.pragma_update(None, "user_version", store::SCHEMA_VERSION)
            .expect("set current schema");
    }
    let context = upgrade::new_context(&resolved, "1.2.3");

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
    let mut context = upgrade::new_context(&resolved, "1.2.3");
    let mut detection = upgrade::Detection::default();
    detection.has_workspace = false;
    detection.legacy_identity_exists = true;
    context.inspection = Some(upgrade::Inspection {
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

fn validate_first_migration(context: &upgrade::Context) -> Result<(), upgrade::MigrationError> {
    let upgrader = upgrade::new_default_upgrader();
    let plan = upgrader.plan(0, 1).expect("v0 to v1 plan");
    plan[0].validate(context)
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
