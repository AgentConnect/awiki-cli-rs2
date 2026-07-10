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

use support::{tenant_workspace, write_default_tenant_registry, write_tenant_config};

#[test]
fn identity_register_phone_otp_live_posts_register_and_persists_identity_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","message":"Registration successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#,
    )]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            " Alice ",
            "--phone",
            "13800138000",
            "--otp",
            " 12 34 56 ",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Handle alice.awiki.ai registered successfully"
    );
    assert_eq!(envelope["data"]["action"], "register_handle");
    assert_eq!(envelope["data"]["full_handle"], "alice.awiki.ai");
    assert_eq!(envelope["data"]["method"], "phone");
    assert_eq!(envelope["data"]["verification_state"], "completed");
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    assert_eq!(
        envelope["data"]["identity"]["full_handle"],
        "alice.awiki.ai"
    );
    let registered_did = envelope["data"]["identity"]["did"]
        .as_str()
        .expect("registered identity did")
        .to_string();
    assert!(
        registered_did.starts_with("did:wba:awiki.ai:alice:e1_"),
        "registration should persist the locally generated key-bound DID: {registered_did}"
    );
    assert!(envelope["data"]["identity"]["has_jwt"]
        .as_bool()
        .expect("identity has_jwt bool"));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], "req-1");
    assert_eq!(body["method"], "register");
    assert_eq!(body["params"]["handle"], "alice");
    assert_eq!(body["params"]["phone"], "+8613800138000");
    assert_eq!(body["params"]["otp_code"], "123456");
    assert!(body["params"]["did_document"].is_object());
    assert_eq!(body["params"]["did_document"]["id"], registered_did);
    assert!(body["params"].get("invite_code").is_none());

    let stored = read_stored_identity(workspace.path(), "alice");
    assert_eq!(stored.index["handle"], "alice");
    assert_eq!(stored.index["full_handle"], "alice.awiki.ai");
    assert_eq!(stored.index["did"], registered_did);
    assert_eq!(stored.index["user_id"], "user-alice");
    assert_eq!(stored.identity["handle"], "alice");
    assert_eq!(stored.identity["full_handle"], "alice.awiki.ai");
    assert_eq!(stored.identity["did"], registered_did);
    assert_eq!(stored.identity["user_id"], "user-alice");
    assert_eq!(stored.auth, Value::Null);
    assert_vault_identity_has_no_plaintext_secret_files(workspace.path(), "alice");
}

#[test]
fn identity_refresh_token_live_posts_signed_get_me_and_persists_jwt_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","message":"Registration successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#,
        ),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"access_token":"fresh-token","handle":"alice"},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());

    let register = awiki_cmd(
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
    );
    assert_success(&register);
    let output = awiki_cmd(
        &["--identity", "alice", "id", "refresh-token"],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "JWT refreshed for identity alice");
    assert_eq!(envelope["data"]["action"], "refresh_token");
    assert_eq!(envelope["data"]["previous_token_present"], true);
    assert_eq!(
        envelope["data"]["auth_flow"],
        "did_auth_get_me_without_stored_bearer"
    );
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    assert!(envelope["data"]["identity"]["has_jwt"]
        .as_bool()
        .expect("identity has_jwt bool"));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert_header_absent(&requests[1], "Authorization", "Bearer jwt-register");
    assert_contains_text(&requests[1], "Signature-Input:");
    assert_contains_text(&requests[1], "Signature:");
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

    assert_vault_identity_has_no_plaintext_secret_files(workspace.path(), "alice");
}

#[test]
fn identity_bind_phone_without_otp_live_posts_authenticated_phone_bind_send_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(r#"{"sent":true}"#),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "id",
            "bind",
            "--phone",
            "13800138000",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Phone binding OTP sent");
    assert_eq!(envelope["data"]["action"], "send_bind_phone_otp");
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    assert_eq!(envelope["data"]["phone"], "+8613800138000");
    assert_eq!(envelope["data"]["verification_state"], "otp_sent");
    assert_eq!(envelope["data"]["result"], json!({ "sent": true }));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/auth/phone-bind-send HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body, json!({ "phone": "+8613800138000" }));
}

