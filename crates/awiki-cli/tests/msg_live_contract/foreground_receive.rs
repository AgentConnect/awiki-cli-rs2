//! Command-level receive regressions. All identities and HTTP responses are synthetic.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct ReceiveState {
    anchor: Option<u64>,
    tail: u64,
    messages: Vec<u64>,
    sparse_pages: bool,
    fail_bootstrap: bool,
    methods: Vec<String>,
    websocket_requests: usize,
}

struct ReceiveServer {
    address: String,
    state: Arc<Mutex<ReceiveState>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ReceiveServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(ReceiveState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&state);
        let stopped = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let request = read_http_request(&mut stream);
                        let body = receive_response(&request, &shared);
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(), body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("receive fixture accept failed: {error}"),
                }
            }
        });
        Self {
            address,
            state,
            stop,
            worker: Some(worker),
        }
    }

    fn enqueue(&self, seq: u64) {
        let mut state = self.state.lock().unwrap();
        assert!(seq > state.tail);
        state.tail = seq;
        state.messages.push(seq);
    }
}

impl Drop for ReceiveServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

fn receive_response(request: &str, state: &Arc<Mutex<ReceiveState>>) -> String {
    let rpc: Value = serde_json::from_str(request_body(request)).unwrap();
    let method = rpc["method"].as_str().unwrap();
    let mut state = state.lock().unwrap();
    state.methods.push(method.to_owned());
    if request.to_ascii_lowercase().contains("upgrade: websocket") {
        state.websocket_requests += 1;
    }
    match method {
        "register" => registration_response(request),
        "direct.e2ee.publish_prekey_bundle" => prekey_publication_response(request),
        "anp.get_capabilities" => dynamic_response_body(request, "__DYNAMIC_CAPABILITIES_RESPONSE__"),
        "sync.bootstrap" if state.fail_bootstrap => rpc_result_for_request(request, json!({"invalid":true})),
        "sync.bootstrap" => {
            let tail = state.tail;
            let anchor = *state.anchor.get_or_insert(tail);
            let mut response: Value = serde_json::from_str(&dynamic_response_body(request, "__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__")).unwrap();
            response["result"]["cursor"]["scan_seq"] = json!(anchor.to_string());
            response.to_string()
        }
        "sync.delta" => {
            let cursor = rpc["params"]["body"]["cursor"]["scan_seq"].as_str().unwrap().parse::<u64>().unwrap();
            let anchor = state.anchor.expect("bootstrap must precede delta");
            let sparse = state.sparse_pages && cursor < 20;
            let next = if sparse { cursor + 1 } else { state.tail };
            let template: Value = serde_json::from_str(&dynamic_response_body(request, "__DYNAMIC_SYNC_DELTA_DIRECT_RESPONSE__")).unwrap();
            let events: Vec<Value> = state.messages.iter().filter(|seq| **seq > cursor && **seq > anchor && **seq <= next).map(|seq| {
                let mut event = template["result"]["events"][0].clone();
                event["event_id"] = json!(format!("event-direct-{seq}"));
                event["event_seq"] = json!(seq.to_string());
                event["aggregate_id"] = json!(format!("msg-direct-{seq}"));
                event["payload"]["client_message_id"] = json!(format!("msg-direct-{seq}"));
                event
            }).collect();
            rpc_result_for_request(request, json!({
                "mode":"delta", "server_time":"2026-09-07T00:00:00Z",
                "events":events, "next_cursor":{"stream_epoch":"1","scan_seq":next.to_string()},
                "has_more":sparse, "recovery":null, "warnings":[]
            }))
        }
        "message.get_batch" => {
            let binding = device_binding_from_request(request);
            let items: Vec<Value> = rpc["params"]["body"]["event_ids"].as_array().unwrap().iter().map(|id| {
                let seq = id.as_str().unwrap().rsplit('-').next().unwrap();
                let mut message = direct_message(&binding.did);
                message["id"] = json!(format!("msg-direct-{seq}"));
                message["client_msg_id"] = json!(format!("msg-direct-{seq}"));
                message["server_seq"] = json!(seq);
                json!({"event_id":id,"message":message})
            }).collect();
            rpc_result_for_request(request, json!({"items":items,"unavailable":[]}))
        }
        value if value.contains("lookup") => dynamic_response_body(request, "__DYNAMIC_DIRECTORY_LOOKUP_RESPONSE__"),
        _ => json!({"jsonrpc":"2.0","id":rpc["id"],"error":{"code":-32601,"message":"synthetic method unavailable"}}).to_string(),
    }
}

fn receive_command(workspace: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_awiki-cli"))
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        // These cases isolate ordinary messages; P5/P6 are separate contracts.
        .env("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED", "0")
        .env("AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED", "0")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AWIKI_FORMAT")
        .output()
        .unwrap()
}

fn receive_workspace(server: &ReceiveServer) -> TempDir {
    let workspace = TempDir::new("foreground-receive").unwrap();
    write_msg_config(workspace.path(), &server.address);
    // Exercise WebSocket-preferred mode with no listener/bridge present.
    write_tenant_config(workspace.path(), "runtime:\n  mode: websocket\n");
    let registered = register_receiver(workspace.path());
    assert_success(&registered);
    workspace
}

