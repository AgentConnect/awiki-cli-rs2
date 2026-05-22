use awiki_cli::im_core_adapter::realtime::{
    listener_runner_selection, ListenerRunHostKind, ListenerRunnerAction, ListenerRunnerMode,
};
use awiki_cli::runtime::listener_foreground::{
    listener_accept_loop_step, listener_foreground_run_plan,
    listener_foreground_run_plan_with_sdk_runner, listener_start_socket_plan,
    ListenerAcceptLoopAction, ListenerAcceptLoopDecision, ListenerAcceptLoopEvent,
    ListenerForegroundDecision, ListenerForegroundRunAction, ListenerStartSocketAction,
};
use awiki_cli::{
    config::{Paths, Resolved},
    runtime::listener_handle_lookup::lookup_listener_handle_by_did,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn run_rejects_non_websocket_mode_before_side_effects_like_go() {
    let plan = listener_foreground_run_plan(
        "http",
        "/tmp/awiki.sock",
        Some("pid should not run"),
        Some("status should not run"),
        Some("listen should not run"),
        Some("sessions should not run"),
    );

    assert_eq!(
        plan.actions,
        vec![ListenerForegroundRunAction::ValidateWebSocketMode {
            mode: "http".to_string(),
        }]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError(
            "runtime mode must be websocket before starting the listener".to_string(),
        )
    );
}

#[test]
fn run_writes_pid_then_status_before_starting_socket() {
    let plan = listener_foreground_run_plan("websocket", "/tmp/awiki.sock", None, None, None, None);

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            }),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::StoreListener),
            ListenerForegroundRunAction::StartSocket(
                ListenerStartSocketAction::SetBridgeAvailable { available: true },
            ),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::SpawnAcceptLoop),
            ListenerForegroundRunAction::StartKnownSessions,
            ListenerForegroundRunAction::SpawnWatchNewIdentities,
            ListenerForegroundRunAction::WaitForContextDone,
        ]
    );
    assert_eq!(plan.decision, ListenerForegroundDecision::ReturnOk);
}

#[test]
fn im_core_mvp_run_keeps_cli_host_setup_then_runs_sdk_runner() {
    let plan = listener_foreground_run_plan_with_sdk_runner(
        "websocket",
        "/tmp/awiki.sock",
        true,
        None,
        None,
        None,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            }),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::StoreListener),
            ListenerForegroundRunAction::StartSocket(
                ListenerStartSocketAction::SetBridgeAvailable { available: true },
            ),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::SpawnAcceptLoop),
            ListenerForegroundRunAction::StartKnownSessions,
            ListenerForegroundRunAction::SpawnWatchNewIdentities,
            ListenerForegroundRunAction::RunImCoreRealtimeRunner,
            ListenerForegroundRunAction::WaitForContextDone,
        ]
    );
    assert_eq!(plan.decision, ListenerForegroundDecision::ReturnOk);
}

#[test]
fn listener_runner_selection_keeps_legacy_default_and_flags_sdk_hosts() {
    let legacy = listener_runner_selection(false, ListenerRunHostKind::Foreground);
    assert_eq!(legacy.mode, ListenerRunnerMode::Legacy);
    assert_eq!(
        legacy.actions,
        vec![ListenerRunnerAction::UseLegacySupervisor]
    );

    let foreground = listener_runner_selection(true, ListenerRunHostKind::Foreground);
    assert_eq!(foreground.mode, ListenerRunnerMode::ImCore);
    assert_eq!(
        foreground.actions,
        vec![ListenerRunnerAction::UseImCoreRunner {
            host: ListenerRunHostKind::Foreground,
        }]
    );

    let service = listener_runner_selection(true, ListenerRunHostKind::Service);
    assert_eq!(service.mode, ListenerRunnerMode::ImCore);
    assert_eq!(
        service.actions,
        vec![ListenerRunnerAction::UseImCoreRunner {
            host: ListenerRunHostKind::Service,
        }]
    );
}

#[test]
fn pid_error_stops_before_status_write_like_go() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        Some("write pid failed"),
        None,
        None,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("write pid failed".to_string())
    );
}

#[test]
fn status_error_stops_before_socket_start_like_go() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        None,
        Some("write status failed"),
        None,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("write status failed".to_string())
    );
}

#[test]
fn start_socket_listen_error_stops_before_listener_store_and_bridge_available() {
    let plan = listener_start_socket_plan("/tmp/awiki.sock", Some("listen failed"));

    assert_eq!(
        plan.actions,
        vec![ListenerStartSocketAction::ListenBridge {
            socket_path: "/tmp/awiki.sock".to_string(),
        }]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("listen failed".to_string())
    );
}

