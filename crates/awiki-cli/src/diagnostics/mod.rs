use crate::build_info::BuildInfo;
use crate::host_runtime;
use crate::workspace_config::Resolved;
use crate::workspace_upgrade;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

pub(crate) mod legacy_identity;
pub(crate) mod legacy_sqlite;

use self::legacy_identity::{IdentityError, Manager};
use self::legacy_sqlite as store;

const ANP_MLS_BINARY_ENV: &str = "AWIKI_ANP_MLS_BINARY";
const DEFAULT_ANP_MLS_BINARY: &str = "anp-mls";

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub details: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct Counts {
    pub ok: usize,
    pub warn: usize,
    pub error: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    pub summary: String,
    pub counts: Counts,
}

pub fn run(resolved: &Resolved) -> Report {
    let checks = vec![
        build_check(),
        config_file_check(resolved),
        environment_check(resolved),
        anp_service_check(resolved),
        runtime_check(resolved),
        identity_store_check(resolved),
        sqlite_check(resolved),
        anp_mls_check(resolved),
        workspace_upgrade_check(resolved),
        legacy_paths_check(resolved),
    ];
    let counts = count_checks(&checks);
    let summary = if counts.error > 0 {
        "Doctor found blocking issues"
    } else if counts.warn > 0 {
        "Doctor found warnings"
    } else {
        "Doctor completed successfully"
    };
    Report {
        checks,
        summary: summary.to_string(),
        counts,
    }
}

fn count_checks(checks: &[Check]) -> Counts {
    let mut counts = Counts::default();
    for check in checks {
        match check.status.as_str() {
            "ok" => counts.ok += 1,
            "warn" => counts.warn += 1,
            "error" => counts.error += 1,
            _ => counts.info += 1,
        }
    }
    counts
}

fn build_check() -> Check {
    let info = BuildInfo::current();
    let mut status = "ok";
    let mut summary = "Pure-Go build target is aligned";
    if info.cgo_enabled.eq_ignore_ascii_case("1") || info.cgo_enabled.eq_ignore_ascii_case("true") {
        status = "warn";
        summary = "Build metadata indicates CGO was enabled";
    }
    check("build", status, summary, Some(json!(info)))
}

fn config_file_check(resolved: &Resolved) -> Check {
    let mut status = "warn";
    let mut summary = "No config file found yet";
    if resolved.config_exists {
        status = "ok";
        summary = "Config file loaded";
    }
    if resolved.config_exists
        && resolved.config_schema_version < crate::workspace_config::CONFIG_SCHEMA_VERSION
    {
        status = "warn";
        summary = "Config file exists but schema version is not current";
    }
    if !resolved.config_error.is_empty() {
        status = "error";
        summary = "Config file exists but failed to parse";
    }
    check(
        "config_file",
        status,
        summary,
        Some(json!({
            "path": resolved.paths.config_file,
            "exists": resolved.config_exists,
            "schema_version": resolved.config_schema_version,
            "error": resolved.config_error,
        })),
    )
}

fn environment_check(resolved: &Resolved) -> Check {
    let (status, summary) = if resolved.env_hits.is_empty() {
        ("info", "No workspace environment override detected")
    } else {
        ("ok", "Workspace environment override detected")
    };
    check(
        "environment",
        status,
        summary,
        Some(json!({ "hits": resolved.env_hits })),
    )
}

fn anp_service_check(resolved: &Resolved) -> Check {
    let mut status = "ok";
    let mut summary = "ANP service discovery fields are ready for DID generation";
    let mut details = object(json!({
        "anp_service_endpoint": resolved.anp_service_endpoint,
        "anp_service_did": resolved.anp_service_did,
    }));
    if let Err(err) = validate_anp_service_endpoint(&resolved.anp_service_endpoint) {
        status = "error";
        summary = "ANP service endpoint is invalid for public DID discovery";
        details.insert("endpoint_error".to_string(), json!(err));
    }
    if let Err(err) = validate_anp_service_did(&resolved.anp_service_did) {
        if status != "error" {
            status = "error";
            summary = "ANP service DID is invalid for public DID discovery";
        }
        details.insert("service_did_error".to_string(), json!(err));
    }
    Check {
        name: "anp_service".to_string(),
        status: status.to_string(),
        summary: summary.to_string(),
        details,
    }
}

