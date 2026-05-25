use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use im_core::prelude::*;
use serde_json::{json, Value};

#[test]
fn content_public_api_dispatches_authenticated_content_rpc() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        json!({
            "slug": "hello",
            "title": "Hello",
            "body": "Body",
            "visibility": "draft"
        }),
        json!({
            "count": 1,
            "pages": [{
                "slug": "hello",
                "title": "Hello",
                "visibility": "draft"
            }]
        }),
    ]);
    let core = fixture.core(server.base_url());
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_owned()))
        .unwrap();

    let created = client
        .content()
        .create_page(
            PageDraft::new(
                PageSlug::parse("hello").unwrap(),
                "Hello",
                "Body",
                Visibility::Draft,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(created.slug.as_str(), "hello");
    assert_eq!(created.title.as_deref(), Some("Hello"));
    assert!(matches!(created.visibility, Some(Visibility::Draft)));

    let listed = client
        .content()
        .list_pages(ContentPageQuery::default())
        .unwrap();
    assert_eq!(listed.items.len(), 1);
    assert!(!listed.has_more);
    assert_eq!(listed.items[0].slug.as_str(), "hello");

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/content/rpc");
    assert_eq!(requests[0].rpc_method, "create");
    assert_eq!(
        requests[0].params,
        json!({
            "slug": "hello",
            "title": "Hello",
            "body": "Body",
            "visibility": "draft"
        })
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/content/rpc");
    assert_eq!(requests[1].rpc_method, "list");
    assert_eq!(requests[1].params, json!({}));
}

#[test]
fn content_public_api_dispatches_all_page_rpc_methods() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        RpcResponse::success(json!({
            "slug": "hello",
            "title": "Hello",
            "body": "Body",
            "visibility": "public"
        })),
        RpcResponse::success(json!({
            "slug": "hello",
            "title": "Updated",
            "body": "Updated Body",
            "visibility": "unlisted"
        })),
        RpcResponse::success(json!({
            "slug": "new",
            "title": "Updated",
            "body": "Updated Body",
            "visibility": "unlisted"
        })),
        RpcResponse::success(json!({ "deleted": true })),
    ]);
    let core = fixture.core(server.base_url());
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_owned()))
        .unwrap();

    let page = PageRef::new(PageSlug::parse("hello").unwrap());
    let fetched = client.content().get_page(page.clone()).unwrap();
    assert_eq!(fetched.slug.as_str(), "hello");

    let updated = client
        .content()
        .update_page(
            page.clone(),
            PageUpdate {
                title: Some("Updated".to_owned()),
                body: Some("Updated Body".to_owned()),
                visibility: Some(Visibility::Unlisted),
            },
        )
        .unwrap();
    assert_eq!(updated.title.as_deref(), Some("Updated"));
    assert!(matches!(updated.visibility, Some(Visibility::Unlisted)));

    let renamed = client
        .content()
        .rename_page(page.clone(), PageSlug::parse("new").unwrap())
        .unwrap();
    assert_eq!(renamed.slug.as_str(), "new");

    let deleted = client.content().delete_page(page).unwrap();
    assert!(deleted.deleted);

    let requests = server.join();
    let methods = requests
        .iter()
        .map(|request| request.rpc_method.as_str())
        .collect::<Vec<_>>();
    assert_eq!(methods, ["get", "update", "rename", "delete"]);
    assert_eq!(requests[0].path, "/content/rpc");
    assert_eq!(requests[0].params, json!({ "slug": "hello" }));
    assert_eq!(
        requests[1].params,
        json!({
            "slug": "hello",
            "title": "Updated",
            "body": "Updated Body",
            "visibility": "unlisted"
        })
    );
    assert_eq!(
        requests[2].params,
        json!({ "old_slug": "hello", "new_slug": "new" })
    );
    assert_eq!(requests[3].params, json!({ "slug": "hello" }));
}

#[test]
fn content_public_api_maps_remote_error_statuses() {
    for status in [400, 401, 403, 404, 409] {
        let fixture = Fixture::new();
        let mut responses = vec![RpcResponse::http_error(
            status,
            format!("content error {status}"),
        )];
        if status == 401 {
            responses.push(RpcResponse::http_error(status, "content error 401 retry"));
        }
        let server = RpcTestServer::spawn(responses);
        let core = fixture.core(server.base_url());
        let client = core
            .client(IdentitySelector::LocalAlias("alice".to_owned()))
            .unwrap();

        let err = client
            .content()
            .get_page(PageRef::new(PageSlug::parse("hello").unwrap()))
            .expect_err("remote status should map to service error");
        assert_service_status(err, status);
        let expected_requests = if status == 401 { 2 } else { 1 };
        assert_eq!(server.join().len(), expected_requests);
    }
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root("im-core-content-api");
        write_identity_fixture(&root);
        Self { root }
    }

    fn core(&self, base_url: String) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                transport_policy: MessageTransportPolicy::HttpOnly,
            },
            ImCorePaths {
                identities: IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
        )
        .unwrap()
    }
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    path: String,
    rpc_method: String,
    params: Value,
    id: Value,
}