#[test]
fn start_socket_success_stores_listener_sets_bridge_available_and_spawns_accept_loop() {
    let plan = listener_start_socket_plan("/tmp/awiki.sock", None);

    assert_eq!(
        plan.actions,
        vec![
            ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            },
            ListenerStartSocketAction::StoreListener,
            ListenerStartSocketAction::SetBridgeAvailable { available: true },
            ListenerStartSocketAction::SpawnAcceptLoop,
        ]
    );
    assert_eq!(plan.decision, ListenerForegroundDecision::ReturnOk);
}

#[test]
fn run_propagates_socket_listen_error_before_starting_known_sessions() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        None,
        None,
        Some("listen failed"),
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            }),
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("listen failed".to_string())
    );
}

#[test]
fn known_sessions_error_stops_before_identity_watch_like_go() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        None,
        None,
        None,
        Some("list identities failed"),
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            }),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::StoreListener),
            ListenerForegroundRunAction::StartSocket(
                ListenerStartSocketAction::SetBridgeAvailable { available: true },
            ),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::SpawnAcceptLoop),
            ListenerForegroundRunAction::StartKnownSessions,
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("list identities failed".to_string())
    );
}

#[test]
fn accept_loop_accepted_connection_spawns_handle_conn_and_continues() {
    let step = listener_accept_loop_step(ListenerAcceptLoopEvent::Accepted {
        connection_id: "conn-1".to_string(),
    });

    assert_eq!(
        step.actions,
        vec![ListenerAcceptLoopAction::SpawnHandleConn {
            connection_id: "conn-1".to_string(),
        }]
    );
    assert_eq!(step.decision, ListenerAcceptLoopDecision::Continue);
}

#[test]
fn accept_loop_accept_error_returns_without_spawning_handler() {
    let step = listener_accept_loop_step(ListenerAcceptLoopEvent::AcceptError {
        error: "listener closed".to_string(),
    });

    assert!(step.actions.is_empty());
    assert_eq!(step.decision, ListenerAcceptLoopDecision::Exit);
}

#[test]
fn listener_handle_lookup_posts_did_rpc_and_returns_raw_handle_for_sync_normalization() {
    let did = "did:wba:awiki.ai:user:bob:e1_bob";
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"handle":"Bob.Remote","did":"did:wba:awiki.ai:user:bob:e1_bob","domain":"awiki.ai","status":"active","full_handle":"Bob.Remote.awiki.ai"},"id":"req-1"}"#,
    )]);
    let resolved = test_resolved(&server.base_url());

    let handle = lookup_listener_handle_by_did(&resolved, did)
        .expect("lookup succeeds")
        .expect("handle present");

    assert_eq!(handle, "Bob.Remote");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /user-service/handle/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "lookup",
            "params": { "did": did },
        })
    );
}

#[test]
fn listener_handle_lookup_treats_not_found_and_empty_results_like_go() {
    let did = "did:wba:awiki.ai:user:missing:e1_missing";
    let server = TestServer::new(vec![
        TestResponse::new(404, "missing"),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","error":{"code":-32002,"message":"not found"},"id":"req-1"}"#,
        ),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"handle":"","did":"did:wba:awiki.ai:user:missing:e1_missing"},"id":"req-1"}"#,
        ),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"handle":"missing","did":""},"id":"req-1"}"#,
        ),
    ]);
    let resolved = test_resolved(&server.base_url());

    for _ in 0..4 {
        assert_eq!(
            lookup_listener_handle_by_did(&resolved, did).expect("not-found lookup succeeds"),
            None
        );
    }

    assert_eq!(server.requests().len(), 4);
}

#[test]
fn listener_handle_lookup_rejects_blank_did_before_http_like_go() {
    let err = lookup_listener_handle_by_did(&test_resolved("http://127.0.0.1:9"), "   ")
        .expect_err("blank did fails");

    assert!(err.to_string().contains("did is required"));
}

#[derive(Debug, Clone)]
struct TestResponse {
    status: u16,
    body: String,
}

impl TestResponse {
    fn new(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
        }
    }

    fn ok(body: &str) -> Self {
        Self::new(200, body)
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

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn test_resolved(service_base_url: &str) -> Resolved {
    Resolved {
        paths: Paths {
            workspace_home_dir: String::new(),
            root_dir: String::new(),
            config_dir: String::new(),
            data_dir: String::new(),
            state_dir: String::new(),
            cache_dir: String::new(),
            logs_dir: String::new(),
            config_file: String::new(),
            identity_dir: String::new(),
            database_file: String::new(),
            legacy_credentials_dir: String::new(),
            legacy_data_dir: String::new(),
        },
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
        service_base_url: service_base_url.to_string(),
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
