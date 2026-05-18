use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn mail_inbox_live_posts_authenticated_json_rpc_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"total":2,"messages":[{"id":"m1"},{"id":"m2"}]},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-mail",
            "mail",
            "inbox",
            "--folder",
            "inbox",
            "--limit",
            "2",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 2 messages from inbox");
    assert_eq!(envelope["data"]["total"], 2);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /mail/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-mail\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "mail.getInbox",
            "params": {
                "folder": "inbox",
                "limit": 2,
                "offset": 0,
                "unread_only": false,
            }
        })
    );
}

#[test]
fn mail_inbox_trace_timing_reports_remote_rpc_phase() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"total":1,"messages":[{"id":"m1"}]},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_trace_cmd(
        &[
            "--identity",
            "alice-mail",
            "mail",
            "inbox",
            "--folder",
            "inbox",
            "--limit",
            "1",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json_with_stderr(&output);
    assert_eq!(envelope["summary"], "Loaded 1 messages from inbox");
    let trace = stderr_text(&output);
    assert_text_contains(&trace, "[awiki-cli 耗时追踪]");
    assert_text_contains(&trace, "远端 RPC");
    assert_text_contains(&trace, "mail getInbox");
}

#[test]
fn mail_inbox_trace_timing_reports_bootstrap_jwt_without_nested_get_me_rpc() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "");
    let server = TestServer::new(vec![
        TestResponse::ok(r#"{"jsonrpc":"2.0","result":{"access_token":"jwt-mail"},"id":"req-1"}"#),
        TestResponse::ok(
            r#"{"jsonrpc":"2.0","result":{"total":1,"messages":[{"id":"m1"}]},"id":"req-1"}"#,
        ),
    ]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_trace_cmd(
        &[
            "--identity",
            "alice-mail",
            "mail",
            "inbox",
            "--folder",
            "inbox",
            "--limit",
            "1",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json_with_stderr(&output);
    assert_eq!(envelope["summary"], "Loaded 1 messages from inbox");
    let trace = stderr_text(&output);
    assert_text_contains(&trace, "JWT 续期");
    assert_text_contains(&trace, "邮件服务启动鉴权");
    assert_text_contains(&trace, "远端 RPC / mail getInbox");
    assert_text_not_contains(&trace, "远端 RPC / get me");

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert!(requests[1].starts_with("POST /mail/rpc HTTP/1.1"));
}

#[test]
fn mail_read_live_maps_rpc_not_found_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","error":{"code":-32002,"message":"message not found","data":{"message_id":"missing"}},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-mail",
            "mail",
            "read",
            "--id",
            "missing",
        ],
        workspace.path(),
    );

    assert_code(&output, 5);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_eq!(
        envelope["error"]["message"],
        "service rpc error -32002: message not found"
    );
    assert_contains(
        &envelope["error"]["hint"],
        "Ensure the message id is valid and mail service is reachable.",
    );
}

#[test]
fn mail_read_live_posts_get_message_and_returns_service_data_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"id":"msg-42","subject":"Hello","body_text":"Body text","is_read":false},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &["--identity", "alice-mail", "mail", "read", "--id", "msg-42"],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded message msg-42");
    assert_eq!(
        envelope["data"],
        json!({
            "id": "msg-42",
            "subject": "Hello",
            "body_text": "Body text",
            "is_read": false,
        })
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /mail/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-mail\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "mail.getMessage",
            "params": {
                "message_id": "msg-42",
            }
        })
    );
}

#[test]
fn mail_mark_read_live_posts_message_ids_as_read_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"updated":2,"message_ids":["msg-1","msg-2"]},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-mail",
            "mail",
            "mark-read",
            "msg-1",
            "msg-2",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Marked 2 message(s) as read");
    assert_eq!(envelope["data"]["updated"], 2);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /mail/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "mail.markRead",
            "params": {
                "message_ids": ["msg-1", "msg-2"],
                "is_read": true,
            }
        })
    );
}

