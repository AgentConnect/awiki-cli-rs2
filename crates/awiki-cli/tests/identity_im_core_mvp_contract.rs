use base64::Engine;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{
    tenant_workspace, write_default_tenant_registry, write_ready_identity, write_tenant_config,
    TestIdentityOptions,
};

#[test]
fn identity_default_cutover_register_and_refresh_dry_run_keep_legacy_contract() {
    let workspace = TempDir::new().expect("workspace");

    let register = success_json(&awiki_cmd_with_env(
        &[
            "--dry-run",
            "--identity",
            "alice-local",
            "id",
            "register",
            "--handle",
            "Alice",
            "--phone",
            "+15551234567",
            "--otp",
            "123456",
            "--invite-code",
            "invite-1",
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(
        register["summary"],
        "Dry run: handle registration flow planned"
    );
    assert_eq!(register["data"]["plan"]["action"], "register_handle");
    assert_eq!(register["data"]["plan"]["identity_name"], "alice-local");
    assert_eq!(register["data"]["plan"]["full_handle"], "alice.awiki.ai");
    assert_eq!(register["data"]["plan"]["phone"], "+15551234567");
    assert!(register["data"]["plan"]["remote_calls"]
        .as_array()
        .unwrap()
        .contains(&json!("did-auth.register")));

    let refresh = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice-local",
            "id",
            "refresh-token",
            "--dry-run",
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(refresh["data"]["plan"]["action"], "refresh_token");
    assert_eq!(refresh["data"]["plan"]["identity_name"], "alice-local");
    assert_eq!(
        refresh["data"]["plan"]["auth_flow"],
        "did_auth_get_me_without_stored_bearer"
    );
}

#[test]
fn identity_default_cutover_refresh_selects_identity_before_legacy_auth() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    let server = TestServer::new(vec![TestResponse::legacy_get_me()]);
    write_service_config(&workspace_home, &server.base_url());
    write_ready_identity(
        &workspace_home,
        TestIdentityOptions {
            identity_name: "alice",
            handle: "alice",
            display_name: "Alice",
            jwt_token: "",
            make_default: true,
        },
    );
    write_ready_identity(
        &workspace_home,
        TestIdentityOptions {
            identity_name: "bob",
            handle: "bob",
            display_name: "Bob",
            jwt_token: "",
            make_default: false,
        },
    );
    success_json(&awiki_cmd_with_env(
        &["--identity", "bob", "--migration", "id", "vault", "migrate"],
        workspace.path(),
        &[],
    ));
    std::fs::remove_file(
        tenant_workspace(&workspace_home)
            .join("identities")
            .join("bob")
            .join("key-1-private.pem"),
    )
    .expect("remove bob private key");

    let result = success_json(&awiki_cmd_with_env(
        &["--identity", "bob", "id", "refresh-token"],
        workspace.path(),
        &[],
    ));
    assert_eq!(result["summary"], "JWT refreshed for identity bob");
    assert_eq!(result["data"]["identity"]["identity_name"], "bob");
    assert_eq!(result["data"]["identity"]["has_jwt"], true);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "get_me");
    assert_contains_text(&requests[0], "signature-input:");
    assert_contains_text(&requests[0], "did:wba:awiki.ai:user:bob:");
}

#[test]
fn identity_default_cutover_profile_get_self_routes_get_me_through_public_api() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::ok(
            r###"{"jsonrpc":"2.0","result":{"nick_name":"Alice Remote","bio":"Rust port","tags":["rust","cli"],"profile_md":"## Alice"},"id":"req-1"}"###,
        ),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let profile = success_json(&awiki_cmd_with_env(
        &["--identity", "alice", "id", "profile", "get", "--self"],
        workspace.path(),
        &[],
    ));
    assert_eq!(profile["summary"], "Fetched current identity profile");
    assert_eq!(profile["data"]["subject"], "self");
    assert_eq!(profile["data"]["profile"]["nick_name"], "Alice Remote");
    assert_eq!(profile["data"]["profile"]["profile_md"], "## Alice");

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_prekey_publication(&requests[1]);
    assert!(requests[2].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    assert_has_bearer(&requests[2]);
    let body: Value = serde_json::from_str(request_body(&requests[2])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "get_me",
            "params": {},
        })
    );
}