fn runtime_check(resolved: &Resolved) -> Check {
    let runtime_resolved = host_runtime::resolve(resolved);
    let listener_status = host_runtime::current_listener_status(resolved);
    let listener_running = listener_status
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut status = "ok";
    let mut summary = "Runtime mode is http";
    if runtime_resolved.mode == "websocket" {
        summary = "Runtime mode is websocket";
        if !runtime_resolved.listener.enabled {
            status = "warn";
            summary = "Runtime mode is websocket but listener is disabled";
        } else if !listener_running {
            status = "warn";
            summary = "Runtime mode is websocket but listener service is not running";
        }
    }
    check(
        "runtime",
        status,
        summary,
        Some(json!({
            "mode": resolved.runtime_mode,
            "socket_path": resolved.runtime_socket_path,
            "listener_enabled": resolved.runtime_listener_enabled,
            "listener_auto_install": resolved.runtime_listener_auto_install,
            "listener_auto_start": resolved.runtime_listener_auto_start,
            "listener_status": listener_status,
        })),
    )
}

fn identity_store_check(resolved: &Resolved) -> Check {
    let manager = Manager::new(resolved.paths.clone());
    let index_path = Path::new(&resolved.paths.identity_dir).join("index.json");
    let identity_dir_exists = Path::new(&resolved.paths.identity_dir).exists();
    let index_exists = index_path.exists();
    let mut status = "warn";
    let mut summary = "Identity store has not been initialized";
    if identity_dir_exists || index_exists {
        status = "ok";
        summary = "Identity store path resolved";
    }

    let (index, index_error) = match manager.load_index() {
        Ok(index) => (index, String::new()),
        Err(err) => {
            status = "error";
            summary = "Identity index exists but failed to parse";
            (Default::default(), err.to_string())
        }
    };

    let current_result = manager.current();
    let (current, current_unexpected_error) = match current_result {
        Ok(current) => (Some(current), false),
        Err(IdentityError::NoDefaultIdentity(_)) => (None, false),
        Err(_) => (None, true),
    };
    let (identities, list_error) = match manager.list() {
        Ok(identities) => (identities, String::new()),
        Err(err) => (Vec::new(), err.to_string()),
    };
    let legacy_k1_dids = identities
        .iter()
        .filter(|item| is_k1_did(&item.did))
        .map(|item| item.did.clone())
        .collect::<Vec<_>>();

    if current_unexpected_error && !index.credentials.is_empty() {
        status = "error";
        summary = "Identity index is missing a valid default identity";
    } else if !legacy_k1_dids.is_empty() {
        status = "warn";
        summary = "Identity store still contains legacy k1 DID material";
    } else if current
        .as_ref()
        .is_some_and(|identity| !identity.user_state.ready_for_messaging)
    {
        status = "warn";
        summary = "Default identity is local-only and cannot be used for messaging yet";
    }

    check(
        "identity_store",
        status,
        summary,
        Some(json!({
            "identity_dir": resolved.paths.identity_dir,
            "dir_exists": identity_dir_exists,
            "index_path": index_path.to_string_lossy(),
            "index_exists": index_exists,
            "index_entries": index.credentials.len(),
            "default_identity": current,
            "user_state": current.as_ref().map(|identity| identity.user_state.clone()),
            "index_error": index_error,
            "list_error": list_error,
            "legacy_k1_dids": legacy_k1_dids,
        })),
    )
}

