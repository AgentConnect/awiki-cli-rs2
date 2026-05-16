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
fn group_lifecycle_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let get = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "get",
            "--dry-run",
            "--group",
            group,
        ],
        workspace.path(),
    ));
    assert_eq!(get["summary"], "Dry run: group show planned");
    assert_eq!(get["data"]["plan"]["action"], "group.show");
    assert_eq!(get["data"]["plan"]["group"], group);

    let show = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "show",
            "--dry-run",
            "--group",
            group,
        ],
        workspace.path(),
    ));
    assert_eq!(show["data"]["plan"]["action"], "group.show");

    let join = success_json(&awiki_cmd(
        &[
            "--identity",
            "bob",
            "group",
            "join",
            "--dry-run",
            "--group",
            group,
            "--reason",
            "joinable group",
        ],
        workspace.path(),
    ));
    assert_eq!(join["summary"], "Dry run: group join planned");
    let join_request = &join["data"]["plan"]["request"];
    assert_eq!(join_request["IdentityName"], "bob");
    assert_eq!(join_request["Group"], group);
    assert_eq!(join_request["ReasonText"], "joinable group");

    let add = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "add",
            "--dry-run",
            "--group",
            group,
            "--member",
            "bob",
            "--e2ee",
        ],
        workspace.path(),
    ));
    assert_eq!(add["summary"], "Dry run: group membership change planned");
    assert_eq!(add["data"]["plan"]["action"], "group.add");
    assert_eq!(add["data"]["plan"]["member_handle"], "bob.awiki.ai");
    let add_request = &add["data"]["plan"]["request"];
    assert_eq!(add_request["Member"], "bob");
    assert_eq!(add_request["Role"], "member");
    assert_eq!(add_request["ReasonText"], "");
    assert_eq!(add_request["E2EE"], true);
    assert_eq!(add_request["LeaveRequestID"], "");

    let remove = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "remove",
            "--dry-run",
            "--group",
            group,
            "--member",
            "bob",
            "--reason",
            "cleanup",
            "--e2ee",
        ],
        workspace.path(),
    ));
    assert_eq!(remove["data"]["plan"]["action"], "group.kick");
    let remove_request = &remove["data"]["plan"]["request"];
    assert_eq!(remove_request["ReasonText"], "cleanup");
    assert_eq!(remove_request["E2EE"], true);

    let kick = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "kick",
            "--dry-run",
            "--group",
            group,
            "--member",
            "did:wba:awiki.ai:user:bob:e1",
        ],
        workspace.path(),
    ));
    assert_eq!(kick["data"]["plan"]["action"], "group.kick");
    assert_eq!(kick["data"]["plan"]["member_handle"], Value::Null);

    let leave = success_json(&awiki_cmd(
        &[
            "--identity",
            "bob",
            "group",
            "leave",
            "--dry-run",
            "--group",
            group,
            "--reason",
            "done",
            "--e2ee",
        ],
        workspace.path(),
    ));
    assert_eq!(leave["summary"], "Dry run: group leave planned");
    let leave_request = &leave["data"]["plan"]["request"];
    assert_eq!(leave_request["ReasonText"], "done");
    assert_eq!(leave_request["E2EE"], true);

    let list = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "list",
            "--dry-run",
            "--limit",
            "25",
        ],
        workspace.path(),
    ));
    assert_eq!(list["summary"], "Dry run: group list planned");
    assert_eq!(list["data"]["plan"]["request"]["Limit"], 25);

    let members = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "members",
            "--dry-run",
            "--group",
            group,
        ],
        workspace.path(),
    ));
    assert_eq!(members["summary"], "Dry run: group members planned");
    assert_eq!(members["data"]["plan"]["action"], "group.list_members");
    assert_eq!(members["data"]["plan"]["request"]["Limit"], 100);

    let messages = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "messages",
            "--dry-run",
            "--group",
            group,
            "--limit",
            "25",
            "--cursor",
            "42",
        ],
        workspace.path(),
    ));
    assert_eq!(messages["summary"], "Dry run: group messages planned");
    let messages_request = &messages["data"]["plan"]["request"];
    assert_eq!(messages["data"]["plan"]["action"], "group.list_messages");
    assert_eq!(messages_request["Cursor"], "42");
    assert_eq!(messages_request["Limit"], 25);
    assert_eq!(messages_request["Skip"], 0);
}