#[test]
fn identity_register_vault_required_persists_without_plaintext_secret_files() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    let root_key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let server = TestServer::new(vec![TestResponse::registration()]);
    write_service_config_with_secret_storage(&workspace_home, &server.base_url(), "vault_required");

    let register = success_json(&awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64", root_key)],
    ));
    assert_eq!(register["data"]["identity"]["identity_name"], "alice");
    assert_eq!(register["data"]["identity"]["has_jwt"], true);
    assert_eq!(register["data"]["identity"]["has_key1_private"], true);

    let identity_dir = tenant_workspace(&workspace_home)
        .join("identities")
        .join("alice");
    for file in [
        "auth.json",
        "key-1-private.pem",
        "e2ee-signing-private.pem",
        "e2ee-agreement-private.pem",
    ] {
        assert!(
            !identity_dir.join(file).exists(),
            "vault_required register must not persist plaintext {file}"
        );
    }
    let status = success_json(&awiki_cmd_with_env(
        &["id", "vault", "status"],
        workspace.path(),
        &[("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64", root_key)],
    ));
    assert_eq!(
        status["data"]["vault"]["identity"]["selected_backend"],
        "vault"
    );
    assert_eq!(
        status["data"]["vault"]["identity"]["plaintext_compat_retained"],
        false
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_prekey_publication(&requests[1]);
    let encoded = serde_json::to_string(&status).expect("status json");
    assert!(
        !encoded.contains(root_key)
            && !encoded.contains("\"access_token\"")
            && !encoded.contains("\"jwt_token\"")
            && !encoded.contains("-----BEGIN PRIVATE KEY-----"),
        "vault status must be redacted: {encoded}"
    );
}

#[test]
fn identity_default_cutover_profile_set_routes_update_me_through_public_api() {
    let workspace = TempDir::new().expect("workspace");
    let markdown_file = workspace.path().join("profile.md");
    std::fs::write(&markdown_file, " \n# Alice\n\nProfile body\n ").expect("write markdown");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::ok(
            r###"{"jsonrpc":"2.0","result":{"nick_name":"Alice Updated","bio":"Rust port","tags":["rust","cli"],"profile_md":" \n# Alice\n\nProfile body\n "},"id":"req-1"}"###,
        ),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let profile = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice",
            "id",
            "profile",
            "set",
            "--display-name",
            " Alice Updated ",
            "--bio",
            " Rust port ",
            "--tags",
            " rust, ,cli ",
            "--markdown-file",
            markdown_file.to_str().unwrap(),
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(profile["summary"], "Profile updated successfully");
    assert_eq!(profile["data"]["action"], "update_profile");
    assert_eq!(
        profile["data"]["changed_fields"],
        json!(["display_name", "bio", "tags", "profile_md"])
    );
    assert_eq!(profile["data"]["profile"]["nick_name"], "Alice Updated");
    assert_eq!(
        profile["data"]["profile"]["profile_md"],
        " \n# Alice\n\nProfile body\n "
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_prekey_publication(&requests[1]);
    assert!(requests[2].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    assert_has_bearer(&requests[2]);
    let body: Value = serde_json::from_str(request_body(&requests[2])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "update_me",
            "params": {
                "nick_name": "Alice Updated",
                "bio": "Rust port",
                "tags": ["rust", "cli"],
                "profile_md": " \n# Alice\n\nProfile body\n ",
            },
        })
    );
}

#[test]
fn identity_default_cutover_bind_phone_routes_authenticated_rest_through_bridge() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::ok(r#"{"sent":true}"#),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let bind = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice",
            "id",
            "bind",
            "--phone",
            "13800138001",
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(bind["summary"], "Phone binding OTP sent");
    assert_eq!(bind["data"]["action"], "send_bind_phone_otp");
    assert_eq!(bind["data"]["phone"], "+8613800138001");
    assert_eq!(bind["data"]["verification_state"], "otp_sent");

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_prekey_publication(&requests[1]);
    assert!(requests[2].starts_with("POST /user-service/auth/phone-bind-send HTTP/1.1"));
    assert_has_bearer(&requests[2]);
    let body: Value = serde_json::from_str(request_body(&requests[2])).expect("request body");
    assert_eq!(body, json!({ "phone": "+8613800138001" }));
}

