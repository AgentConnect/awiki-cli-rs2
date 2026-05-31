use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

use support::{open_local_state, write_ready_identity, TestIdentity, TestIdentityOptions};

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

    let empty_name = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "group",
            "create",
            "--dry-run",
            "--name=",
        ],
        workspace.path(),
    ));
    assert_eq!(empty_name["summary"], "Dry run: group create planned");
    assert_eq!(empty_name["data"]["plan"]["request"]["Name"], "");

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

    let empty_group = success_json(&awiki_cmd(
        &["group", "get", "--dry-run", "--group="],
        workspace.path(),
    ));
    assert_eq!(empty_group["summary"], "Dry run: group show planned");
    assert_eq!(empty_group["data"]["plan"]["group"], "");

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
    assert_eq!(add["data"]["plan"]["request"]["Secure"], "required");
    assert_eq!(add["data"]["plan"]["request"]["E2EE"], true);
    assert_warning_contains(&add, "--e2ee is deprecated; use --secure required.");

    let empty_membership = success_json(&awiki_cmd(
        &["group", "add", "--dry-run", "--group=", "--member="],
        workspace.path(),
    ));
    let empty_membership_request = &empty_membership["data"]["plan"]["request"];
    assert_eq!(empty_membership_request["Group"], "");
    assert_eq!(empty_membership_request["Member"], "");

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
    assert_eq!(
        remove["summary"],
        "Dry run: group membership change planned"
    );
    assert_eq!(remove["data"]["plan"]["action"], "group.kick");
    assert_eq!(remove["data"]["plan"]["request"]["Secure"], "required");
    assert_eq!(remove["data"]["plan"]["request"]["E2EE"], true);
    assert_warning_contains(&remove, "--e2ee is deprecated; use --secure required.");

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
    assert_eq!(leave["data"]["plan"]["action"], "group.leave");
    assert_eq!(leave["data"]["plan"]["request"]["Secure"], "required");
    assert_eq!(leave["data"]["plan"]["request"]["E2EE"], true);
    assert_warning_contains(&leave, "--e2ee is deprecated; use --secure required.");

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
fn group_reads_default_cutover_route_through_group_service_bridge() {
    let workspace = TempDir::new().expect("workspace");
    let alice = register_generated_group_identity(
        workspace.path(),
        "alice-group-cutover",
        "alice",
        "jwt-alice",
    );
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "group_state_version": "v7",
            "group_event_seq": 7,
            "group_profile": {
                "display_name": "Demo Group",
                "description": "Group contract fixture"
            },
            "member_role": "member",
            "member_status": "active",
            "member_count": 2,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "groups": [{
                "group_did": group_did,
                "name": "Demo Group",
                "member_role": "member",
                "member_status": "active"
            }],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [{
                "member_did": "did:wba:awiki.ai:bob:e1_bob",
                "member_handle": "bob.awiki.ai",
                "role": "member",
                "status": "active"
            }],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [{
                "id": "group-msg-1",
                "sender_did": alice.did,
                "group_did": group_did,
                "content": "hello group",
                "content_type": "text/plain",
                "server_seq": 42,
                "sent_at": "2026-05-21T00:00:00Z"
            }],
            "total": 1,
            "has_more": false,
            "next_since_seq": 43,
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let get = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-cutover",
            "group",
            "get",
            "--group",
            group_did,
        ],
        workspace.path(),
    ));
    assert_eq!(get["summary"], "Loaded group snapshot");
    assert_eq!(get["data"]["group"]["group_did"], group_did);
    assert_eq!(get["data"]["group"]["name"], "Demo Group");

    let list = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-cutover",
            "group",
            "list",
            "--limit",
            "25",
        ],
        workspace.path(),
    ));
    assert_eq!(list["summary"], "Loaded 1 groups");
    assert_eq!(list["data"]["groups"][0]["group_did"], group_did);

    let members = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-cutover",
            "group",
            "members",
            "--group",
            group_did,
            "--limit",
            "10",
        ],
        workspace.path(),
    ));
    assert_eq!(members["summary"], "Loaded 1 group members");
    assert_eq!(members["data"]["members"][0]["member_handle"], "bob");

    let messages = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-cutover",
            "group",
            "messages",
            "--group",
            group_did,
            "--limit",
            "5",
            "--cursor",
            "41",
        ],
        workspace.path(),
    ));
    assert_eq!(messages["summary"], "Loaded 1 group messages");
    assert_eq!(messages["data"]["messages"][0]["msg_id"], "group-msg-1");
    assert_eq!(messages["data"]["next_since_seq"], 43);

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    let bodies = requests
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(bodies[0]["method"], "group.get");
    assert_eq!(bodies[1]["method"], "group.list");
    assert_eq!(bodies[2]["method"], "group.list_members");
    assert_eq!(bodies[3]["method"], "group.list_messages");
    for body in &bodies {
        assert_eq!(body["params"]["meta"]["profile"], "anp.group.local.v1");
        assert_eq!(body["params"]["meta"]["sender_did"], alice.did);
        assert!(body["params"].get("auth").is_none());
    }
    assert_eq!(bodies[1]["params"]["body"]["limit"], 25);
    assert_eq!(bodies[2]["params"]["body"]["limit"], 10);
    assert_eq!(bodies[3]["params"]["body"]["limit"], 5);
    assert_eq!(bodies[3]["params"]["body"]["since_seq"], "41");
}

