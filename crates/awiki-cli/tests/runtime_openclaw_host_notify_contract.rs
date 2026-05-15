use awiki_cli::config::{Paths, Resolved, ValueSource};
use awiki_cli::runtime::host_notify::{
    DirectMessageNotificationData, GroupMessageNotificationData, GroupStateChangedNotificationData,
    HostNotificationData, HostNotificationEvent,
};
use awiki_cli::runtime::openclaw_host_notify::{
    build_openclaw_agent_hook_message, build_openclaw_event_text, build_openclaw_hook_request,
    new_openclaw_host_notify_sink, FIXED_HOOK_NAME,
};
use awiki_cli::runtime::openclaw_routes::{self, Route};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn openclaw_hook_request_includes_channel_delivery_like_go() {
    let request = build_openclaw_hook_request(
        &direct_event(DirectMessageNotificationData {
            channel: "direct".to_string(),
            message_id: "msg-001".to_string(),
            conversation_id: "conv-alice-bob".to_string(),
            sender_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
            recipient_did: "did:wba:example.com:user:bob:e1_bob".to_string(),
            content_type: "text/plain".to_string(),
            text: "hello".to_string(),
            ..DirectMessageNotificationData::default()
        }),
        FIXED_HOOK_NAME,
        "telegram",
        "123456",
    );

    assert!(request.deliver);
    assert_eq!(request.name, "AWiki");
    assert_eq!(request.channel, "telegram");
    assert_eq!(request.to, "123456");
    assert_eq!(request.wake_mode, "now");
    assert!(request
        .message
        .contains("You received a new im message from awiki."));

    let raw = serde_json::to_value(&request).expect("request json");
    assert_eq!(raw["wakeMode"], "now");
    assert!(raw.get("wake_mode").is_none());
}

#[test]
fn openclaw_event_text_uses_main_agent_session_format_like_go() {
    let text = build_openclaw_event_text(&direct_event(DirectMessageNotificationData {
        sender_handle: "alice".to_string(),
        sender_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
        recipient_handle: "bob".to_string(),
        recipient_did: "did:wba:example.com:user:bob:e1_bob".to_string(),
        created_at: "2026-04-07T00:00:00Z".to_string(),
        text: "hello back".to_string(),
        ..DirectMessageNotificationData::default()
    }));

    assert!(text.contains("[Awiki New Direct Message]"));
    assert!(text.contains("sender_did: did:wba:example.com:user:alice:e1_alice"));
    assert!(text.contains("sender_handle: alice"));
    assert!(text.contains("recipient_handle: bob"));
    assert!(text.contains("sent_at: 2026-04-07T00:00:00Z"));
    assert!(text.ends_with("hello back"));
}

#[test]
fn openclaw_event_text_uses_mail_format_like_go() {
    let text = build_openclaw_event_text(&direct_event(mail_data(true)));

    assert!(text.contains("[Awiki New Mail]"));
    assert!(text.contains("from_addr: sender@example.com"));
    assert!(text.contains("mailbox_address: alice@example.com"));
    assert!(text.contains("subject: Mail Subject"));
    assert!(text.contains("has_attachments: true"));
    assert!(text.contains("Subject: Mail Subject"));
    assert!(text.contains("Preview text"));
    assert!(text.contains("(This message has attachments.)"));
}

#[test]
fn openclaw_hook_request_includes_mail_prompt_like_go() {
    let request = build_openclaw_hook_request(
        &direct_event(mail_data(false)),
        FIXED_HOOK_NAME,
        "telegram",
        "123456",
    );

    assert!(request
        .message
        .contains("You received a new mail notification from awiki."));
    assert!(request.message.contains("Message type: mail"));
    assert!(request.message.contains("Sender DID: sender@example.com"));
    assert!(request
        .message
        .contains("Receiver handle: alice@example.com"));
}

