//! Real vNext workspace startup contract for the feature-gated system-test probe.
//!
//! The fixture uses the production CLI Genesis flow against a loopback control plane. Identity
//! private keys and vNext device state are generated and persisted only by production code.

#![cfg(feature = "system-test-probe")]

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod support;

use support::{write_default_tenant_registry, write_tenant_config};

const TEST_HANDLE: &str = "probe-startup";

#[test]
fn probe_starts_from_real_vnext_workspace() {
    let temp = TempDir::new().expect("temporary workspace");
    let product_home = temp.path().join("product");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    let server = GenesisServer::spawn();
    write_default_tenant_registry(&product_home, server.base_url(), "awiki.test");
    write_tenant_config(
        &product_home,
        &format!(
            concat!(
                "services:\n",
                "  anp_service_endpoint: {}/im/rpc\n",
                "  anp_service_did: did:wba:awiki.test\n",
                "secret_storage:\n",
                "  mode: vault_required\n",
                "  workspace_id: probe-startup-workspace\n",
                "  device_id: probe-startup-local\n"
            ),
            server.base_url()
        ),
    );
    let vault_root_key = random_vault_root_key();

    let registration = workspace_command(
        env!("CARGO_BIN_EXE_awiki-cli"),
        &product_home,
        &home,
        &vault_root_key,
    )
    .args([
        "id",
        "register",
        "--handle",
        TEST_HANDLE,
        "--phone",
        "+15551234567",
        "--otp",
        "123456",
    ])
    .output()
    .expect("run production vNext Genesis flow");
    assert_success(&registration, "vNext Genesis");
    assert_secret_safe(&registration, &vault_root_key);

    let requests = server.join();
    assert_eq!(
        requests.len(),
        2,
        "Genesis must use account exchange followed by device_genesis"
    );
    assert_eq!(
        requests[0].path,
        "/user-service/auth/account-verification/exchange"
    );
    assert_eq!(requests[1].path, "/user-service/did-auth/rpc");
    assert_eq!(requests[1].json_body()["method"], "device_genesis");

    // The Genesis server is gone before probe startup. A valid cached device session and the
    // shutdown action make this a local startup contract rather than a service integration test.
    let mut child = workspace_command(
        env!("CARGO_BIN_EXE_awiki-system-test-probe"),
        &product_home,
        &home,
        &vault_root_key,
    )
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("start system-test probe");
    child
        .stdin
        .take()
        .expect("probe stdin")
        .write_all(b"{\"id\":\"startup\",\"action\":\"shutdown\",\"params\":{}}\n")
        .expect("write shutdown request");
    wait_for_exit(&mut child, Duration::from_secs(5));
    let output = child.wait_with_output().expect("collect probe output");

    assert_success(&output, "probe startup");
    assert_secret_safe(&output, &vault_root_key);
    assert!(
        output.stderr.is_empty(),
        "probe stderr must be empty, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = String::from_utf8(output.stdout)
        .expect("probe stdout UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("probe response JSON"))
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        vec![json!({
            "id": "startup",
            "ok": true,
            "result": {"shutdown": true},
        })]
    );
}

fn workspace_command(
    binary: &str,
    product_home: &Path,
    home: &Path,
    vault_root_key: &str,
) -> Command {
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", product_home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("AWIKI_IM_CORE_VAULT_ROOT_KEY_B64", vault_root_key)
        .env("AWIKI_MULTI_DEVICE_JOIN_ENABLED", "1")
        .env("AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED", "0")
        .env("AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED", "0")
        .env("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED", "0")
        .env("AWIKI_MULTI_DEVICE_HANDLE_RECOVERY_ENABLED", "0")
        .env("AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED", "0");
    command
}

fn random_vault_root_key() -> String {
    let mut key = [0_u8; 32];
    OsRng.fill_bytes(&mut key);
    URL_SAFE_NO_PAD.encode(key)
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_secret_safe(output: &Output, vault_root_key: &str) {
    assert!(!output
        .stdout
        .windows(vault_root_key.len())
        .any(|window| { window == vault_root_key.as_bytes() }));
    assert!(!output
        .stderr
        .windows(vault_root_key.len())
        .any(|window| { window == vault_root_key.as_bytes() }));
    for forbidden in ["PRIVATE KEY", "refresh_token", "pairing_secret"] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(forbidden));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(forbidden));
    }
}