#[test]
fn group_lifecycle_default_cutover_routes_plain_create_join_and_leave() {
    let workspace = TempDir::new().expect("workspace");
    let alice = register_generated_group_identity(
        workspace.path(),
        "alice-group-life-cutover",
        "alice",
        "jwt-alice",
    );
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group_lifecycle";
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "group_profile": {
                "display_name": "Lifecycle Group"
            },
            "member_role": "owner",
            "member_status": "active",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [{
                "member_did": alice.did,
                "role": "owner",
                "status": "active"
            }],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "group_profile": {
                "display_name": "Lifecycle Group"
            },
            "member_role": "member",
            "member_status": "active",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [{
                "member_did": alice.did,
                "role": "member",
                "status": "active"
            }],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "left": true,
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let create = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-life-cutover",
            "group",
            "create",
            "--name",
            "Lifecycle Group",
            "--description",
            "Plain group",
            "--discoverability",
            "public",
        ],
        workspace.path(),
    ));
    assert_eq!(create["summary"], format!("Created group {group_did}"));
    assert_eq!(create["data"]["group"]["group_did"], group_did);
    assert_eq!(create["data"]["members"][0]["member_did"], alice.did);

    let join = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-life-cutover",
            "group",
            "join",
            "--group",
            group_did,
            "--reason",
            "  join me  ",
        ],
        workspace.path(),
    ));
    assert_eq!(join["summary"], format!("Joined group {group_did}"));
    assert_eq!(join["data"]["group"]["member_role"], "member");

    let leave = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-life-cutover",
            "group",
            "leave",
            "--group",
            group_did,
            "--reason",
            "ignored by plain leave",
        ],
        workspace.path(),
    ));
    assert_eq!(leave["summary"], format!("Left group {group_did}"));
    assert_eq!(leave["data"]["group"], group_did);

    let requests = server.requests();
    assert_eq!(requests.len(), 7);
    let bodies = requests
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).unwrap())
        .collect::<Vec<_>>();
    let methods = bodies
        .iter()
        .map(|body| body["method"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec![
            "group.create",
            "group.get",
            "group.list_members",
            "group.join",
            "group.get",
            "group.list_members",
            "group.leave",
        ]
    );
    assert_eq!(bodies[0]["params"]["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        bodies[0]["params"]["meta"]["target"],
        json!({"kind":"service","did":"did:wba:127.0.0.1"})
    );
    assert_eq!(
        bodies[0]["params"]["body"]["group_profile"]["display_name"],
        "Lifecycle Group"
    );
    assert_eq!(
        bodies[0]["params"]["body"]["group_policy"]["message_security_profile"],
        "transport-protected"
    );
    assert_eq!(
        bodies[0]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
    assert_eq!(bodies[3]["params"]["body"]["reason_text"], "join me");
    assert_eq!(
        bodies[3]["params"]["meta"]["target"],
        json!({"kind":"group","did":group_did})
    );
    assert_eq!(
        bodies[6]["params"]["body"],
        json!({"reason_text":"ignored by plain leave"})
    );
    assert_eq!(
        bodies[6]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );

    let db = open_local_state(workspace.path());
    let membership_status: String = db
        .query_row(
            "SELECT membership_status FROM groups WHERE owner_did = ?1 AND (group_id = ?2 OR group_did = ?2)",
            rusqlite::params![alice.did, group_did],
            |row| row.get(0),
        )
        .expect("cached group snapshot after leave");
    assert_eq!(membership_status, "left");
}

#[test]
fn group_lifecycle_dry_run_maps_e2ee_create_alias_to_secure_required() {
    let workspace = TempDir::new().expect("workspace");

    let output = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-e2ee-cutover",
            "group",
            "create",
            "--name",
            "Secure Group",
            "--e2ee",
            "--dry-run",
        ],
        workspace.path(),
    ));

    assert_eq!(output["summary"], "Dry run: group create planned");
    assert_eq!(output["data"]["plan"]["action"], "group.create");
    assert_eq!(
        output["data"]["plan"]["request"]["MessageSecurityProfile"],
        "group-e2ee"
    );
    assert_eq!(output["data"]["plan"]["request"]["Secure"], "required");
    assert_eq!(output["data"]["plan"]["request"]["E2EE"], true);
    assert_warning_contains(&output, "--e2ee is deprecated; use --secure required.");
}