#[test]
fn identity_default_cutover_bind_email_maps_sent_and_wait_completed_states() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::status(404, "not found"),
        TestResponse::ok(r#"{"sent":true}"#),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let sent = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice",
            "id",
            "bind",
            "--email",
            " Alice@Example.COM ",
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(sent["summary"], "Binding email sent");
    assert_eq!(sent["data"]["action"], "send_bind_email");
    assert_eq!(sent["data"]["email"], "alice@example.com");
    assert_eq!(sent["data"]["verification_state"], "email_sent");

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    assert_prekey_publication(&requests[1]);
    assert!(requests[2]
        .starts_with("GET /user-service/auth/email-status?email=alice%40example.com HTTP/1.1"));
    assert!(requests[3].starts_with("POST /user-service/auth/email-send HTTP/1.1"));
    let send_body: Value = serde_json::from_str(request_body(&requests[3])).expect("send body");
    assert_eq!(send_body, json!({ "email": "alice@example.com" }));
    assert_has_bearer(&requests[2]);
    assert_has_bearer(&requests[3]);

    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::ok(r#"{"verified":true,"verified_at":"2026-05-21T00:00:00Z"}"#),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let completed = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice",
            "id",
            "bind",
            "--email",
            "Alice@Example.COM",
            "--wait",
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(completed["summary"], "Email binding verified successfully");
    assert_eq!(completed["data"]["action"], "bind_email");
    assert_eq!(completed["data"]["verification_state"], "completed");
}

#[test]
fn identity_default_cutover_resolve_handle_routes_directory_sequence() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","status":"active"},"id":"req-1"}"#,
        ),
        TestResponse::ok(r#"{"jsonrpc":"2.0","result":{"nick_name":"Alice Public"},"id":"req-1"}"#),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","service_endpoint":"https://service.example"},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let resolve = success_json(&awiki_cmd_with_env(
        &["id", "resolve", "--handle", "Alice"],
        workspace.path(),
        &[],
    ));
    assert_eq!(resolve["summary"], "Identity resolved successfully");
    assert_eq!(
        resolve["data"]["lookup"]["did"],
        "did:wba:awiki.ai:alice:e1_remote"
    );
    assert_eq!(
        resolve["data"]["public_profile"]["nick_name"],
        "Alice Public"
    );
    assert_eq!(
        resolve["data"]["resolve"]["did"],
        "did:wba:awiki.ai:alice:e1_remote"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    assert_prekey_publication(&requests[1]);
    let lookup_body: Value = serde_json::from_str(request_body(&requests[2])).unwrap();
    let profile_body: Value = serde_json::from_str(request_body(&requests[3])).unwrap();
    let resolve_body: Value = serde_json::from_str(request_body(&requests[4])).unwrap();
    assert_eq!(lookup_body["method"], "lookup");
    assert_eq!(lookup_body["params"], json!({ "handle": "alice.awiki.ai" }));
    assert_eq!(profile_body["method"], "get_public_profile");
    assert_eq!(
        profile_body["params"],
        json!({ "did": "did:wba:awiki.ai:alice:e1_remote" })
    );
    assert_eq!(resolve_body["method"], "resolve");
    assert_eq!(
        resolve_body["params"],
        json!({ "did": "did:wba:awiki.ai:alice:e1_remote" })
    );
}

#[test]
fn identity_default_cutover_resolve_did_keeps_nonfatal_directory_warnings() {
    let workspace = TempDir::new().expect("workspace");
    let did = "did:wba:awiki.ai:alice:e1_remote";
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::ok(&format!(
            r#"{{"jsonrpc":"2.0","result":{{"did":"{did}","service_endpoint":"https://service.example"}},"id":"req-1"}}"#
        )),
        TestResponse::status(502, "lookup unavailable"),
        TestResponse::status(502, "profile unavailable"),
    ]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&register, 0);

    let resolve = success_json(&awiki_cmd_with_env(
        &["id", "resolve", "--did", did],
        workspace.path(),
        &[],
    ));
    assert_eq!(resolve["summary"], "Identity resolved successfully");
    assert_eq!(resolve["data"]["resolve"]["did"], did);
    assert!(resolve["data"].get("lookup").is_none());
    assert!(resolve["data"].get("public_profile").is_none());
    assert_eq!(
        resolve["warnings"],
        json!([
            "Handle lookup failed: service error 502: lookup unavailable",
            "Public profile lookup failed: service error 502: profile unavailable",
        ])
    );
}

