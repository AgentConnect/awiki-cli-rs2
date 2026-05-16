use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-e2ee-add";
const AGENT_DID: &str = "did:wba:awiki.ai:alice:e1_alice";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:demo:e1_group";
const MEMBER_DID: &str = "did:wba:awiki.ai:bob:e1_bob";
const SERVICE_DID: &str = "did:wba:awiki.ai:service:e1_message";

#[test]
fn group_add_e2ee_live_adds_member_leases_key_package_prepares_mls_and_publishes_hidden_add() {
    let workspace = TempDir::new("group-e2ee-add-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-add-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_group_add_member(&fake_mls, &args_log, &stdin_log, true, "", 0);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "member_did": MEMBER_DID,
            "role": "member",
            "operation_id": "op-add-member",
            "group_event_seq": 7,
            "group_state_version": "v7",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(group_snapshot_with_e2ee_object_metadata())),
        TestResponse::ok(&json_rpc_result(group_members())),
        TestResponse::ok(&json_rpc_result(leased_key_package())),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "member_did": MEMBER_DID,
            "operation_id": "op-e2ee-add",
            "group_event_seq": 8,
            "group_state_version": "v8",
            "source": "remote_http"
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "add",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_group_add_member(&args_log);
    let provider_stdin = provider_stdin_json(&stdin_log);
    assert_eq!(provider_stdin["api_version"], "anp-mls/v1");
    assert_eq!(provider_stdin["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin["device_id"], "default");
    assert_eq!(provider_stdin["params"]["agent_did"], AGENT_DID);
    assert_eq!(provider_stdin["params"]["device_id"], "default");
    assert_eq!(provider_stdin["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin["params"]["member_did"], MEMBER_DID);
    assert_eq!(
        provider_stdin["params"]["group_key_package"]["owner_did"],
        MEMBER_DID
    );
    assert_eq!(
        provider_stdin["params"]["group_key_package"]["purpose"],
        "normal"
    );
    assert!(
        provider_stdin["params"]["group_key_package"]
            .get("private_key_package_b64u")
            .is_none(),
        "provider stdin must receive only the service-leased public KeyPackage"
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Added member to group");
    assert!(envelope.get("warnings").is_none());
    assert_eq!(envelope["data"]["group"]["group_did"], GROUP_DID);
    assert_contains_member(&envelope["data"]["members"], AGENT_DID);
    assert_contains_member(&envelope["data"]["members"], MEMBER_DID);
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-add-member"
    );
    assert_eq!(envelope["data"]["member"]["did"], MEMBER_DID);
    assert_eq!(
        envelope["data"]["e2ee"]["mls"]["crypto_group_id_b64u"],
        "Y3J5cHRv"
    );
    assert_eq!(envelope["data"]["e2ee"]["mls"]["epoch"], 8);
    assert_eq!(
        envelope["data"]["e2ee"]["delivery"]["operation_id"],
        "op-e2ee-add"
    );
    assert_eq!(
        envelope["data"]["e2ee"]["leased_key_package"]["key_package_id"],
        "kp-bob-default"
    );
    assert!(
        envelope["data"]["e2ee"]["leased_key_package"]
            .get("private_key_package_b64u")
            .is_none(),
        "output must not expose private KeyPackage material"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    let bodies = request_json_bodies(&requests);
    assert_eq!(bodies[0]["method"], "group.add");
    assert_eq!(bodies[0]["params"]["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        bodies[0]["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(bodies[0]["params"]["body"]["member_did"], MEMBER_DID);
    assert_eq!(bodies[0]["params"]["body"]["role"], "member");
    assert_eq!(
        bodies[0]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
    assert_eq!(bodies[1]["method"], "group.get");
    assert_eq!(bodies[1]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[2]["method"], "group.list_members");
    assert_eq!(bodies[2]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[2]["params"]["body"]["limit"], 100);
    assert_eq!(bodies[3]["method"], "group.e2ee.get_key_package");
    assert_eq!(bodies[3]["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(
        bodies[3]["params"]["meta"]["security_profile"],
        "transport-protected"
    );
    assert_eq!(
        bodies[3]["params"]["meta"]["target"],
        json!({"kind": "service", "did": SERVICE_DID})
    );
    assert_eq!(bodies[3]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[3]["params"]["body"]["target_did"], MEMBER_DID);
    assert!(bodies[3]["params"]["body"].get("purpose").is_none());
    assert_eq!(bodies[4]["method"], "group.e2ee.add");
    assert_eq!(bodies[4]["params"]["meta"]["profile"], "anp.group.e2ee.v1");
    assert_eq!(
        bodies[4]["params"]["meta"]["security_profile"],
        "group-e2ee"
    );
    assert_eq!(
        bodies[4]["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(bodies[4]["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(bodies[4]["params"]["body"]["member_did"], MEMBER_DID);
    assert_eq!(bodies[4]["params"]["body"]["subject_did"], MEMBER_DID);
    assert_eq!(
        bodies[4]["params"]["body"]["key_package_id"],
        "kp-bob-default"
    );
    assert_eq!(
        bodies[4]["params"]["body"]["subject_key_package_id"],
        "kp-bob-default"
    );
    assert_eq!(
        bodies[4]["params"]["body"]["group_key_package"]["owner_did"],
        MEMBER_DID
    );
    assert!(
        bodies[4]["params"]["body"]["group_key_package"]
            .get("private_key_package_b64u")
            .is_none(),
        "hidden add RPC must not leak private KeyPackage material"
    );
    assert_eq!(
        bodies[4]["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

#[test]
fn group_add_live_to_e2ee_group_downgrades_provider_failure_after_p4_add_to_warning() {
    let workspace = TempDir::new("group-e2ee-add-warning").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-add-warning-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_group_add_member(
        &fake_mls,
        &args_log,
        &stdin_log,
        false,
        "simulated add-member failure",
        1,
    );
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "member_did": MEMBER_DID,
            "role": "member",
            "operation_id": "op-add-member",
            "group_event_seq": 7,
            "group_state_version": "v7",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(group_members())),
        TestResponse::ok(&json_rpc_result(leased_key_package())),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "add",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_group_add_member(&args_log);
    let provider_stdin = provider_stdin_json(&stdin_log);
    assert_eq!(provider_stdin["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin["params"]["member_did"], MEMBER_DID);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Added member to group");
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-add-member"
    );
    assert_eq!(envelope["data"]["group"]["group_did"], GROUP_DID);
    assert_contains_member(&envelope["data"]["members"], MEMBER_DID);
    assert_eq!(envelope["data"]["member"]["did"], MEMBER_DID);
    assert_eq!(
        envelope["data"]["e2ee"]["leased_key_package"]["key_package_id"],
        "kp-bob-default"
    );
    assert!(
        envelope["data"]["e2ee"].get("mls").is_none(),
        "provider failure should expose only the redacted leased KeyPackage summary"
    );
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0]
            .as_str()
            .expect("warning")
            .starts_with("Group E2EE MLS add-member failed:"),
        "unexpected warning: {warnings:?}"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    let bodies = request_json_bodies(&requests);
    assert_eq!(bodies[0]["method"], "group.add");
    assert_eq!(bodies[1]["method"], "group.get");
    assert_eq!(bodies[2]["method"], "group.list_members");
    assert_eq!(bodies[3]["method"], "group.e2ee.get_key_package");
}

#[test]
fn group_add_e2ee_key_package_lookup_failure_keeps_p4_add_successful_without_e2ee_data() {
    let workspace = TempDir::new("group-e2ee-add-lease-failure").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "member_did": MEMBER_DID,
            "operation_id": "op-add-member",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(group_members())),
        TestResponse::ok(&json_rpc_error(-32001, "no key package available")),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "group",
            "add",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--e2ee",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Added member to group");
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-add-member"
    );
    assert!(envelope["data"].get("e2ee").is_none());
    assert_warning_starts_with(&envelope, "Group E2EE member KeyPackage lookup failed:");

    let bodies = request_json_bodies(&server.requests());
    assert_eq!(bodies.len(), 4);
    assert_eq!(bodies[3]["method"], "group.e2ee.get_key_package");
}

#[test]
fn group_add_e2ee_hidden_add_delivery_failure_keeps_mls_and_warning() {
    let workspace = TempDir::new("group-e2ee-add-delivery-failure").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-add-delivery-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_group_add_member(&fake_mls, &args_log, &stdin_log, true, "", 0);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "member_did": MEMBER_DID,
            "operation_id": "op-add-member",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(group_members())),
        TestResponse::ok(&json_rpc_result(leased_key_package())),
        TestResponse::ok(&json_rpc_error(-32002, "hidden add rejected")),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            IDENTITY,
            "group",
            "add",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
            "--e2ee",
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_group_add_member(&args_log);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Added member to group");
    assert_eq!(envelope["data"]["e2ee"]["mls"]["epoch"], 8);
    assert_eq!(
        envelope["data"]["e2ee"]["leased_key_package"]["key_package_id"],
        "kp-bob-default"
    );
    assert!(envelope["data"]["e2ee"].get("delivery").is_none());
    assert_warning_starts_with(&envelope, "Group E2EE add delivery failed:");

    let bodies = request_json_bodies(&server.requests());
    assert_eq!(bodies.len(), 5);
    assert_eq!(bodies[4]["method"], "group.e2ee.add");
}

#[test]
fn group_e2ee_rejoin_live_delegates_to_group_add_e2ee_and_inserts_plan() {
    let workspace = TempDir::new("group-e2ee-rejoin-live").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let bin_dir = TempDir::new("group-e2ee-rejoin-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    let args_log = workspace.path().join("mls-args.log");
    let stdin_log = workspace.path().join("mls-stdin.json");
    write_fake_anp_mls_group_add_member(&fake_mls, &args_log, &stdin_log, true, "", 0);
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "member_did": MEMBER_DID,
            "role": "member",
            "operation_id": "op-rejoin-add",
            "group_event_seq": 9,
            "group_state_version": "v9",
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(group_snapshot())),
        TestResponse::ok(&json_rpc_result(group_members())),
        TestResponse::ok(&json_rpc_result(leased_key_package())),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "group_did": GROUP_DID,
            "member_did": MEMBER_DID,
            "operation_id": "op-e2ee-rejoin-add",
            "group_event_seq": 10,
            "group_state_version": "v10",
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
            "rejoin",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    assert_provider_called_group_add_member(&args_log);
    let provider_stdin = provider_stdin_json(&stdin_log);
    assert_eq!(provider_stdin["params"]["group_did"], GROUP_DID);
    assert_eq!(provider_stdin["params"]["member_did"], MEMBER_DID);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Updated group membership via add");
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-rejoin-add"
    );
    assert_eq!(
        envelope["data"]["e2ee"]["delivery"]["operation_id"],
        "op-e2ee-rejoin-add"
    );
    assert_eq!(envelope["data"]["plan"]["action"], "group.e2ee.rejoin");
    assert_eq!(
        envelope["data"]["plan"]["canonical_command"],
        "group add --e2ee"
    );
    assert_eq!(envelope["data"]["plan"]["key_package_purpose"], "normal");
    assert_eq!(envelope["data"]["plan"]["p4_membership_mutate"], true);

    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    let bodies = request_json_bodies(&requests);
    assert_eq!(bodies[0]["method"], "group.add");
    assert_eq!(bodies[1]["method"], "group.get");
    assert_eq!(bodies[2]["method"], "group.list_members");
    assert_eq!(bodies[3]["method"], "group.e2ee.get_key_package");
    assert_eq!(bodies[4]["method"], "group.e2ee.add");
}

#[test]
fn group_e2ee_rejoin_wrapper_plans_canonical_group_add_e2ee_when_current_cli_exposes_it() {
    let workspace = TempDir::new("group-e2ee-rejoin-wrapper").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    write_group_config(workspace.path(), "http://127.0.0.1:9");

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "group",
            "e2ee",
            "rejoin",
            "--dry-run",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Dry run: group e2ee rejoin planned");
    assert_eq!(envelope["data"]["plan"]["action"], "group.e2ee.rejoin");
    assert_eq!(
        envelope["data"]["plan"]["canonical_command"],
        "group add --e2ee"
    );
    assert_eq!(envelope["data"]["plan"]["group"], GROUP_DID);
    assert_eq!(envelope["data"]["plan"]["member"], MEMBER_DID);
    assert_eq!(envelope["data"]["plan"]["role"], "member");
    assert_eq!(envelope["data"]["plan"]["key_package_purpose"], "normal");
    assert_eq!(envelope["data"]["plan"]["p4_membership_mutate"], true);
}

fn group_snapshot() -> Value {
    json!({
        "group_did": GROUP_DID,
        "group_state_version": "v7",
        "group_event_seq": 7,
        "group_profile": {
            "display_name": "Encrypted Group",
            "description": "Group E2EE add contract",
            "slug": "encrypted-group"
        },
        "group_policy": {
            "message_security_profile": "group-e2ee",
            "bootstrap_security_profile": "group-e2ee",
            "admission_mode": "closed"
        },
        "member_role": "owner",
        "member_status": "active",
        "member_count": 2,
        "source": "remote_http"
    })
}

fn group_snapshot_with_e2ee_object_metadata() -> Value {
    json!({
        "group_did": GROUP_DID,
        "group_state_version": "v7",
        "group_event_seq": 7,
        "group_profile": {
            "display_name": "Encrypted Group",
            "description": "Group E2EE add contract",
            "slug": "encrypted-group"
        },
        "metadata": {
            "message_security_profile": "group-e2ee",
            "group_e2ee": {
                "group_state_version": "v7",
                "crypto_group_id_b64u": "Y3J5cHRv"
            }
        },
        "member_role": "owner",
        "member_status": "active",
        "member_count": 2,
        "source": "remote_http"
    })
}

fn group_members() -> Value {
    json!({
        "members": [
            {
                "member_did": AGENT_DID,
                "member_handle": "alice.awiki.ai",
                "role": "owner",
                "status": "active",
                "joined_at": "2026-05-16T01:02:03Z"
            },
            {
                "member_did": MEMBER_DID,
                "member_handle": "bob.awiki.ai",
                "role": "member",
                "status": "active",
                "joined_at": "2026-05-16T01:03:03Z"
            }
        ],
        "total": 2,
        "source": "remote_http"
    })
}

fn leased_key_package() -> Value {
    json!({
        "leased": true,
        "lease_id": "lease-bob-default",
        "group_key_package": service_leased_group_key_package(),
        "key_package": service_leased_group_key_package(),
        "key_package_id": "kp-bob-default",
        "expires_at": "2030-01-01T00:00:00Z"
    })
}

fn service_leased_group_key_package() -> Value {
    json!({
        "owner_did": MEMBER_DID,
        "device_id": "default",
        "key_package_id": "kp-bob-default",
        "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
        "mls_key_package_b64u": "bWxzLWtleS1wYWNrYWdl",
        "did_wba_binding": {
            "agent_did": MEMBER_DID,
            "device_id": "default",
            "leaf_signature_key_b64u": "bGVhZg",
            "issued_at": "2026-01-01T00:00:00Z",
            "expires_at": "2030-01-01T00:00:00Z",
            "proof": {
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "created": "2026-05-16T00:00:00Z",
                "proofValue": "zproof-bob-default"
            }
        },
        "expires_at": "2030-01-01T00:00:00Z",
        "purpose": "normal"
    })
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

fn write_fake_anp_mls_group_add_member(
    path: &Path,
    args_log: &Path,
    stdin_log: &Path,
    ok: bool,
    error_message: &str,
    exit_code: i32,
) {
    let response = if ok {
        json!({
            "ok": true,
            "api_version": "anp-mls/v1",
            "request_id": "group-e2ee-add-test",
            "result": {
                "crypto_group_id_b64u": "Y3J5cHRv",
                "epoch": 8,
                "epoch_authenticator_b64u": "YXV0aDg",
                "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
                "last_handshake_digest": "aGFuZHNoYWtl",
                "welcome_b64u": "d2VsY29tZQ",
                "commit_b64u": "Y29tbWl0",
                "ratchet_tree_b64u": "cmF0Y2hldA",
                "key_package_id": "kp-bob-default",
                "group_key_package": service_leased_group_key_package(),
                "group_state_ref": {
                    "group_did": GROUP_DID,
                    "group_state_version": "v8",
                    "group_event_seq": 8
                }
            }
        })
    } else {
        json!({
            "ok": false,
            "api_version": "anp-mls/v1",
            "request_id": "group-e2ee-add-test",
            "error": {
                "code": "add-member-failed",
                "message": error_message
            }
        })
    }
    .to_string();
    let wrong_command = json!({
        "ok": false,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-add-test",
        "error": {
            "code": "wrong-command",
            "message": "expected group add-member"
        }
    })
    .to_string();
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$@" > {args_log}
body=$(cat)
printf '%s\n' "$body" > {stdin_log}
if [ "$1" != "group" ] || [ "$2" != "add-member" ]; then
  printf '%s\n' {wrong_command}
  exit 2
fi
printf '%s\n' {response}
exit {exit_code}
"#,
        args_log = shell_quote_path(args_log),
        stdin_log = shell_quote_path(stdin_log),
        wrong_command = shell_quote(&wrong_command),
        response = shell_quote(&response),
        exit_code = exit_code,
    );
    std::fs::write(path, script).expect("write fake anp-mls");
    make_executable(path);
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

fn assert_provider_called_group_add_member(args_log: &Path) {
    let args = std::fs::read_to_string(args_log).expect("read fake anp-mls args");
    let lines = args.lines().collect::<Vec<_>>();
    assert!(
        lines.len() >= 2,
        "expected fake anp-mls args to include domain/action, got:\n{args}"
    );
    assert_eq!(lines[0], "group");
    assert_eq!(lines[1], "add-member");
    assert!(
        lines.windows(2).any(|window| window == ["--json-in", "-"]),
        "expected fake anp-mls args to include --json-in -, got:\n{args}"
    );
}

fn provider_stdin_json(stdin_log: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(stdin_log).expect("read fake anp-mls stdin"))
        .expect("fake anp-mls stdin should be JSON")
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

fn assert_warning_starts_with(envelope: &Value, prefix: &str) {
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].as_str().expect("warning").starts_with(prefix),
        "unexpected warning: {warnings:?}"
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

fn json_rpc_error(code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "error": {
            "code": code,
            "message": message,
        },
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
