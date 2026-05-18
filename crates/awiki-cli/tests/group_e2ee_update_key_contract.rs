use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-e2ee-update";
const AGENT_DID: &str = "did:wba:awiki.ai:alice:e1_alice";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:update:e1_group";
const MEMBER_DID: &str = "did:wba:awiki.ai:bob:e1_bob";
const SERVICE_DID: &str = "did:wba:awiki.ai:service:e1_message";

#[test]
fn group_e2ee_update_key_live_leases_update_key_package_prepares_hidden_update_and_finalizes_without_p4_add(
) {
    let workspace = TempDir::new("group-e2ee-update-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-update-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_update_key(&fake_mls, &args_log, &stdin_log);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": GROUP_DID,
            "epoch": "5",
            "actor_membership_role": "owner",
            "actor_membership_status": "active"
        }))),
        TestResponse::ok(&json_rpc_result(update_key_package())),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "operation_id": "op-e2ee-update",
            "epoch": "6",
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "update-key",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--device",
            "bob-main",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_eq!(
        provider_commands(&args_log),
        vec![
            "group update-member-prepare",
            "group update-member-finalize"
        ]
    );
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 2);
    assert_eq!(provider_stdin[0]["api_version"], "anp-mls/v1");
    assert_eq!(provider_stdin[0]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin[0]["device_id"], "default");
    assert_eq!(provider_stdin[0]["params"]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin[0]["params"]["actor_did"], AGENT_DID);
    assert_eq!(provider_stdin[0]["params"]["device_id"], "default");
    assert_eq!(provider_stdin[0]["params"]["group_did"], GROUP_DID);
    assert_eq!(
        provider_stdin[0]["params"]["target"]["agent_did"],
        MEMBER_DID
    );
    assert_eq!(
        provider_stdin[0]["params"]["target"]["device_id"],
        "bob-main"
    );
    assert_eq!(provider_stdin[0]["params"]["target_did"], MEMBER_DID);
    assert_eq!(provider_stdin[0]["params"]["target_device_id"], "bob-main");
    assert_eq!(
        provider_stdin[0]["params"]["update_key_package_id"],
        "kp-bob-update"
    );
    assert_eq!(
        provider_stdin[0]["params"]["group_key_package"]["purpose"],
        "update"
    );
    assert_eq!(
        provider_stdin[0]["params"]["group_key_package"]["device_id"],
        "bob-main"
    );
    assert_eq!(
        provider_stdin[0]["params"]["update_operation_purpose"],
        "same-did-device-key-rotation"
    );
    assert!(
        provider_stdin[0]["params"]["target_key_package"]
            .get("private_key_package_b64u")
            .is_none(),
        "provider stdin must not receive private KeyPackage material"
    );
    assert_eq!(provider_stdin[1]["params"]["group_did"], GROUP_DID);
    assert_eq!(
        provider_stdin[1]["params"]["commit_b64u"],
        "dXBkYXRlLWNvbW1pdA"
    );
    assert_eq!(
        provider_stdin[1]["params"]["pending_commit_id"],
        "pc-update-1"
    );
    assert_eq!(provider_stdin[1]["params"]["from_epoch"], "5");
    assert_eq!(provider_stdin[1]["params"]["to_epoch"], "6");

    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Updated group E2EE member key without P4 membership mutation"
    );
    assert!(envelope.get("warnings").is_none());
    assert_eq!(envelope["data"]["group"], GROUP_DID);
    assert_eq!(envelope["data"]["member"]["did"], MEMBER_DID);
    assert_eq!(envelope["data"]["target"]["agent_did"], MEMBER_DID);
    assert_eq!(envelope["data"]["target"]["device_id"], "bob-main");
    assert_eq!(
        envelope["data"]["update_key_package"]["key_package_id"],
        "kp-bob-update"
    );
    assert_eq!(
        envelope["data"]["update_key_package"]["private_material"],
        false
    );
    assert_eq!(
        envelope["data"]["mls_prepare"]["pending_commit_id"],
        "pc-update-1"
    );
    assert_eq!(
        envelope["data"]["mls_finalize"]["finalized_commit_id"],
        "fc-update-1"
    );
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-e2ee-update"
    );
    assert_eq!(envelope["data"]["p4_membership_mutate"], false);
    assert_eq!(envelope["data"]["argv_sensitive_fields"], "stdin-json-only");
    assert_eq!(envelope["data"]["hidden_awiki_extension"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "group.e2ee.update_key");
    assert_eq!(envelope["data"]["plan"]["device"], "bob-main");

    let bodies = request_json_bodies(&server.requests());
    let methods = rpc_methods(&bodies);
    assert_eq!(
        methods,
        vec![
            "group.e2ee.head",
            "group.e2ee.get_key_package",
            "group.e2ee.update"
        ]
    );
    assert!(
        !methods.contains(&"group.add"),
        "update-key must not call public P4 group.add"
    );
    assert!(
        !methods.contains(&"group.e2ee.recover_member"),
        "update-key must not call recover-member"
    );
    assert_eq!(bodies[0]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(
        bodies[1]["params"]["meta"]["security_profile"],
        "transport-protected"
    );
    assert_eq!(
        bodies[1]["params"]["meta"]["target"],
        json!({"kind": "service", "did": SERVICE_DID})
    );
    assert_eq!(bodies[1]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[1]["params"]["body"]["target_did"], MEMBER_DID);
    assert_eq!(bodies[1]["params"]["body"]["device_id"], "bob-main");
    assert_eq!(bodies[1]["params"]["body"]["purpose"], "update");
    assert_eq!(bodies[2]["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(
        bodies[2]["params"]["meta"]["security_profile"],
        "group-e2ee"
    );
    assert_eq!(
        bodies[2]["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(bodies[2]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(
        bodies[2]["params"]["body"]["target"]["agent_did"],
        MEMBER_DID
    );
    assert_eq!(
        bodies[2]["params"]["body"]["target"]["device_id"],
        "bob-main"
    );
    assert_eq!(
        bodies[2]["params"]["body"]["update_key_package_id"],
        "kp-bob-update"
    );
    assert!(
        bodies[2]["params"]["body"]
            .get("recovery_key_package_id")
            .is_none(),
        "update body must not carry recovery_key_package_id"
    );
    assert!(
        bodies[2]["params"]["body"].get("member_did").is_none(),
        "update body must not carry P4 member_did"
    );
    assert!(
        bodies[2]["params"]["body"].get("role").is_none(),
        "update body must not carry P4 role"
    );
    assert!(
        bodies[2]["params"]["body"]["group_key_package"]
            .get("private_key_package_b64u")
            .is_none(),
        "hidden update RPC must not leak private KeyPackage material"
    );
}

#[test]
fn group_e2ee_update_key_deterministic_submit_failure_aborts_pending_update_like_go() {
    let workspace = TempDir::new("group-e2ee-update-submit-403").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-update-submit-403-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_update_key_with_terminal(
        &fake_mls,
        &args_log,
        &stdin_log,
        TerminalBehavior::AbortSucceeds,
    );
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": GROUP_DID,
            "epoch": "5",
            "actor_membership_role": "owner",
            "actor_membership_status": "active"
        }))),
        TestResponse::ok(&json_rpc_result(update_key_package())),
        TestResponse::status(403, "deterministic rejection"),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "update-key",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--device",
            "bob-main",
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
        "local group E2EE update-key pending commit aborted",
    );
    assert_eq!(
        provider_commands(&args_log),
        vec!["group update-member-prepare", "group update-member-abort"]
    );
    let provider_stdin = provider_stdin_jsonl(&stdin_log);
    assert_eq!(provider_stdin.len(), 2);
    assert_eq!(provider_stdin[1]["params"]["group_did"], GROUP_DID);
    assert_eq!(
        provider_stdin[1]["params"]["commit_b64u"],
        "dXBkYXRlLWNvbW1pdA"
    );
    assert_eq!(
        provider_stdin[1]["params"]["pending_commit_id"],
        "pc-update-1"
    );
    assert_eq!(provider_stdin[1]["params"]["from_epoch"], "5");
    assert_eq!(provider_stdin[1]["params"]["to_epoch"], "6");
    assert!(provider_stdin[1]["params"]["operation_id"].is_null());

    let bodies = request_json_bodies(&server.requests());
    assert_eq!(
        rpc_methods(&bodies),
        vec![
            "group.e2ee.head",
            "group.e2ee.get_key_package",
            "group.e2ee.update"
        ]
    );
}