fn sqlite_check(resolved: &Resolved) -> Check {
    let database_exists = Path::new(&resolved.paths.database_file).exists();
    let mut status = "info";
    let mut summary = "SQLite target path resolved";
    if database_exists {
        status = "ok";
        summary = "SQLite database file already exists";
    }
    let mut schema_version = 0;
    let mut schema_error = String::new();
    let mut handle_bindings_exists = false;
    let mut handle_bindings_count = 0_i64;
    if database_exists {
        match store::open_read_only(&resolved.paths.database_file) {
            Ok(db) => {
                match store::current_schema_version(&db) {
                    Ok(version) => {
                        schema_version = version;
                        if version != store::SCHEMA_VERSION {
                            status = "warn";
                            summary = "SQLite database exists but schema version is not current";
                        }
                    }
                    Err(err) => {
                        status = "error";
                        summary =
                            "SQLite database is readable but schema version could not be inspected";
                        schema_error = err.to_string();
                    }
                }
                if let Ok(count) = db.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'contact_handle_bindings'",
                    [],
                    |row| row.get::<_, i64>(0),
                ) {
                    handle_bindings_exists = count > 0;
                    if handle_bindings_exists {
                        handle_bindings_count = db
                            .query_row("SELECT COUNT(*) FROM contact_handle_bindings", [], |row| {
                                row.get::<_, i64>(0)
                            })
                            .unwrap_or(0);
                    }
                }
            }
            Err(err) => {
                status = "error";
                summary = "SQLite database file exists but cannot be opened";
                schema_error = err.to_string();
            }
        }
    }
    check(
        "sqlite",
        status,
        summary,
        Some(json!({
            "database_file": resolved.paths.database_file,
            "exists": database_exists,
            "parent_dir": Path::new(&resolved.paths.database_file).parent().map(path_string).unwrap_or_default(),
            "schema_version": schema_version,
            "target_schema_version": store::SCHEMA_VERSION,
            "contact_handle_bindings_exists": handle_bindings_exists,
            "contact_handle_bindings_count": handle_bindings_count,
            "schema_error": schema_error,
        })),
    )
}

fn anp_mls_check(resolved: &Resolved) -> Check {
    let binary_result = resolve_anp_mls_binary();
    let data_dir = Path::new(&resolved.paths.workspace_home_dir).join("mls");
    let state = inspect_mls_state(resolved, &data_dir);
    let mut details = object(json!({
        "binary_available": binary_result.is_ok(),
        "env_override": ANP_MLS_BINARY_ENV,
        "plain_unaffected": true,
        "resolve_error": binary_result.as_ref().err().cloned().unwrap_or_default(),
        "remediation": anp_mls_remediation(binary_result.is_err(), false, false, &state),
        "data_dir_status": state.data_dir_status,
        "data_dir_exists": state.data_dir_exists,
        "data_dir_error": state.data_dir_error,
        "state_db_status": state.state_db_status,
        "state_db_error": state.state_db_error,
        "state_lock_status": state.state_lock_status,
        "state_lock_error": state.state_lock_error,
        "scoped_state_count": state.scoped_state_count,
        "scoped_state_db_count": state.scoped_state_db_count,
        "scoped_state_lock_count": state.scoped_state_lock_count,
        "scoped_state_warning_count": state.scoped_state_warning_count,
        "e2ee_group_count": state.e2ee_group_count,
    }));
    let Ok(binary) = binary_result else {
        return Check {
            name: "anp_mls".to_string(),
            status: "info".to_string(),
            summary:
                "anp-mls binary not found; plain messaging is unaffected, but group E2EE commands will fail"
                    .to_string(),
            details,
        };
    };

    let probe = probe_anp_mls_version(&binary);
    let compat_error = probe
        .version
        .as_ref()
        .and_then(anp_mls_compatibility_error)
        .unwrap_or_default();
    details.insert(
        "version".to_string(),
        sanitized_anp_mls_version(probe.version.as_ref()),
    );
    details.insert("probe_error".to_string(), json!(probe.error));
    details.insert("compatibility_error".to_string(), json!(compat_error));
    details.insert(
        "remediation".to_string(),
        json!(anp_mls_remediation(
            false,
            !probe.error.is_empty(),
            !compat_error.is_empty(),
            &state
        )),
    );

    let (status, summary) = if !probe.error.is_empty() {
        (
            "warn",
            "anp-mls binary is present but the version compatibility probe failed",
        )
    } else if !compat_error.is_empty() {
        (
            "warn",
            "anp-mls binary version is not compatible with this awiki-cli build",
        )
    } else if state.has_warning() {
        (
            "warn",
            "anp-mls binary is compatible but MLS state needs attention",
        )
    } else {
        (
            "ok",
            "anp-mls binary and compatibility probe are ready for group E2EE operations",
        )
    };
    Check {
        name: "anp_mls".to_string(),
        status: status.to_string(),
        summary: summary.to_string(),
        details,
    }
}