#[test]
fn group_lifecycle_default_cutover_preserves_owner_cannot_leave_guard() {
    let workspace = TempDir::new().expect("workspace");
    let alice = register_generated_group_identity(
        workspace.path(),
        "alice-owner-guard-cutover",
        "alice",
        "jwt-alice",
    );
    let group_did = "did:wba:awiki.ai:groups:demo:e1_owner_guard";
    write_group_config(workspace.path(), "http://127.0.0.1:9");
    seed_group_snapshot(
        workspace.path(),
        &alice.unique_id,
        &alice.did,
        &alice.identity_name,
        group_did,
        "owner",
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-owner-guard-cutover",
            "group",
            "leave",
            "--group",
            group_did,
        ],
        workspace.path(),
    );

    assert_code(&output, 2);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(&envelope["error"]["message"], "group owner cannot leave");
}

#[test]
fn group_mutation_dry_run_maps_e2ee_member_aliases_to_secure_required() {
    let workspace = TempDir::new().expect("workspace");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group_e2ee_cached";
    let bob_did = "did:wba:awiki.ai:user:bob:e1_bob";
    let add = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-e2ee-mutation-cutover",
            "group",
            "add",
            "--group",
            group_did,
            "--member",
            bob_did,
            "--e2ee",
            "--dry-run",
        ],
        workspace.path(),
    ));
    assert_eq!(add["summary"], "Dry run: group membership change planned");
    assert_eq!(add["data"]["plan"]["action"], "group.add");
    assert_eq!(add["data"]["plan"]["request"]["Secure"], "required");
    assert_eq!(add["data"]["plan"]["request"]["E2EE"], true);
    assert_warning_contains(&add, "--e2ee is deprecated; use --secure required.");

    let canonical_secure_add = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-e2ee-mutation-cutover",
            "group",
            "add",
            "--group",
            group_did,
            "--member",
            bob_did,
            "--secure",
            "required",
            "--dry-run",
        ],
        workspace.path(),
    ));
    assert_eq!(
        canonical_secure_add["summary"],
        "Dry run: group membership change planned"
    );
    assert_eq!(
        canonical_secure_add["data"]["plan"]["request"]["Secure"],
        "required"
    );
    assert_eq!(
        canonical_secure_add["data"]["plan"]["request"]["E2EE"],
        true
    );
}