#[test]
fn mail_account_live_posts_get_mailbox_with_empty_params_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"address":"alice@awiki.ai","quota_used":7},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &["--identity", "alice-mail", "mail", "account"],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded mailbox account");
    assert_eq!(
        envelope["data"],
        json!({
            "address": "alice@awiki.ai",
            "quota_used": 7,
        })
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /mail/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "mail.getMailbox",
            "params": {}
        })
    );
}

#[test]
fn mail_send_live_splits_recipients_and_posts_null_html_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"message_id":"sent-1","queued":true},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_owned(
        &[
            "--identity".to_string(),
            "alice-mail".to_string(),
            "mail".to_string(),
            "send".to_string(),
            "--to".to_string(),
            "ada@example.com, grace@example.com;linus@example.com".to_string(),
            "--cc".to_string(),
            "ops@example.com\tqa@example.com\nreview@example.com".to_string(),
            "--subject".to_string(),
            "Release notes".to_string(),
            "--body".to_string(),
            "Plain body".to_string(),
            "--html".to_string(),
            "".to_string(),
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Mail send request accepted");
    assert_eq!(envelope["data"]["message_id"], "sent-1");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /mail/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(
        body,
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "mail.send",
            "params": {
                "to": ["ada@example.com", "grace@example.com", "linus@example.com"],
                "cc": ["ops@example.com", "qa@example.com", "review@example.com"],
                "subject": "Release notes",
                "body_text": "Plain body",
                "body_html": null,
            }
        })
    );
}

#[test]
fn mail_attachment_live_decodes_base64_and_writes_output_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"filename":"report.txt","content_type":"text/plain","content_base64":"SGVsbG8gbWFpbAo=","size":11},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());
    let output_path = workspace.path().join("downloads").join("report.txt");

    let output = awiki_cmd_owned(
        &[
            "--identity".to_string(),
            "alice-mail".to_string(),
            "mail".to_string(),
            "attachment".to_string(),
            "download".to_string(),
            "--message-id".to_string(),
            "msg-1".to_string(),
            "--attachment-index".to_string(),
            "0".to_string(),
            "--output".to_string(),
            output_path.to_string_lossy().into_owned(),
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        format!("Attachment saved to {}", output_path.to_string_lossy())
    );
    assert_eq!(envelope["data"]["filename"], "report.txt");
    assert_eq!(envelope["data"]["content_type"], "text/plain");
    assert_eq!(
        envelope["data"]["path"],
        output_path.to_string_lossy().to_string()
    );
    assert_eq!(
        std::fs::read_to_string(&output_path).expect("attachment output"),
        "Hello mail\n"
    );
    #[cfg(unix)]
    {
        let dir_mode = std::fs::metadata(output_path.parent().expect("download dir"))
            .expect("download dir metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = std::fs::metadata(&output_path)
            .expect("attachment metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}

#[test]
fn mail_attachment_live_overwrites_existing_output_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"filename":"report.txt","content_type":"text/plain","content_base64":"bmV3Cg==","size":4},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());
    let output_path = workspace.path().join("existing.txt");
    std::fs::write(&output_path, "old content that must be truncated").expect("seed output");
    #[cfg(unix)]
    std::fs::set_permissions(&output_path, std::fs::Permissions::from_mode(0o644))
        .expect("seed mode");

    let output = awiki_cmd_owned(
        &mail_attachment_download_args(&output_path),
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(
        envelope["data"]["path"],
        output_path.to_string_lossy().to_string()
    );
    assert_eq!(
        std::fs::read_to_string(&output_path).expect("attachment output"),
        "new\n"
    );
    #[cfg(unix)]
    {
        let file_mode = std::fs::metadata(&output_path)
            .expect("attachment metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            file_mode, 0o644,
            "Go os.WriteFile truncates existing files without chmodding them"
        );
    }
}

#[test]
fn mail_attachment_live_rejects_empty_content_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"filename":"empty.txt","content_base64":"","size":0},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());
    let output_path = workspace.path().join("empty.txt");

    let output = awiki_cmd_owned(
        &mail_attachment_download_args(&output_path),
        workspace.path(),
    );

    assert_code(&output, 1);
    assert!(
        !output_path.exists(),
        "empty content should fail before writing output"
    );
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_eq!(envelope["error"]["message"], "attachment content is empty");
    assert_contains(
        &envelope["error"]["hint"],
        "Try fetching the attachment again or verify the mail service response.",
    );
}

