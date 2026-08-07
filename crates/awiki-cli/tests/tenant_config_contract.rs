use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn empty_workspace_auto_creates_default_tenant_under_product_home() {
    let workspace = TempDir::new("tenant-default").expect("workspace");

    let output = awiki_cmd(&["config", "show"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    let tenant_dir = workspace.path().join("tenants").join("default");
    assert_eq!(envelope["data"]["tenant"]["active"], "default");
    assert_eq!(
        envelope["data"]["tenant"]["profile"]["display_name"],
        "AWiki"
    );
    assert_eq!(
        envelope["data"]["tenant"]["profile"]["backend_base_url"],
        "https://awiki.ai"
    );
    assert_eq!(
        envelope["data"]["tenant"]["profile"]["did_host"],
        "awiki.ai"
    );
    assert_eq!(
        envelope["data"]["paths"]["workspace_home_dir"],
        tenant_dir.to_string_lossy().as_ref()
    );
    assert_eq!(
        envelope["data"]["paths"]["config_file"],
        tenant_dir.join("config.yaml").to_string_lossy().as_ref()
    );
    assert!(workspace.path().join("global.json").is_file());
    assert!(workspace
        .path()
        .join("tenants")
        .join("registry.json")
        .is_file());
    assert!(tenant_dir.join("config.yaml").is_file());
    assert!(!workspace.path().join("config.yaml").exists());
}

#[test]
fn tenant_create_use_and_global_tenant_override_switch_whole_workspace() {
    let workspace = TempDir::new("tenant-switch").expect("workspace");

    let create = awiki_cmd(
        &[
            "tenant",
            "create",
            "acme",
            "--backend-base-url",
            "https://api.acme.test/",
            "--did-host",
            "Acme.Test.",
            "--display-name",
            "Acme Team",
        ],
        workspace.path(),
    );
    assert_success(&create);
    let envelope = success_json(&create);
    assert_eq!(envelope["command"], "awiki-cli tenant create");
    assert_eq!(envelope["summary"], "Tenant created");
    assert_eq!(envelope["data"]["tenant"]["active"], "acme");
    assert_eq!(
        envelope["data"]["tenant"]["profile"]["backend_base_url"],
        "https://api.acme.test"
    );
    assert_eq!(
        envelope["data"]["tenant"]["profile"]["did_host"],
        "acme.test"
    );

    let list = awiki_cmd(&["tenant", "list"], workspace.path());
    assert_success(&list);
    let envelope = success_json(&list);
    assert_eq!(envelope["data"]["active"], "default");
    assert_eq!(
        envelope["data"]["tenants"]
            .as_array()
            .expect("tenants")
            .iter()
            .map(|tenant| tenant["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["acme", "default"]
    );

    let switch = awiki_cmd(&["tenant", "use", "acme"], workspace.path());
    assert_success(&switch);
    let envelope = success_json(&switch);
    assert_eq!(envelope["summary"], "Tenant switched");
    assert_eq!(envelope["data"]["tenant"]["active"], "acme");

    let show = awiki_cmd(&["config", "show"], workspace.path());
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(envelope["data"]["tenant"]["active"], "acme");
    assert_eq!(
        envelope["data"]["service_base_url"],
        "https://api.acme.test"
    );
    assert_eq!(envelope["data"]["did_domain"], "acme.test");
    assert_eq!(
        envelope["data"]["paths"]["workspace_home_dir"],
        workspace
            .path()
            .join("tenants")
            .join("acme")
            .to_string_lossy()
            .as_ref()
    );

    let override_default = awiki_cmd(&["--tenant", "default", "config", "show"], workspace.path());
    assert_success(&override_default);
    let envelope = success_json(&override_default);
    assert_eq!(envelope["data"]["tenant"]["active"], "default");
    assert_eq!(envelope["data"]["service_base_url"], "https://awiki.ai");

    let current_override = awiki_cmd(
        &["--tenant", "default", "tenant", "current"],
        workspace.path(),
    );
    assert_success(&current_override);
    let envelope = success_json(&current_override);
    assert_eq!(envelope["data"]["tenant"]["active"], "default");
    assert_eq!(envelope["data"]["tenant"]["active_source"], "flag");

    let list_override = awiki_cmd(&["--tenant", "default", "tenant", "list"], workspace.path());
    assert_success(&list_override);
    let envelope = success_json(&list_override);
    assert_eq!(envelope["data"]["active"], "default");
    assert_eq!(
        read_json(&workspace.path().join("global.json"))["active_tenant"],
        "acme"
    );
}

#[test]
fn tenant_setup_is_idempotent_switches_and_rejects_endpoint_drift() {
    let workspace = TempDir::new("tenant-setup").expect("workspace");
    let args = [
        "tenant",
        "setup",
        "acme",
        "--backend-base-url",
        "https://api.acme.test/",
        "--did-host",
        "Acme.Test.",
        "--display-name",
        "Acme Team",
    ];

    let first = awiki_cmd(&args, workspace.path());
    assert_success(&first);
    let first = success_json(&first);
    assert_eq!(first["data"]["result"]["action"], "created");
    assert_eq!(first["data"]["result"]["tenant"]["active"], "acme");
    assert_eq!(first["data"]["next_command"], "awiki-cli init");

    assert_success(&awiki_cmd(&["tenant", "use", "default"], workspace.path()));
    let repeated = awiki_cmd(&args, workspace.path());
    assert_success(&repeated);
    let repeated = success_json(&repeated);
    assert_eq!(repeated["data"]["result"]["action"], "reused");
    assert_eq!(
        read_json(&workspace.path().join("global.json"))["active_tenant"],
        "acme"
    );

    let conflict = awiki_cmd(
        &[
            "tenant",
            "setup",
            "acme",
            "--backend-base-url",
            "https://other.acme.test",
            "--did-host",
            "acme.test",
        ],
        workspace.path(),
    );
    assert_code(&conflict, 1);
    let conflict = error_json(&conflict);
    assert_eq!(conflict["error"]["code"], "conflict");
    assert_value_contains(&conflict["error"]["message"], "different backend_base_url");
}

#[test]
fn tenant_setup_reuses_release_default_when_requested_name_has_the_same_endpoints() {
    let workspace = TempDir::new("tenant-setup-release-default").expect("workspace");
    let release_tenant_env = [
        (
            "AWIKI_CLI_DEFAULT_BACKEND_BASE_URL",
            "https://agent-connect.cn",
        ),
        ("AWIKI_CLI_DEFAULT_DID_HOST", "agent-connect.cn"),
    ];

    let setup = awiki_cmd_extra(
        &[
            "tenant",
            "setup",
            "agent-connect-cn",
            "--backend-base-url",
            "https://agent-connect.cn",
            "--did-host",
            "agent-connect.cn",
            "--display-name",
            "agent-connect.cn",
        ],
        workspace.path(),
        &release_tenant_env,
    );

    assert_success(&setup);
    let setup = success_json(&setup);
    assert_eq!(setup["data"]["result"]["action"], "reused");
    assert_eq!(
        setup["data"]["result"]["tenant"]["profile"]["name"],
        "default"
    );
    assert_eq!(
        read_json(&workspace.path().join("global.json"))["active_tenant"],
        "default"
    );
    assert!(!workspace
        .path()
        .join("tenants")
        .join("agent-connect-cn")
        .exists());
}

#[test]
fn tenant_setup_dry_run_does_not_create_target_tenant() {
    let workspace = TempDir::new("tenant-setup-dry-run").expect("workspace");
    let output = awiki_cmd(
        &[
            "--dry-run",
            "tenant",
            "setup",
            "acme",
            "--backend-base-url",
            "https://api.acme.test",
            "--did-host",
            "acme.test",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let output = success_json(&output);
    assert_eq!(output["data"]["plan"]["action"], "tenant_setup");
    assert_eq!(output["data"]["plan"]["result"]["action"], "created");
    assert!(!workspace.path().join("tenants").join("acme").exists());
}

#[test]
fn empty_workspace_can_take_atomic_default_tenant_endpoints_from_release_wrapper() {
    let workspace = TempDir::new("tenant-default-env").expect("workspace");
    let output = awiki_cmd_extra(
        &["config", "show"],
        workspace.path(),
        &[
            ("AWIKI_CLI_DEFAULT_BACKEND_BASE_URL", "https://anpclaw.com/"),
            ("AWIKI_CLI_DEFAULT_DID_HOST", "AnpClaw.Com."),
        ],
    );
    assert_success(&output);
    let output = success_json(&output);
    assert_eq!(output["data"]["service_base_url"], "https://anpclaw.com");
    assert_eq!(output["data"]["did_domain"], "anpclaw.com");

    let second = awiki_cmd_extra(
        &["config", "show"],
        workspace.path(),
        &[
            ("AWIKI_CLI_DEFAULT_BACKEND_BASE_URL", "https://other.test"),
            ("AWIKI_CLI_DEFAULT_DID_HOST", "other.test"),
        ],
    );
    assert_success(&second);
    let second = success_json(&second);
    assert_eq!(second["data"]["service_base_url"], "https://anpclaw.com");
    assert_eq!(second["data"]["did_domain"], "anpclaw.com");
}

#[test]
fn tenant_use_accepts_only_existing_tenant_names_not_ad_hoc_fields() {
    let workspace = TempDir::new("tenant-use-boundary").expect("workspace");

    let with_fields = awiki_cmd(
        &[
            "tenant",
            "use",
            "acme",
            "--backend-base-url",
            "https://api.acme.test",
            "--did-host",
            "acme.test",
        ],
        workspace.path(),
    );
    assert_code(&with_fields, 2);
    let envelope = error_json(&with_fields);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(
        envelope["error"]["message"],
        "unknown flag: --backend-base-url"
    );

    let missing = awiki_cmd(&["tenant", "use", "missing"], workspace.path());
    assert_code(&missing, 5);
    let envelope = error_json(&missing);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_value_contains(
        &envelope["error"]["message"],
        "tenant \"missing\" does not exist",
    );
    assert_value_contains(&envelope["error"]["hint"], "tenant list");
}

#[test]
fn tenant_reconfigure_only_allows_empty_tenants_and_preserves_local_settings() {
    let workspace = TempDir::new("tenant-reconfigure").expect("workspace");

    assert_success(&awiki_cmd(
        &[
            "tenant",
            "create",
            "empty",
            "--backend-base-url",
            "https://old.example.test",
            "--did-host",
            "old.example.test",
        ],
        workspace.path(),
    ));
    assert_success(&awiki_cmd(&["tenant", "use", "empty"], workspace.path()));
    let config_path = workspace
        .path()
        .join("tenants")
        .join("empty")
        .join("config.yaml");
    let text = fs::read_to_string(&config_path).expect("config");
    fs::write(
        &config_path,
        text.replace("  sink: log\n", "  sink: file\n"),
    )
    .expect("write local setting");

    let reconfigure = awiki_cmd(
        &[
            "tenant",
            "reconfigure",
            "empty",
            "--backend-base-url",
            "https://new.example.test/",
            "--did-host",
            "New.Example.Test.",
        ],
        workspace.path(),
    );
    assert_success(&reconfigure);
    let envelope = success_json(&reconfigure);
    assert_eq!(
        envelope["data"]["tenant"]["profile"]["backend_base_url"],
        "https://new.example.test"
    );
    assert_eq!(
        envelope["data"]["tenant"]["profile"]["did_host"],
        "new.example.test"
    );
    let config_text = fs::read_to_string(&config_path).expect("config");
    assert!(!config_text.contains("service_base_url:"));
    assert!(!config_text.contains("did_domain:"));
    assert_contains(&config_text, "    sink: file\n");

    let show = awiki_cmd(&["--tenant", "empty", "config", "show"], workspace.path());
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(
        envelope["data"]["service_base_url"],
        "https://new.example.test"
    );
    assert_eq!(envelope["data"]["did_domain"], "new.example.test");

    fs::create_dir_all(config_path.parent().unwrap().join("identities")).expect("identities dir");
    fs::write(
        config_path
            .parent()
            .unwrap()
            .join("identities")
            .join("default"),
        "alice\n",
    )
    .expect("identity data");
    let blocked = awiki_cmd(
        &[
            "tenant",
            "reconfigure",
            "empty",
            "--backend-base-url",
            "https://blocked.example.test",
            "--did-host",
            "blocked.example.test",
        ],
        workspace.path(),
    );
    assert_code(&blocked, 1);
    let envelope = error_json(&blocked);
    assert_eq!(envelope["error"]["code"], "conflict");
    assert_value_contains(&envelope["error"]["message"], "already has local data");
    assert_value_contains(&envelope["error"]["message"], "create a new tenant");
}

#[test]
fn tenant_user_errors_are_typed_and_actionable() {
    let workspace = TempDir::new("tenant-user-errors").expect("workspace");

    let invalid_name = awiki_cmd(
        &[
            "tenant",
            "create",
            "acme_team",
            "--backend-base-url",
            "https://api.example.test",
            "--did-host",
            "example.test",
        ],
        workspace.path(),
    );
    assert_code(&invalid_name, 2);
    let envelope = error_json(&invalid_name);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_value_contains(
        &envelope["error"]["message"],
        "tenant name may only contain ASCII letters",
    );
    assert_value_contains(&envelope["error"]["hint"], "--display-name");

    let invalid_did_host = awiki_cmd(
        &[
            "tenant",
            "create",
            "bad-did",
            "--backend-base-url",
            "https://api.example.test",
            "--did-host",
            "https://tenant.example",
        ],
        workspace.path(),
    );
    assert_code(&invalid_did_host, 2);
    let envelope = error_json(&invalid_did_host);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_value_contains(
        &envelope["error"]["message"],
        "did_host must be a bare domain",
    );
    assert_value_contains(&envelope["error"]["hint"], "bare DID host");

    let invalid_backend = awiki_cmd(
        &[
            "tenant",
            "create",
            "bad-backend",
            "--backend-base-url",
            "not-a-url",
            "--did-host",
            "example.test",
        ],
        workspace.path(),
    );
    assert_code(&invalid_backend, 2);
    let envelope = error_json(&invalid_backend);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_value_contains(&envelope["error"]["message"], "backend_base_url is invalid");
    assert_value_contains(&envelope["error"]["hint"], "https://awiki.ai");

    assert_success(&awiki_cmd(
        &[
            "tenant",
            "create",
            "acme",
            "--backend-base-url",
            "https://api.acme.test",
            "--did-host",
            "acme.test",
        ],
        workspace.path(),
    ));

    let duplicate_name = awiki_cmd(
        &[
            "tenant",
            "create",
            "Acme",
            "--backend-base-url",
            "https://api2.acme.test",
            "--did-host",
            "api2.acme.test",
        ],
        workspace.path(),
    );
    assert_code(&duplicate_name, 1);
    let envelope = error_json(&duplicate_name);
    assert_eq!(envelope["error"]["code"], "conflict");
    assert_value_contains(
        &envelope["error"]["message"],
        "tenant \"acme\" already exists",
    );

    let invalid_override = awiki_cmd(
        &["--tenant", "acme_team", "config", "show"],
        workspace.path(),
    );
    assert_code(&invalid_override, 2);
    let envelope = error_json(&invalid_override);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_value_contains(
        &envelope["error"]["message"],
        "tenant name may only contain ASCII",
    );
}

#[test]
fn tenant_create_rejects_duplicate_backend_and_did_combinations() {
    let workspace = TempDir::new("tenant-duplicate").expect("workspace");
    assert_success(&awiki_cmd(
        &[
            "tenant",
            "create",
            "one",
            "--backend-base-url",
            "https://api.example.test",
            "--did-host",
            "example.test",
        ],
        workspace.path(),
    ));

    let duplicate = awiki_cmd(
        &[
            "tenant",
            "create",
            "two",
            "--backend-base-url",
            "https://api.example.test/",
            "--did-host",
            "Example.Test.",
        ],
        workspace.path(),
    );
    assert_code(&duplicate, 1);
    let envelope = error_json(&duplicate);
    assert_eq!(envelope["error"]["code"], "conflict");
    assert_value_contains(
        &envelope["error"]["message"],
        "same backend_base_url and did_host",
    );
}

#[test]
fn legacy_single_workspace_state_is_archived_and_not_used() {
    let workspace = TempDir::new("tenant-legacy-archive").expect("workspace");
    fs::write(
        workspace.path().join("config.yaml"),
        "services:\n  service_base_url: https://legacy.example.test\n  did_domain: legacy.example.test\n",
    )
    .expect("legacy config");
    fs::create_dir_all(workspace.path().join("data")).expect("data");
    fs::write(workspace.path().join("data").join("awiki-cli.db"), "legacy").expect("legacy db");

    let output = awiki_cmd(&["config", "show"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["service_base_url"], "https://awiki.ai");
    assert_eq!(envelope["data"]["did_domain"], "awiki.ai");
    assert!(!workspace.path().join("config.yaml").exists());
    assert!(!workspace.path().join("data").exists());
    let archive = workspace.path().join("legacy-archive");
    let entries = fs::read_dir(&archive)
        .expect("archive dir")
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].path().join("config.yaml").is_file());
    assert!(entries[0]
        .path()
        .join("data")
        .join("awiki-cli.db")
        .is_file());
}

#[test]
fn update_metadata_cache_is_global_not_tenant_local() {
    let workspace = TempDir::new("tenant-global-update-cache").expect("workspace");
    seed_metadata(workspace.path(), "0.0.1-beta.5", "10.0.1");
    assert_success(&awiki_cmd(
        &[
            "tenant",
            "create",
            "acme",
            "--backend-base-url",
            "https://api.acme.test",
            "--did-host",
            "acme.test",
        ],
        workspace.path(),
    ));
    assert_success(&awiki_cmd(&["tenant", "use", "acme"], workspace.path()));

    let output = awiki_cmd_extra(
        &["upgrade", "--format", "json"],
        workspace.path(),
        &[("AWIKI_CLI_UPDATE_CACHE_TTL", "3153600000")],
    );
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["update_metadata_source"], "cache");
    assert_eq!(envelope["data"]["latest_version"], "0.0.1-beta.5");
    assert!(
        !workspace
            .path()
            .join("tenants")
            .join("acme")
            .join("cache")
            .join("update")
            .join("metadata.json")
            .exists(),
        "update metadata cache must not move into a tenant workspace"
    );
}

fn seed_metadata(workspace: &Path, latest: &str, minimum: &str) {
    let path = workspace.join("cache").join("update").join("metadata.json");
    fs::create_dir_all(path.parent().unwrap()).expect("create cache dir");
    let payload = json!({
        "latest_version": latest,
        "min_supported_version": minimum,
        "retrieved_at": "2020-01-01T00:00:00Z",
        "source": "network",
    });
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload).unwrap()),
    )
    .expect("write metadata");
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read json")).expect("json")
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_extra(args, workspace, &[])
}

fn awiki_cmd_extra(args: &[&str], workspace: &Path, extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_CLI_DISABLE_STRICT_VERSION")
        .env_remove("AWIKI_CLI_UPDATE_CACHE_TTL")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_CLI_TRACE_TIMING");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true, "success envelope should set ok=true");
    envelope
}

fn error_json(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false, "error envelope should set ok=false");
    envelope
}

fn assert_value_contains(value: &Value, needle: &str) {
    assert_contains_text(value.as_str().unwrap_or_default(), needle);
}

fn assert_contains(haystack: &str, needle: &str) {
    assert_contains_text(haystack, needle);
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
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
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{prefix}-{}-{nanos}-{counter}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
