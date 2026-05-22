use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn identity_default_cutover_replace_did_executes_through_bridge_and_rebinds_local_state() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(register_alice_response()),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"user_id":"user-alice-replaced","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-replaced"},"id":"req-1"}"#,
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

    let old = read_stored_identity(workspace.path(), "alice");
    let old_did = old.index["did"].as_str().unwrap().to_string();
    let old_dir_name = old.index["dir_name"].as_str().unwrap().to_string();
    seed_store_rows(workspace.path(), &old_did);

    let replace = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice",
            "id",
            "replace-did",
            "--is-public=false",
            "--is-agent",
            "--role",
            "",
            "--endpoint-url",
            " https://agent.example/rpc ",
        ],
        workspace.path(),
        &[],
    ));

    assert_eq!(
        replace["summary"],
        "Identity alice DID replaced successfully"
    );
    assert_eq!(replace["data"]["action"], "replace_did");
    assert_eq!(replace["data"]["old_did"], old_did);
    let new_did = replace["data"]["did"].as_str().expect("new did");
    assert_e1_alice_did(new_did);
    assert_eq!(replace["data"]["identity"]["identity_name"], "alice");
    assert_eq!(replace["data"]["identity"]["did"], new_did);
    assert_eq!(replace["data"]["identity"]["handle"], "alice");
    assert_eq!(replace["data"]["identity"]["full_handle"], "alice.awiki.ai");
    assert_eq!(replace["data"]["identity"]["has_jwt"], true);
    assert_eq!(
        replace["data"]["store_rebind"],
        json!({
            "messages": 1,
            "contacts": 1,
            "contact_handle_bindings": 1,
            "relationship_events": 1,
            "groups": 1,
            "group_members": 1,
        })
    );
    assert_eq!(
        replace["data"]["e2ee_cleanup"],
        json!({
            "e2ee_outbox": 1,
            "e2ee_sessions": 1,
        })
    );
    assert!(replace["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap()
            .contains("Dangerous command: replace-did")));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert_contains_text(&requests[1], "Authorization: Bearer jwt-register\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body["method"], "replace_did");
    assert_eq!(body["params"]["is_public"], false);
    assert_eq!(body["params"]["is_agent"], true);
    assert!(body["params"]["role"].is_null());
    assert_eq!(body["params"]["endpoint_url"], "https://agent.example/rpc");
    assert!(body["params"]["new_did_document"].is_object());
    assert_eq!(body["params"]["new_did_document"]["id"], new_did);

    let backup_path = PathBuf::from(replace["data"]["backup_path"].as_str().unwrap());
    assert!(backup_path.join("backup_manifest.json").is_file());
    assert!(!workspace
        .path()
        .join(".awiki-cli")
        .join("identities")
        .join(old_dir_name)
        .exists());

    let stored = read_stored_identity(workspace.path(), "alice");
    assert_eq!(stored.index["did"], new_did);
    assert_eq!(stored.index["user_id"], "user-alice-replaced");
    assert_eq!(stored.identity["did"], new_did);
    assert_eq!(stored.identity["user_id"], "user-alice-replaced");
    assert_eq!(stored.auth["jwt_token"], "jwt-replaced");
    assert_key_file_not_empty(workspace.path(), "alice", "key-1-private.pem");
    assert_key_file_not_empty(workspace.path(), "alice", "key-1-public.pem");
    assert_key_file_not_empty(workspace.path(), "alice", "e2ee-signing-private.pem");
    assert_key_file_not_empty(workspace.path(), "alice", "e2ee-agreement-private.pem");

    let database_file = workspace
        .path()
        .join(".awiki-cli")
        .join("data")
        .join("awiki-cli.db");
    let connection =
        awiki_cli::store::open_read_only(&path_string(&database_file)).expect("open store");
    assert_owner_count(&connection, "messages", new_did, 1);
    assert_owner_count(&connection, "contacts", new_did, 1);
    assert_owner_count(&connection, "e2ee_outbox", &old_did, 0);
    assert_owner_count(&connection, "e2ee_sessions", &old_did, 0);
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

fn seed_store_rows(workspace: &Path, owner_did: &str) {
    let paths = identity_test_paths(&workspace.join(".awiki-cli"));
    let connection = awiki_cli::store::open(&paths).expect("open store");
    awiki_cli::store::ensure_schema(&connection).expect("store schema");
    seed_rebind_rows(&connection, owner_did).expect("seed rebind rows");
}

fn seed_rebind_rows(connection: &rusqlite::Connection, owner_did: &str) -> rusqlite::Result<()> {
    connection.execute(
        r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type,
    content, stored_at, credential_name
) VALUES (?1, ?2, ?3, 1, ?2, ?4, 'text', ?5, ?6, 'alice')
"#,
        rusqlite::params![
            "msg-1",
            owner_did,
            format!("dm:{owner_did}:did:peer"),
            "did:peer",
            "hello",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO contacts (
    owner_did, did, handle, first_seen_at, last_seen_at
) VALUES (?1, ?2, ?3, ?4, ?5)
"#,
        rusqlite::params![
            owner_did,
            "did:peer",
            "bob",
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO contact_handle_bindings (
    owner_did, handle, did, is_current, first_seen_at, last_seen_at, credential_name
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 'alice')
"#,
        rusqlite::params![
            owner_did,
            "bob",
            "did:peer",
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO relationship_events (
    event_id, owner_did, target_did, event_type, status, created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'alice')
"#,
        rusqlite::params![
            "event-1",
            owner_did,
            "did:peer",
            "recommended",
            "pending",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO groups (
    owner_did, group_id, name, group_mode, membership_status, stored_at, credential_name
) VALUES (?1, ?2, ?3, 'general', 'active', ?4, 'alice')
"#,
        rusqlite::params![owner_did, "group-1", "Group One", "2026-01-01T00:00:00Z"],
    )?;
    connection.execute(
        r#"
INSERT INTO group_members (
    owner_did, group_id, user_id, member_did, status, last_synced_at, credential_name
) VALUES (?1, ?2, ?3, ?4, 'active', ?5, 'alice')
"#,
        rusqlite::params![
            owner_did,
            "group-1",
            "user-1",
            "did:peer",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO e2ee_outbox (
    outbox_id, owner_did, peer_did, plaintext, created_at, updated_at, credential_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?5, 'alice')
"#,
        rusqlite::params![
            "out-1",
            owner_did,
            "did:peer",
            "secret",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    connection.execute(
        r#"
INSERT INTO e2ee_sessions (
    owner_did, peer_did, session_id, is_initiator, send_chain_key, recv_chain_key,
    send_seq, recv_seq, expires_at, created_at, active_at, peer_confirmed,
    credential_name, updated_at
) VALUES (?1, ?2, ?3, 1, ?4, ?5, 0, 0, NULL, ?6, NULL, 0, 'alice', ?6)
"#,
        rusqlite::params![
            owner_did,
            "did:peer",
            "session-1",
            "send-key",
            "recv-key",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    Ok(())
}

fn identity_test_paths(root: &Path) -> awiki_cli::config::Paths {
    let data_dir = root.join("data");
    awiki_cli::config::Paths {
        workspace_home_dir: path_string(root),
        root_dir: path_string(root),
        config_dir: path_string(&root.join("config")),
        data_dir: path_string(&data_dir),
        state_dir: path_string(&root.join("state")),
        cache_dir: path_string(&root.join("cache")),
        logs_dir: path_string(&root.join("logs")),
        config_file: path_string(&root.join("config").join("config.yaml")),
        identity_dir: path_string(&root.join("identities")),
        database_file: path_string(&data_dir.join("awiki-cli.db")),
        legacy_credentials_dir: path_string(&root.join("legacy")),
        legacy_data_dir: path_string(&root.join("legacy-data")),
    }
}

fn assert_owner_count(connection: &rusqlite::Connection, table: &str, owner_did: &str, want: i64) {
    let got: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE owner_did = ?1"),
            rusqlite::params![owner_did],
            |row| row.get(0),
        )
        .expect("owner count");
    assert_eq!(got, want, "owner count for {table}/{owner_did}");
}

struct StoredIdentity {
    index: Value,
    identity: Value,
    auth: Value,
}

fn read_stored_identity(workspace: &Path, identity_name: &str) -> StoredIdentity {
    let identity_root = workspace.join(".awiki-cli").join("identities");
    let index_path = identity_root.join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let entry = index["credentials"][identity_name].clone();
    let dir_name = entry["dir_name"].as_str().unwrap();
    let identity_dir = identity_root.join(dir_name);
    StoredIdentity {
        index: entry,
        identity: serde_json::from_slice(
            &std::fs::read(identity_dir.join("identity.json")).unwrap(),
        )
        .unwrap(),
        auth: serde_json::from_slice(&std::fs::read(identity_dir.join("auth.json")).unwrap())
            .unwrap(),
    }
}

fn assert_key_file_not_empty(workspace: &Path, identity_name: &str, file_name: &str) {
    let identity_root = workspace.join(".awiki-cli").join("identities");
    let index_path = identity_root.join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let content = std::fs::read_to_string(identity_root.join(dir_name).join(file_name))
        .unwrap_or_else(|err| panic!("read {file_name}: {err}"));
    assert!(
        !content.trim().is_empty(),
        "{file_name} should be written for the replacement identity"
    );
}

fn assert_e1_alice_did(did: &str) {
    assert!(
        did.starts_with("did:wba:awiki.ai:alice:e1_"),
        "replacement should generate an alice handle e1 DID, got {did}"
    );
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

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
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
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-identity-im-core-replace-did-{}-{nanos}",
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
