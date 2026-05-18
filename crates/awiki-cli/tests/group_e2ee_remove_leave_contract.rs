use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-e2ee-remove";
const AGENT_DID: &str = "did:wba:awiki.ai:alice:e1_alice";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:demo:e1_group";
const MEMBER_DID: &str = "did:wba:awiki.ai:bob:e1_bob";
const SERVICE_DID: &str = "did:wba:awiki.ai:service:e1_message";
const DEFAULT_PROCESS_LEAVE_REASON: &str = "leave request processed by owner";

#[test]
fn group_remove_e2ee_live_uses_hidden_remove_mls_finalize_and_syncs_without_p4_remove() {
    let workspace = TempDir::new("group-e2ee-remove-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-remove-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_remove_member(&fake_mls, &args_log, &stdin_log);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(hidden_remove_delivery("op-e2ee-remove"))),
        TestResponse::ok(&json_rpc_result(group_snapshot_after_remove())),
        TestResponse::ok(&json_rpc_result(group_members_after_remove())),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "remove",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--reason",
            "cleanup",
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_eq!(
        provider_commands(&args_log),
        vec!["group remove-member", "group commit-finalize"]
    );
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 2);
    assert_eq!(provider_stdin[0]["api_version"], "anp-mls/v1");
    assert_eq!(provider_stdin[0]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin[0]["device_id"], "default");
    assert_eq!(provider_stdin[0]["params"]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin[0]["params"]["device_id"], "default");
    assert_eq!(provider_stdin[0]["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin[0]["params"]["member_did"], MEMBER_DID);
    assert_eq!(provider_stdin[1]["params"]["subject_did"], MEMBER_DID);
    assert_eq!(provider_stdin[1]["params"]["subject_status"], "removed");
    assert_eq!(provider_stdin[1]["params"]["group_did"], GROUP_DID);
    assert_eq!(
        provider_stdin[1]["params"]["commit_b64u"],
        "cmVtb3ZlLWNvbW1pdA"
    );
    assert_eq!(
        provider_stdin[1]["params"]["pending_commit_id"],
        "pc-remove-1"
    );
    assert_eq!(provider_stdin[1]["params"]["from_epoch"], 8);
    assert_eq!(provider_stdin[1]["params"]["to_epoch"], 9);
    assert!(provider_stdin[1]["params"]["operation_id"].is_null());

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Removed member from group");
    assert!(envelope.get("warnings").is_none());
    assert_eq!(envelope["data"]["group"]["group_did"], GROUP_DID);
    assert_contains_member(&envelope["data"]["members"], AGENT_DID);
    assert_not_contains_member(&envelope["data"]["members"], MEMBER_DID);
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-e2ee-remove"
    );
    assert_eq!(envelope["data"]["e2ee"]["subject_did"], MEMBER_DID);
    assert_eq!(envelope["data"]["e2ee"]["reason_text"], "cleanup");
    assert_eq!(
        envelope["data"]["e2ee"]["mls_prepare"]["pending_commit_id"],
        "pc-remove-1"
    );
    assert_eq!(
        envelope["data"]["e2ee"]["mls_finalize"]["finalized_commit_id"],
        "fc-remove-1"
    );
    assert_eq!(
        envelope["data"]["e2ee"]["delivery"]["operation_id"],
        "op-e2ee-remove"
    );

    let requests = server.requests();
    let bodies = request_json_bodies(&requests);
    let methods = rpc_methods(&bodies);
    assert_eq!(
        methods,
        vec!["group.e2ee.remove", "group.get", "group.list_members"]
    );
    assert!(
        !methods.contains(&"group.remove"),
        "group remove --e2ee must not call normal P4 group.remove"
    );
    assert_eq!(bodies[0]["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(
        bodies[0]["params"]["meta"]["security_profile"],
        "group-e2ee"
    );
    assert_eq!(
        bodies[0]["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(bodies[0]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[0]["params"]["body"]["member_did"], MEMBER_DID);
    assert_eq!(bodies[0]["params"]["body"]["subject_did"], MEMBER_DID);
    assert_eq!(bodies[0]["params"]["body"]["subject_status"], "removed");
    assert_eq!(bodies[0]["params"]["body"]["reason_text"], "cleanup");
    assert_eq!(
        bodies[0]["params"]["body"]["pending_commit_id"],
        "pc-remove-1"
    );
    assert_eq!(
        bodies[0]["params"]["body"]["commit_b64u"],
        "cmVtb3ZlLWNvbW1pdA"
    );
    assert_eq!(
        bodies[0]["params"]["body"]["group_state_ref"]["group_did"],
        GROUP_DID
    );
    assert_eq!(
        bodies[0]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
    assert_eq!(bodies[1]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[2]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[2]["params"]["body"]["limit"], 100);
}

#[test]
fn group_leave_e2ee_live_creates_hidden_leave_request_without_p4_leave() {
    let workspace = TempDir::new("group-e2ee-leave-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "accepted": true,
        "group_did": GROUP_DID,
        "member_did": AGENT_DID,
        "subject_did": AGENT_DID,
        "leave_request_id": "leave-req-alice-1",
        "operation_id": "op-e2ee-leave-request",
        "source": "remote_http"
    })))]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "group",
            "leave",
            "--group",
            GROUP_DID,
            "--reason",
            "done",
            "--e2ee",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        format!("Requested group E2EE leave for {GROUP_DID}")
    );
    assert_eq!(
        envelope["data"]["e2ee"]["leave_request_id"],
        "leave-req-alice-1"
    );
    assert_warning_contains_all(&envelope, &["owner", "process"]);

    let bodies = request_json_bodies(&server.requests());
    let methods = rpc_methods(&bodies);
    assert_eq!(methods, vec!["group.e2ee.leave_request"]);
    assert!(
        !methods.contains(&"group.leave"),
        "group leave --e2ee must not call normal P4 group.leave"
    );
    assert_eq!(bodies[0]["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(
        bodies[0]["params"]["meta"]["security_profile"],
        "transport-protected"
    );
    assert_eq!(
        bodies[0]["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(bodies[0]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[0]["params"]["body"]["member_did"], AGENT_DID);
    assert_eq!(bodies[0]["params"]["body"]["subject_did"], AGENT_DID);
    assert_eq!(
        bodies[0]["params"]["body"]["subject_status"],
        "leave_requested"
    );
    assert_eq!(bodies[0]["params"]["body"]["reason_text"], "done");
    assert_eq!(
        bodies[0]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

#[test]
fn group_e2ee_process_leave_request_live_delegates_to_e2ee_remove_with_leave_request_id() {
    assert_process_leave_request_remove_path(None, DEFAULT_PROCESS_LEAVE_REASON);
    assert_process_leave_request_remove_path(Some("owner approved"), "owner approved");
}

#[test]
fn group_remove_e2ee_deterministic_submit_failure_aborts_pending_commit_like_go() {
    let workspace = TempDir::new("group-e2ee-remove-submit-403").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-remove-submit-403-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_remove_member_with_terminal(
        &fake_mls,
        &args_log,
        &stdin_log,
        TerminalBehavior::AbortSucceeds,
    );
    let server = TestServer::new(vec![TestResponse::status(403, "deterministic rejection")]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "remove",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--reason",
            "cleanup",
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    let error = error_json(&output);
    assert_eq!(error["error"]["code"], "internal_error");
    assert_text_contains(
        error["error"]["message"].as_str().expect("error message"),
        "service http error 403: deterministic rejection",
    );
    assert_text_contains(
        error["error"]["message"].as_str().expect("error message"),
        "local group E2EE pending commit aborted",
    );
    assert_eq!(
        provider_commands(&args_log),
        vec!["group remove-member", "group commit-abort"]
    );
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 2);
    assert_eq!(
        provider_stdin[1]["params"]["pending_commit_id"],
        "pc-remove-1"
    );
    assert!(provider_stdin[1]["params"]["operation_id"].is_null());
    let bodies = request_json_bodies(&server.requests());
    assert_eq!(rpc_methods(&bodies), vec!["group.e2ee.remove"]);
}

#[test]
fn group_remove_e2ee_retryable_submit_failure_retains_pending_commit_like_go() {
    let workspace = TempDir::new("group-e2ee-remove-submit-500").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-remove-submit-500-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_remove_member_with_terminal(
        &fake_mls,
        &args_log,
        &stdin_log,
        TerminalBehavior::FinalizeSucceeds,
    );
    let server = TestServer::new(vec![TestResponse::status(500, "retry later")]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "remove",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--reason",
            "cleanup",
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    let error = error_json(&output);
    assert_eq!(error["error"]["code"], "internal_error");
    assert_text_contains(
        error["error"]["message"].as_str().expect("error message"),
        "service http error 500: retry later",
    );
    assert_text_contains(
        error["error"]["message"].as_str().expect("error message"),
        "local group E2EE pending commit retained for retry",
    );
    assert_eq!(provider_commands(&args_log), vec!["group remove-member"]);
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 1);
    let bodies = request_json_bodies(&server.requests());
    assert_eq!(rpc_methods(&bodies), vec!["group.e2ee.remove"]);
}

#[test]
fn group_remove_e2ee_finalize_failure_keeps_service_delivery_with_warning_like_go() {
    let workspace = TempDir::new("group-e2ee-remove-finalize-fails").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-remove-finalize-fails-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_remove_member_with_terminal(
        &fake_mls,
        &args_log,
        &stdin_log,
        TerminalBehavior::FinalizeFails,
    );
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(hidden_remove_delivery(
            "op-e2ee-remove-finalize-fails",
        ))),
        TestResponse::ok(&json_rpc_result(group_snapshot_after_remove())),
        TestResponse::ok(&json_rpc_result(group_members_after_remove())),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "remove",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--reason",
            "cleanup",
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_eq!(
        provider_commands(&args_log),
        vec!["group remove-member", "group commit-finalize"]
    );
    let envelope = success_json(&output);
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-e2ee-remove-finalize-fails"
    );
    assert_eq!(envelope["data"]["e2ee"]["mls_finalize"], Value::Null);
    assert_warning_contains_all(
        &envelope,
        &[
            "service accepted commit",
            "local finalize failed",
            "anp-mls error",
        ],
    );
    let bodies = request_json_bodies(&server.requests());
    assert_eq!(
        rpc_methods(&bodies),
        vec!["group.e2ee.remove", "group.get", "group.list_members"]
    );
}

fn assert_process_leave_request_remove_path(reason: Option<&str>, expected_reason: &str) {
    let suffix = reason.unwrap_or("default").replace(' ', "-");
    let workspace = TempDir::new(&format!("group-e2ee-process-leave-{suffix}")).expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new(&format!("group-e2ee-process-leave-bin-{suffix}")).expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_remove_member(&fake_mls, &args_log, &stdin_log);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(hidden_remove_delivery(&format!(
            "op-e2ee-process-leave-{suffix}"
        )))),
        TestResponse::ok(&json_rpc_result(group_snapshot_after_remove())),
        TestResponse::ok(&json_rpc_result(group_members_after_remove())),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let mut args = vec![
        "--identity",
        IDENTITY,
        "group",
        "e2ee",
        "process-leave-request",
        "--group",
        GROUP_DID,
        "--member",
        MEMBER_DID,
        "--leave-request-id",
        "  lr-bob-1  ",
    ];
    if let Some(reason) = reason {
        args.extend(["--reason", reason]);
    }
    let output = awiki_cmd_with_env(
        &args,
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_eq!(
        provider_commands(&args_log),
        vec!["group remove-member", "group commit-finalize"]
    );
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin[0]["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin[0]["params"]["member_did"], MEMBER_DID);
    assert_eq!(provider_stdin[1]["params"]["subject_did"], MEMBER_DID);
    assert_eq!(provider_stdin[1]["params"]["subject_status"], "removed");
    assert_eq!(provider_stdin[1]["params"]["group_did"], GROUP_DID);
    assert_eq!(
        provider_stdin[1]["params"]["commit_b64u"],
        "cmVtb3ZlLWNvbW1pdA"
    );
    assert_eq!(
        provider_stdin[1]["params"]["pending_commit_id"],
        "pc-remove-1"
    );
    assert_eq!(provider_stdin[1]["params"]["from_epoch"], 8);
    assert_eq!(provider_stdin[1]["params"]["to_epoch"], 9);
    assert!(provider_stdin[1]["params"]["operation_id"].is_null());

    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Processed group E2EE leave request with epoch-advancing remove"
    );
    assert_eq!(envelope["data"]["e2ee"]["subject_did"], MEMBER_DID);
    assert_eq!(envelope["data"]["e2ee"]["reason_text"], expected_reason);
    assert_eq!(envelope["data"]["leave_request_id"], "lr-bob-1");

    let bodies = request_json_bodies(&server.requests());
    let methods = rpc_methods(&bodies);
    assert_eq!(
        methods,
        vec!["group.e2ee.remove", "group.get", "group.list_members"]
    );
    assert!(
        !methods.contains(&"group.remove"),
        "process-leave-request must use the E2EE remove path, not P4 group.remove"
    );
    assert_eq!(bodies[0]["params"]["body"]["subject_did"], MEMBER_DID);
    assert_eq!(bodies[0]["params"]["body"]["subject_status"], "removed");
    assert_eq!(bodies[0]["params"]["body"]["leave_request_id"], "lr-bob-1");
    assert_eq!(bodies[0]["params"]["body"]["reason_text"], expected_reason);
}

fn group_snapshot_after_remove() -> Value {
    json!({
        "group_did": GROUP_DID,
        "group_state_version": "v9",
        "group_event_seq": 9,
        "group_profile": {
            "display_name": "Encrypted Group",
            "description": "Group E2EE remove contract",
            "slug": "encrypted-group"
        },
        "metadata": {
            "message_security_profile": "group-e2ee",
            "group_e2ee": {
                "group_state_version": "v9",
                "crypto_group_id_b64u": "Y3J5cHRv"
            }
        },
        "member_role": "owner",
        "member_status": "active",
        "member_count": 1,
        "source": "remote_http"
    })
}

fn group_members_after_remove() -> Value {
    json!({
        "members": [
            {
                "member_did": AGENT_DID,
                "member_handle": "alice.awiki.ai",
                "role": "owner",
                "status": "active",
                "joined_at": "2026-05-16T01:02:03Z"
            }
        ],
        "total": 1,
        "source": "remote_http"
    })
}

fn hidden_remove_delivery(operation_id: &str) -> Value {
    json!({
        "accepted": true,
        "group_did": GROUP_DID,
        "member_did": MEMBER_DID,
        "subject_did": MEMBER_DID,
        "operation_id": operation_id,
        "group_event_seq": 9,
        "group_state_version": "v9",
        "source": "remote_http"
    })
}

fn write_fake_anp_mls_group_remove_member(path: &Path, args_log: &Path, stdin_log: &Path) {
    write_fake_anp_mls_group_remove_member_with_terminal(
        path,
        args_log,
        stdin_log,
        TerminalBehavior::FinalizeSucceeds,
    );
}

#[derive(Copy, Clone)]
enum TerminalBehavior {
    FinalizeSucceeds,
    FinalizeFails,
    AbortSucceeds,
}

fn write_fake_anp_mls_group_remove_member_with_terminal(
    path: &Path,
    args_log: &Path,
    stdin_log: &Path,
    terminal_behavior: TerminalBehavior,
) {
    let remove_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-remove-test",
        "result": {
            "crypto_group_id_b64u": "Y3J5cHRv",
            "from_epoch": 8,
            "to_epoch": 9,
            "epoch": 9,
            "pending_commit_id": "pc-remove-1",
            "operation_id": "op-mls-remove-prepare",
            "subject_did": MEMBER_DID,
            "subject_status": "removed",
            "commit_b64u": "cmVtb3ZlLWNvbW1pdA",
            "ratchet_tree_b64u": "cmF0Y2hldA",
            "group_info_b64u": "Z3JvdXAtaW5mbw",
            "epoch_authenticator_b64u": "YXV0aDk",
            "last_handshake_digest": "cmVtb3ZlLWhhbmRzaGFrZQ",
            "group_state_ref": {
                "group_did": GROUP_DID,
                "group_state_version": "v9",
                "group_event_seq": 9,
                "epoch": 9
            },
            "application_plaintext": "must-not-leak",
            "provider_private_material": "must-not-leak"
        }
    })
    .to_string();
    let finalize_ok_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-finalize-test",
        "result": {
            "crypto_group_id_b64u": "Y3J5cHRv",
            "epoch": 9,
            "finalized_commit_id": "fc-remove-1",
            "epoch_authenticator_b64u": "YXV0aDk",
            "group_state_ref": {
                "group_did": GROUP_DID,
                "group_state_version": "v9",
                "group_event_seq": 9,
                "epoch": 9
            }
        }
    })
    .to_string();
    let finalize_fail_response = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-finalize-test",
        "error": {
            "code": "finalize-failed",
            "message": "local finalize unavailable"
        }
    })
    .to_string();
    let abort_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-abort-test",
        "result": {
            "pending_commit_id": "pc-remove-1",
            "aborted": true,
            "group_did": GROUP_DID
        }
    })
    .to_string();
    let wrong_command = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-remove-test",
        "error": {
            "code": "wrong-command",
            "message": "expected group remove-member, group commit-finalize, or group commit-abort"
        }
    })
    .to_string();
    let finalize_response = match terminal_behavior {
        TerminalBehavior::FinalizeSucceeds | TerminalBehavior::AbortSucceeds => {
            finalize_ok_response.as_str()
        }
        TerminalBehavior::FinalizeFails => finalize_fail_response.as_str(),
    };
    let script = format!(
        r#"#!/bin/sh
printf '%s %s\n' "$1" "$2" >> {args_log}
body=$(cat)
printf '%s\n' "$body" >> {stdin_log}
if [ "$1" = "group" ] && [ "$2" = "remove-member" ]; then
  printf '%s\n' {remove_response}
  exit 0
fi
if [ "$1" = "group" ] && [ "$2" = "commit-finalize" ]; then
  printf '%s\n' {finalize_response}
  {finalize_exit}
fi
if [ "$1" = "group" ] && [ "$2" = "commit-abort" ]; then
  printf '%s\n' {abort_response}
  exit 0
fi
printf '%s\n' {wrong_command}
exit 2
"#,
        args_log = shell_quote_path(args_log),
        stdin_log = shell_quote_path(stdin_log),
        remove_response = shell_quote(&remove_response),
        finalize_response = shell_quote(finalize_response),
        finalize_exit = match terminal_behavior {
            TerminalBehavior::FinalizeFails => "exit 2",
            TerminalBehavior::FinalizeSucceeds | TerminalBehavior::AbortSucceeds => "exit 0",
        },
        abort_response = shell_quote(&abort_response),
        wrong_command = shell_quote(&wrong_command),
    );
    std::fs::write(path, script).expect("write fake anp-mls");
    make_executable(path);
}