fn workspace_upgrade_check(resolved: &Resolved) -> Check {
    let inspection = match workspace_upgrade::inspect(resolved, crate::build_info::VERSION) {
        Ok(inspection) => inspection,
        Err(err) => {
            let paths = workspace_upgrade::resolve_paths(resolved);
            return check(
                "workspace_upgrade",
                "error",
                "Workspace upgrade state inspection failed",
                Some(json!({
                    "meta_path": paths.meta_path,
                    "journal_path": paths.journal_path,
                    "error": err.to_string(),
                })),
            );
        }
    };
    let mut status = "ok";
    let mut summary = "Workspace upgrade metadata is up to date";
    if inspection.journal.is_some() {
        status = "warn";
        summary = "Workspace upgrade journal indicates an interrupted upgrade";
    } else if inspection
        .meta
        .as_ref()
        .is_some_and(|meta| !meta.warnings.is_empty())
    {
        status = "warn";
        summary = "Workspace upgrade completed with migration warnings";
    } else if inspection.detection.current_version < inspection.detection.latest_version {
        status = "warn";
        summary = "Workspace data still needs to be upgraded";
    } else if inspection.detection.current_version_source == "legacy_detector" {
        status = "warn";
        summary = "Workspace upgrade metadata has not been initialized yet";
    }
    check(
        "workspace_upgrade",
        status,
        summary,
        Some(json!({
            "meta": inspection.meta,
            "journal": inspection.journal,
            "detection": inspection.detection,
        })),
    )
}

fn legacy_paths_check(resolved: &Resolved) -> Check {
    let manager = Manager::new(resolved.paths.clone());
    let scan_result = manager.scan_legacy();
    let credentials_exists = Path::new(&resolved.paths.legacy_credentials_dir).exists();
    let data_exists = Path::new(&resolved.paths.legacy_data_dir).exists();
    let legacy_database_result = store::scan_legacy_database(&resolved.paths);
    let legacy_database = legacy_database_result.as_ref().ok().cloned();
    let legacy_database_exists = legacy_database
        .as_ref()
        .map(|scan| scan.exists)
        .unwrap_or(false);
    let mut status = "info";
    let mut summary = "No legacy v1 paths detected";
    match &scan_result {
        Err(_) => {
            status = "error";
            summary = "Legacy credential scan failed";
        }
        Ok(scan) if scan.has_legacy => {
            status = "warn";
            summary = "Legacy awiki-agent-id-message credential layout detected";
        }
        Ok(_) if legacy_database_exists || credentials_exists || data_exists => {
            status = "warn";
            summary = "Legacy awiki-agent-id-message paths detected";
        }
        Ok(_) => {}
    }
    check(
        "legacy_paths",
        status,
        summary,
        Some(json!({
            "legacy_credentials_dir": resolved.paths.legacy_credentials_dir,
            "credentials_exists": credentials_exists,
            "legacy_data_dir": resolved.paths.legacy_data_dir,
            "data_exists": data_exists,
            "legacy_scan": scan_result.as_ref().ok(),
            "scan_error": scan_result.as_ref().err().map(ToString::to_string).unwrap_or_default(),
            "legacy_database": legacy_database,
            "legacy_database_error": legacy_database_result.as_ref().err().map(ToString::to_string).unwrap_or_default(),
        })),
    )
}

#[derive(Debug, Clone, Default)]
struct MlsStateInspection {
    data_dir_exists: bool,
    data_dir_status: String,
    data_dir_error: String,
    state_db_path: PathBuf,
    state_db_status: String,
    state_db_error: String,
    state_lock_path: PathBuf,
    state_lock_status: String,
    state_lock_error: String,
    scoped_states: Vec<MlsScopedStateInspection>,
    scoped_state_count: usize,
    scoped_state_db_count: usize,
    scoped_state_lock_count: usize,
    scoped_state_warning_count: usize,
    e2ee_group_count: i64,
}

impl MlsStateInspection {
    fn has_warning(&self) -> bool {
        self.data_dir_status.starts_with("warn")
            || self.state_db_status.starts_with("warn")
            || self.state_lock_status.starts_with("warn")
            || self.scoped_state_warning_count > 0
    }
}

#[derive(Debug, Clone, Default)]
struct MlsScopedStateInspection {
    state_db_status: String,
    state_lock_status: String,
}