#[test]
fn identity_default_cutover_replace_did_dry_run_returns_sdk_plan_without_remote_replace() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![TestResponse::registration()]);
    write_service_config(&workspace.path().join(".awiki-cli"), &server.base_url());

    let register = success_json(&awiki_cmd_with_env(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
        &[],
    ));
    let registered_did = register["data"]["identity"]["did"]
        .as_str()
        .expect("registered identity did")
        .to_string();
    assert!(
        registered_did.starts_with("did:wba:awiki.ai:user:alice:e1_"),
        "registration should persist the locally generated key-bound DID: {registered_did}"
    );

    let replace = success_json(&awiki_cmd_with_env(
        &[
            "--diagnostic",
            "--identity",
            "alice",
            "id",
            "replace-did",
            "--dry-run",
            "--is-public=false",
            "--role",
            "",
            "--endpoint-url",
            "https://example.com/agent",
        ],
        workspace.path(),
        &[],
    ));
    let plan = &replace["data"]["plan"];
    assert_eq!(replace["summary"], "Dry run: DID replacement planned");
    assert_eq!(plan["action"], "replace_did");
    assert_eq!(plan["dangerous"], true);
    assert_eq!(plan["identity"]["local_alias"], "alice");
    assert_eq!(plan["identity"]["did"], registered_did);
    assert_eq!(plan["backup_plan"]["required"], true);
    assert!(plan["backup_plan"]["backup_path_preview"]
        .as_str()
        .unwrap()
        .contains(".legacy-backup/replace-did/<timestamp>-alice-"));
    assert_eq!(
        plan["backup_plan"]["manifest_preview"]["old_did"],
        registered_did
    );
    assert_eq!(plan["local_rebind_plan"]["old_owner_did"], registered_did);
    assert_eq!(plan["local_rebind_plan"]["dry_run_only"], true);
    assert_eq!(
        plan["remote_replace_did_call_preview"]["method"],
        "replace_did"
    );
    assert_eq!(
        plan["remote_replace_did_call_preview"]["params"]["is_public"],
        false
    );
    assert_eq!(
        plan["remote_replace_did_call_preview"]["params"]["role"],
        Value::Null
    );
    assert_eq!(
        plan["remote_replace_did_call_preview"]["params"]["endpoint_url"],
        "https://example.com/agent"
    );
    assert_eq!(
        plan["affected_local_state"]["store_rebind_counts"]["messages"],
        0
    );
    assert!(plan["rollback_notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str().unwrap().contains("backup manifest")));
    assert!(replace["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning.as_str().unwrap().contains("Dangerous command")));

    let requests = server.requests();
    assert_eq!(
        requests.len(),
        2,
        "replace-did dry-run must not call remote replace_did"
    );
    assert_prekey_publication(&requests[1]);
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("HOME", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli")
}

fn write_service_config(workspace: &Path, base_url: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(
        workspace,
        "services:\n  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n  anp_service_did: did:wba:awiki.ai\n",
    );
}

fn write_service_config_with_secret_storage(workspace: &Path, base_url: &str, mode: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(
        workspace,
        &format!(
            concat!(
                "services:\n",
                "  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n",
                "  anp_service_did: did:wba:awiki.ai\n",
                "secret_storage:\n",
                "  mode: {}\n",
                "  workspace_id: test-workspace\n",
                "  device_id: test-device\n"
            ),
            mode
        ),
    );
}

fn json_rpc_result(result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": "req-1",
    })
    .to_string()
}

fn legacy_access_token(did: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    encode_test_token(json!({
        "iss": "user-service",
        "sub": did,
        "type": "access",
        "iat": now,
        "exp": now + 3600,
    }))
}

fn registration_response(request: &str) -> String {
    let rpc: Value =
        serde_json::from_str(request_body(request)).expect("registration request JSON");
    let params = &rpc["params"];
    let document = &params["did_document"];
    let did = document["id"]
        .as_str()
        .expect("registration DID document id");
    let device = &document["deviceManifest"]["devices"][0];
    let device_id = device["device_id"]
        .as_str()
        .expect("registration manifest device_id");
    let key_id = device["signing_key_id"]
        .as_str()
        .expect("registration manifest signing_key_id");
    let handle = params["handle"]
        .as_str()
        .expect("registration handle");
    let domain = did
        .strip_prefix("did:wba:")
        .and_then(|suffix| suffix.split(':').next())
        .expect("registration DID domain");
    let user_id = format!("user-{handle}");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let access_token = encode_test_token(json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": did,
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": did,
        "user_id": user_id,
        "device_id": device_id,
        "key_id": key_id,
        "auth_generation": 1,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "iat": now,
        "nbf": now,
        "exp": now + 3600,
        "jti": format!("registration-{device_id}"),
    }));
    json!({
        "jsonrpc": "2.0",
        "result": {
            "state": "registered",
            "did": did,
            "user_id": user_id,
            "message": "Registration successful",
            "access_token": access_token,
            "handle": handle,
            "domain": domain,
            "full_handle": format!("{handle}.{domain}"),
        },
        "id": rpc["id"].clone(),
    })
    .to_string()
}