#[test]
fn identity_bind_phone_with_otp_live_posts_authenticated_phone_bind_verify_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(r#"{"bound":true}"#),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "id",
            "bind",
            "--phone",
            "13800138000",
            "--otp",
            " 12 34 56 ",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Phone bound successfully");
    assert_eq!(envelope["data"]["action"], "bind_phone");
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    assert_eq!(envelope["data"]["phone"], "+8613800138000");
    assert_eq!(envelope["data"]["verification_state"], "completed");
    assert_eq!(envelope["data"]["result"], json!({ "bound": true }));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/auth/phone-bind-verify HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body, json!({ "phone": "+8613800138000", "code": "123456" }));
}

#[test]
fn identity_bind_email_without_wait_live_checks_status_then_sends_authenticated_email_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(r#"{"email":"alice@example.com","verified":false}"#),
        TestResponse::ok(r#"{"message":"Activation email sent."}"#),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "id",
            "bind",
            "--email",
            "Alice@Example.COM",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Binding email sent");
    assert_eq!(envelope["data"]["action"], "send_bind_email");
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    assert_eq!(envelope["data"]["email"], "alice@example.com");
    assert_eq!(envelope["data"]["verification_state"], "email_sent");
    assert_eq!(
        envelope["data"]["result"],
        json!({ "message": "Activation email sent." })
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[1]
        .starts_with("GET /user-service/auth/email-status?email=alice%40example.com HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    assert!(
        !requests[1].contains("handle="),
        "bind email status request must not send a handle:\n{}",
        requests[1]
    );
    assert!(requests[2].starts_with("POST /user-service/auth/email-send HTTP/1.1"));
    assert_contains_text(&requests[2], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[2])).expect("request body");
    assert_eq!(body, json!({ "email": "alice@example.com" }));
}

#[test]
fn identity_bind_email_wait_already_verified_live_completes_without_sending_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"email":"alice@example.com","verified":true,"verified_at":"2026-01-01T00:00:00Z"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
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
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Email binding verified successfully");
    assert_eq!(envelope["data"]["action"], "bind_email");
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
    assert_eq!(envelope["data"]["email"], "alice@example.com");
    assert_eq!(envelope["data"]["verification_state"], "completed");

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1]
        .starts_with("GET /user-service/auth/email-status?email=alice%40example.com HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    assert!(
        requests
            .iter()
            .all(|request| !request.starts_with("POST /user-service/auth/email-send HTTP/1.1")),
        "already verified bind email --wait must not send an activation email:\n{requests:#?}"
    );
}

#[test]
fn identity_register_phone_without_otp_live_posts_send_otp_and_does_not_create_identity_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"sent":true},"id":"req-1"}"#,
    )]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "Alice",
            "--phone",
            "13800138000",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "OTP sent for handle alice.awiki.ai");
    assert_eq!(envelope["data"]["action"], "send_handle_otp");
    assert_eq!(envelope["data"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["handle"], "alice");
    assert_eq!(envelope["data"]["full_handle"], "alice.awiki.ai");
    assert_eq!(envelope["data"]["method"], "phone");
    assert_eq!(envelope["data"]["phone"], "+8613800138000");
    assert_eq!(envelope["data"]["verification_state"], "otp_sent");
    assert!(envelope["data"].get("result").is_none());

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /user-service/handle/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "send_otp",
            "params": { "phone": "+8613800138000" },
        })
    );
    assert!(
        !workspace
            .path()
            .join("tenants")
            .join("default")
            .join("identities")
            .join("index.json")
            .exists(),
        "send_otp should not create a local identity index"
    );
}

#[test]
fn identity_profile_set_live_posts_authenticated_update_me_and_persists_display_name_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r###"{"jsonrpc":"2.0","result":{"nick_name":"Alice Example","bio":"Rust port","tags":["rust","cli","parity"],"profile_md":"## Alice\nProfile"},"id":"req-1"}"###,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "id",
            "profile",
            "set",
            "--display-name",
            " Alice Example ",
            "--bio",
            " Rust port ",
            "--tags",
            " rust, ,cli, parity , ",
            "--markdown",
            "## Alice\nProfile",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Profile updated successfully");
    assert_eq!(envelope["data"]["action"], "update_profile");
    assert_eq!(
        envelope["data"]["changed_fields"],
        json!(["display_name", "bio", "tags", "profile_md"])
    );
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["display_name"], "alice");
    assert_eq!(envelope["data"]["profile"]["nick_name"], "Alice Example");

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
                "nick_name": "Alice Example",
                "bio": "Rust port",
                "tags": ["rust", "cli", "parity"],
                "profile_md": "## Alice\nProfile",
            },
        })
    );

    let stored = read_stored_identity(workspace.path(), "alice");
    assert_eq!(stored.identity["name"], "Alice Example");
    assert_eq!(stored.index["name"], "Alice Example");
}