#[test]
fn group_e2ee_self_leave_error_matches_go_handler_boundary() {
    let workspace = TempDir::new().expect("workspace");
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "leave",
            "--group",
            group,
            "--e2ee",
        ],
        workspace.path(),
    );

    assert_code(&output, 1);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "unsupported_mode");
    assert_contains(
        &envelope["error"]["message"],
        "group E2EE self-leave is not cryptographically supported yet",
    );
    assert_eq!(
        envelope["error"]["hint"],
        "For PR-A group E2EE, ask the group owner to remove the member; self-leave requires a future epoch-advancing leave-request flow."
    );
}

#[test]
fn group_schema_exposes_non_e2ee_group_children() {
    let workspace = TempDir::new().expect("workspace");
    let schema = success_json(&awiki_cmd(&["schema", "group"], workspace.path()));
    let children: Vec<_> = schema["data"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    assert!(children.contains(&"group.create"));
    assert!(children.contains(&"group.get"));
    assert!(children.contains(&"group.join"));
    assert!(children.contains(&"group.add"));
    assert!(children.contains(&"group.remove"));
    assert!(children.contains(&"group.leave"));
    assert!(children.contains(&"group.update"));
    assert!(children.contains(&"group.list"));
    assert!(children.contains(&"group.members"));
    assert!(children.contains(&"group.messages"));

    let create = success_json(&awiki_cmd(&["schema", "group", "create"], workspace.path()));
    assert_eq!(create["data"]["command"]["side_effect"], true);
    assert_eq!(create["data"]["command"]["flags"][0]["name"], "name");
    assert_eq!(create["data"]["command"]["flags"][0]["required"], true);

    let get = success_json(&awiki_cmd(&["schema", "group", "get"], workspace.path()));
    assert_eq!(get["data"]["command"]["aliases"][0], "show");
    assert_eq!(get["data"]["command"]["outputs"][2], "table");

    let remove = success_json(&awiki_cmd(&["schema", "group", "remove"], workspace.path()));
    assert_eq!(remove["data"]["command"]["aliases"][0], "kick");
    assert_eq!(remove["data"]["command"]["side_effect"], true);
}

#[test]
fn group_e2ee_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let status = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "status",
            "--dry-run",
            "--group",
            group,
        ],
        workspace.path(),
    ));
    assert_eq!(status["summary"], "Dry run: group e2ee status planned");
    let status_plan = &status["data"]["plan"];
    assert_eq!(status_plan["action"], "group.e2ee.status");
    assert_eq!(status_plan["profile"], "anp.group.e2ee.v1");
    assert_eq!(status_plan["security_profile"], "group-e2ee");
    assert_eq!(status_plan["provider"], "exec");
    assert_eq!(status_plan["binary"], "");
    assert_eq!(status_plan["discovery_advertised"], false);
    assert_eq!(status_plan["artifact_mode"], Value::Null);
    assert!(status_plan["mls_data_dir"]
        .as_str()
        .unwrap()
        .ends_with("/mls"));

    let publish = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "publish-key-package",
            "--dry-run",
            "--group",
            group,
            "--purpose",
            "update",
            "--device",
            "bob-main",
            "--contract-test",
        ],
        workspace.path(),
    ));
    assert_eq!(
        publish["summary"],
        "Dry run: group e2ee key package publish planned"
    );
    let publish_plan = &publish["data"]["plan"];
    assert_eq!(publish_plan["action"], "group.e2ee.publish_key_package");
    assert_eq!(publish_plan["purpose"], "update");
    assert_eq!(publish_plan["recovery"], false);
    assert_eq!(publish_plan["device"], "bob-main");
    assert_eq!(publish_plan["contract_test_only"], true);

    let recovery_alias = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "publish-key-package",
            "--dry-run",
            "--recovery",
        ],
        workspace.path(),
    ));
    assert_eq!(recovery_alias["data"]["plan"]["purpose"], "recovery");
    assert_eq!(recovery_alias["data"]["plan"]["recovery"], true);

    let pending = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "pending",
            "--dry-run",
            "--group",
            group,
        ],
        workspace.path(),
    ));
    assert_eq!(pending["summary"], "Dry run: group e2ee pending planned");
    assert_eq!(pending["data"]["plan"]["action"], "group.e2ee.pending");
    assert_eq!(pending["data"]["plan"]["provider"], "exec");
    assert_eq!(pending["data"]["plan"]["group"], group);

    let repair = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "repair",
            "--dry-run",
            "--group",
            group,
        ],
        workspace.path(),
    ));
    assert_eq!(repair["summary"], "Dry run: group e2ee repair planned");
    assert_eq!(repair["data"]["plan"]["action"], "group.e2ee.repair");
    assert!(repair["data"]["plan"]["scope"]
        .as_str()
        .unwrap()
        .contains("replay welcome/commit notices"));

    let process_leave = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "process-leave-request",
            "--dry-run",
            "--group",
            group,
            "--member",
            "bob",
            "--leave-request-id",
            "lr-bob-1",
            "--reason",
            "owner remove",
        ],
        workspace.path(),
    ));
    assert_eq!(
        process_leave["summary"],
        "Dry run: group e2ee leave request process planned"
    );
    let process_plan = &process_leave["data"]["plan"];
    assert_eq!(process_plan["action"], "group.e2ee.process_leave_request");
    assert_eq!(process_plan["member"], "bob");
    assert_eq!(process_plan["leave_request_id"], "lr-bob-1");
    assert_eq!(process_plan["request"]["LeaveRequestID"], "lr-bob-1");
    assert_eq!(process_plan["request"]["ReasonText"], "owner remove");

    let recover = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "recover-member",
            "--dry-run",
            "--group",
            group,
            "--member",
            "bob",
            "--device",
            "bob-main",
        ],
        workspace.path(),
    ));
    assert_eq!(
        recover["summary"],
        "Dry run: group e2ee recover-member planned"
    );
    assert_eq!(
        recover["data"]["plan"]["action"],
        "group.e2ee.recover_member"
    );
    assert_eq!(recover["data"]["plan"]["p4_membership_mutate"], false);
    assert!(recover["data"]["plan"]["orchestration"]
        .as_array()
        .unwrap()
        .contains(&Value::String(
            "hidden group.e2ee.recover_member".to_string()
        )));

    let update = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "update-key",
            "--dry-run",
            "--group",
            group,
            "--member",
            "bob",
            "--device",
            "bob-main",
        ],
        workspace.path(),
    ));
    assert_eq!(update["summary"], "Dry run: group e2ee update-key planned");
    assert_eq!(update["data"]["plan"]["action"], "group.e2ee.update_key");
    assert_eq!(update["data"]["plan"]["key_package_purpose"], "update");
    assert_eq!(update["data"]["plan"]["hidden_awiki_extension"], true);
    assert_eq!(update["data"]["plan"]["p4_membership_mutate"], false);

    let rejoin = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "e2ee",
            "rejoin",
            "--dry-run",
            "--group",
            group,
            "--member",
            "bob",
        ],
        workspace.path(),
    ));
    assert_eq!(rejoin["summary"], "Dry run: group e2ee rejoin planned");
    assert_eq!(rejoin["data"]["plan"]["action"], "group.e2ee.rejoin");
    assert_eq!(
        rejoin["data"]["plan"]["canonical_command"],
        "group add --e2ee"
    );
    assert_eq!(rejoin["data"]["plan"]["role"], "member");
    assert_eq!(rejoin["data"]["plan"]["key_package_purpose"], "normal");
    assert_eq!(rejoin["data"]["plan"]["external_commit"], false);
    assert_eq!(rejoin["data"]["plan"]["p4_membership_mutate"], true);
}

