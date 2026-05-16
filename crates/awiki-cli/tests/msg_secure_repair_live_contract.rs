use awiki_cli::config::Paths;
use awiki_cli::identity::{generate_identity, types::SaveInput, Manager};
use awiki_cli::message::new_secure_e2ee_client_for_record;
use awiki_cli::store::{self, E2EEOutboxRecord};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn msg_secure_repair_live_resets_peer_state_requeues_failed_outbox_and_starts_init_like_go() {
    let workspace = TempDir::new("msg-live-secure-repair").expect("workspace");
    write_msg_config(workspace.path(), "https://placeholder.invalid");
    let manager = Manager::new(test_paths(workspace.path()));
    let alice = register_generated_msg_identity(&manager, "alice-repair", "alice", "jwt-alice");
    let bob = register_generated_msg_identity(&manager, "bob-repair", "bob", "jwt-bob");
    let carol = register_generated_msg_identity(&manager, "carol-repair", "carol", "jwt-carol");

    seed_established_secure_session(&manager, "alice-repair", &alice, &bob);
    seed_secure_outbox_row(
        workspace.path(),
        &alice.did,
        &bob.did,
        "repair-failed-bob",
        "failed",
        "failed repair live",
        "2026-05-16T00:00:00Z",
        "alice-repair",
    );
    seed_secure_outbox_row(
        workspace.path(),
        &alice.did,
        &carol.did,
        "repair-failed-carol",
        "failed",
        "do not requeue",
        "2026-05-16T00:01:00Z",
        "alice-repair",
    );

    let mut bob_seed = new_secure_e2ee_client_for_record(
        Some(&manager),
        Some(&bob),
        Box::new(|method, _params| {
            assert_eq!(method, "direct.e2ee.publish_prekey_bundle");
            Ok(Map::new())
        }),
    )
    .expect("construct bob seed client");
    let bob_bundle = bob_seed
        .ensure_fresh_prekey_bundle()
        .expect("seed bob prekey bundle");
    let bob_opk = first_one_time_prekey(&manager, "bob-repair");
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({}))),
        TestResponse::ok(&json_rpc_result(json!({}))),
        TestResponse::ok(&json_rpc_result(json!({}))),
        TestResponse::ok(&json_rpc_result(json!({}))),
        TestResponse::ok(&json_rpc_result(json!({
            "target_did": bob.did,
            "prekey_bundle": bob_bundle,
            "one_time_prekey": bob_opk,
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "accepted_at": "2026-05-16T01:02:03Z",
            "delivery_state": "accepted"
        }))),
    ]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-repair",
            "msg",
            "secure",
            "repair",
            "--with",
            &bob.did,
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        format!("Repaired secure session with {}", bob.did)
    );
    assert_eq!(envelope["data"]["initialized"], true);
    assert_eq!(envelope["data"]["target"]["did"], bob.did);
    assert_eq!(envelope["data"]["repair"]["peer_did"], bob.did);
    assert_eq!(envelope["data"]["repair"]["peer_handle"], "");
    assert_eq!(envelope["data"]["repair"]["reset_records"], 2);
    assert_eq!(envelope["data"]["session"]["peer_did"], bob.did);
    assert_eq!(
        envelope["data"]["session"]["status"],
        "pending-confirmation"
    );
    let message_id = envelope["data"]["delivery"]["message_id"]
        .as_str()
        .expect("delivery message id");
    assert!(message_id.starts_with("secure-init-"));
    assert_eq!(envelope["data"]["delivery"]["operation_id"], message_id);
    assert_eq!(envelope["data"]["delivery"]["target_did"], bob.did);

    let requests = server.requests();
    assert_eq!(requests.len(), 6);
    let bodies = requests
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).expect("json body"))
        .collect::<Vec<_>>();
    assert_eq!(bodies[0]["method"], "direct.e2ee.publish_prekey_bundle");
    assert_eq!(bodies[1]["method"], "direct.e2ee.publish_prekey_bundle");
    assert_eq!(bodies[2]["method"], "direct.e2ee.publish_prekey_bundle");
    assert_eq!(bodies[3]["method"], "direct.e2ee.publish_prekey_bundle");
    assert_eq!(bodies[4]["method"], "direct.e2ee.get_prekey_bundle");
    assert_eq!(bodies[4]["params"]["body"]["target_did"], bob.did);
    assert_eq!(bodies[5]["method"], "direct.send");
    assert_eq!(bodies[5]["params"]["meta"]["sender_did"], alice.did);
    assert_eq!(bodies[5]["params"]["meta"]["target"]["did"], bob.did);
    assert_eq!(
        bodies[5]["params"]["meta"]["content_type"],
        "application/anp-direct-init+json"
    );
    assert_eq!(bodies[5]["params"]["meta"]["message_id"], message_id);
    assert_eq!(bodies[5]["params"]["meta"]["operation_id"], message_id);
    assert_eq!(bodies[5]["params"].get("auth"), None);

    assert_old_session_removed_and_new_pending_session_exists(&manager, "alice-repair", &bob.did);
    let bob_rows = query_rows(
        workspace.path(),
        "SELECT outbox_id, local_status, peer_did FROM e2ee_outbox WHERE outbox_id = 'repair-failed-bob'",
    );
    assert_eq!(bob_rows.len(), 1);
    assert_eq!(bob_rows[0]["local_status"], "queued");
    assert_eq!(bob_rows[0]["peer_did"], bob.did);
    let carol_rows = query_rows(
        workspace.path(),
        "SELECT outbox_id, local_status, peer_did FROM e2ee_outbox WHERE outbox_id = 'repair-failed-carol'",
    );
    assert_eq!(carol_rows.len(), 1);
    assert_eq!(carol_rows[0]["local_status"], "failed");
    assert_eq!(carol_rows[0]["peer_did"], carol.did);
}

