#![cfg(unix)]

use awiki_cli::config::{Paths, Resolved};
use awiki_cli::message::{
    GroupCreateRequest, GroupGetRequest, GroupInfoRequest, GroupJoinRequest, GroupLeaveRequest,
    GroupListRequest, GroupMemberRequest, GroupMembersRequest, GroupMessagesRequest,
    HistoryRequest, InboxRequest, MarkReadRequest, MessageError, SendRequest, WSProxyTransport,
};
use awiki_cli::runtime::bridge::{self, BridgeRequest, BridgeResponse};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ws_proxy_transport_calls_local_bridge_and_decodes_responses() {
    let cases = [
        ProxyCase {
            name: "send direct",
            response_result: json!({
                "message_id": "msg-1",
                "operation_id": "op-1",
            }),
            call: |transport| {
                let result = transport.send_direct(SendRequest {
                    target: "did:bob".to_string(),
                    text: "hello".to_string(),
                    message_type: "text".to_string(),
                    ..SendRequest::default()
                })?;
                assert_eq!(result.message_id, "msg-1");
                assert_eq!(result.operation_id, "op-1");
                Ok(())
            },
            want_method: "direct.send",
            verify: |params| {
                assert_eq!(params["target"], "did:bob");
                assert_eq!(params["text"], "hello");
                assert_eq!(params["type"], "text");
            },
        },
        ProxyCase {
            name: "send group",
            response_result: json!({
                "message_id": "group-msg-1",
                "operation_id": "group-op-1",
            }),
            call: |transport| {
                let result = transport.send_group(SendRequest {
                    group: "did:group".to_string(),
                    text: "hello group".to_string(),
                    message_type: "text".to_string(),
                    ..SendRequest::default()
                })?;
                assert_eq!(result.message_id, "group-msg-1");
                assert_eq!(result.operation_id, "group-op-1");
                Ok(())
            },
            want_method: "group.send",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["text"], "hello group");
                assert_eq!(params["type"], "text");
            },
        },
        ProxyCase {
            name: "get inbox",
            response_result: json!({ "messages": [] }),
            call: |transport| {
                let _ = transport.get_inbox(InboxRequest {
                    with: "did:bob".to_string(),
                    limit: 6,
                    scope: "all".to_string(),
                    mark_read: true,
                    unread_only: true,
                    ..InboxRequest::default()
                })?;
                Ok(())
            },
            want_method: "inbox.get",
            verify: |params| {
                assert_eq!(params["with"], "did:bob");
                assert_eq!(params["limit"], 6);
                assert_eq!(params["scope"], "all");
                assert_eq!(params["mark_read"], true);
                assert_eq!(params["unread"], true);
            },
        },
        ProxyCase {
            name: "get history preserves skip",
            response_result: json!({ "messages": [] }),
            call: |transport| {
                let _ = transport.get_history(HistoryRequest {
                    with: "bob".to_string(),
                    limit: 5,
                    cursor: "seq-2".to_string(),
                    skip: 3,
                    ..HistoryRequest::default()
                })?;
                Ok(())
            },
            want_method: "direct.get_history",
            verify: |params| {
                assert_eq!(params["with"], "bob");
                assert_eq!(params["cursor"], "seq-2");
                assert_eq!(params["skip"], 3);
            },
        },
        ProxyCase {
            name: "mark read",
            response_result: json!({ "updated": 2 }),
            call: |transport| {
                let _ = transport.mark_read(MarkReadRequest {
                    message_ids: vec!["msg-1".to_string(), "msg-2".to_string()],
                    ..MarkReadRequest::default()
                })?;
                Ok(())
            },
            want_method: "inbox.mark_read",
            verify: |params| {
                assert_eq!(params["message_ids"], json!(["msg-1", "msg-2"]));
            },
        },
        ProxyCase {
            name: "create group",
            response_result: json!({ "group": "did:group" }),
            call: |transport| {
                let _ = transport.create_group(GroupCreateRequest {
                    name: "team".to_string(),
                    description: "daily work".to_string(),
                    discoverability: "public".to_string(),
                    admission_mode: "approval".to_string(),
                    slug: "team".to_string(),
                    goal: "ship".to_string(),
                    rules: "be clear".to_string(),
                    message_prompt: "brief".to_string(),
                    doc_url: "https://example.test/doc".to_string(),
                    attachments_allowed: Some(true),
                    max_members: "25".to_string(),
                    member_max_messages: Some(9),
                    member_max_total_chars: Some(4000),
                    ..GroupCreateRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.create",
            verify: |params| {
                assert_eq!(params["name"], "team");
                assert_eq!(params["description"], "daily work");
                assert_eq!(params["discoverability"], "public");
                assert_eq!(params["admission_mode"], "approval");
                assert_eq!(params["slug"], "team");
                assert_eq!(params["goal"], "ship");
                assert_eq!(params["rules"], "be clear");
                assert_eq!(params["message_prompt"], "brief");
                assert_eq!(params["doc_url"], "https://example.test/doc");
                assert_eq!(params["attachments_allowed"], true);
                assert_eq!(params["max_members"], "25");
                assert_eq!(params["member_max_messages"], 9);
                assert_eq!(params["member_max_total_chars"], 4000);
            },
        },
        ProxyCase {
            name: "get group info",
            response_result: json!({ "group": "did:group" }),
            call: |transport| {
                let _ = transport.get_group_info(GroupInfoRequest {
                    group: "did:group".to_string(),
                    include_policy: true,
                    include_member_list: true,
                    ..GroupInfoRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.get_info",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["include_policy"], true);
                assert_eq!(params["include_member_list"], true);
            },
        },
        ProxyCase {
            name: "join group",
            response_result: json!({ "joined": true }),
            call: |transport| {
                let _ = transport.join_group(GroupJoinRequest {
                    group: "did:group".to_string(),
                    reason_text: "invite".to_string(),
                    ..GroupJoinRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.join",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["reason_text"], "invite");
            },
        },
        ProxyCase {
            name: "add group member",
            response_result: json!({ "added": true }),
            call: |transport| {
                let _ = transport.add_group_member(GroupMemberRequest {
                    group: "did:group".to_string(),
                    member: "did:bob".to_string(),
                    role: "admin".to_string(),
                    reason_text: "ops".to_string(),
                    ..GroupMemberRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.add",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["member"], "did:bob");
                assert_eq!(params["role"], "admin");
                assert_eq!(params["reason_text"], "ops");
            },
        },
        ProxyCase {
            name: "remove group member",
            response_result: json!({ "removed": true }),
            call: |transport| {
                let _ = transport.remove_group_member(GroupMemberRequest {
                    group: "did:group".to_string(),
                    member: "did:bob".to_string(),
                    reason_text: "rotate".to_string(),
                    ..GroupMemberRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.remove",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["member"], "did:bob");
                assert_eq!(params["reason_text"], "rotate");
            },
        },
        ProxyCase {
            name: "leave group",
            response_result: json!({ "left": true }),
            call: |transport| {
                let _ = transport.leave_group(GroupLeaveRequest {
                    group: "did:group".to_string(),
                    ..GroupLeaveRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.leave",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
            },
        },
        ProxyCase {
            name: "get group",
            response_result: json!({ "group": "did:group" }),
            call: |transport| {
                let _ = transport.get_group(GroupGetRequest {
                    group: "did:group".to_string(),
                    ..GroupGetRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.get",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
            },
        },
        ProxyCase {
            name: "list group messages preserves cursor and skip",
            response_result: json!({ "messages": [] }),
            call: |transport| {
                let _ = transport.list_group_messages(GroupMessagesRequest {
                    group: "did:group".to_string(),
                    limit: 10,
                    cursor: "7".to_string(),
                    skip: 2,
                    ..GroupMessagesRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.list_messages",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["cursor"], "7");
                assert_eq!(params["skip"], 2);
            },
        },
        ProxyCase {
            name: "list groups preserves limit",
            response_result: json!({ "groups": [] }),
            call: |transport| {
                let _ = transport.list_groups(GroupListRequest {
                    limit: 12,
                    ..GroupListRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.list",
            verify: |params| {
                assert_eq!(params["limit"], 12);
            },
        },
        ProxyCase {
            name: "list group members",
            response_result: json!({ "members": [] }),
            call: |transport| {
                let _ = transport.list_group_members(GroupMembersRequest {
                    group: "did:group".to_string(),
                    limit: 13,
                    ..GroupMembersRequest::default()
                })?;
                Ok(())
            },
            want_method: "group.list_members",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["limit"], 13);
            },
        },
        ProxyCase {
            name: "update group profile",
            response_result: json!({ "updated": true }),
            call: |transport| {
                let _ = transport.update_group_profile(
                    GroupGetRequest {
                        group: "did:group".to_string(),
                        ..GroupGetRequest::default()
                    },
                    serde_json::Map::from_iter([
                        ("name".to_string(), json!("new name")),
                        ("description".to_string(), json!("new description")),
                    ]),
                )?;
                Ok(())
            },
            want_method: "group.update_profile",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["patch"]["name"], "new name");
                assert_eq!(params["patch"]["description"], "new description");
            },
        },
        ProxyCase {
            name: "update group policy",
            response_result: json!({ "updated": true }),
            call: |transport| {
                let _ = transport.update_group_policy(
                    GroupGetRequest {
                        group: "did:group".to_string(),
                        ..GroupGetRequest::default()
                    },
                    serde_json::Map::from_iter([
                        ("admission_mode".to_string(), json!("open")),
                        ("attachments_allowed".to_string(), json!(false)),
                    ]),
                )?;
                Ok(())
            },
            want_method: "group.update_policy",
            verify: |params| {
                assert_eq!(params["group"], "did:group");
                assert_eq!(params["patch"]["admission_mode"], "open");
                assert_eq!(params["patch"]["attachments_allowed"], false);
            },
        },
    ];

    for case in cases {
        let workspace = TempDir::new().expect("temp workspace");
        let socket_path = workspace.path().join("runtime").join("message-daemon.sock");
        let (_server, requests) = spawn_bridge_server(
            socket_path.to_str().expect("socket path"),
            BridgeResponse {
                ok: true,
                result: case
                    .response_result
                    .as_object()
                    .expect("object bridge result")
                    .clone(),
                error: None,
            },
        );

        let resolved = test_resolved_with_socket(socket_path.to_str().expect("socket path"));
        let transport = WSProxyTransport::new(&resolved, "alice");
        (case.call)(&transport).unwrap_or_else(|err| panic!("{} call error: {err}", case.name));

        let request = requests
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap_or_else(|err| panic!("{} bridge request missing: {err}", case.name));
        assert_eq!(request.method, case.want_method, "{}", case.name);
        assert_eq!(request.identity_name, "alice", "{}", case.name);
        (case.verify)(&request.params);
    }
}

#[test]
fn ws_proxy_transport_wraps_missing_bridge_as_transport_unavailable() {
    let workspace = TempDir::new().expect("temp workspace");
    let socket_path = workspace.path().join("runtime").join("missing.sock");
    let resolved = test_resolved_with_socket(socket_path.to_str().expect("socket path"));
    let transport = WSProxyTransport::new(&resolved, "alice");

    let err = transport
        .mark_read(MarkReadRequest {
            message_ids: vec!["msg-1".to_string()],
            ..MarkReadRequest::default()
        })
        .expect_err("missing bridge should fail");

    assert!(
        matches!(err, MessageError::TransportUnavailable(_)),
        "expected MessageError::TransportUnavailable, got {err:?}"
    );
    assert!(
        err.to_string().contains("message transport is unavailable"),
        "unexpected display: {err}"
    );
}

#[test]
fn ws_proxy_transport_decodes_send_results_with_go_zero_value_tolerance() {
    let workspace = TempDir::new().expect("temp workspace");
    let socket_path = workspace.path().join("runtime").join("message-daemon.sock");
    let (_server, _requests) = spawn_bridge_server(
        socket_path.to_str().expect("socket path"),
        BridgeResponse {
            ok: true,
            result: json!({
                "message_id": 123,
                "operation_id": "op-1",
                "accepted": "true",
                "final_acceptance": true,
                "delivery_state": "queued",
            })
            .as_object()
            .expect("object bridge result")
            .clone(),
            error: None,
        },
    );

    let resolved = test_resolved_with_socket(socket_path.to_str().expect("socket path"));
    let transport = WSProxyTransport::new(&resolved, "alice");
    let result = transport
        .send_direct(SendRequest {
            target: "did:bob".to_string(),
            text: "hello".to_string(),
            message_type: "text".to_string(),
            ..SendRequest::default()
        })
        .expect("send direct");

    assert_eq!(result.message_id, "");
    assert_eq!(result.operation_id, "op-1");
    assert!(!result.accepted);
    assert!(result.final_acceptance);
    assert_eq!(result.delivery_state, "queued");
}

struct ProxyCase {
    name: &'static str,
    response_result: serde_json::Value,
    call: fn(&WSProxyTransport) -> Result<(), MessageError>,
    want_method: &'static str,
    verify: fn(&serde_json::Map<String, serde_json::Value>),
}

fn spawn_bridge_server(
    socket_path: &str,
    response: BridgeResponse,
) -> (std::thread::JoinHandle<()>, mpsc::Receiver<BridgeRequest>) {
    let listener = bridge::listen_bridge(socket_path).expect("listen bridge");
    listener
        .set_nonblocking(true)
        .expect("set bridge listener nonblocking");
    let (requests_tx, requests_rx) = mpsc::channel();

    let handle = std::thread::spawn(move || loop {
        let (mut conn, _) = accept_unix_connection(&listener).expect("accept bridge client");
        conn.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("set bridge read timeout");
        let mut request_line = String::new();
        let read = BufReader::new(conn.try_clone().expect("clone bridge client"))
            .read_line(&mut request_line)
            .expect("read bridge request");
        if read == 0 || request_line.trim().is_empty() {
            continue;
        }

        let request: BridgeRequest =
            serde_json::from_str(request_line.trim_end()).expect("decode bridge request");
        requests_tx.send(request).expect("send bridge request");
        let response_json = serde_json::to_vec(&response).expect("encode bridge response");
        conn.write_all(&[response_json.as_slice(), b"\n"].concat())
            .expect("write bridge response");
        break;
    });

    (handle, requests_rx)
}

fn accept_unix_connection(
    listener: &std::os::unix::net::UnixListener,
) -> std::io::Result<(
    std::os::unix::net::UnixStream,
    std::os::unix::net::SocketAddr,
)> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok(accepted) => return Ok(accepted),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out accepting bridge test connection",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(err) => return Err(err),
        }
    }
}

fn test_resolved_with_socket(socket_path: &str) -> Resolved {
    let mut paths = test_paths();
    if let Some(parent) = std::path::Path::new(socket_path).parent() {
        paths.state_dir = parent.to_string_lossy().into_owned();
    }
    Resolved {
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: socket_path.to_string(),
        paths,
        ..test_resolved()
    }
}

fn test_resolved() -> Resolved {
    Resolved {
        paths: test_paths(),
        config_schema_version: 1,
        active_identity: String::new(),
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:awiki.ai".to_string(),
        mail_service_url: "https://awiki.ai".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: std::collections::BTreeMap::new(),
    }
}

fn test_paths() -> Paths {
    Paths {
        workspace_home_dir: "/tmp/awiki-workspace".to_string(),
        root_dir: "/tmp/awiki-workspace".to_string(),
        config_dir: "/tmp/awiki-workspace".to_string(),
        data_dir: "/tmp/awiki-workspace/data".to_string(),
        state_dir: "/tmp/awiki-workspace/runtime".to_string(),
        cache_dir: "/tmp/awiki-workspace/cache".to_string(),
        logs_dir: "/tmp/awiki-workspace/logs".to_string(),
        config_file: "/tmp/awiki-workspace/config.yaml".to_string(),
        identity_dir: "/tmp/awiki-workspace/identities".to_string(),
        database_file: "/tmp/awiki-workspace/awiki-cli.db".to_string(),
        legacy_credentials_dir: String::new(),
        legacy_data_dir: String::new(),
    }
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-message-ws-proxy-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