fn legacy_get_me_response(request: &str) -> String {
    let signature_input = request
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("signature-input:"))
        .expect("signed get_me request signature-input");
    let key_id = signature_input
        .split_once("keyid=\"")
        .and_then(|(_, value)| value.split_once('"').map(|(key_id, _)| key_id))
        .expect("signed get_me request keyid");
    let did = key_id
        .strip_suffix("#key-1")
        .expect("legacy get_me signing key");
    json_rpc_result(json!({"access_token": legacy_access_token(did)}))
}

fn prekey_publication_response(request: &str) -> String {
    let rpc: Value = serde_json::from_str(request_body(request)).expect("P5 publish request JSON");
    let body = &rpc["params"]["body"];
    let bundle = &body["prekey_bundle"];
    let published_opk_count = body["one_time_prekeys"]
        .as_array()
        .map(Vec::len)
        .expect("P5 publish one_time_prekeys");
    json!({
        "jsonrpc": "2.0",
        "result": {
            "published": true,
            "owner_did": bundle["owner_did"].clone(),
            "owner_device_id": bundle["owner_device_id"].clone(),
            "bundle_id": bundle["bundle_id"].clone(),
            "published_at": "2026-07-25T00:00:00Z",
            "published_opk_count": published_opk_count,
        },
        "id": rpc["id"].clone(),
    })
    .to_string()
}

fn encode_test_token(claims: Value) -> String {
    format!(
        "e30.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialize test access token claims"))
    )
}

fn success_json(output: &Output) -> Value {
    assert_code(output, 0);
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("success JSON")
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

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    if let Some((header_name, expected_value)) = needle
        .strip_suffix("\r\n")
        .and_then(|line| line.split_once(':'))
    {
        let header_name = header_name.trim();
        let expected_value = expected_value.trim();
        if haystack.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case(header_name) && value.trim() == expected_value
            })
        }) {
            return;
        }
    }
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn assert_has_bearer(request: &str) {
    assert!(
        request.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case("authorization")
                    && value.trim().starts_with("Bearer ")
                    && value.trim().len() > "Bearer ".len()
            })
        }),
        "expected a non-empty bearer access token, got:\n{request}"
    );
}

fn assert_prekey_publication(request: &str) {
    assert!(
        request.starts_with("POST /im/rpc HTTP/1.1"),
        "registration must publish its P5 PreKey bundle through Message Service:\n{request}"
    );
    let body: Value = serde_json::from_str(request_body(request)).expect("P5 publish request body");
    assert_eq!(body["method"], "direct.e2ee.publish_prekey_bundle");
    assert!(body["params"].get("auth").is_none());
    assert_eq!(body["params"]["meta"]["target"]["kind"], "service");
}

#[derive(Clone)]
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

    fn registration() -> Self {
        Self::ok("__DYNAMIC_REGISTRATION_RESPONSE__")
    }

    fn prekey_publication() -> Self {
        Self::ok("__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__")
    }

    fn legacy_get_me() -> Self {
        Self::ok("__DYNAMIC_LEGACY_GET_ME_RESPONSE__")
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
                let follows_with_prekey = response.body == "__DYNAMIC_REGISTRATION_RESPONSE__";
                let stream = accept_with_timeout(&listener);
                let Some(stream) = stream else {
                    break;
                };
                handle_connection(stream, &server_requests, response);
                if follows_with_prekey {
                    let stream = accept_with_timeout(&listener);
                    let Some(stream) = stream else {
                        break;
                    };
                    handle_connection(
                        stream,
                        &server_requests,
                        TestResponse::prekey_publication(),
                    );
                }
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
    // The command under test performs real identity/vault bootstrap before its
    // first HTTP request. Keep the fake server alive across cold or contended
    // debug runs so host speed cannot masquerade as transport_unavailable.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("set test stream blocking");
                return Some(stream);
            }
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
    let body = if response.body == "__DYNAMIC_REGISTRATION_RESPONSE__" {
        registration_response(&request)
    } else if response.body == "__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__" {
        prekey_publication_response(&request)
    } else if response.body == "__DYNAMIC_LEGACY_GET_ME_RESPONSE__" {
        legacy_get_me_response(&request)
    } else {
        response.body
    };
    requests.lock().expect("requests mutex").push(request);
    let body_bytes = body.as_bytes();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        body_bytes.len(),
        body
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
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-id-im-core-test-{}-{nanos}-{thread_id}-{counter}",
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