fn register_generated_msg_identity(
    manager: &Manager,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> awiki_cli::identity::types::StoredIdentity {
    let generated = generate_identity(
        "awiki.ai",
        "https://awiki.ai/anp-im/rpc",
        "did:wba:awiki.ai",
    )
    .expect("generate identity");
    manager
        .save(SaveInput {
            identity_name: identity_name.to_string(),
            did: generated.did,
            unique_id: generated.unique_id,
            user_id: format!("user-{handle}"),
            display_name: identity_name.to_string(),
            handle: handle.to_string(),
            full_handle: format!("{handle}.awiki.ai"),
            jwt_token: jwt_token.to_string(),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            ..SaveInput::default()
        })
        .expect("save generated message identity")
}

fn write_msg_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("services:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn test_paths(workspace: &Path) -> Paths {
    for directory in ["data", "runtime", "cache", "logs"] {
        std::fs::create_dir_all(workspace.join(directory)).expect("create workspace subdir");
    }
    Paths {
        workspace_home_dir: path_string(workspace),
        root_dir: path_string(workspace),
        config_dir: path_string(workspace),
        data_dir: path_string(&workspace.join("data")),
        state_dir: path_string(&workspace.join("runtime")),
        cache_dir: path_string(&workspace.join("cache")),
        logs_dir: path_string(&workspace.join("logs")),
        config_file: path_string(&workspace.join("config.yaml")),
        identity_dir: path_string(&workspace.join("identities")),
        database_file: path_string(&workspace.join("data").join("awiki-cli.db")),
        legacy_credentials_dir: path_string(&workspace.join("legacy-credentials")),
        legacy_data_dir: path_string(&workspace.join("legacy-data")),
    }
}

fn first_one_time_prekey(
    manager: &Manager,
    identity_name: &str,
) -> awiki_cli::anpsdk::OneTimePrekey {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let mut prekeys = std::fs::read_dir(Path::new(&paths.identity_dir).join("p5-one-time-prekeys"))
        .expect("read one-time prekey root")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .map(|path| {
            serde_json::from_slice(&std::fs::read(&path).expect("read one-time prekey json"))
                .expect("parse one-time prekey json")
        })
        .collect::<Vec<_>>();
    prekeys
        .sort_by(|left: &awiki_cli::anpsdk::OneTimePrekey, right| left.key_id.cmp(&right.key_id));
    prekeys.into_iter().next().expect("at least one OPK")
}

fn seed_established_secure_session(
    manager: &Manager,
    identity_name: &str,
    owner: &awiki_cli::identity::types::StoredIdentity,
    peer: &awiki_cli::identity::types::StoredIdentity,
) {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let root = Path::new(&paths.identity_dir).join("p5-e2ee-sessions");
    let mut store = awiki_cli::anpsdk::FileSessionStore::new(&root).expect("session store");
    store
        .save_session(&awiki_cli::anpsdk::DirectSessionState {
            session_id: "old-session".to_string(),
            suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_string(),
            peer_did: peer.did.clone(),
            local_key_agreement_id: format!("{}#key-3", owner.did),
            peer_key_agreement_id: format!("{}#key-3", peer.did),
            root_key_b64u: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
            send_chain_key_b64u: Some("AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE".to_string()),
            recv_chain_key_b64u: Some("AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI".to_string()),
            ratchet_private_key_b64u: "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM".to_string(),
            ratchet_public_key_b64u: "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ".to_string(),
            peer_ratchet_public_key_b64u: Some(
                "BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU".to_string(),
            ),
            send_n: 1,
            recv_n: 1,
            previous_send_chain_length: 0,
            skipped_message_keys: Vec::new(),
            is_initiator: true,
            status: "established".to_string(),
        })
        .expect("save established session");
}

fn seed_secure_outbox_row(
    workspace: &Path,
    owner_did: &str,
    peer_did: &str,
    outbox_id: &str,
    local_status: &str,
    plaintext: &str,
    created_at: &str,
    credential_name: &str,
) {
    let paths = test_paths(workspace);
    let connection = store::open(&paths).expect("open store");
    store::ensure_schema(&connection).expect("ensure store schema");
    store::queue_e2ee_outbox(
        &connection,
        E2EEOutboxRecord {
            outbox_id: outbox_id.to_string(),
            owner_did: owner_did.to_string(),
            peer_did: peer_did.to_string(),
            session_id: "old-session".to_string(),
            original_type: "text".to_string(),
            plaintext: plaintext.to_string(),
            local_status: local_status.to_string(),
            last_error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            created_at: created_at.to_string(),
            updated_at: created_at.to_string(),
            credential_name: credential_name.to_string(),
            ..E2EEOutboxRecord::default()
        },
    )
    .expect("seed secure outbox row");
}

fn assert_old_session_removed_and_new_pending_session_exists(
    manager: &Manager,
    identity_name: &str,
    peer_did: &str,
) {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let sessions = std::fs::read_dir(Path::new(&paths.identity_dir).join("p5-e2ee-sessions"))
        .expect("read session root")
        .filter_map(Result::ok)
        .map(|entry| {
            serde_json::from_slice::<Value>(&std::fs::read(entry.path()).expect("read session"))
                .expect("parse session")
        })
        .collect::<Vec<_>>();
    assert!(!sessions
        .iter()
        .any(|session| session["session_id"] == "old-session"));
    assert!(sessions.iter().any(|session| {
        session["peer_did"] == peer_did
            && session["status"] == "pending-confirmation"
            && session["is_initiator"] == true
    }));
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
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

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn query_rows(workspace: &Path, sql: &str) -> Vec<Value> {
    let query = awiki_cmd(&["debug", "db", "query", sql], workspace);
    assert_success(&query);
    success_json(&query)["data"]["rows"]
        .as_array()
        .cloned()
        .unwrap()
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