#[test]
fn identity_profile_set_live_markdown_file_preserves_raw_nonblank_profile_md_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let markdown = " \n# Alice\n\nProfile body\n ";
    let markdown_path = workspace.path().join("profile.md");
    std::fs::write(&markdown_path, markdown).unwrap();
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"profile_md":" \n# Alice\n\nProfile body\n "},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "id",
            "profile",
            "set",
            "--markdown-file",
            markdown_path.to_str().unwrap(),
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Profile updated successfully");
    assert_eq!(envelope["data"]["changed_fields"], json!(["profile_md"]));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body["method"], "update_me");
    assert_eq!(body["params"], json!({ "profile_md": markdown }));
}

#[test]
fn identity_profile_set_live_empty_profile_fields_fail_before_remote_update_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![TestResponse::ok(register_alice_response())]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "id",
            "profile",
            "set",
            "--display-name",
            " \t ",
            "--bio",
            "\n ",
            "--tags",
            " \t ",
            "--markdown",
            " \n\t ",
        ],
        workspace.path(),
    );

    assert_code(&output, 2);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    let requests = server.requests();
    assert_eq!(
        requests.len(),
        1,
        "empty profile update must not call remote update endpoint; got requests:\n{requests:#?}"
    );
    let stored = read_stored_identity(workspace.path(), "alice");
    assert_eq!(stored.identity["name"], "alice");
    assert_eq!(stored.index["name"], "alice");
}

#[test]
fn identity_profile_get_self_live_posts_authenticated_get_me_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"nick_name":"Alice","bio":"Self profile"},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &["--identity", "alice", "id", "profile", "get", "--self"],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Fetched current identity profile");
    assert_eq!(envelope["data"]["subject"], "self");
    assert_eq!(envelope["data"]["profile"]["nick_name"], "Alice");

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
fn identity_profile_get_handle_live_resolves_handle_then_reads_public_profile_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","handle":"alice","full_handle":"alice.awiki.ai","domain":"awiki.ai","status":"active"},"id":"req-1"}"#,
        ),
        TestResponse::ok(r#"{"jsonrpc":"2.0","result":{"nick_name":"Alice Public"},"id":"req-1"}"#),
    ]);
    write_service_config(workspace.path(), &server.base_url());
    register_alice(workspace.path());

    let output = awiki_cmd(
        &["id", "profile", "get", "--handle", "Alice"],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Fetched public profile");
    assert_eq!(envelope["data"]["subject"]["handle"], "alice");
    assert_eq!(envelope["data"]["subject"]["full_handle"], "alice.awiki.ai");
    assert_eq!(
        envelope["data"]["subject"]["did"],
        "did:wba:awiki.ai:alice:e1_remote"
    );
    assert_eq!(envelope["data"]["profile"]["nick_name"], "Alice Public");

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].starts_with("POST /user-service/handle/rpc HTTP/1.1"));
    let lookup_body: Value =
        serde_json::from_str(request_body(&requests[1])).expect("lookup request body");
    assert_eq!(
        lookup_body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "lookup",
            "params": { "handle": "alice.awiki.ai" },
        })
    );
    assert!(requests[2].starts_with("POST /user-service/did/profile/rpc HTTP/1.1"));
    let profile_body: Value =
        serde_json::from_str(request_body(&requests[2])).expect("profile request body");
    assert_eq!(
        profile_body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "get_public_profile",
            "params": { "did": "did:wba:awiki.ai:alice:e1_remote" },
        })
    );
}

#[test]
fn identity_profile_get_did_live_without_identity_returns_unsupported_cutover() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "id",
            "profile",
            "get",
            "--did",
            "did:wba:awiki.ai:alice:e1_remote",
        ],
        workspace.path(),
    );

    assert_code(&output, 2);
    let envelope = error_json(&output);
    assert_unsupported_cutover(
        &envelope,
        "id.profile.get",
        "unauthenticated public profile lookup",
        "anonymous directory client support",
    );

    let requests = server.requests();
    assert!(
        requests.is_empty(),
        "unsupported anonymous profile lookup must not call remote service: {requests:#?}"
    );
}

#[test]
fn identity_resolve_handle_live_without_identity_returns_unsupported_cutover() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(&["id", "resolve", "--handle", "Alice"], workspace.path());

    assert_code(&output, 2);
    let envelope = error_json(&output);
    assert_unsupported_cutover(
        &envelope,
        "id.resolve",
        "unauthenticated directory resolve",
        "anonymous directory client support",
    );

    let requests = server.requests();
    assert!(
        requests.is_empty(),
        "unsupported anonymous resolve must not call remote service: {requests:#?}"
    );
}