fn inspect_mls_state(resolved: &Resolved, data_dir: &Path) -> MlsStateInspection {
    let mut state = MlsStateInspection {
        data_dir_status: "missing".to_string(),
        state_db_path: data_dir.join("state.db"),
        state_db_status: "missing".to_string(),
        state_lock_path: data_dir.join("state.lock"),
        state_lock_status: "missing".to_string(),
        e2ee_group_count: cached_group_e2ee_count(resolved),
        ..MlsStateInspection::default()
    };
    match fs::metadata(data_dir) {
        Ok(metadata) if !metadata.is_dir() => {
            state.data_dir_status = "warn_not_directory".to_string();
            return state;
        }
        Ok(_) => {
            state.data_dir_exists = true;
            state.data_dir_status = "ok".to_string();
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if state.e2ee_group_count > 0 {
                state.data_dir_status = "warn_missing_with_cached_groups".to_string();
            }
            return state;
        }
        Err(err) => {
            state.data_dir_status = "warn_stat_failed".to_string();
            state.data_dir_error = io_error_kind(&err);
            return state;
        }
    }
    if let Err(err) = fs::read_dir(data_dir) {
        state.data_dir_status = "warn_not_readable".to_string();
        state.data_dir_error = io_error_kind(&err);
    } else if let Err(err) = can_write_dir(data_dir) {
        state.data_dir_status = "warn_not_writable".to_string();
        state.data_dir_error = io_error_kind(&err);
    }

    state.scoped_states = inspect_scoped_mls_states(data_dir);
    state.scoped_state_count = state.scoped_states.len();
    for scoped in &state.scoped_states {
        if scoped.state_db_status == "ok" {
            state.scoped_state_db_count += 1;
        }
        if scoped.state_lock_status != "missing" {
            state.scoped_state_lock_count += 1;
        }
        if scoped.state_db_status.starts_with("warn")
            || scoped.state_lock_status.starts_with("warn")
        {
            state.scoped_state_warning_count += 1;
        }
    }

    let (db_status, db_error) = inspect_mls_state_db(&state.state_db_path);
    state.state_db_status = db_status;
    state.state_db_error = db_error;
    if state.state_db_status == "missing"
        && state.e2ee_group_count > 0
        && state.scoped_state_db_count == 0
    {
        state.state_db_status = "warn_missing_with_cached_groups".to_string();
    }
    let (lock_status, lock_error) = inspect_mls_lock(&state.state_lock_path);
    state.state_lock_status = lock_status;
    state.state_lock_error = lock_error;
    state
}

fn inspect_scoped_mls_states(data_dir: &Path) -> Vec<MlsScopedStateInspection> {
    let agents_dir = data_dir.join("agents");
    let Ok(agent_entries) = fs::read_dir(agents_dir) else {
        return Vec::new();
    };
    let mut states = Vec::new();
    for agent_entry in agent_entries.flatten() {
        if !agent_entry.path().is_dir() {
            continue;
        }
        let agent_dir = agent_entry.path();
        let Ok(device_entries) = fs::read_dir(&agent_dir) else {
            states.push(MlsScopedStateInspection {
                state_db_status: "warn_read_devices_failed".to_string(),
                state_lock_status: "missing".to_string(),
            });
            continue;
        };
        for device_entry in device_entries.flatten() {
            if !device_entry.path().is_dir() {
                continue;
            }
            let dir = device_entry.path();
            let db_path = dir.join("state.db");
            let lock_path = dir.join("state.lock");
            let (state_db_status, _) = inspect_mls_state_db(&db_path);
            let (state_lock_status, _) = inspect_mls_lock(&lock_path);
            states.push(MlsScopedStateInspection {
                state_db_status,
                state_lock_status,
            });
        }
    }
    states
}

fn inspect_mls_state_db(path: &Path) -> (String, String) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => ("warn_not_file".to_string(), String::new()),
        Ok(_) => match fs::File::open(path) {
            Ok(_) => ("ok".to_string(), String::new()),
            Err(err) => ("warn_not_readable".to_string(), io_error_kind(&err)),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ("missing".to_string(), String::new())
        }
        Err(err) => ("warn_stat_failed".to_string(), io_error_kind(&err)),
    }
}

fn inspect_mls_lock(path: &Path) -> (String, String) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => ("warn_not_file".to_string(), String::new()),
        Ok(metadata) => {
            if let Err(err) = fs::File::open(path) {
                return ("warn_not_readable".to_string(), io_error_kind(&err));
            }
            if metadata
                .modified()
                .ok()
                .and_then(|time| SystemTime::now().duration_since(time).ok())
                .is_some_and(|age| age > Duration::from_secs(15 * 60))
            {
                return ("warn_stale_candidate".to_string(), String::new());
            }
            ("present_active_or_recent".to_string(), String::new())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ("missing".to_string(), String::new())
        }
        Err(err) => ("warn_stat_failed".to_string(), io_error_kind(&err)),
    }
}