#[test]
fn openclaw_prompt_uses_group_and_group_state_parts_like_go() {
    let group = HostNotificationEvent {
        version: "1.0".to_string(),
        id: "group-msg-1".to_string(),
        topic: "im.group.message.received".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: Some(HostNotificationData::Group(GroupMessageNotificationData {
            channel: "group".to_string(),
            message_id: "group-msg-1".to_string(),
            group_did: "did:group".to_string(),
            sender_handle: "alice".to_string(),
            sender_did: "did:alice".to_string(),
            recipient_handle: "bob".to_string(),
            recipient_did: "did:bob".to_string(),
            content_type: "application/json".to_string(),
            accepted_at: "2026-04-07T09:11:01Z".to_string(),
            ..GroupMessageNotificationData::default()
        })),
    };

    let text = build_openclaw_event_text(&group);
    assert!(text.contains("[Awiki New Group Message]"));
    assert!(text.contains("group_did: did:group"));
    assert!(text.contains("sent_at: 2026-04-07T09:11:01Z"));
    assert!(text.ends_with("[application/json]"));

    let prompt = build_openclaw_agent_hook_message(&HostNotificationEvent {
        version: "1.0".to_string(),
        id: "state-1".to_string(),
        topic: "im.group.state.changed".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: Some(HostNotificationData::GroupState(
            GroupStateChangedNotificationData {
                channel: "group".to_string(),
                event_id: "state-1".to_string(),
                event_type: "member-removed".to_string(),
                group_did: "did:group".to_string(),
                recipient_did: "did:bob".to_string(),
                actor_did: "did:alice".to_string(),
                subject_did: "did:carol".to_string(),
                subject_method: "group.remove".to_string(),
                membership_status: "removed".to_string(),
                ..GroupStateChangedNotificationData::default()
            },
        )),
    });
    assert!(prompt.contains("Message type: group"));
    assert!(prompt.contains("Group ID: did:group"));
    assert!(prompt.contains("Sender DID: did:alice"));
    assert!(prompt.contains("Receiver DID: did:bob"));
    assert!(prompt.contains("Group state changed. event_type=member-removed"));
}