struct RpcTestServer {
    address: String,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    join: Option<thread::JoinHandle<()>>,
}

impl RpcTestServer {
    fn spawn(responses: Vec<impl Into<RpcResponse>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let responses = responses
            .into_iter()
            .map(Into::into)
            .collect::<Vec<RpcResponse>>();
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            for response in responses {
                let mut stream = accept_before_deadline(&listener, deadline);
                let request = read_rpc_request(&mut stream);
                let id = request.id.clone();
                server_requests.lock().unwrap().push(request);
                write_rpc_response(&mut stream, id, response);
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

    fn join(mut self) -> Vec<CapturedRequest> {
        if let Some(join) = self.join.take() {
            join.join().unwrap();
        }
        self.requests.lock().unwrap().clone()
    }
}

struct RpcResponse {
    status: u16,
    body: Value,
}

impl RpcResponse {
    fn success(result: Value) -> Self {
        Self {
            status: 200,
            body: json!({
                "jsonrpc": "2.0",
                "result": result,
            }),
        }
    }

    fn http_error(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({
                "error": message.into(),
            }),
        }
    }
}

impl From<Value> for RpcResponse {
    fn from(result: Value) -> Self {
        Self::success(result)
    }
}

impl Drop for RpcTestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn read_rpc_request(stream: &mut TcpStream) -> CapturedRequest {
    let raw = read_http_request(stream);
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
        .unwrap();
    let head = String::from_utf8_lossy(&raw[..header_end]);
    let request_line = head.lines().next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap().to_owned();
    let path = request_parts.next().unwrap().to_owned();
    let body: Value = serde_json::from_slice(&raw[header_end..]).unwrap();
    CapturedRequest {
        method,
        path,
        rpc_method: body["method"].as_str().unwrap().to_owned(),
        params: body.get("params").cloned().unwrap_or(Value::Null),
        id: body.get("id").cloned().unwrap_or(Value::Null),
    }
}

fn write_rpc_response(stream: &mut TcpStream, id: Value, mut response: RpcResponse) {
    if let Some(body) = response.body.as_object_mut() {
        body.entry("id".to_owned()).or_insert(id);
    }
    let body = serde_json::to_vec(&response.body).unwrap();
    write_http_response(stream, response.status, &body);
}

fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut content_length = None;
    loop {
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            content_length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            });
            if let Some(length) = content_length {
                if buffer.len() >= header_end + length {
                    break;
                }
            }
        }
    }
    let header_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
        .unwrap();
    let body_len = content_length.unwrap_or(0);
    buffer.truncate(header_end + body_len);
    buffer
}

fn write_http_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}

fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("accept failed: {err}"),
        }
    }
}

fn write_identity_fixture(root: &Path) {
    let identity_root = root.join("identities");
    let identity_dir = identity_root.join("alice");
    fs::create_dir_all(&identity_dir).unwrap();
    fs::create_dir_all(root.join("local")).unwrap();
    fs::write(identity_root.join("default"), "alice\n").unwrap();
    fs::write(
        identity_root.join("registry.json"),
        json!({
            "default_identity": "alice",
            "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "local_alias": "alice",
                "handle": "alice.awiki.test",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
            }]
        })
        .to_string(),
    )
    .unwrap();
    let bundle = anp::authentication::create_did_wba_document(
        "awiki.test",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["user".to_owned()],
            domain: Some("awiki.test".to_owned()),
            challenge: Some("content-api-test".to_owned()),
            ..anp::authentication::DidDocumentOptions::default()
        },
    )
    .unwrap();
    fs::write(
        identity_dir.join("did.json"),
        serde_json::to_vec_pretty(&bundle.did_document).unwrap(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("private.key"),
        bundle.private_key_pem("key-1").unwrap(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("auth.json"),
        r#"{"jwt_token":"test-token"}"#,
    )
    .unwrap();
}

fn unique_temp_root(prefix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

fn assert_service_status(err: ImError, expected_status: u16) {
    match err {
        ImError::Service {
            status_code: Some(status),
            ..
        } => assert_eq!(status, expected_status),
        other => panic!("expected service status {expected_status}, got {other:?}"),
    }
}