fn can_write_dir(path: &Path) -> std::io::Result<()> {
    let probe = path.join(format!(
        ".awiki-cli-doctor-{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::File::create(&probe)?.write_all(b"")?;
    fs::remove_file(probe)
}

fn cached_group_e2ee_count(resolved: &Resolved) -> i64 {
    if resolved.paths.database_file.trim().is_empty()
        || !Path::new(&resolved.paths.database_file).exists()
    {
        return 0;
    }
    let Ok(db) = store::open_read_only(&resolved.paths.database_file) else {
        return 0;
    };
    let table_count = db
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'groups'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    if table_count == 0 {
        return 0;
    }
    db.query_row(
        "SELECT COUNT(*) FROM groups WHERE metadata LIKE ?1",
        ["%group-e2ee%"],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
}

#[derive(Debug, Clone, Default)]
struct MlsProbe {
    version: Option<Value>,
    error: String,
}

fn resolve_anp_mls_binary() -> Result<String, String> {
    let mut candidates = Vec::new();
    if let Ok(raw) = std::env::var(ANP_MLS_BINARY_ENV) {
        if !raw.trim().is_empty() {
            candidates.push(raw.trim().to_string());
        }
    }
    candidates.push(DEFAULT_ANP_MLS_BINARY.to_string());
    candidates.dedup();
    for candidate in candidates {
        let path = Path::new(&candidate);
        if path.is_absolute() || candidate.contains(std::path::MAIN_SEPARATOR) {
            if is_executable_file(path) {
                return Ok(candidate);
            }
            continue;
        }
        if let Some(found) = find_on_path(&candidate) {
            return Ok(found);
        }
    }
    Err(format!(
        "unable to locate anp-mls binary (checked {ANP_MLS_BINARY_ENV}, injected path, then PATH). Set {ANP_MLS_BINARY_ENV} to an absolute anp-mls path, build/install anp-mls, or run `awiki-cli doctor` for diagnostics"
    ))
}

fn probe_anp_mls_version(binary: &str) -> MlsProbe {
    let request =
        br#"{"api_version":"anp-mls/v1","request_id":"doctor-system-version","params":{}}"#;
    let mut child = match Command::new(binary)
        .args(["system", "version", "--json-in", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return MlsProbe {
                error: format!("anp-mls version probe failed to start: {}", err.kind()),
                version: None,
            }
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(request);
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            return MlsProbe {
                error: format!("anp-mls version probe did not complete: {}", err.kind()),
                version: None,
            }
        }
    };
    if !output.status.success() && output.stdout.is_empty() {
        return MlsProbe {
            error: "anp-mls version probe returned no JSON result".to_string(),
            version: None,
        };
    }
    let response: Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(err) => {
            return MlsProbe {
                error: format!("decode anp-mls version response failed: {err}"),
                version: None,
            }
        }
    };
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return MlsProbe {
            error: "anp-mls version probe returned ok=false".to_string(),
            version: None,
        };
    }
    let mut result = response.get("result").cloned().unwrap_or_else(|| json!({}));
    if result.get("api_version").and_then(Value::as_str).is_none() {
        if let Some(api_version) = response.get("api_version").cloned() {
            result["api_version"] = api_version;
        }
    }
    MlsProbe {
        version: Some(result),
        error: String::new(),
    }
}

fn anp_mls_compatibility_error(info: &Value) -> Option<String> {
    let api_version = info
        .get("api_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    if api_version != "anp-mls/v1" {
        return Some(format!(
            "api_version {api_version:?} is not supported; want anp-mls/v1"
        ));
    }
    let binary_name = info
        .get("binary_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let sanitized_binary_name = sanitized_binary_name(binary_name);
    if sanitized_binary_name != "anp-mls" {
        return Some(format!(
            "binary_name {sanitized_binary_name:?} is not supported; want anp-mls"
        ));
    }
    let supported = info
        .get("supported_commands")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .any(|item| item.trim().eq_ignore_ascii_case("system version"))
        })
        .unwrap_or(false);
    if !supported {
        return Some("supported_commands does not include system version".to_string());
    }
    None
}