#[test]
fn group_e2ee_schema_exposes_hidden_and_side_effect_contracts() {
    let workspace = TempDir::new().expect("workspace");
    let schema = success_json(&awiki_cmd(&["schema", "group", "e2ee"], workspace.path()));
    let children: Vec<_> = schema["data"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    assert!(children.contains(&"group.e2ee.status"));
    assert!(children.contains(&"group.e2ee.publish-key-package"));
    assert!(children.contains(&"group.e2ee.pending"));
    assert!(children.contains(&"group.e2ee.repair"));
    assert!(children.contains(&"group.e2ee.update-key"));
    assert!(children.contains(&"group.e2ee.rejoin"));
    assert!(children.contains(&"group.e2ee.recover-member"));
    assert!(children.contains(&"group.e2ee.process-leave-request"));

    let update = success_json(&awiki_cmd(
        &["schema", "group", "e2ee", "update-key"],
        workspace.path(),
    ));
    assert_eq!(update["data"]["command"]["hidden"], true);
    assert_eq!(update["data"]["command"]["side_effect"], true);

    let rejoin = success_json(&awiki_cmd(
        &["schema", "group", "e2ee", "rejoin"],
        workspace.path(),
    ));
    assert_eq!(rejoin["data"]["command"]["hidden"], true);
    assert_eq!(rejoin["data"]["command"]["flags"][2]["default"], "member");

    let publish = success_json(&awiki_cmd(
        &["schema", "group", "e2ee", "publish-key-package"],
        workspace.path(),
    ));
    assert_eq!(publish["data"]["command"]["flags"][0]["default"], "default");
    assert_eq!(publish["data"]["command"]["flags"][1]["default"], "normal");
    assert_eq!(publish["data"]["command"]["side_effect"], true);
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
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

fn error_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("error JSON")
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value.as_str().unwrap_or_default();
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
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
