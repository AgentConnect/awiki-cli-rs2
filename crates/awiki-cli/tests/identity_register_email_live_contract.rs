use base64::Engine;
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
        TestResponse::registration(),
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
        registered_did.starts_with("did:wba:awiki.ai:user:alice:e1_"),
        "registration should persist the locally generated key-bound DID: {registered_did}"
    );
    assert!(envelope["data"]["identity"]["has_jwt"]
        .as_bool()
        .expect("identity has_jwt bool"));

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
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
    assert_prekey_publication(&requests[2]);

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
        TestResponse::registration(),
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
    assert_eq!(requests.len(), 5);
    assert!(requests[0].starts_with(
        "GET /user-service/auth/email-status?email=alice%40example.com&handle=alice.awiki.ai HTTP/1.1"
    ));
    assert!(requests[1].starts_with("POST /user-service/auth/email-send HTTP/1.1"));
    assert!(requests[2].starts_with(
        "GET /user-service/auth/email-status?email=alice%40example.com&handle=alice.awiki.ai HTTP/1.1"
    ));
    assert!(requests[3].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert_prekey_publication(&requests[4]);
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

fn registration_response(request: &str) -> String {
    let rpc: Value =
        serde_json::from_str(request_body(request)).expect("registration request JSON");
    let params = &rpc["params"];
    let document = &params["did_document"];
    let did = document["id"]
        .as_str()
        .expect("registration DID document id");
    let device = &document["deviceManifest"]["devices"][0];
    let device_id = device["device_id"]
        .as_str()
        .expect("registration manifest device_id");
    let key_id = device["signing_key_id"]
        .as_str()
        .expect("registration manifest signing_key_id");
    let handle = params["handle"]
        .as_str()
        .expect("registration handle");
    let domain = did
        .strip_prefix("did:wba:")
        .and_then(|suffix| suffix.split(':').next())
        .expect("registration DID domain");
    let user_id = format!("user-{handle}");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": did,
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": did,
        "user_id": user_id,
        "device_id": device_id,
        "key_id": key_id,
        "auth_generation": 1,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "iat": now,
        "nbf": now,
        "exp": now + 3600,
        "jti": format!("registration-{device_id}"),
    });
    let access_token = format!(
        "e30.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialize test access token claims"))
    );
    json!({
        "jsonrpc": "2.0",
        "result": {
            "state": "registered",
            "did": did,
            "user_id": format!("user-{handle}"),
            "message": "Registration successful",
            "access_token": access_token,
            "handle": handle,
            "domain": domain,
            "full_handle": format!("{handle}.{domain}"),
        },
        "id": rpc["id"].clone(),
    })
    .to_string()
}

fn prekey_publication_response(request: &str) -> String {
    let rpc: Value = serde_json::from_str(request_body(request)).expect("P5 publish request JSON");
    let body = &rpc["params"]["body"];
    let bundle = &body["prekey_bundle"];
    let published_opk_count = body["one_time_prekeys"]
        .as_array()
        .map(Vec::len)
        .expect("P5 publish one_time_prekeys");
    json!({
        "jsonrpc": "2.0",
        "result": {
            "published": true,
            "owner_did": bundle["owner_did"].clone(),
            "owner_device_id": bundle["owner_device_id"].clone(),
            "bundle_id": bundle["bundle_id"].clone(),
            "published_at": "2026-07-25T00:00:00Z",
            "published_opk_count": published_opk_count,
        },
        "id": rpc["id"].clone(),
    })
    .to_string()
}

fn assert_prekey_publication(request: &str) {
    assert!(
        request.starts_with("POST /im/rpc HTTP/1.1"),
        "registration must publish its P5 PreKey bundle through Message Service:\n{request}"
    );
    let body: Value = serde_json::from_str(request_body(request)).expect("P5 publish request body");
    assert_eq!(body["method"], "direct.e2ee.publish_prekey_bundle");
    assert!(body["params"].get("auth").is_none());
    assert_eq!(body["params"]["meta"]["target"]["kind"], "service");
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

    fn registration() -> Self {
        Self::ok("__DYNAMIC_REGISTRATION_RESPONSE__")
    }

    fn prekey_publication() -> Self {
        Self::ok("__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__")
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
                let follows_with_prekey = response.body == "__DYNAMIC_REGISTRATION_RESPONSE__";
                let stream = accept_with_timeout(&listener);
                let Some(stream) = stream else {
                    break;
                };
                handle_connection(stream, &server_requests, response);
                if follows_with_prekey {
                    let stream = accept_with_timeout(&listener);
                    let Some(stream) = stream else {
                        break;
                    };
                    handle_connection(
                        stream,
                        &server_requests,
                        TestResponse::prekey_publication(),
                    );
                }
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
    let body = if response.body == "__DYNAMIC_REGISTRATION_RESPONSE__" {
        registration_response(&request)
    } else if response.body == "__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__" {
        prekey_publication_response(&request)
    } else {
        response.body
    };
    requests.lock().expect("requests mutex").push(request);
    let body_bytes = body.as_bytes();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        body_bytes.len(),
        body
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