fn sanitized_anp_mls_version(info: Option<&Value>) -> Value {
    let Some(info) = info else {
        return Value::Null;
    };
    let supported_commands = info
        .get("supported_commands")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    json!({
        "api_version": info.get("api_version").and_then(Value::as_str).unwrap_or_default(),
        "binary_name": sanitized_binary_name(info.get("binary_name").and_then(Value::as_str).unwrap_or_default()),
        "binary_version": info.get("binary_version").and_then(Value::as_str).unwrap_or_default(),
        "supports_system_version": supported_commands
            .iter()
            .filter_map(Value::as_str)
            .any(|item| item.trim().eq_ignore_ascii_case("system version")),
        "supported_command_count": supported_commands.len(),
    })
}

fn io_error_kind(err: &std::io::Error) -> String {
    format!("{:?}", err.kind())
}

fn sanitized_binary_name(value: &str) -> String {
    Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(value)
        .to_string()
}

fn anp_mls_remediation(
    resolve_failed: bool,
    probe_failed: bool,
    compat_failed: bool,
    state: &MlsStateInspection,
) -> String {
    if resolve_failed {
        return "Build anp-mls from ../anp/anp/rust, put it next to release artifacts or on PATH, or set AWIKI_ANP_MLS_BINARY to the absolute binary path. Plain messaging does not require anp-mls.".to_string();
    }
    if probe_failed {
        return "Install a current anp-mls build that supports `anp-mls system version --json-in -`; rebuild from ../anp/anp/rust if this probe fails.".to_string();
    }
    if compat_failed {
        return "Replace anp-mls with a build that reports api_version anp-mls/v1, binary_name anp-mls, and supported command `system version`.".to_string();
    }
    if state.data_dir_status == "warn_not_writable" || state.data_dir_status == "warn_not_readable"
    {
        return "Fix permissions on the MLS data directory or move the workspace with AWIKI_CLI_WORKSPACE_HOME_DIR.".to_string();
    }
    if state.state_db_status == "warn_missing_with_cached_groups" {
        return "The business database has cached group-e2ee groups but no root or agent/device-scoped MLS state.db was found; restore the MLS data directory from backup before sending encrypted group messages.".to_string();
    }
    if state.state_lock_status.starts_with("warn") || state.scoped_state_warning_count > 0 {
        return "If no anp-mls process is running, inspect root and agent/device-scoped state.lock files, then remove stale locks only after backing up the MLS data directory.".to_string();
    }
    "No action required.".to_string()
}

fn validate_anp_service_endpoint(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("invalid input: anp_service_endpoint is required".to_string());
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err("invalid input: anp_service_endpoint is invalid".to_string());
    };
    if scheme != "http" && scheme != "https" {
        return Err("invalid input: anp_service_endpoint must use http or https".to_string());
    }
    let host_port = rest.split('/').next().unwrap_or_default();
    let host = host_port
        .strip_prefix('[')
        .and_then(|body| body.split(']').next())
        .unwrap_or_else(|| host_port.split(':').next().unwrap_or_default())
        .trim()
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err("invalid input: anp_service_endpoint must include a hostname".to_string());
    }
    if host == "localhost" {
        return Err("invalid input: anp_service_endpoint must not use localhost".to_string());
    }
    if host
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
    {
        return Err(
            "invalid input: anp_service_endpoint must not use a loopback address".to_string(),
        );
    }
    Ok(())
}

fn validate_anp_service_did(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("invalid input: anp_service_did is required".to_string());
    }
    let Some(remainder) = trimmed.strip_prefix("did:wba:") else {
        return Err("invalid input: anp_service_did must use did:wba".to_string());
    };
    if trimmed.contains('#') {
        return Err("invalid input: anp_service_did must not include a fragment".to_string());
    }
    if remainder.is_empty() {
        return Err("invalid input: anp_service_did must include a domain".to_string());
    }
    if remainder.contains([':', '/', '?']) {
        return Err("invalid input: anp_service_did must be a bare-domain did:wba DID".to_string());
    }
    Ok(())
}

fn is_k1_did(did: &str) -> bool {
    did.rsplit(':')
        .next()
        .unwrap_or(did)
        .trim()
        .starts_with("k1_")
}

fn check(name: &str, status: &str, summary: &str, details: Option<Value>) -> Check {
    Check {
        name: name.to_string(),
        status: status.to_string(),
        summary: summary.to_string(),
        details: details.map(object).unwrap_or_default(),
    }
}

fn object(value: Value) -> serde_json::Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_on_path(binary: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if is_executable_file(&candidate) {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}
