use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{set_secret_storage_mode, tenant_config_path, tenant_workspace};

#[test]
fn msg_group_attachment_send_live_uploads_commits_and_group_sends_like_go() {
    let workspace = TempDir::new("attachment-live-send").expect("workspace");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let payload = b"live group attachment bytes\n";
    let attachment_path = workspace.path().join("demo.txt");
    std::fs::write(&attachment_path, payload).expect("attachment fixture");

    let server = TestServer::new(vec![
        TestResponse::json(&json_rpc_error(1401, "jwt expired")),
        TestResponse::json(&json_rpc_result(json!({
            "access_token": "jwt-alice-refreshed"
        }))),
        TestResponse::json(&json_rpc_result(json!({
            "attachment_id": "__REQUEST_ATTACHMENT_ID__",
            "slot_id": "slot-live-1",
            "upload_uri": "__BASE__/objects/upload/slot-live-1",
            "upload_headers": {
                "x-awiki-upload-token": "slot-token-1",
                "content-type": "text/plain"
            },
            "object_uri": "__BASE__/objects/att-live-1",
            "commit_token": "commit-token-1",
            "expires_at": "2026-04-07T01:07:03Z"
        }))),
        TestResponse::bytes("application/json", b"{}"),
        TestResponse::json(&json_rpc_result(json!({
            "committed": true,
            "attachment_id": "__REQUEST_ATTACHMENT_ID__",
            "object_uri": "__BASE__/objects/att-live-1",
            "committed_at": "2026-04-07T01:08:03Z"
        }))),
        TestResponse::json(&json_rpc_result(json!({
            "accepted": true,
            "final_acceptance": true,
            "group_did": group_did,
            "message_id": "server-attachment-message",
            "operation_id": "server-attachment-operation",
            "group_event_seq": "77",
            "group_state_version": "v77",
            "accepted_at": "2026-04-07T01:09:03Z",
            "source": "remote_http"
        }))),
    ]);
    register_ready_identity(
        workspace.path(),
        "alice-attachment",
        "alice",
        "jwt-alice",
        &server.base_url(),
        "/alice/attachment/rpc",
        "did:wba:awiki.ai",
    );
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_owned(
        &[
            "--identity".to_string(),
            "alice-attachment".to_string(),
            "msg".to_string(),
            "send".to_string(),
            "--group".to_string(),
            group_did.to_string(),
            "--text".to_string(),
            "caption for group".to_string(),
            "--file".to_string(),
            path_string(&attachment_path),
            "--mime-type".to_string(),
            "text/plain".to_string(),
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a group attachment message");
    assert_eq!(envelope["data"]["action"], "send_attachment");
    assert_eq!(
        envelope["data"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(envelope["data"]["message"]["id"], format!("{group_did}:77"));
    assert_eq!(envelope["data"]["message"]["type"], "attachment_manifest");
    assert_eq!(
        envelope["data"]["message"]["content_type"],
        "application/anp-attachment-manifest+json"
    );
    assert_eq!(envelope["data"]["message"]["caption"], "caption for group");
    assert_eq!(envelope["data"]["message"]["secure"], false);
    assert_eq!(
        envelope["data"]["message"]["sent_at"],
        "2026-04-07T01:09:03Z"
    );
    let response_attachment_id = envelope["data"]["attachment"]["attachment_id"]
        .as_str()
        .filter(|value| value.starts_with("att-") && value.len() > 4)
        .expect("v2 response must echo the caller-generated attachment id")
        .to_owned();
    assert_eq!(envelope["data"]["attachment"]["filename"], "demo.txt");
    assert_eq!(envelope["data"]["attachment"]["mime_type"], "text/plain");
    assert_eq!(
        envelope["data"]["attachment"]["size"],
        payload.len().to_string()
    );
    assert_eq!(
        envelope["data"]["attachment"]["object_uri"],
        format!("{}/objects/att-live-1", server.base_url())
    );
    assert_eq!(
        envelope["data"]["attachment"]["object_encryption_mode"],
        "none"
    );
    assert_eq!(
        envelope["data"]["attachment"]["plaintext_size_bytes"],
        Value::Null
    );
    assert_eq!(
        envelope["data"]["attachment"]["digest"]["value_b64u"],
        digest_b64u(payload)
    );
    assert_eq!(
        envelope["data"]["delivery"]["message_id"],
        "server-attachment-message"
    );
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "server-attachment-operation"
    );
    assert_eq!(envelope["data"]["delivery"]["group_event_seq"], "77");

    let requests = server.requests();
    assert_eq!(requests.len(), 6);

    let create_text = request_text(&requests[0]);
    assert!(
        create_text.starts_with("POST /im/rpc HTTP/1.1"),
        "{create_text}"
    );
    assert_contains_text(&create_text, "Authorization: Bearer jwt-alice\r\n");
    let create_body = json_body(&requests[0]);
    assert_eq!(create_body["method"], "attachment.create_slot");
    assert_eq!(
        create_body["params"]["meta"]["profile"],
        "anp.attachment.v2"
    );
    assert_eq!(create_body["params"]["meta"]["anp_version"], "2.0");
    let requested_attachment_id = create_body["params"]["body"]["attachment_id"]
        .as_str()
        .filter(|value| value.starts_with("att-") && value.len() > 4)
        .expect("v2 create_slot must carry a caller-generated attachment_id")
        .to_owned();
    assert_eq!(response_attachment_id, requested_attachment_id);
    assert_eq!(
        create_body["params"]["meta"]["target"],
        json!({"kind": "service", "did": "did:wba:awiki.ai"})
    );
    assert_eq!(
        create_body["params"]["body"]["expected_size"],
        payload.len().to_string()
    );
    assert_eq!(
        create_body["params"]["body"]["expected_digest"]["value_b64u"],
        digest_b64u(payload)
    );
    assert_eq!(create_body["params"]["body"]["mime_type"], "text/plain");
    assert_eq!(create_body["params"]["body"]["filename"], "demo.txt");
    assert_eq!(
        create_body["params"]["body"]["intended_target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(
        create_body["params"]["body"]["object_encryption_mode"],
        "none"
    );

    let refresh_text = request_text(&requests[1]);
    assert!(
        refresh_text.starts_with("POST /user-service/did-auth/rpc HTTP/1.1"),
        "{refresh_text}"
    );
    let refresh_body = json_body(&requests[1]);
    assert_eq!(refresh_body["method"], "get_me");

    let retry_create_text = request_text(&requests[2]);
    assert!(
        retry_create_text.starts_with("POST /im/rpc HTTP/1.1"),
        "{retry_create_text}"
    );
    assert_contains_text(
        &retry_create_text,
        "Authorization: Bearer jwt-alice-refreshed\r\n",
    );
    let retry_create_body = json_body(&requests[2]);
    assert_eq!(retry_create_body["method"], "attachment.create_slot");
    assert_eq!(
        retry_create_body["params"]["body"]["attachment_id"],
        requested_attachment_id
    );
    assert_eq!(
        retry_create_body["params"]["body"]["expected_digest"]["value_b64u"],
        digest_b64u(payload)
    );

    let upload_text = request_text(&requests[3]);
    assert!(
        upload_text.starts_with("PUT /objects/upload/slot-live-1 HTTP/1.1"),
        "{upload_text}"
    );
    assert_contains_text(&upload_text, "x-awiki-upload-token: slot-token-1\r\n");
    assert_eq!(request_body_bytes(&requests[3]), payload);

    let commit_text = request_text(&requests[4]);
    assert!(
        commit_text.starts_with("POST /im/rpc HTTP/1.1"),
        "{commit_text}"
    );
    assert_contains_text(
        &commit_text,
        "Authorization: Bearer jwt-alice-refreshed\r\n",
    );
    let commit_body = json_body(&requests[4]);
    assert_eq!(commit_body["method"], "attachment.commit_object");
    assert_eq!(
        commit_body["params"]["meta"]["profile"],
        "anp.attachment.v2"
    );
    assert_eq!(commit_body["params"]["meta"]["anp_version"], "2.0");
    assert_eq!(
        commit_body["params"]["body"]["attachment_id"],
        requested_attachment_id
    );
    assert_eq!(commit_body["params"]["body"]["slot_id"], "slot-live-1");
    assert_eq!(
        commit_body["params"]["body"]["commit_token"],
        "commit-token-1"
    );
    assert_eq!(
        commit_body["params"]["body"]["size"],
        payload.len().to_string()
    );
    assert_eq!(
        commit_body["params"]["body"]["digest"]["value_b64u"],
        digest_b64u(payload)
    );

    let send_text = request_text(&requests[5]);
    assert!(
        send_text.starts_with("POST /im/rpc HTTP/1.1"),
        "{send_text}"
    );
    assert_contains_text(&send_text, "Authorization: Bearer jwt-alice-refreshed\r\n");
    let send_body = json_body(&requests[5]);
    assert_eq!(send_body["method"], "group.send");
    assert_eq!(send_body["params"]["meta"]["profile"], "anp.group.base.v1");
    assert_eq!(
        send_body["params"]["meta"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(
        send_body["params"]["meta"]["content_type"],
        "application/anp-attachment-manifest+json"
    );
    assert!(send_body["params"]["meta"]["message_id"]
        .as_str()
        .expect("message id")
        .starts_with("msg-"));
    assert_eq!(
        send_body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
    let manifest = &send_body["params"]["body"]["payload"];
    assert_eq!(manifest["primary_attachment_id"], requested_attachment_id);
    assert_eq!(manifest["caption"], "caption for group");
    assert_eq!(
        manifest["attachments"][0]["attachment_id"],
        requested_attachment_id
    );
    assert_eq!(manifest["attachments"][0]["filename"], "demo.txt");
    assert_eq!(manifest["attachments"][0]["mime_type"], "text/plain");
    assert_eq!(
        manifest["attachments"][0]["size"],
        payload.len().to_string()
    );
    assert_eq!(
        manifest["attachments"][0]["digest"]["value_b64u"],
        digest_b64u(payload)
    );
    assert_eq!(
        manifest["attachments"][0]["access_info"]["object_uri"],
        format!("{}/objects/att-live-1", server.base_url())
    );
    assert_eq!(
        manifest["attachments"][0]["encryption_info"]["mode"],
        "none"
    );
}

#[test]
fn msg_group_attachment_download_live_uses_sender_attachment_service_and_writes_exact_file() {
    let workspace = TempDir::new("attachment-live-download").expect("workspace");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let visible_message_id = format!("{group_did}:88");
    let raw_message_id = "raw-group-message-88";
    let downloaded_payload = b"downloaded attachment bytes\n";
    let output_path = workspace.path().join("downloads").join("report.txt");

    let server = TestServer::new(vec![
        TestResponse::json(&json_rpc_result(json!({
            "messages": [{
                "id": visible_message_id,
                "message_id": raw_message_id,
                "type": "attachment_manifest",
                "sender_did": "did:wba:awiki.ai:bob:e1_bob",
                "group_did": group_did,
                "content_type": "application/anp-attachment-manifest+json",
                "content": {
                    "caption": "download caption",
                    "primary_attachment_id": "att-download-1",
                    "attachments": [{
                        "attachment_id": "att-download-1",
                        "filename": "report.txt",
                        "mime_type": "text/plain",
                        "size": downloaded_payload.len().to_string(),
                        "digest": {
                            "alg": "sha-256",
                            "value_b64u": digest_b64u(downloaded_payload)
                        },
                        "access_info": {
                            "object_uri": "__BASE__/objects/att-download-1"
                        },
                        "encryption_info": {
                            "mode": "none"
                        }
                    }]
                },
                "sent_at": "2026-04-07T01:10:03Z"
            }],
            "total": 1,
            "has_more": false,
            "source": "remote_http"
        }))),
        TestResponse::json(&json_rpc_result(json!({
            "download_ticket_b64u": "download-ticket-1",
            "expires_at": "2026-04-07T01:11:03Z",
            "ticket_binding": {
                "attachment_id": "att-download-1"
            }
        }))),
        TestResponse::bytes("text/plain", downloaded_payload),
    ]);
    register_ready_identity(
        workspace.path(),
        "alice-attachment",
        "alice",
        "jwt-alice",
        &server.base_url(),
        "/alice/attachment/rpc",
        "did:wba:awiki.ai",
    );
    register_ready_identity(
        workspace.path(),
        "bob-sender",
        "bob",
        "jwt-bob",
        &server.base_url(),
        "/bob/attachment/rpc",
        "did:wba:bob-attachment.test",
    );
    write_msg_config_with_runtime(workspace.path(), &server.base_url(), "websocket");

    let output = awiki_cmd_owned(
        &[
            "--identity".to_string(),
            "alice-attachment".to_string(),
            "msg".to_string(),
            "attachment".to_string(),
            "download".to_string(),
            "--group".to_string(),
            group_did.to_string(),
            "--message-id".to_string(),
            visible_message_id.clone(),
            "--attachment-id".to_string(),
            "att-download-1".to_string(),
            "--output".to_string(),
            path_string(&output_path),
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        format!("Downloaded attachment to {}", path_string(&output_path))
    );
    assert_eq!(
        envelope["warnings"],
        json!(["Attachment downloads use HTTP transport even when runtime.mode is websocket."])
    );
    assert_eq!(envelope["data"]["action"], "download_attachment");
    assert_eq!(envelope["data"]["message_id"], raw_message_id);
    assert_eq!(
        envelope["data"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(
        envelope["data"]["attachment"]["attachment_id"],
        "att-download-1"
    );
    assert_eq!(envelope["data"]["attachment"]["filename"], "report.txt");
    assert_eq!(envelope["data"]["attachment"]["mime_type"], "text/plain");
    assert_eq!(
        envelope["data"]["attachment"]["size"],
        downloaded_payload.len().to_string()
    );
    assert_eq!(
        envelope["data"]["attachment"]["digest"]["value_b64u"],
        digest_b64u(downloaded_payload)
    );
    assert_eq!(
        envelope["data"]["attachment"]["object_uri"],
        format!("{}/objects/att-download-1", server.base_url())
    );
    assert_eq!(
        envelope["data"]["attachment"]["sender_did"],
        "did:wba:awiki.ai:bob:e1_bob"
    );
    assert_eq!(
        envelope["data"]["attachment"]["caption"],
        "download caption"
    );
    assert_eq!(
        envelope["data"]["output"]["path"],
        path_string(&output_path)
    );
    assert_eq!(
        envelope["data"]["output"]["size_bytes"],
        json!(downloaded_payload.len())
    );
    assert_eq!(envelope["data"]["output"]["content_type"], "text/plain");
    assert_eq!(
        std::fs::read(&output_path).expect("downloaded file"),
        downloaded_payload
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 3);

    let list_text = request_text(&requests[0]);
    assert!(
        list_text.starts_with("POST /im/rpc HTTP/1.1"),
        "{list_text}"
    );
    assert_contains_text(&list_text, "Authorization: Bearer jwt-alice\r\n");
    let list_body = json_body(&requests[0]);
    assert_eq!(list_body["method"], "group.list_messages");
    assert_eq!(list_body["params"]["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(
        list_body["params"]["meta"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(list_body["params"]["body"]["group_did"], group_did);
    assert_eq!(list_body["params"]["body"]["limit"], 100);
    assert_eq!(list_body["params"]["body"].get("skip"), None);

    let ticket_text = request_text(&requests[1]);
    assert!(
        ticket_text.starts_with("POST /im/rpc HTTP/1.1"),
        "{ticket_text}"
    );
    assert_contains_text(&ticket_text, "Authorization: Bearer jwt-alice\r\n");
    let ticket_body = json_body(&requests[1]);
    assert_eq!(ticket_body["method"], "attachment.get_download_ticket");
    assert_eq!(
        ticket_body["params"]["meta"]["profile"],
        "anp.attachment.v2"
    );
    assert_eq!(ticket_body["params"]["meta"]["anp_version"], "2.0");
    assert_eq!(
        ticket_body["params"]["meta"]["target"],
        json!({"kind": "service", "did": "did:wba:bob-attachment.test"})
    );
    assert_eq!(
        ticket_body["params"]["body"]["attachment_id"],
        "att-download-1"
    );
    assert_eq!(
        ticket_body["params"]["body"]["object_uri"],
        format!("{}/objects/att-download-1", server.base_url())
    );
    assert_eq!(ticket_body["params"]["body"].get("sender_did"), None);
    assert_eq!(
        ticket_body["params"]["body"]["requester_did"],
        "did:wba:awiki.ai:alice:e1_alice"
    );
    assert_eq!(ticket_body["params"]["body"]["message_id"], raw_message_id);
    assert_eq!(ticket_body["params"]["body"]["group_did"], group_did);
    assert_eq!(ticket_body["params"]["body"]["one_time"], true);

    let get_text = request_text(&requests[2]);
    assert!(
        get_text.starts_with("GET /objects/att-download-1 HTTP/1.1"),
        "{get_text}"
    );
    assert_contains_text(&get_text, "Authorization: Bearer download-ticket-1\r\n");
    assert_eq!(request_body_bytes(&requests[2]), b"");
}

#[test]
fn msg_direct_attachment_send_live_posts_direct_manifest_like_go() {
    let workspace = TempDir::new("attachment-live-direct-send").expect("workspace");
    let target_did = "did:wba:awiki.ai:bob:e1_bob";
    let payload = b"direct attachment bytes\n";
    let attachment_path = workspace.path().join("direct.txt");
    std::fs::write(&attachment_path, payload).expect("attachment fixture");

    let server = TestServer::new(vec![
        TestResponse::json(&json_rpc_result(json!({
            "attachment_id": "__REQUEST_ATTACHMENT_ID__",
            "slot_id": "slot-direct-1",
            "upload_uri": "__BASE__/objects/upload/slot-direct-1",
            "upload_headers": {
                "x-awiki-upload-token": "slot-direct-token"
            },
            "object_uri": "__BASE__/objects/att-direct-1",
            "commit_token": "commit-direct-1",
            "expires_at": "2026-04-07T01:07:03Z"
        }))),
        TestResponse::bytes("application/json", b"{}"),
        TestResponse::json(&json_rpc_result(json!({
            "committed": true,
            "attachment_id": "__REQUEST_ATTACHMENT_ID__",
            "object_uri": "__BASE__/objects/att-direct-1",
            "committed_at": "2026-04-07T01:08:03Z"
        }))),
        TestResponse::json(&json_rpc_result(json!({
            "accepted": true,
            "message_id": "msg-direct-att-1",
            "operation_id": "op-direct-att-1",
            "target_did": target_did,
            "accepted_at": "2026-04-07T01:09:03Z",
            "final_acceptance": true,
            "delivery_state": "accepted"
        }))),
    ]);
    register_ready_identity(
        workspace.path(),
        "alice-attachment",
        "alice",
        "jwt-alice",
        &server.base_url(),
        "/alice/attachment/rpc",
        "did:wba:awiki.ai",
    );
    write_msg_config_with_runtime(workspace.path(), &server.base_url(), "websocket");

    let output = awiki_cmd_owned(
        &[
            "--identity".to_string(),
            "alice-attachment".to_string(),
            "msg".to_string(),
            "send".to_string(),
            "--to".to_string(),
            target_did.to_string(),
            "--text".to_string(),
            "direct caption".to_string(),
            "--file".to_string(),
            path_string(&attachment_path),
            "--mime-type".to_string(),
            "text/plain".to_string(),
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a direct attachment message");
    assert_eq!(
        envelope["warnings"],
        json!(["Attachment messages use HTTP transport even when runtime.mode is websocket."])
    );
    assert_eq!(envelope["data"]["action"], "send_attachment");
    assert_eq!(
        envelope["data"]["target"],
        json!({"did": target_did, "handle": "", "kind": "direct"})
    );
    assert_eq!(envelope["data"]["message"]["id"], "msg-direct-att-1");
    assert_eq!(envelope["data"]["message"]["caption"], "direct caption");
    let response_attachment_id = envelope["data"]["attachment"]["attachment_id"]
        .as_str()
        .filter(|value| value.starts_with("att-") && value.len() > 4)
        .expect("v2 response must echo the caller-generated attachment id")
        .to_owned();

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    let create_body = json_body(&requests[0]);
    assert_eq!(create_body["method"], "attachment.create_slot");
    assert_eq!(
        create_body["params"]["body"]["attachment_id"],
        response_attachment_id
    );
    assert_eq!(
        create_body["params"]["body"]["intended_target"],
        json!({"kind": "agent", "did": target_did})
    );
    assert_eq!(request_body_bytes(&requests[1]), payload);
    let send_body = json_body(&requests[3]);
    assert_eq!(send_body["method"], "direct.send");
    assert_eq!(
        send_body["params"]["meta"]["target"],
        json!({"kind": "agent", "did": target_did})
    );
    assert_eq!(
        send_body["params"]["meta"]["content_type"],
        "application/anp-attachment-manifest+json"
    );
    assert_eq!(
        send_body["params"]["body"]["payload"]["attachments"][0]["attachment_id"],
        response_attachment_id
    );
}

#[test]
fn msg_attachment_download_error_mapping_matches_go_attachment_codes() {
    let cases = [
        (
            vec![TestResponse::json(&json_rpc_error(
                6000,
                "attachment not found",
            ))],
            5,
            "not_found",
            "service rpc error 6000: attachment not found",
        ),
        (
            vec![TestResponse::json(&json_rpc_error(
                6006,
                "invalid attachment id",
            ))],
            2,
            "invalid_argument",
            "service rpc error 6006: invalid attachment id",
        ),
        (
            vec![TestResponse::json(&json_rpc_result(json!({
                "messages": [],
                "has_more": false,
            })))],
            5,
            "not_found",
            "message not found",
        ),
    ];

    for (index, (responses, exit_code, error_code, message)) in cases.into_iter().enumerate() {
        let workspace = TempDir::new(&format!("attachment-live-error-{index}")).expect("workspace");
        let server = TestServer::new(responses);
        register_ready_identity(
            workspace.path(),
            "alice-attachment",
            "alice",
            "jwt-alice",
            &server.base_url(),
            "/alice/attachment/rpc",
            "did:wba:awiki.ai",
        );
        write_msg_config(workspace.path(), &server.base_url());

        let output = awiki_cmd(
            &[
                "--identity",
                "alice-attachment",
                "msg",
                "attachment",
                "download",
                "--group",
                "did:wba:awiki.ai:groups:demo:e1_group",
                "--message-id",
                "msg-missing",
                "--attachment-id",
                "att-missing",
                "--output",
                "out.bin",
            ],
            workspace.path(),
        );

        assert_code(&output, exit_code);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], error_code);
        assert_contains_text(
            envelope["error"]["message"].as_str().unwrap_or_default(),
            message,
        );
    }
}

fn register_ready_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
    service_base_url: &str,
    attachment_service_path: &str,
    attachment_service_did: &str,
) {
    configure_default_tenant_if_needed(workspace, service_base_url);
    write_msg_config_with_runtime(workspace, service_base_url, "http");
    set_secret_storage_mode(workspace, "file_compat");
    let create = awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Attachment User",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let did = format!("did:wba:awiki.ai:{handle}:e1_{handle}");
    let tenant_workspace = tenant_workspace(workspace);
    let index_path = tenant_workspace.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    index["credentials"][identity_name]["did"] = json!(did);
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}.awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = tenant_workspace.join("identities").join(dir_name);
    let identity_path = identity_dir.join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
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
    document["id"] = json!(did);
    document["service"] = json!([{
        "id": "#message",
        "type": "ANPMessageService",
        "serviceEndpoint": format!("{service_base_url}{attachment_service_path}"),
        "serviceDid": attachment_service_did,
        "profiles": [
            "anp.core.binding.v2",
            "anp.direct.base.v1",
            "anp.attachment.v2"
        ],
        "securityProfiles": ["transport-protected"],
        "priority": 1
    }]);
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

    set_secret_storage_mode(workspace, "vault_required");
    let migrate = awiki_cmd(&["--migration", "id", "vault", "migrate"], workspace);
    assert_success(&migrate);
}

fn write_msg_config(workspace: &Path, base_url: &str) {
    write_msg_config_with_runtime(workspace, base_url, "http");
}

fn write_msg_config_with_runtime(workspace: &Path, base_url: &str, runtime_mode: &str) {
    configure_default_tenant_if_needed(workspace, base_url);
    let config_path = tenant_config_path(workspace);
    let text = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut output = String::new();
    let mut in_runtime = false;
    let mut in_services = false;
    let mut wrote_mode = false;
    let mut wrote_anp_endpoint = false;
    let mut wrote_anp_did = false;
    let mut saw_runtime = false;
    let mut saw_services = false;
    for line in text.lines() {
        let is_top_level =
            !line.chars().next().is_some_and(char::is_whitespace) && !line.trim().is_empty();
        if in_runtime && is_top_level {
            if !wrote_mode {
                output.push_str(&format!("  mode: {runtime_mode}\n"));
            }
            in_runtime = false;
        }
        if in_services && is_top_level {
            if !wrote_anp_endpoint {
                output.push_str("  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n");
            }
            if !wrote_anp_did {
                output.push_str("  anp_service_did: did:wba:awiki.ai\n");
            }
            in_services = false;
        }
        if is_top_level && line.trim() == "runtime:" {
            saw_runtime = true;
            in_runtime = true;
            wrote_mode = false;
            output.push_str(line);
            output.push('\n');
            continue;
        }
        if is_top_level && line.trim() == "services:" {
            saw_services = true;
            in_services = true;
            wrote_anp_endpoint = false;
            wrote_anp_did = false;
            output.push_str(line);
            output.push('\n');
            continue;
        }
        if in_runtime && line.trim_start().starts_with("mode:") {
            output.push_str(&format!("  mode: {runtime_mode}\n"));
            wrote_mode = true;
            continue;
        }
        if in_services && line.trim_start().starts_with("anp_service_endpoint:") {
            output.push_str("  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n");
            wrote_anp_endpoint = true;
            continue;
        }
        if in_services && line.trim_start().starts_with("anp_service_did:") {
            output.push_str("  anp_service_did: did:wba:awiki.ai\n");
            wrote_anp_did = true;
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    if in_runtime && !wrote_mode {
        output.push_str(&format!("  mode: {runtime_mode}\n"));
    }
    if in_services {
        if !wrote_anp_endpoint {
            output.push_str("  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n");
        }
        if !wrote_anp_did {
            output.push_str("  anp_service_did: did:wba:awiki.ai\n");
        }
    }
    if !saw_runtime {
        output.push_str(&format!("\nruntime:\n  mode: {runtime_mode}\n"));
    }
    if !saw_services {
        output.push_str(
            "\nservices:\n  anp_service_endpoint: https://awiki.ai/anp-im/rpc\n  anp_service_did: did:wba:awiki.ai\n",
        );
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(config_path, output).unwrap();
}

fn configure_default_tenant_if_needed(workspace: &Path, base_url: &str) {
    let current = awiki_cmd(&["tenant", "current"], workspace);
    assert_success(&current);
    let envelope: Value = serde_json::from_slice(&current.stdout).expect("tenant current JSON");
    if envelope["data"]["tenant"]["profile"]["backend_base_url"]
        .as_str()
        .map(|value| value.trim_end_matches('/') == base_url.trim_end_matches('/'))
        .unwrap_or(false)
    {
        return;
    }
    let output = awiki_cmd(
        &[
            "tenant",
            "reconfigure",
            "default",
            "--backend-base-url",
            base_url,
            "--did-host",
            "awiki.ai",
        ],
        workspace,
    );
    assert_success(&output);
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_owned(
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        workspace,
    )
}

fn awiki_cmd_owned(args: &[String], workspace: &Path) -> Output {
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
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope")
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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

fn digest_b64u(payload: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(payload))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn json_body(raw: &[u8]) -> Value {
    serde_json::from_slice(request_body_bytes(raw)).expect("JSON request body")
}

fn request_text(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}

fn request_body_bytes(raw: &[u8]) -> &[u8] {
    find_header_end(raw)
        .map(|header_end| &raw[header_end..])
        .unwrap_or_default()
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

#[derive(Debug, Clone)]
struct TestResponse {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

impl TestResponse {
    fn json(body: &str) -> Self {
        Self::bytes("application/json", body.as_bytes())
    }

    fn bytes(content_type: &str, body: &[u8]) -> Self {
        Self {
            status: 200,
            content_type: content_type.to_string(),
            body: body.to_vec(),
        }
    }

    fn with_base_url(mut self, base_url: &str) -> Self {
        let body = String::from_utf8_lossy(&self.body)
            .replace("__BASE__", base_url)
            .into_bytes();
        self.body = body;
        self
    }

    fn with_request_attachment_id(mut self, request: &[u8]) -> Self {
        if !self
            .body
            .windows("__REQUEST_ATTACHMENT_ID__".len())
            .any(|window| window == "__REQUEST_ATTACHMENT_ID__".as_bytes())
        {
            return self;
        }
        let attachment_id = json_body(request)["params"]["body"]["attachment_id"]
            .as_str()
            .expect("response marker requires a request attachment_id")
            .to_owned();
        self.body = String::from_utf8_lossy(&self.body)
            .replace("__REQUEST_ATTACHMENT_ID__", &attachment_id)
            .into_bytes();
        self
    }
}

struct TestServer {
    address: String,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(responses: Vec<TestResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        listener
            .set_nonblocking(true)
            .expect("set test server nonblocking");
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let responses = responses
            .into_iter()
            .map(|response| response.with_base_url(&address))
            .collect::<Vec<_>>();
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

    fn requests(&self) -> Vec<Vec<u8>> {
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
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
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
    requests: &Arc<Mutex<Vec<Vec<u8>>>>,
    response: TestResponse,
) {
    let request = read_http_request(&mut stream);
    let response = response.with_request_attachment_id(&request);
    requests.lock().expect("requests mutex").push(request);
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len(),
    );
    stream
        .write_all(raw.as_bytes())
        .expect("write response head");
    stream
        .write_all(&response.body)
        .expect("write response body");
}

fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
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
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim())
                })
                .and_then(|value| value.parse::<usize>().ok())
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
    raw
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
            "awiki-cli-rs2-{prefix}-{}-{nanos}-{thread_id}-{counter}",
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