fn register_ready_group_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    let create = awiki_cmd(
        &[
            "id",
            "create",
            "--name",
            "Group User",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let index_path = workspace.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let did = format!("did:wba:awiki.ai:{handle}:e1_{handle}");
    index["credentials"][identity_name]["did"] = json!(did);
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}.awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = workspace.join("identities").join(dir_name);
    let identity_path = identity_dir.join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    let original_did = identity["did"].as_str().unwrap().to_string();
    identity["did"] = json!(did);
    identity["handle"] = json!(handle);
    identity["full_handle"] = json!(format!("{handle}.awiki.ai"));
    identity["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(
        &identity_path,
        serde_json::to_vec_pretty(&identity).unwrap(),
    )
    .unwrap();

    let document_path = identity_dir.join("did_document.json");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&document_path).unwrap()).unwrap();
    rewrite_did_document_ids(&mut document, &original_did, &did);
    std::fs::write(
        &document_path,
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();

    std::fs::write(
        identity_dir.join("auth.json"),
        serde_json::to_vec_pretty(&json!({ "jwt_token": jwt_token })).unwrap(),
    )
    .unwrap();
}

fn write_group_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n  anp_service_did: {SERVICE_DID}\n"
        ),
    )
    .unwrap();
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake anp-mls");
    }
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_with_env(args, workspace, &[])
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &Path)]) -> Output {
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
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_ANP_MLS_BINARY");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn error_json(output: &Output) -> Value {
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected failure; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on failure: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn provider_commands(args_log: &Path) -> Vec<String> {
    std::fs::read_to_string(args_log)
        .expect("read fake anp-mls args")
        .lines()
        .map(str::to_string)
        .collect()
}

fn provider_stdin_jsonl(stdin_log: &Path) -> Vec<Value> {
    std::fs::read_to_string(stdin_log)
        .expect("read fake anp-mls stdin")
        .lines()
        .map(|line| serde_json::from_str(line).expect("fake anp-mls stdin line should be JSON"))
        .collect()
}

fn assert_contains_member(members: &Value, member_did: &str) {
    assert!(
        members
            .as_array()
            .expect("members array")
            .iter()
            .any(|member| member["member_did"] == member_did),
        "members should contain {member_did}: {members}"
    );
}

fn assert_not_contains_member(members: &Value, member_did: &str) {
    assert!(
        !members
            .as_array()
            .expect("members array")
            .iter()
            .any(|member| member["member_did"] == member_did),
        "members should not contain {member_did}: {members}"
    );
}

fn assert_warning_contains_all(envelope: &Value, needles: &[&str]) {
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    let warning = warnings[0].as_str().expect("warning").to_ascii_lowercase();
    for needle in needles {
        assert!(
            warning.contains(&needle.to_ascii_lowercase()),
            "warning {warning:?} should contain {needle:?}"
        );
    }
}

fn assert_text_contains(text: &str, expected: &str) {
    assert!(
        text.contains(expected),
        "text {text:?} should contain {expected:?}"
    );
}

fn rewrite_did_document_ids(document: &mut Value, old_did: &str, new_did: &str) {
    if let Some(text) = document.as_str() {
        *document = Value::String(text.replace(old_did, new_did));
        return;
    }
    if let Some(array) = document.as_array_mut() {
        for value in array {
            rewrite_did_document_ids(value, old_did, new_did);
        }
        return;
    }
    if let Some(object) = document.as_object_mut() {
        for value in object.values_mut() {
            rewrite_did_document_ids(value, old_did, new_did);
        }
    }
}

fn request_json_bodies(requests: &[String]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| serde_json::from_str(request_body(request)).expect("json body"))
        .collect()
}

fn rpc_methods(bodies: &[Value]) -> Vec<&str> {
    bodies
        .iter()
        .map(|body| body["method"].as_str().expect("rpc method"))
        .collect()
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

    fn status(status: u16, body: &str) -> Self {
        Self {
            status,
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
                let stream = accept_with_timeout(&listener);
                let Some(stream) = stream else {
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
                .find_map(|line| line.strip_prefix("Content-Length: "))
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
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{prefix}-{}-{nanos}",
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
