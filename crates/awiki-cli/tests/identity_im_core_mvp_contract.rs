use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{write_ready_identity, TestIdentityOptions};

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
    std::fs::remove_file(
        workspace_home
            .join("identities")
            .join("bob")
            .join("key-1-private.pem"),
    )
    .expect("remove bob private key");

    let result = awiki_cmd_with_env(
        &["--identity", "bob", "id", "refresh-token"],
        workspace.path(),
        &[],
    );
    assert_code(&result, 3);
    let result = error_json(&result);
    assert_eq!(result["error"]["code"], "auth_required");
    assert!(result["error"]["message"].as_str().unwrap().contains("bob"));
}

#[test]
fn identity_default_cutover_profile_get_self_routes_get_me_through_public_api() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
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
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
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
fn identity_default_cutover_profile_set_routes_update_me_through_public_api() {
    let workspace = TempDir::new().expect("workspace");
    let markdown_file = workspace.path().join("profile.md");
    std::fs::write(&markdown_file, " \n# Alice\n\nProfile body\n ").expect("write markdown");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
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
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
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
        TestResponse::ok(register_alice_response()),
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
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/auth/phone-bind-send HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body, json!({ "phone": "+8613800138001" }));
}

#[test]
fn identity_default_cutover_bind_email_maps_sent_and_wait_completed_states() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
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
    assert_eq!(requests.len(), 3);
    assert!(requests[1]
        .starts_with("GET /user-service/auth/email-status?email=alice%40example.com HTTP/1.1"));
    assert!(requests[2].starts_with("POST /user-service/auth/email-send HTTP/1.1"));
    let send_body: Value = serde_json::from_str(request_body(&requests[2])).expect("send body");
    assert_eq!(send_body, json!({ "email": "alice@example.com" }));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    assert_contains_text(&requests[2], "Authorization: Bearer jwt-register\r\n");

    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
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
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","handle":"alice","full_handle":"alice.awiki.ai"},"id":"req-1"}"#,
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
    assert_eq!(requests.len(), 4);
    let lookup_body: Value = serde_json::from_str(request_body(&requests[1])).unwrap();
    let profile_body: Value = serde_json::from_str(request_body(&requests[2])).unwrap();
    let resolve_body: Value = serde_json::from_str(request_body(&requests[3])).unwrap();
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
        TestResponse::ok(register_alice_response()),
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
fn identity_default_cutover_recover_without_otp_routes_send_otp_through_bridge() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(r#"{"jsonrpc":"2.0","result":{"sent":true},"id":"req-1"}"#),
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

    let recover = success_json(&awiki_cmd_with_env(
        &[
            "id",
            "recover",
            "--handle",
            "Alice",
            "--phone",
            "13800138000",
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(
        recover["summary"],
        "OTP sent for handle alice.awiki.ai recovery"
    );
    assert_eq!(recover["data"]["action"], "send_recover_otp");
    assert_eq!(recover["data"]["phone"], "+8613800138000");
    assert_eq!(recover["data"]["verification_state"], "otp_sent");

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/handle/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body["method"], "send_otp");
    assert_eq!(body["params"], json!({ "phone": "+8613800138000" }));
}

#[test]
fn identity_default_cutover_recover_with_otp_routes_recover_handle_and_finalizes() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_recovered","user_id":"user-alice-recovered","message":"Recovery successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-recover"},"id":"req-1"}"#,
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

    let recover = success_json(&awiki_cmd_with_env(
        &[
            "id",
            "recover",
            "--handle",
            " Alice ",
            "--phone",
            "13800138000",
            "--otp",
            " 65 43 21 ",
        ],
        workspace.path(),
        &[],
    ));
    assert_eq!(
        recover["summary"],
        "Handle alice.awiki.ai recovered successfully"
    );
    assert_eq!(recover["data"]["action"], "recover_handle");
    assert_eq!(recover["data"]["final_identity_name"], "alice");
    assert_eq!(recover["data"]["archived_identities"], json!(["alice"]));
    assert_eq!(
        recover["data"]["identity"]["did"],
        "did:wba:awiki.ai:alice:e1_recovered"
    );
    assert!(recover["data"].get("temp_identity_name").is_none());
    assert!(recover["data"].get("old_dids").is_none());
    assert_eq!(
        recover["data"]["store_merge_counts"],
        json!({
            "messages": 0,
            "contacts": 0,
            "contact_handle_bindings": 0,
            "relationship_events": 0,
            "groups": 0,
            "group_members": 0,
        })
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body["method"], "recover_handle");
    assert_eq!(body["params"]["handle"], "alice.awiki.ai");
    assert_eq!(body["params"]["phone"], "+8613800138000");
    assert_eq!(body["params"]["otp_code"], "654321");
    assert!(body["params"]["did_document"].is_object());
}

#[test]
fn identity_default_cutover_replace_did_dry_run_returns_sdk_plan_without_remote_replace() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![TestResponse::ok(register_alice_response())]);
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
    assert_eq!(plan["identity"]["did"], "did:wba:awiki.ai:alice:e1_remote");
    assert_eq!(plan["backup_plan"]["required"], true);
    assert!(plan["backup_plan"]["backup_path_preview"]
        .as_str()
        .unwrap()
        .contains(".legacy-backup/replace-did/<timestamp>-alice-"));
    assert_eq!(
        plan["backup_plan"]["manifest_preview"]["old_did"],
        "did:wba:awiki.ai:alice:e1_remote"
    );
    assert_eq!(
        plan["local_rebind_plan"]["old_owner_did"],
        "did:wba:awiki.ai:alice:e1_remote"
    );
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
        1,
        "replace-did dry-run must not call remote replace_did"
    );
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
        .env_remove("AVIKI_FORMAT");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli")
}

fn write_service_config(workspace: &Path, base_url: &str) {
    std::fs::create_dir_all(workspace).unwrap();
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "services:\n  service_base_url: {base_url}\n  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n  anp_service_did: did:wba:awiki.ai\n"
        ),
    )
    .unwrap();
}

fn register_alice_response() -> &'static str {
    r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","message":"Registration successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#
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
            "awiki-cli-rs2-id-im-core-test-{}-{nanos}",
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