fn wait_for_exit(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().expect("poll probe").is_some() {
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("probe did not exit within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

struct GenesisServer {
    base_url: String,
    join: thread::JoinHandle<Vec<CapturedRequest>>,
}

impl GenesisServer {
    fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind Genesis server");
        listener
            .set_nonblocking(true)
            .expect("set Genesis server nonblocking");
        let base_url = format!("http://{}", listener.local_addr().expect("Genesis address"));
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut requests = Vec::new();
            for step in 0..2 {
                let Some(mut stream) = accept_before(&listener, deadline) else {
                    break;
                };
                let request = read_request(&mut stream);
                let response = match step {
                    0 => json!({
                        "account_verification_token": "test-account-grant",
                        "purpose": "awiki.device.genesis.v1",
                        "expires_at": future_rfc3339(time::Duration::minutes(5)),
                    }),
                    1 => genesis_response(&request),
                    _ => unreachable!(),
                };
                write_json_response(&mut stream, &response);
                requests.push(request);
            }
            requests
        });
        Self { base_url, join }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn join(self) -> Vec<CapturedRequest> {
        self.join.join().expect("join Genesis server")
    }
}

#[derive(Debug)]
struct CapturedRequest {
    path: String,
    body: Vec<u8>,
}

impl CapturedRequest {
    fn json_body(&self) -> Value {
        serde_json::from_slice(&self.body).expect("request JSON")
    }
}

fn accept_before(listener: &TcpListener, deadline: Instant) -> Option<TcpStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept Genesis request: {error}"),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set request read timeout");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read request headers");
        assert!(read > 0, "request closed before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break offset + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let request_line = headers.lines().next().expect("HTTP request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("HTTP request path")
        .to_owned();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut chunk).expect("read request body");
        assert!(read > 0, "request closed before body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    CapturedRequest {
        path,
        body: bytes[header_end..header_end + content_length].to_vec(),
    }
}

fn write_json_response(stream: &mut TcpStream, response: &Value) {
    let body = serde_json::to_vec(response).expect("serialize response");
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write response headers");
    stream.write_all(&body).expect("write response body");
    stream.flush().expect("flush response");
}

fn genesis_response(request: &CapturedRequest) -> Value {
    let rpc = request.json_body();
    assert_eq!(rpc["method"], "device_genesis");
    let params = &rpc["params"];
    let document = &params["did_document"];
    let did = document["id"].as_str().expect("generated DID");
    let device = &document["deviceManifest"]["devices"][0];
    let device_id = params["bootstrap_device_id"]
        .as_str()
        .expect("bootstrap device ID");
    assert_eq!(device["device_id"], device_id);
    let signing_key_id = device["signing_key_id"]
        .as_str()
        .expect("device signing key ID");
    let e2ee_key_id = device["e2ee_key_id"].as_str().expect("device E2EE key ID");
    let canonical = serde_json_canonicalizer::to_vec(document).expect("canonical DID document");
    let document_hash = format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    );
    let access_expiry = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("truncate access expiry")
        + time::Duration::hours(1);
    let refresh_expiry = access_expiry + time::Duration::days(7);
    json!({
        "jsonrpc": "2.0",
        "id": rpc["id"],
        "result": {
            "did": did,
            "user_id": "user-probe-startup",
            "checkpoint": {
                "document_version": 1,
                "document_hash": document_hash,
                "registry_version": 1,
            },
            "device": {
                "device_id": device_id,
                "signing_key_id": signing_key_id,
                "e2ee_key_id": e2ee_key_id,
                "status": "active",
                "role": "admin",
                "management_ready": true,
                "auth_generation": 1,
            },
            "access_token": device_token(
                did,
                device_id,
                signing_key_id,
                "access",
                "awiki.device.access.v1",
                access_expiry,
            ),
            "refresh_token": device_token(
                did,
                device_id,
                signing_key_id,
                "refresh",
                "awiki.device.refresh.v1",
                refresh_expiry,
            ),
            "token_expires_at": access_expiry
                .format(&time::format_description::well_known::Rfc3339)
                .expect("format access expiry"),
        }
    })
}

fn device_token(
    did: &str,
    device_id: &str,
    signing_key_id: &str,
    token_type: &str,
    purpose: &str,
    expires_at: time::OffsetDateTime,
) -> String {
    let issued_at = time::OffsetDateTime::now_utc().unix_timestamp() - 60;
    let payload = json!({
        "profile": "awiki-device-token-v1",
        "purpose": purpose,
        "type": token_type,
        "sub": did,
        "did": did,
        "user_id": "user-probe-startup",
        "device_id": device_id,
        "key_id": signing_key_id,
        "auth_generation": 1,
        "aud": ["awiki.test", "message.awiki.test"],
        "jti": format!("probe-{token_type}"),
        "iat": issued_at,
        "nbf": issued_at,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "exp": expires_at.unix_timestamp(),
    });
    format!(
        "e30.{}.test-signature-{token_type}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("serialize token claims"))
    )
}

fn future_rfc3339(after: time::Duration) -> String {
    (time::OffsetDateTime::now_utc() + after)
        .replace_nanosecond(0)
        .expect("truncate grant expiry")
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format grant expiry")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-probe-startup-{}-{unique}",
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
