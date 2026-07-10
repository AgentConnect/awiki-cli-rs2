use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{tenant_workspace, write_default_tenant_registry, write_tenant_config};

#[test]
fn identity_register_email_without_wait_checks_status_then_sends_activation_like_go() {
    let workspace = TempDir::new("identity-register-email-send").expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(r#"{"email":"alice@example.com","verified":false}"#),
        TestResponse::ok(r#"{"message":"Activation email sent."}"#),
    ]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "Alice",
            "--email",
            "Alice@Example.COM",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Activation email sent for handle alice.awiki.ai"
    );
    assert_eq!(envelope["data"]["action"], "send_registration_email");
    assert_eq!(envelope["data"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["handle"], "alice");
    assert_eq!(envelope["data"]["full_handle"], "alice.awiki.ai");
    assert_eq!(envelope["data"]["method"], "email");
    assert_eq!(envelope["data"]["email"], "alice@example.com");
    assert_eq!(envelope["data"]["verification_state"], "email_sent");
    assert!(envelope["data"].get("result").is_none());

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with(
        "GET /user-service/auth/email-status?email=alice%40example.com&handle=alice.awiki.ai HTTP/1.1"
    ));
    assert!(
        !requests[0].contains("Authorization: Bearer "),
        "registration email status must be unauthenticated:\n{}",
        requests[0]
    );
    assert!(requests[1].starts_with("POST /user-service/auth/email-send HTTP/1.1"));
    assert!(
        !requests[1].contains("Authorization: Bearer "),
        "registration email send must be unauthenticated:\n{}",
        requests[1]
    );
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(
        body,
        json!({ "email": "alice@example.com", "handle": "alice.awiki.ai" })
    );
    assert!(
        !workspace
            .path()
            .join("tenants")
            .join("default")
            .join("identities")
            .join("index.json")
            .exists(),
        "email send should not create a local identity index"
    );
}

#[test]
fn identity_register_email_wait_already_verified_registers_and_persists_identity_like_go() {
    let workspace = TempDir::new("identity-register-email-verified").expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(
            r#"{"email":"alice@example.com","verified":true,"verified_at":"2026-01-01T00:00:00Z"}"#,
        ),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","message":"Registration successful","handle":"alice","domain":"awiki.ai","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "Alice",
            "--email",
            "Alice@Example.COM",
            "--wait",
            "--invite-code",
            "invite-1",
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
    assert_eq!(envelope["data"]["method"], "email");
    assert_eq!(envelope["data"]["verification_state"], "completed");
    assert_eq!(envelope["data"]["identity"]["identity_name"], "alice");
    assert_eq!(envelope["data"]["identity"]["handle"], "alice");
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
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with(
        "GET /user-service/auth/email-status?email=alice%40example.com&handle=alice.awiki.ai HTTP/1.1"
    ));
    assert!(
        requests
            .iter()
            .all(|request| !request.starts_with("POST /user-service/auth/email-send HTTP/1.1")),
        "already verified registration must not send activation email:\n{requests:#?}"
    );
    assert!(requests[1].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[1])).expect("request body");
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], "req-1");
    assert_eq!(body["method"], "register");
    assert_eq!(body["params"]["handle"], "alice");
    assert_eq!(body["params"]["email"], "alice@example.com");
    assert_eq!(body["params"]["invite_code"], "invite-1");
    assert!(body["params"].get("phone").is_none());
    assert!(body["params"].get("otp_code").is_none());
    assert_eq!(body["params"]["did_document"]["id"], registered_did);

    let stored = read_stored_identity(workspace.path(), "alice");
    assert_eq!(stored.index["handle"], "alice");
    assert_eq!(stored.index["full_handle"], "alice.awiki.ai");
    assert_eq!(stored.index["did"], registered_did);
    assert_eq!(stored.index["user_id"], "user-alice");
    assert_eq!(stored.identity["handle"], "alice");
    assert_eq!(stored.identity["full_handle"], "alice.awiki.ai");
    assert_eq!(stored.identity["did"], registered_did);
    assert_eq!(stored.identity["user_id"], "user-alice");
    assert_vault_identity_has_auth_ref_and_no_plaintext_secret_files(workspace.path(), "alice");
}

#[test]
fn identity_register_email_wait_sends_then_polls_before_register_like_go() {
    let workspace = TempDir::new("identity-register-email-wait").expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::ok(r#"{"email":"alice@example.com","verified":false}"#),
        TestResponse::ok(r#"{"message":"Activation email sent."}"#),
        TestResponse::ok(
            r#"{"email":"alice@example.com","verified":true,"verified_at":"2026-01-01T00:00:00Z"}"#,
        ),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"did":"did:wba:awiki.ai:alice:e1_remote","user_id":"user-alice","handle":"alice","full_handle":"alice.awiki.ai","access_token":"jwt-register"},"id":"req-1"}"#,
        ),
    ]);
    write_service_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "Alice",
            "--email",
            "Alice@Example.COM",
            "--wait",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["action"], "register_handle");
    assert_eq!(envelope["data"]["method"], "email");
    assert_eq!(envelope["data"]["verification_state"], "completed");

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with(
        "GET /user-service/auth/email-status?email=alice%40example.com&handle=alice.awiki.ai HTTP/1.1"
    ));
    assert!(requests[1].starts_with("POST /user-service/auth/email-send HTTP/1.1"));
    assert!(requests[2].starts_with(
        "GET /user-service/auth/email-status?email=alice%40example.com&handle=alice.awiki.ai HTTP/1.1"
    ));
    assert!(requests[3].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
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

struct StoredIdentity {
    index: Value,
    identity: Value,
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
    }
}

fn assert_vault_identity_has_auth_ref_and_no_plaintext_secret_files(
    workspace: &Path,
    identity_name: &str,
) {
    let tenant = tenant_workspace(workspace);
    let index_path = tenant.join("identities").join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let entry = &index["credentials"][identity_name];
    let vault = &entry["vault_migration"];
    assert_eq!(vault["status"], "verified");
    assert_eq!(vault["backend"], "vault");
    assert_eq!(vault["plaintext_compat_retained"], false);
    assert!(
        vault["refs"]["auth_jwt"].is_object(),
        "vault-backed identity should store auth JWT as a vault ref: {vault:?}"
    );
    let dir_name = entry["dir_name"].as_str().unwrap();
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
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
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
    fn new(name: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{name}-{}-{nanos}",
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
