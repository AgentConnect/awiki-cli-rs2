use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn group_create_and_update_dry_run_match_go_policy_contracts() {
    let workspace = TempDir::new().expect("workspace");

    let create = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "create",
            "--dry-run",
            "--name",
            "Policy Group",
            "--description",
            "Policy group description",
            "--discoverability",
            "public",
            "--admission-mode",
            "open-join",
            "--slug",
            "policy-group",
            "--goal",
            "ship tests",
            "--rules",
            "be nice",
            "--message-prompt",
            "answer clearly",
            "--doc-url",
            "https://example.com/group",
            "--attachments-allowed",
            "--member-max-messages",
            "25",
            "--member-max-total-chars",
            "2048",
        ],
        workspace.path(),
    ));
    assert_eq!(create["summary"], "Dry run: group create planned");
    assert_eq!(create["data"]["plan"]["action"], "group.create");
    assert_eq!(create["data"]["plan"]["identity"], "alice");
    assert_eq!(create["data"]["plan"]["runtime_mode"], "websocket");
    let create_request = &create["data"]["plan"]["request"];
    assert_eq!(create_request["IdentityName"], "alice");
    assert_eq!(create_request["Name"], "Policy Group");
    assert_eq!(create_request["Discoverability"], "public");
    assert_eq!(create_request["AdmissionMode"], "open-join");
    assert_eq!(
        create_request["MessageSecurityProfile"],
        "transport-protected"
    );
    assert_eq!(create_request["E2EE"], false);
    assert_eq!(create_request["AttachmentsAllowed"], true);
    assert_eq!(create_request["MemberMaxMessages"], 25);
    assert_eq!(create_request["MemberMaxTotalChars"], 2048);

    let absent_pointers = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "create",
            "--dry-run",
            "--name",
            "Default Group",
        ],
        workspace.path(),
    ));
    let request = &absent_pointers["data"]["plan"]["request"];
    assert_eq!(request["Discoverability"], "private");
    assert_eq!(request["AdmissionMode"], "open-join");
    assert_eq!(request["AttachmentsAllowed"], Value::Null);
    assert_eq!(request["MemberMaxMessages"], Value::Null);
    assert_eq!(request["MemberMaxTotalChars"], Value::Null);

    let update = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "update",
            "--dry-run",
            "--group",
            "did:wba:awiki.ai:group:e1_policy",
            "--goal",
            "update tests",
            "--rules",
            "keep output stable",
            "--message-prompt",
            "reply in english",
            "--doc-url",
            "https://example.com/updated-group",
            "--attachments-allowed=false",
        ],
        workspace.path(),
    ));
    assert_eq!(update["summary"], "Dry run: group update planned");
    assert_eq!(update["data"]["plan"]["action"], "group.update");
    let update_request = &update["data"]["plan"]["request"];
    assert_eq!(update_request["Group"], "did:wba:awiki.ai:group:e1_policy");
    assert_eq!(update_request["Goal"], "update tests");
    assert_eq!(update_request["Rules"], "keep output stable");
    assert_eq!(update_request["MessagePrompt"], "reply in english");
    assert_eq!(
        update_request["DocURL"],
        "https://example.com/updated-group"
    );
    assert_eq!(update_request["AttachmentsAllowed"], false);
    assert_eq!(update_request["MemberMaxMessages"], Value::Null);
}

#[test]
fn group_schema_exposes_create_and_update_children() {
    let workspace = TempDir::new().expect("workspace");
    let schema = success_json(&awiki_cmd(&["schema", "group"], workspace.path()));
    let children: Vec<_> = schema["data"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    assert!(children.contains(&"group.create"));
    assert!(children.contains(&"group.update"));

    let create = success_json(&awiki_cmd(&["schema", "group", "create"], workspace.path()));
    assert_eq!(create["data"]["command"]["side_effect"], true);
    assert_eq!(create["data"]["command"]["flags"][0]["name"], "name");
    assert_eq!(create["data"]["command"]["flags"][0]["required"], true);
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli")
}

fn success_json(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("success JSON")
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
            "awiki-cli-rs2-group-test-{}-{nanos}",
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