#[test]
fn group_e2ee_update_key_finalize_failure_keeps_service_delivery_with_warning_like_go() {
    let workspace = TempDir::new("group-e2ee-update-finalize-fails").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-update-finalize-fails-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.jsonl");
    write_fake_anp_mls_group_update_key_with_terminal(
        &fake_mls,
        &args_log,
        &stdin_log,
        TerminalBehavior::FinalizeFails,
    );
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": GROUP_DID,
            "epoch": "5",
            "actor_membership_role": "owner",
            "actor_membership_status": "active"
        }))),
        TestResponse::ok(&json_rpc_result(update_key_package())),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "operation_id": "op-e2ee-update-finalize-fails",
            "epoch": "6",
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "update-key",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--device",
            "bob-main",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_eq!(
        provider_commands(&args_log),
        vec![
            "group update-member-prepare",
            "group update-member-finalize"
        ]
    );
    let envelope = success_json(&output);
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-e2ee-update-finalize-fails"
    );
    assert_eq!(envelope["data"]["mls_finalize"], Value::Null);
    assert_warning_contains_all(
        &envelope,
        &[
            "update-key accepted by service",
            "local finalize failed",
            "anp-mls error",
        ],
    );

    let bodies = request_json_bodies(&server.requests());
    assert_eq!(
        rpc_methods(&bodies),
        vec![
            "group.e2ee.head",
            "group.e2ee.get_key_package",
            "group.e2ee.update"
        ]
    );
}