#[test]
fn mail_attachment_live_rejects_invalid_base64_like_go() {
    let workspace = TempDir::new().expect("workspace");
    register_ready_mail_identity(workspace.path(), "alice-mail", "alice", "jwt-mail");
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"filename":"bad.txt","content_base64":"not-base64!","size":11},"id":"req-1"}"#,
    )]);
    write_mail_config(workspace.path(), &server.base_url());
    let output_path = workspace.path().join("bad.txt");

    let output = awiki_cmd_owned(
        &mail_attachment_download_args(&output_path),
        workspace.path(),
    );

    assert_code(&output, 1);
    assert!(
        !output_path.exists(),
        "invalid base64 should fail before writing output"
    );
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "attachment base64 decode failed:",
    );
    assert_contains(
        &envelope["error"]["hint"],
        "Ensure the mail service response is valid.",
    );
}

fn register_ready_mail_identity(
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
            "Alice Mail",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let index_path = workspace.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}@awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!("user-alice");
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = workspace.join("identities").join(dir_name);
    let identity_path = identity_dir.join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    identity["handle"] = json!(handle);
    identity["full_handle"] = json!(format!("{handle}@awiki.ai"));
    identity["user_id"] = json!("user-alice");
    std::fs::write(
        &identity_path,
        serde_json::to_vec_pretty(&identity).unwrap(),
    )
    .unwrap();

    std::fs::write(
        identity_dir.join("auth.json"),
        serde_json::to_vec_pretty(&json!({ "jwt_token": jwt_token })).unwrap(),
    )
    .unwrap();
}

fn write_mail_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("services:\n  service_base_url: {base_url}\n  mail_service_url: {base_url}\n"),
    )
    .unwrap();
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_owned(
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        workspace,
    )
}

fn awiki_trace_cmd(args: &[&str], workspace: &Path) -> Output {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    awiki_trace_cmd_owned(&args, workspace)
}

fn mail_attachment_download_args(output_path: &Path) -> Vec<String> {
    vec![
        "--identity".to_string(),
        "alice-mail".to_string(),
        "mail".to_string(),
        "attachment".to_string(),
        "download".to_string(),
        "--message-id".to_string(),
        "msg-1".to_string(),
        "--attachment-index".to_string(),
        "0".to_string(),
        "--output".to_string(),
        output_path.to_string_lossy().into_owned(),
    ]
}

fn awiki_cmd_owned(args: &[String], workspace: &Path) -> Output {
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

fn awiki_trace_cmd_owned(args: &[String], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("AWIKI_CLI_TRACE_TIMING", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
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
    success_json_with_stderr(output)
}

fn success_json_with_stderr(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn error_json(output: &Output) -> Value {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn stderr_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.is_empty(), "stderr should contain trace output");
    stderr
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value
        .as_str()
        .unwrap_or_else(|| panic!("expected string containing {needle:?}, got {value:?}"));
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
    );
}

fn assert_text_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected text to contain {needle:?}, got:\n{haystack}"
    );
}

fn assert_text_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "expected text not to contain {needle:?}, got:\n{haystack}"
    );
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
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
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let join = thread::spawn(move || {
            for response in responses {
                let Ok((stream, _)) = listener.accept() else {
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
            "awiki-cli-rs2-mail-live-test-{}-{nanos}",
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