fn register_receiver(workspace: &Path) -> Output {
    receive_command(
        workspace,
        &[
            "id",
            "register",
            "--handle",
            "bob",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
    )
}

fn receive_inbox(workspace: &Path) -> Value {
    success_json(&receive_command(
        workspace,
        &["--identity", "bob", "msg", "inbox", "--scope", "direct"],
    ))
}

#[test]
fn foreground_http_receives_new_message_without_websocket_after_bootstrap() {
    let server = ReceiveServer::new();
    let workspace = receive_workspace(&server);
    assert_eq!(receive_inbox(workspace.path())["data"]["total"], 0);
    server.enqueue(1);
    let inbox = receive_inbox(workspace.path());
    assert_eq!(inbox["data"]["total"], 1);
    assert_eq!(inbox["data"]["messages"][0]["id"], "msg-direct-1");
    let history = success_json(&receive_command(
        workspace.path(),
        &[
            "--identity",
            "bob",
            "msg",
            "history",
            "--with",
            "did:wba:awiki.ai:alice:e1_alice",
        ],
    ));
    assert_eq!(history["data"]["messages"][0]["id"], "msg-direct-1");
    assert_eq!(receive_inbox(workspace.path())["data"]["total"], 1);
    let state = server.state.lock().unwrap();
    assert_eq!(state.websocket_requests, 0);
    assert!(state.methods.iter().any(|method| method == "sync.delta"));
    println!("HTTP control: one incoming message, history agrees, no duplicates, no WebSocket");
}

#[test]
fn foreground_pending_pagination_must_not_be_reported_as_empty_success() {
    let server = ReceiveServer::new();
    let workspace = receive_workspace(&server);
    receive_inbox(workspace.path());
    server.enqueue(21);
    server.state.lock().unwrap().sparse_pages = true;
    let output = receive_command(
        workspace.path(),
        &["--identity", "bob", "msg", "inbox", "--scope", "direct"],
    );
    let envelope: Value = serde_json::from_slice(if output.status.success() {
        &output.stdout
    } else {
        &output.stderr
    })
    .unwrap();
    let pending = query_rows(
        workspace.path(),
        "SELECT sync_pending, last_result_json FROM message_sync_run_state",
    );
    println!(
        "pagination diagnostic: exit={:?}, total={}, warnings={}, durable_state={pending:?}",
        output.status.code(),
        envelope["data"]["total"],
        envelope["warnings"]
    );
    assert_eq!(server.state.lock().unwrap().websocket_requests, 0);
    let signals_incomplete = !output.status.success()
        || envelope["warnings"]
            .to_string()
            .contains("sync.budget_exhausted");
    assert!(signals_incomplete, "CLI reported an empty success while an incoming message remained beyond its sync page budget");
    assert!(envelope.to_string().contains("sync.budget_exhausted"));
    let resumed = receive_inbox(workspace.path());
    assert_eq!(resumed["data"]["total"], 1);
    assert_eq!(resumed["data"]["messages"][0]["id"], "msg-direct-21");
}

#[test]
fn foreground_registration_must_not_lose_messages_before_first_inbox() {
    let server = ReceiveServer::new();
    let workspace = receive_workspace(&server);
    let registration_anchor = server.state.lock().unwrap().anchor;
    server.enqueue(1);
    server.enqueue(2);
    let inbox = receive_inbox(workspace.path());
    let anchor = server.state.lock().unwrap().anchor;
    // Retrying the same installation cannot move the history floor or restore old rows.
    let retry = receive_inbox(workspace.path());
    server.enqueue(3);
    let fresh = receive_inbox(workspace.path());
    println!("initialization diagnostic: anchor_after_registration={registration_anchor:?}, first_inbox_anchor={anchor:?}, first_total={}, retry_total={}, after_new_message_total={}", inbox["data"]["total"], retry["data"]["total"], fresh["data"]["total"]);
    assert_eq!(fresh["data"]["messages"][0]["id"], "msg-direct-3");
    assert_eq!(server.state.lock().unwrap().websocket_requests, 0);
    assert_eq!(
        inbox["data"]["total"], 2,
        "registration reported success before the receiving installation anchor existed"
    );
}

#[test]
fn foreground_registration_sync_failure_keeps_identity_and_inbox_can_resume() {
    let server = ReceiveServer::new();
    server.state.lock().unwrap().fail_bootstrap = true;
    let workspace = TempDir::new("registration-receive-pending").unwrap();
    write_msg_config(workspace.path(), &server.address);
    let output = register_receiver(workspace.path());
    assert!(!output.status.success());
    let envelope: Value = serde_json::from_slice(if output.status.success() {
        &output.stdout
    } else {
        &output.stderr
    })
    .unwrap();
    assert!(envelope
        .to_string()
        .contains("registration_receive_pending"));
    assert!(envelope.to_string().contains("committed"));
    server.state.lock().unwrap().fail_bootstrap = false;
    assert_eq!(receive_inbox(workspace.path())["data"]["total"], 0);
    server.enqueue(1);
    assert_eq!(receive_inbox(workspace.path())["data"]["total"], 1);
    assert_eq!(
        server
            .state
            .lock()
            .unwrap()
            .methods
            .iter()
            .filter(|method| method.as_str() == "register")
            .count(),
        1
    );
}
