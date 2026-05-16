use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn group_e2ee_status_live_prefers_service_pending_notice_count_like_go() {
    let workspace = TempDir::new("group-e2ee-status-live").expect("workspace");
    register_ready_group_identity(workspace.path(), "alice-group-e2ee", "alice", "jwt-alice");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let bin_dir = TempDir::new("group-e2ee-status-bin").expect("bin dir");
    let fake_mls = bin_dir.path().join("anp-mls");
    write_fake_anp_mls(
        &fake_mls,
        &json!({
            "status": "active",
            "epoch": "1",
            "crypto_group_id_b64u": "crypto-1",
            "pending_commits": []
        }),
    );
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "epoch": "2",
            "group_state_version": "v2",
            "crypto_group_id_b64u": "crypto-1",
            "actor_membership_status": "active",
            "actor_membership_role": "member"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "notices": [
                {
                    "notice_id": "notice-service-1",
                    "notice_type": "commit-delivery",
                    "group_did": group_did,
                    "recipient_did": "did:wba:awiki.ai:alice:e1_alice",
                    "subject_did": "did:wba:awiki.ai:carol:e1_carol",
                    "commit_b64u": "Y29tbWl0",
                    "from_epoch": "1",
                    "to_epoch": "2",
                    "delivery_state": "pending"
                }
            ],
            "pending_count": 1,
            "delivered": 0
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            "alice-group-e2ee",
            "group",
            "e2ee",
            "status",
            "--group",
            group_did,
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Group E2EE recovery status inspected");
    assert_eq!(envelope["data"]["plan"]["action"], "group.e2ee.status");
    assert_eq!(envelope["data"]["plan"]["group"], group_did);
    assert_eq!(envelope["data"]["pending_notice_count"], 1);
    assert_eq!(envelope["data"]["diagnosis"]["state"], "pending_notices");
    assert_eq!(
        envelope["data"]["diagnosis"]["next_action"],
        "run_group_e2ee_repair"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let bodies = requests
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).expect("json body"))
        .collect::<Vec<_>>();
    assert_eq!(bodies[0]["method"], "group.e2ee.head");
    assert_eq!(bodies[0]["params"]["body"]["group_did"], group_did);
    assert_eq!(bodies[1]["method"], "group.e2ee.notice");
    assert_eq!(bodies[1]["params"]["body"]["group_did"], group_did);
    assert_eq!(bodies[1]["params"]["body"]["limit"], 50);
    assert!(bodies[1]["params"]["body"]
        .as_object()
        .expect("notice body")
        .get("mark_delivered")
        .is_none());
}

#[test]
fn group_e2ee_status_scans_non_default_device_state_like_go() {
    let workspace = TempDir::new("group-e2ee-status-device-scan").expect("workspace");
    register_ready_group_identity(workspace.path(), "alice-group-e2ee", "alice", "jwt-alice");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let device_id = "bob-main";
    let bin_dir = TempDir::new("group-e2ee-device-bin").expect("bin dir");
    let log_path = workspace.path().join("mls-calls.log");
    let fake_mls = bin_dir.path().join("anp-mls");
    write_device_scanning_fake_anp_mls(&fake_mls, &log_path);
    std::fs::create_dir_all(
        workspace
            .path()
            .join("mls")
            .join("agents")
            .join(mls_agent_key_for_alice())
            .join(device_id),
    )
    .expect("create non-default scoped MLS state dir");
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "group_did": group_did,
            "epoch": "2",
            "group_state_version": "v2",
            "crypto_group_id_b64u": "crypto-1",
            "actor_membership_status": "active",
            "actor_membership_role": "member"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "notices": [],
            "pending_count": 0,
            "delivered": 0
        }))),
    ]);
    write_group_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            "alice-group-e2ee",
            "group",
            "e2ee",
            "status",
            "--group",
            group_did,
        ],
        workspace.path(),
        &[("AWIKI_ANP_MLS_BINARY", fake_mls.as_path())],
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["local_device_id"], device_id);
    assert_eq!(envelope["data"]["mls"]["device_id"], device_id);
    assert_eq!(envelope["data"]["mls"]["epoch"], "2");
    assert_eq!(envelope["data"]["mls"]["status"], "active");
    assert_eq!(envelope["data"]["plan"]["action"], "group.e2ee.status");

    let log = std::fs::read_to_string(&log_path).expect("read fake mls log");
    assert_contains_text(&log, &format!(r#""agent_did":"{alice_did}""#));
    assert_contains_text(&log, r#""device_id":"default""#);
    assert_contains_text(&log, &format!(r#""device_id":"{device_id}""#));
}

fn register_ready_group_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    let create = awiki_cmd(
        &[
            "id",
            "create",
            "--name",
            "Group User",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let index_path = workspace.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let did = format!("did:wba:awiki.ai:{handle}:e1_{handle}");
    index["credentials"][identity_name]["did"] = json!(did);
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}.awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = workspace.join("identities").join(dir_name);
    let identity_path = identity_dir.join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    let original_did = identity["did"].as_str().unwrap().to_string();
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
    document["id"] = json!(original_did);
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
}

fn write_group_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn write_fake_anp_mls(path: &Path, result: &Value) {
    let response = json!({
        "ok": true,
        "api_version": "anp-mls/v1",
        "request_id": "group-e2ee-status-test",
        "result": result,
    })
    .to_string();
    let body = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{}'\n",
        response.replace('\'', "'\\''")
    );
    std::fs::write(path, body).expect("write fake anp-mls");
    make_executable(path);
}

fn write_device_scanning_fake_anp_mls(path: &Path, log_path: &Path) {
    let script = format!(
        r#"#!/bin/sh
body=$(cat)
printf '%s\n' "$body" >> '{}'
case "$body" in
  *'"device_id":"bob-main"'*)
    printf '%s\n' '{{"ok":true,"api_version":"anp-mls/v1","request_id":"group-e2ee-status-test","result":{{"status":"active","epoch":"2","crypto_group_id_b64u":"crypto-1","pending_commits":[]}}}}'
    ;;
  *)
    printf '%s\n' '{{"ok":true,"api_version":"anp-mls/v1","request_id":"group-e2ee-status-test","result":{{"status":"empty"}}}}'
    ;;
esac
"#,
        log_path.to_string_lossy().replace('\'', "'\\''")
    );
    std::fs::write(path, script).expect("write device scanning fake anp-mls");
    make_executable(path);
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake anp-mls");
    }
}

fn mls_agent_key_for_alice() -> &'static str {
    "_pENj2YsESvetd02aD8KaYAY"
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_with_env(args, workspace, &[])
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &Path)]) -> Output {
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
    for (key, value) in envs {
        command.env(key, value);
    }
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

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected text to contain {needle:?}, got:\n{haystack}"
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