fn update_key_package() -> Value {
    json!({
        "leased": true,
        "lease_id": "lease-bob-update",
        "group_key_package": service_leased_update_key_package(),
        "key_package": service_leased_update_key_package(),
        "key_package_id": "kp-bob-update",
        "expires_at": "2030-01-01T00:00:00Z"
    })
}

fn service_leased_update_key_package() -> Value {
    json!({
        "owner_did": MEMBER_DID,
        "device_id": "bob-main",
        "key_package_id": "kp-bob-update",
        "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
        "mls_key_package_b64u": "bWxzLXVwZGF0ZS1rZXktcGFja2FnZQ",
        "did_wba_binding": {
            "agent_did": MEMBER_DID,
            "device_id": "bob-main",
            "leaf_signature_key_b64u": "bGVhZg",
            "issued_at": "2026-01-01T00:00:00Z",
            "expires_at": "2030-01-01T00:00:00Z",
            "proof": {
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "created": "2026-05-16T00:00:00Z",
                "proofValue": "zproof-bob-update"
            }
        },
        "expires_at": "2030-01-01T00:00:00Z",
        "purpose": "update",
        "private_key_package_b64u": "must-not-leak"
    })
}

fn write_fake_anp_mls_group_update_key(path: &Path, args_log: &Path, stdin_log: &Path) {
    write_fake_anp_mls_group_update_key_with_terminal(
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

fn write_fake_anp_mls_group_update_key_with_terminal(
    path: &Path,
    args_log: &Path,
    stdin_log: &Path,
    terminal_behavior: TerminalBehavior,
) {
    let update_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-update-test",
        "result": {
            "operation_id": "op-mls-update-prepare",
            "pending_commit_id": "pc-update-1",
            "update_key_package_id": "kp-bob-update",
            "crypto_group_id_b64u": "Y3J5cHRv",
            "from_epoch": "5",
            "to_epoch": "6",
            "epoch": "6",
            "epoch_authenticator_b64u": "YXV0aDY",
            "commit_b64u": "dXBkYXRlLWNvbW1pdA",
            "welcome_b64u": "dXBkYXRlLXdlbGNvbWU",
            "ratchet_tree_b64u": "cmF0Y2hldA",
            "group_info_b64u": "Z3JvdXAtaW5mbw",
            "group_state_ref": {
                "group_did": GROUP_DID,
                "group_state_version": "v6",
                "group_event_seq": 6,
                "epoch": "6"
            },
            "application_plaintext": "must-not-leak",
            "provider_private_material": "must-not-leak"
        }
    })
    .to_string();
    let finalize_ok_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-update-finalize-test",
        "result": {
            "crypto_group_id_b64u": "Y3J5cHRv",
            "epoch": "6",
            "finalized_commit_id": "fc-update-1",
            "epoch_authenticator_b64u": "YXV0aDY",
            "group_state_ref": {
                "group_did": GROUP_DID,
                "group_state_version": "v6",
                "group_event_seq": 6,
                "epoch": "6"
            }
        }
    })
    .to_string();
    let finalize_fail_response = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-update-finalize-test",
        "error": {
            "code": "finalize-failed",
            "message": "local update finalize unavailable"
        }
    })
    .to_string();
    let abort_response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-update-abort-test",
        "result": {
            "pending_commit_id": "pc-update-1",
            "aborted": true,
            "group_did": GROUP_DID
        }
    })
    .to_string();
    let wrong_command = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-update-test",
        "error": {
            "code": "wrong-command",
            "message": "expected group update-member-prepare, group update-member-finalize, or group update-member-abort"
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
if [ "$1" = "group" ] && [ "$2" = "update-member-prepare" ]; then
  printf '%s\n' {update_response}
  exit 0
fi
if [ "$1" = "group" ] && [ "$2" = "update-member-finalize" ]; then
  printf '%s\n' {finalize_response}
  {finalize_exit}
fi
if [ "$1" = "group" ] && [ "$2" = "update-member-abort" ]; then
  printf '%s\n' {abort_response}
  exit 0
fi
printf '%s\n' {wrong_command}
exit 2
"#,
        args_log = shell_quote_path(args_log),
        stdin_log = shell_quote_path(stdin_log),
        update_response = shell_quote(&update_response),
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
