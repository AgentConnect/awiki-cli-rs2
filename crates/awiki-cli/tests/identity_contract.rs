use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn identity_create_list_current_use_and_status_match_local_contract() {
    let workspace = TempDir::new().expect("workspace");

    let alice = awiki_cmd(
        &[
            "id",
            "create",
            "--name",
            "Alice Example",
            "--identity",
            "alice",
        ],
        workspace.path(),
    );
    assert_success(&alice);
    let alice = success_json(&alice);
    assert_eq!(alice["data"]["action"], "create_identity");
    assert_eq!(alice["data"]["identity"]["identity_name"], "alice");
    assert_eq!(alice["data"]["identity"]["is_default"], true);
    assert_eq!(alice["data"]["identity"]["has_key1_private"], true);
    assert!(alice["data"]["identity"].get("user_id").is_none());

    let bob = awiki_cmd(
        &["id", "create", "--name", "Bob Example", "--identity", "bob"],
        workspace.path(),
    );
    assert_success(&bob);

    let current = success_json(&awiki_cmd(&["id", "current"], workspace.path()));
    assert_eq!(current["data"]["identity"]["identity_name"], "alice");

    let use_bob = success_json(&awiki_cmd(&["id", "use", "bob"], workspace.path()));
    assert_eq!(use_bob["data"]["action"], "set_default_identity");
    assert_eq!(use_bob["data"]["identity"]["identity_name"], "bob");

    let list = success_json(&awiki_cmd(&["id", "list"], workspace.path()));
    let names: Vec<_> = list["data"]["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["identity_name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["alice", "bob"]);
    assert_eq!(list["data"]["default_identity"]["identity_name"], "bob");
    assert!(list["data"]["legacy_scan"].is_object());

    let status = success_json(&awiki_cmd(&["id", "status"], workspace.path()));
    assert_eq!(status["data"]["active_identity"]["identity_name"], "bob");
    assert_eq!(status["data"]["identity_count"], 2);
}

#[test]
fn identity_dry_run_and_validation_contracts_match_go() {
    let workspace = TempDir::new().expect("workspace");
    let create = success_json(&awiki_cmd(
        &[
            "id",
            "create",
            "--dry-run",
            "--name",
            "Dry Run User",
            "--identity",
            "dry-run-user",
        ],
        workspace.path(),
    ));
    assert_eq!(create["meta"]["dry_run"], true);
    assert_eq!(create["data"]["plan"]["action"], "create_identity");
    assert_eq!(create["data"]["plan"]["identity_name"], "dry-run-user");

    let use_plan = success_json(&awiki_cmd(
        &["id", "use", "--dry-run", "dry-run-user"],
        workspace.path(),
    ));
    assert_eq!(use_plan["data"]["plan"]["action"], "set_default_identity");

    let missing = awiki_cmd(&["id", "use"], workspace.path());
    assert_code(&missing, 2);
    let missing = error_json(&missing);
    assert_eq!(missing["error"]["code"], "invalid_argument");
    assert!(missing["error"]["message"]
        .as_str()
        .unwrap()
        .contains("id use requires exactly one identity name"));

    let refresh = success_json(&awiki_cmd(
        &["--identity", "alice", "id", "refresh-token", "--dry-run"],
        workspace.path(),
    ));
    assert_eq!(refresh["data"]["plan"]["action"], "refresh_token");
    assert_eq!(refresh["data"]["plan"]["identity_name"], "alice");
    assert_eq!(
        refresh["data"]["plan"]["auth_flow"],
        "did_auth_get_me_without_stored_bearer"
    );
    assert!(refresh["data"]["plan"]["remote_calls"]
        .as_array()
        .unwrap()
        .contains(&json!("did-auth.get_me")));
    assert!(refresh["data"]["plan"]["local_writes"]
        .as_array()
        .unwrap()
        .contains(&json!("auth.json")));
}

#[test]
fn identity_import_v1_flat_legacy_contract() {
    let workspace = TempDir::new().expect("workspace");
    let home = workspace.path().join("home");
    let legacy = home
        .join(".openclaw")
        .join("credentials")
        .join("awiki-agent-id-message");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("legacy-flat.json"),
        serde_json::to_vec_pretty(&json!({
            "did": "did:wba:example.test:user:e1_legacy",
            "unique_id": "e1_legacy",
            "name": "Legacy Flat",
            "handle": "legacy-flat",
            "jwt_token": "token",
            "private_key_pem": "private",
            "public_key_pem": "public",
            "did_document": {"id": "did:wba:example.test:user:e1_legacy"}
        }))
        .unwrap(),
    )
    .unwrap();

    let imported = success_json(&awiki_cmd_with_home(
        &["id", "import-v1", "--name", "legacy-flat"],
        workspace.path(),
        &home,
    ));
    assert_eq!(
        imported["data"]["result"]["imported"][0]["identity_name"],
        "legacy-flat"
    );
    let current = success_json(&awiki_cmd_with_home(
        &["id", "current"],
        workspace.path(),
        &home,
    ));
    assert_eq!(current["data"]["identity"]["identity_name"], "legacy-flat");
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_with_home(args, workspace, workspace)
}

fn awiki_cmd_with_home(args: &[&str], workspace: &Path, home: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("HOME", home)
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli")
}

fn success_json(output: &Output) -> Value {
    assert_success(output);
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("success JSON")
}

fn error_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stderr).expect("error JSON")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-id-test-{}-{nanos}",
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