#[test]
fn identity_resolve_did_live_without_identity_returns_unsupported_cutover() {
    let workspace = TempDir::new().expect("workspace");
    let did = "did:wba:awiki.ai:alice:e1_remote";
    let server = TestServer::new(vec![]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(&["id", "resolve", "--did", did], workspace.path());

    assert_code(&output, 2);
    let envelope = error_json(&output);
    assert_unsupported_cutover(
        &envelope,
        "id.resolve",
        "unauthenticated directory resolve",
        "anonymous directory client support",
    );

    let requests = server.requests();
    assert!(
        requests.is_empty(),
        "unsupported anonymous resolve must not call remote service: {requests:#?}"
    );
}

#[test]
fn identity_resolve_did_live_without_identity_does_not_issue_non_fatal_lookups() {
    let workspace = TempDir::new().expect("workspace");
    let did = "did:wba:awiki.ai:alice:e1_remote";
    let server = TestServer::new(vec![]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(&["id", "resolve", "--did", did], workspace.path());

    assert_code(&output, 2);
    let envelope = error_json(&output);
    assert_unsupported_cutover(
        &envelope,
        "id.resolve",
        "unauthenticated directory resolve",
        "anonymous directory client support",
    );

    let requests = server.requests();
    assert!(
        requests.is_empty(),
        "unsupported anonymous resolve must not call remote service: {requests:#?}"
    );
}

fn write_service_config(workspace: &Path, base_url: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(
        workspace,
        "services:\n  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n  anp_service_did: did:wba:awiki.ai\n",
    );
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
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
    command.output().expect("run awiki-cli binary")
}

fn register_alice(workspace: &Path) {
    let output = awiki_cmd(
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
        workspace,
    );
    assert_success(&output);
}

fn register_alice_response() -> &'static str {
    r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","message":"Registration successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
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
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn assert_unsupported_cutover(
    envelope: &Value,
    command: &str,
    capability: &str,
    required_phase: &str,
) {
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(envelope["error"]["details"]["capability"], capability);
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        required_phase
    );
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    let header_probe = needle.strip_suffix("\r\n").unwrap_or(needle);
    if let Some((header_name, expected_value)) = header_probe.split_once(':') {
        let header_name = header_name.trim();
        let expected_value = expected_value.trim();
        if !header_name.is_empty()
            && haystack.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.trim().eq_ignore_ascii_case(header_name)
                        && (expected_value.is_empty() || value.trim() == expected_value)
                })
            })
        {
            return;
        }
    }
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn assert_header_absent(haystack: &str, header_name: &str, expected_value: &str) {
    assert!(
        !haystack.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case(header_name) && value.trim().eq(expected_value)
            })
        }),
        "request must not contain {header_name}: {expected_value}:\n{haystack}"
    );
}

struct StoredIdentity {
    index: Value,
    identity: Value,
    auth: Value,
}

fn read_stored_identity(workspace: &Path, identity_name: &str) -> StoredIdentity {
    let tenant = tenant_workspace(workspace);
    let index_path = tenant.join("identities").join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let entry = index["credentials"][identity_name].clone();
    let dir_name = entry["dir_name"].as_str().unwrap();
    let identity_dir = tenant.join("identities").join(dir_name);
    StoredIdentity {
        index: entry,
        identity: serde_json::from_slice(
            &std::fs::read(identity_dir.join("identity.json")).unwrap(),
        )
        .unwrap(),
        auth: std::fs::read(identity_dir.join("auth.json"))
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).unwrap())
            .unwrap_or(Value::Null),
    }
}

fn assert_vault_identity_has_no_plaintext_secret_files(workspace: &Path, identity_name: &str) {
    let tenant = tenant_workspace(workspace);
    let index_path = tenant.join("identities").join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = tenant.join("identities").join(dir_name);
    for file in [
        "auth.json",
        "key-1-private.pem",
        "e2ee-signing-private.pem",
        "e2ee-agreement-private.pem",
    ] {
        assert!(
            !identity_dir.join(file).exists(),
            "vault_required identity must not persist plaintext {file}"
        );
    }
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
            "awiki-cli-rs2-identity-live-test-{}-{nanos}-{thread_id}-{counter}",
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