#[test]
fn openclaw_mail_content_fallbacks_and_unknown_event_match_go() {
    let mail = direct_event(DirectMessageNotificationData {
        source_kind: "mail".to_string(),
        recipient_did: "did:mailbox".to_string(),
        content_type: "mail.notification".to_string(),
        ..DirectMessageNotificationData::default()
    });
    let text = build_openclaw_event_text(&mail);
    assert!(text.ends_with("[mail notification]"));

    let event = HostNotificationEvent {
        version: "1.0".to_string(),
        id: "unknown-1".to_string(),
        topic: "unknown".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: None,
    };
    let text = build_openclaw_event_text(&event);
    assert!(text.contains("[Awiki Notification]"));
    assert!(text.contains(r#""id":"unknown-1""#));

    let prompt = build_openclaw_agent_hook_message(&event);
    assert!(prompt.contains("You received a new notification from awiki."));
    assert!(prompt.contains("Message type: notification"));
    assert!(prompt.contains(r#""topic":"unknown""#));
}

fn direct_event(data: DirectMessageNotificationData) -> HostNotificationEvent {
    HostNotificationEvent {
        version: "1.0".to_string(),
        id: fallback_id(&data.message_id),
        topic: "im.message.received".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: Some(HostNotificationData::Direct(data)),
    }
}

fn mail_data(has_attachments: bool) -> DirectMessageNotificationData {
    DirectMessageNotificationData {
        channel: "mail".to_string(),
        source_kind: "mail".to_string(),
        message_id: "mail-msg-001".to_string(),
        recipient_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
        content_type: "mail.notification".to_string(),
        text: "Preview text".to_string(),
        mailbox_address: "alice@example.com".to_string(),
        mailbox_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
        from_addr: "sender@example.com".to_string(),
        subject: "Mail Subject".to_string(),
        preview: "Preview text".to_string(),
        has_attachments,
        ..DirectMessageNotificationData::default()
    }
}

fn fallback_id(value: &str) -> String {
    if value.trim().is_empty() {
        "event-1".to_string()
    } else {
        value.to_string()
    }
}

#[test]
fn openclaw_hook_request_json_shape_matches_go() {
    let request = build_openclaw_hook_request(
        &direct_event(DirectMessageNotificationData {
            message_id: "msg-001".to_string(),
            sender_did: "did:alice".to_string(),
            recipient_did: "did:bob".to_string(),
            content_type: "text/plain".to_string(),
            text: "hello".to_string(),
            ..DirectMessageNotificationData::default()
        }),
        FIXED_HOOK_NAME,
        "telegram",
        "123456",
    );

    assert_eq!(
        serde_json::to_value(&request).expect("request json"),
        json!({
            "message": request.message,
            "name": "AWiki",
            "wakeMode": "now",
            "deliver": true,
            "channel": "telegram",
            "to": "123456"
        })
    );
}

#[test]
fn openclaw_host_notify_sink_posts_each_route_and_succeeds_if_any_delivery_succeeds_like_go() {
    let _env_lock = ENV_LOCK.lock().expect("env lock");
    let workspace = TempDir::new("openclaw-host-notify-success").expect("temp workspace");
    let config_path = workspace.path().join("config.yaml");
    std::fs::write(
        &config_path,
        "runtime:\n  host_notify:\n    openclaw:\n      token: config-token\n",
    )
    .expect("write config");
    let server = MultiHttpCaptureServer::new(vec![
        b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: 4\r\n\r\nboom"
            .to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 28\r\n\r\n{\"ok\":true,\"runId\":\"run-2\"}"
            .to_vec(),
    ])
    .expect("server");
    let resolved =
        resolved_for_workspace(workspace.path(), &config_path, &server.url("/hooks/agent"));
    openclaw_routes::write_routes(
        &resolved.paths,
        &[
            Route {
                channel: "feishu".to_string(),
                to: "chat-one".to_string(),
            },
            Route {
                channel: "telegram".to_string(),
                to: "chat-two".to_string(),
            },
        ],
    )
    .expect("write routes");
    let sink = new_openclaw_host_notify_sink(&resolved).expect("sink");

    sink.notify(&direct_event(DirectMessageNotificationData {
        message_id: "msg-001".to_string(),
        sender_handle: "alice".to_string(),
        sender_did: "did:alice".to_string(),
        recipient_handle: "bob".to_string(),
        recipient_did: "did:bob".to_string(),
        content_type: "text/plain".to_string(),
        text: "hello".to_string(),
        ..DirectMessageNotificationData::default()
    }))
    .expect("notify succeeds after one accepted route");
    sink.close().expect("close");

    let requests = server.request_texts().expect("requests");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /hooks/agent HTTP/1.1\r\n"));
    assert!(requests[0].contains("Content-Type: application/json\r\n"));
    assert!(requests[0].contains("Authorization: Bearer config-token\r\n"));
    assert!(requests[1].contains("Authorization: Bearer config-token\r\n"));

    let first = posted_json(&requests[0]);
    assert_eq!(first["name"], "AWiki");
    assert_eq!(first["wakeMode"], "now");
    assert_eq!(first["deliver"], true);
    assert_eq!(first["channel"], "feishu");
    assert_eq!(first["to"], "chat-one");
    assert!(first["message"]
        .as_str()
        .unwrap_or_default()
        .contains("Sender handle: alice"));

    let second = posted_json(&requests[1]);
    assert_eq!(second["channel"], "telegram");
    assert_eq!(second["to"], "chat-two");
}

#[test]
fn openclaw_host_notify_sink_reports_per_route_failures_when_all_deliveries_fail_like_go() {
    let _env_lock = ENV_LOCK.lock().expect("env lock");
    let workspace = TempDir::new("openclaw-host-notify-failure").expect("temp workspace");
    let config_path = workspace.path().join("config.yaml");
    std::fs::write(&config_path, "runtime:\n  host_notify:\n    openclaw: {}\n")
        .expect("write config");
    let server = MultiHttpCaptureServer::new(vec![
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: 12\r\n\r\n  no route  "
            .to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"ok\":false}"
            .to_vec(),
    ])
    .expect("server");
    let resolved =
        resolved_for_workspace(workspace.path(), &config_path, &server.url("/hooks/agent"));
    openclaw_routes::write_routes(
        &resolved.paths,
        &[
            Route {
                channel: "feishu".to_string(),
                to: "chat-one".to_string(),
            },
            Route {
                channel: "telegram".to_string(),
                to: "chat-two".to_string(),
            },
        ],
    )
    .expect("write routes");
    let sink = new_openclaw_host_notify_sink(&resolved).expect("sink");

    let err = sink
        .notify(&direct_event(DirectMessageNotificationData {
            message_id: "msg-001".to_string(),
            sender_did: "did:alice".to_string(),
            recipient_did: "did:bob".to_string(),
            content_type: "text/plain".to_string(),
            text: "hello".to_string(),
            ..DirectMessageNotificationData::default()
        }))
        .expect_err("all routes fail");
    assert_eq!(
        err.to_string(),
        "openclaw notify failed: channel=feishu to=chat-one: openclaw hook failed status=503: no route; channel=telegram to=chat-two: openclaw hook was not accepted"
    );
}

#[test]
fn openclaw_host_notify_sink_errors_on_missing_routes_like_go() {
    let _env_lock = ENV_LOCK.lock().expect("env lock");
    let workspace = TempDir::new("openclaw-host-notify-empty").expect("temp workspace");
    let config_path = workspace.path().join("config.yaml");
    std::fs::write(&config_path, "runtime:\n  host_notify:\n    openclaw: {}\n")
        .expect("write config");
    let server = MultiHttpCaptureServer::new(Vec::new()).expect("server");
    let resolved =
        resolved_for_workspace(workspace.path(), &config_path, &server.url("/hooks/agent"));
    let sink = new_openclaw_host_notify_sink(&resolved).expect("sink");

    let err = sink
        .notify(&direct_event(DirectMessageNotificationData::default()))
        .expect_err("missing routes");
    assert_eq!(
        err.to_string(),
        "openclaw notify failed: no configured routes"
    );
}

#[test]
fn new_openclaw_host_notify_sink_validates_hook_url_like_go() {
    let _env_lock = ENV_LOCK.lock().expect("env lock");
    let workspace = TempDir::new("openclaw-host-notify-invalid").expect("temp workspace");
    let config_path = workspace.path().join("config.yaml");
    std::fs::write(&config_path, "runtime:\n  host_notify:\n    openclaw: {}\n")
        .expect("write config");
    let resolved = resolved_for_workspace(
        workspace.path(),
        &config_path,
        "https://example.com/hooks/agent",
    );

    let err = new_openclaw_host_notify_sink(&resolved).expect_err("remote host rejected");
    assert_eq!(
        err.to_string(),
        "runtime.host_notify.openclaw.hook_url must use a loopback host"
    );
}

fn resolved_for_workspace(
    workspace: &std::path::Path,
    config_path: &std::path::Path,
    hook_url: &str,
) -> Resolved {
    let state_dir = workspace.join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    Resolved {
        paths: Paths {
            workspace_home_dir: workspace.to_string_lossy().into_owned(),
            root_dir: String::new(),
            config_dir: String::new(),
            data_dir: String::new(),
            state_dir: state_dir.to_string_lossy().into_owned(),
            cache_dir: String::new(),
            logs_dir: String::new(),
            config_file: config_path.to_string_lossy().into_owned(),
            identity_dir: String::new(),
            database_file: String::new(),
            legacy_credentials_dir: String::new(),
            legacy_data_dir: String::new(),
        },
        config_schema_version: 1,
        active_identity: String::new(),
        runtime_mode: String::new(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "openclaw".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: hook_url.to_string(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: String::new(),
        no_color: false,
        service_base_url: String::new(),
        did_domain: String::new(),
        anp_service_endpoint: String::new(),
        anp_service_did: String::new(),
        mail_service_url: String::new(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: true,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: BTreeMap::from([(
            "host_notify_openclaw_hook_url".to_string(),
            ValueSource {
                source: "config_file".to_string(),
                key: String::new(),
                value: hook_url.to_string(),
            },
        )]),
    }
}

fn posted_json(request: &str) -> serde_json::Value {
    serde_json::from_str(request.split("\r\n\r\n").nth(1).expect("body")).expect("json")
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{prefix}-{}-{nonce}",
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

struct MultiHttpCaptureServer {
    url: String,
    join: Option<thread::JoinHandle<Vec<String>>>,
}

impl MultiHttpCaptureServer {
    fn new(responses: Vec<Vec<u8>>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let join = thread::spawn(move || {
            let mut requests = Vec::with_capacity(responses.len());
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept");
                requests.push(read_request(&mut stream));
                stream.write_all(&response).expect("write response");
            }
            requests
        });
        Ok(Self {
            url: format!("http://{addr}"),
            join: Some(join),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.url, path)
    }

    fn request_texts(mut self) -> std::thread::Result<Vec<String>> {
        self.join.take().expect("server join").join()
    }
}

impl Drop for MultiHttpCaptureServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn read_request(stream: &mut impl Read) -> String {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if request_complete(&raw) {
            break;
        }
    }
    String::from_utf8(raw).expect("request utf8")
}

fn request_complete(raw: &[u8]) -> bool {
    let Some(split) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&raw[..split]);
    let content_length = headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    match content_length {
        Some(length) => raw.len() >= split + 4 + length,
        None => true,
    }
}