#[test]
fn group_mutation_default_cutover_routes_plain_member_and_update_paths() {
    let workspace = TempDir::new().expect("workspace");
    let alice = register_generated_group_identity(
        workspace.path(),
        "alice-group-mutation-cutover",
        "alice",
        "jwt-alice",
    );
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group_mutation";
    let bob_did = "did:wba:awiki.ai:user:bob:e1_bob";
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "did": bob_did,
            "full_handle": "bob.awiki.ai",
            "domain": "awiki.ai",
            "status": "active"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "operation_id": "op-add",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "group_profile": {
                "display_name": "Mutation Group"
            },
            "member_role": "owner",
            "member_status": "active",
            "member_count": 2,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [{
                "member_did": bob_did,
                "member_handle": "bob.awiki.ai",
                "role": "admin",
                "status": "active"
            }],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "operation_id": "op-remove",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "group_profile": {
                "display_name": "Mutation Group"
            },
            "member_role": "owner",
            "member_status": "active",
            "member_count": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "members": [],
            "total": 0,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "operation_id": "op-update-profile",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "operation_id": "op-update-policy",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "group_profile": {
                "display_name": "Renamed Mutation Group",
                "description": "Updated through im-core"
            },
            "group_policy": {
                "admission_mode": "invite-only",
                "attachments_allowed": false
            },
            "member_role": "owner",
            "member_status": "active",
            "member_count": 1,
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let add = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-mutation-cutover",
            "group",
            "add",
            "--group",
            group_did,
            "--member",
            "bob",
            "--role",
            " admin ",
        ],
        workspace.path(),
    ));
    assert_eq!(add["summary"], "Added member to group");
    assert_eq!(add["data"]["group"]["group_did"], group_did);
    assert_eq!(add["data"]["member"]["did"], bob_did);
    assert_eq!(add["data"]["member"]["handle"], "bob");
    assert_eq!(add["data"]["members"][0]["member_handle"], "bob");

    let remove = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-mutation-cutover",
            "group",
            "remove",
            "--group",
            group_did,
            "--member",
            bob_did,
            "--reason",
            " cleanup ",
        ],
        workspace.path(),
    ));
    assert_eq!(remove["summary"], "Removed member from group");
    assert_eq!(remove["data"]["member"]["did"], bob_did);
    assert_eq!(remove["data"]["members"].as_array().unwrap().len(), 0);

    let update = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice-group-mutation-cutover",
            "group",
            "update",
            "--group",
            group_did,
            "--name",
            " Renamed Mutation Group ",
            "--description",
            " Updated through im-core ",
            "--admission-mode",
            " invite-only ",
            "--attachments-allowed=false",
            "--max-members",
            " 25 ",
            "--member-max-messages",
            "5",
            "--member-max-total-chars",
            "4096",
        ],
        workspace.path(),
    ));
    assert_eq!(update["summary"], format!("Updated group {group_did}"));
    assert_eq!(update["data"]["group"]["name"], "Renamed Mutation Group");
    assert_eq!(update["data"]["delivery"].as_array().unwrap().len(), 2);

    let requests = server.requests();
    assert_eq!(requests.len(), 10);
    let bodies = requests
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).unwrap())
        .collect::<Vec<_>>();
    let methods = bodies
        .iter()
        .map(|body| body["method"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec![
            "lookup",
            "group.add",
            "group.get",
            "group.list_members",
            "group.remove",
            "group.get",
            "group.list_members",
            "group.update_profile",
            "group.update_policy",
            "group.get",
        ]
    );
    assert_eq!(bodies[0]["params"]["handle"], "bob.awiki.ai");
    assert_eq!(bodies[1]["params"]["body"]["member_did"], bob_did);
    assert_eq!(bodies[1]["params"]["body"]["role"], "admin");
    assert!(bodies[1]["params"]["body"].get("reason_text").is_none());
    assert_eq!(
        bodies[1]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
    assert_eq!(bodies[4]["params"]["body"]["member_did"], bob_did);
    assert_eq!(bodies[4]["params"]["body"]["reason_text"], "cleanup");
    assert!(bodies[4]["params"]["body"].get("role").is_none());
    assert_eq!(
        bodies[7]["params"]["body"]["group_profile_patch"],
        json!({
            "display_name": "Renamed Mutation Group",
            "description": "Updated through im-core",
        })
    );
    let policy = &bodies[8]["params"]["body"]["group_policy_patch"];
    assert_eq!(policy["admission_mode"], "invite-only");
    assert_eq!(policy["attachments_allowed"], false);
    assert_eq!(policy["max_members"], "25");
    assert_eq!(policy["member_max_messages"], 5);
    assert_eq!(policy["member_max_total_chars"], 4096);
    assert_eq!(policy["message_security_profile"], "transport-protected");
    assert_eq!(policy["permissions"]["update_policy"], "owner");
    for index in [1_usize, 4, 7, 8] {
        assert_eq!(
            bodies[index]["params"]["meta"]["target"],
            json!({"kind":"group","did":group_did})
        );
        assert_eq!(
            bodies[index]["params"]["meta"]["profile"],
            "anp.group.base.v1"
        );
    }
    let db = open_local_state(workspace.path());
    let (name, member_count): (String, i64) = db
        .query_row(
            "SELECT name, member_count FROM groups WHERE owner_did = ?1 AND (group_id = ?2 OR group_did = ?2)",
            rusqlite::params![alice.did, group_did],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cached group snapshot after mutation");
    assert_eq!(name, "Renamed Mutation Group");
    assert_eq!(member_count, 1);
    let member_rows: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM group_members WHERE owner_did = ?1 AND group_id = ?2",
            rusqlite::params![alice.did, group_did],
            |row| row.get(0),
        )
        .expect("cached group members after remove");
    assert_eq!(member_rows, 0);
}

#[test]
fn group_e2ee_leave_reaches_supported_path_before_identity_lookup() {
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

    assert_code(&output, 3);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "identity_required");
    assert_eq!(envelope["error"]["message"], "authentication is required");
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

    let status = success_json(&awiki_internal_cmd(
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
    assert_eq!(status["summary"], "Dry run: group secure status planned");
    let status_plan = &status["data"]["plan"];
    assert_eq!(status_plan["action"], "secure.group.status");
    assert_eq!(status_plan["group"], group);
    assert_eq!(status_plan["runtime_mode"], "websocket");
    assert_warning_contains(
        &status,
        "group e2ee status is deprecated; use group secure status.",
    );

    let publish = success_json(&awiki_internal_cmd(
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
    assert_group_e2ee_plan_is_redacted(publish_plan);

    let recovery_alias = success_json(&awiki_internal_cmd(
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

    let pending = success_json(&awiki_internal_cmd(
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
    assert_eq!(pending["data"]["plan"]["provider"], "internal");
    assert_eq!(pending["data"]["plan"]["group"], group);
    assert_group_e2ee_plan_is_redacted(&pending["data"]["plan"]);

    let repair = success_json(&awiki_internal_cmd(
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
    assert_eq!(repair["summary"], "Dry run: group secure repair planned");
    assert_eq!(repair["data"]["plan"]["action"], "secure.group.repair");
    assert_eq!(
        repair["data"]["plan"]["local_writes"],
        json!(["group_mls_state"])
    );
    assert_warning_contains(
        &repair,
        "group e2ee repair is deprecated; use group secure repair.",
    );

    let process_leave = success_json(&awiki_internal_cmd(
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
    assert_group_e2ee_plan_is_redacted(process_plan);

    let recover = success_json(&awiki_internal_cmd(
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
    assert_group_e2ee_plan_is_redacted(&recover["data"]["plan"]);

    let update = success_json(&awiki_internal_cmd(
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
    assert_group_e2ee_plan_is_redacted(&update["data"]["plan"]);

    let rejoin = success_json(&awiki_internal_cmd(
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

fn assert_group_e2ee_plan_is_redacted(plan: &Value) {
    assert_eq!(plan["provider"], "internal");
    let encoded = serde_json::to_string(plan).expect("plan json");
    assert!(
        !encoded.contains("mls_data_dir"),
        "group E2EE dry-run plan must not expose MLS state paths: {encoded}"
    );
    assert!(
        !encoded.contains("\"binary\""),
        "group E2EE dry-run plan must not expose provider binary paths: {encoded}"
    );
}

#[test]
fn group_e2ee_live_commands_are_cutover_unsupported() {
    let workspace = TempDir::new().expect("workspace");
    let group = "did:wba:awiki.ai:groups:demo:e1_group";
    let commands = [
        (
            "group.e2ee.publish-key-package",
            vec!["group", "e2ee", "publish-key-package", "--group", group],
        ),
        (
            "group.e2ee.pending",
            vec!["group", "e2ee", "pending", "--group", group],
        ),
        (
            "group.e2ee.process-leave-request",
            vec![
                "group",
                "e2ee",
                "process-leave-request",
                "--group",
                group,
                "--member",
                "bob",
            ],
        ),
        (
            "group.e2ee.recover-member",
            vec![
                "group",
                "e2ee",
                "recover-member",
                "--group",
                group,
                "--member",
                "bob",
            ],
        ),
        (
            "group.e2ee.update-key",
            vec![
                "group",
                "e2ee",
                "update-key",
                "--group",
                group,
                "--member",
                "bob",
            ],
        ),
        (
            "group.e2ee.rejoin",
            vec![
                "group", "e2ee", "rejoin", "--group", group, "--member", "bob",
            ],
        ),
    ];
    for (command, args) in commands {
        let output = awiki_cmd(&args, workspace.path());
        assert_group_e2ee_unsupported(&output, command);
    }
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
    awiki_command(args, workspace)
        .output()
        .expect("run awiki-cli")
}

fn awiki_internal_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = awiki_command(args, workspace);
    command.env("AWIKI_CLI_INTERNAL_ENTRY", "1");
    command.output().expect("run awiki-cli")
}

fn awiki_command(args: &[&str], workspace: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command
}

fn register_generated_group_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> TestIdentity {
    write_ready_identity(
        workspace,
        TestIdentityOptions {
            identity_name,
            handle,
            display_name: identity_name,
            jwt_token,
            make_default: true,
        },
    )
}

fn write_group_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn seed_group_snapshot(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    credential_name: &str,
    group_did: &str,
    role: &str,
) {
    let db = open_local_state(workspace);
    db.execute(
        r#"
INSERT INTO groups (
    owner_identity_id, owner_did, group_id, group_did, name, group_owner_did, group_mode,
    my_role, membership_status, stored_at, credential_name
) VALUES (?1, ?2, ?3, ?3, 'Guard Group', ?2, 'general', ?4, 'active',
          '2026-05-25T00:00:00Z', ?5)
ON CONFLICT(owner_identity_id, group_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    group_did = excluded.group_did,
    name = excluded.name,
    group_owner_did = excluded.group_owner_did,
    my_role = excluded.my_role,
    membership_status = excluded.membership_status,
    credential_name = excluded.credential_name
"#,
        rusqlite::params![
            owner_identity_id,
            owner_did,
            group_did,
            role,
            credential_name
        ],
    )
    .expect("seed group snapshot");
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

fn assert_warning_contains(envelope: &Value, expected: &str) {
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|value| value.contains(expected))),
        "expected warning {expected:?}; got {warnings:?}"
    );
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

fn assert_group_e2ee_unsupported(output: &Output, command: &str) {
    assert_code(output, 2);
    let envelope = error_json(output);
    assert_eq!(envelope["error"]["details"]["command"], command);
    if command.starts_with("group.e2ee.") {
        assert_eq!(envelope["error"]["code"], "internal_command");
    } else {
        assert_eq!(envelope["error"]["code"], "unsupported_capability");
        assert_eq!(envelope["error"]["details"]["capability"], "group e2ee");
        assert_eq!(envelope["error"]["details"]["required_phase"], "Phase 6");
    }
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value.as_str().unwrap_or_default();
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn json_rpc_result(result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": "req-1",
    })
    .to_string()
}

#[derive(Debug, Clone)]
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
        self.requests.lock().expect("requests mutex").clone()
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
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
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
    let request = read_http_request(&mut stream);
    requests.lock().expect("requests mutex").push(request);
    let body = response.body.as_bytes();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        body.len(),
        response.body
    );
    stream.write_all(raw.as_bytes()).expect("write response");
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buf = [0_u8; 512];
    loop {
        let count = stream.read(&mut buf).expect("read request");
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..count]);
        if let Some(header_end) = find_header_end(&raw) {
            let headers = String::from_utf8_lossy(&raw[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then_some(value)
                    })
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or_default();
            let expected = header_end + content_length;
            while raw.len() < expected {
                let count = stream.read(&mut buf).expect("read request body");
                if count == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..count]);
            }
            break;
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
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
