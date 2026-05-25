use awiki_cli::cli_http::{self, HttpClientError, HttpRequest};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

const VALID_ROOT_CERT_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIBgDCCASegAwIBAgIUPHDUu9WL36yvTmFeNFZVe/qhClcwCgYIKoZIzj0EAwIw
HTEbMBkGA1UEAwwSUnVzdGxzIFJvYnVzdCBSb290MCAXDTc1MDEwMDAwMDAwMFoY
DzQwOTYwMTAxMDAwMDAwWjAdMRswGQYDVQQDDBJSdXN0bHMgUm9idXN0IFJvb3Qw
WTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAASW/VkDFs5iGDQvH8jaXYT4jMx66jo+
5CWKyMt4OlTDdBfKfnmQ9LYeK/PsYfJ8wVizuSlPzXi9je8SnyYejGP3o0MwQTAP
BgNVHQ8BAf8EBQMDB4QAMB0GA1UdDgQWBBRqY/oMENJbNo7y39iL6GW3tDs0rzAP
BgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCIEUbrmSUjANju9nNpFop
PAl9Wh8tBxI5IY+BPh466+aUAiA1/9+prypt6s3Doo0GDsnoFGJi1UBivUg1qdik
cy4eNw==
-----END CERTIFICATE-----
"#;

#[test]
fn http_client_defaults_to_webpki_roots_and_snapshots_env_config() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unset_transport_env();
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_DIAL", "1234");
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_RESPONSE_HEADER", "2345");

    let client = cli_http::new_http_client("").expect("client");

    assert_eq!(
        client.trusted_root_count(),
        webpki_roots::TLS_SERVER_ROOTS.len()
    );
    assert_eq!(
        client.config().http_dial_timeout,
        Duration::from_millis(1234)
    );
    assert_eq!(
        client.config().http_response_header_timeout,
        Duration::from_millis(2345)
    );
}

#[test]
fn http_client_ca_bundle_errors_match_go_messages() {
    let temp = TempDir::new("transportcfg-http-ca").expect("temp dir");
    let missing = temp.path().join("missing.pem");
    let err = cli_http::new_http_client(&path_string(&missing)).expect_err("missing bundle");
    assert!(
        err.to_string().starts_with("read ca bundle:"),
        "unexpected missing bundle error: {err}"
    );

    let invalid = temp.path().join("invalid.pem");
    std::fs::write(&invalid, "not a pem\n").expect("write invalid bundle");
    let err = cli_http::new_http_client(&path_string(&invalid)).expect_err("invalid bundle");
    assert_eq!(
        err.to_string(),
        format!("invalid ca bundle: {}", path_string(&invalid))
    );
}

#[test]
fn http_client_appends_valid_ca_bundle_to_webpki_roots() {
    let temp = TempDir::new("transportcfg-http-ca-valid").expect("temp dir");
    let bundle = temp.path().join("root.pem");
    std::fs::write(&bundle, VALID_ROOT_CERT_PEM).expect("write bundle");

    let client = cli_http::new_http_client(&path_string(&bundle)).expect("client");

    assert_eq!(
        client.trusted_root_count(),
        webpki_roots::TLS_SERVER_ROOTS.len() + 1
    );
}

#[test]
fn http_client_sends_json_post_with_headers_and_body() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_proxy_env();
    let server = TestServer::new(|raw| {
        assert_contains(&raw, "POST /rpc?x=1 HTTP/1.1\r\n");
        assert_contains(&raw, "Host: 127.0.0.1:");
        assert_contains(&raw, "Content-Type: application/json\r\n");
        assert_contains(&raw, "Authorization: Bearer token\r\n");
        assert_contains(&raw, "Content-Length: 17\r\n");
        assert!(
            raw.ends_with("\r\n\r\n{\"jsonrpc\":\"2.0\"}"),
            "unexpected request body:\n{raw}"
        );
        b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}"
            .to_vec()
    });
    let client = cli_http::new_http_client("").expect("client");
    let response = client
        .execute(
            HttpRequest::new("POST", server.url("/rpc?x=1"))
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer token")
                .body(br#"{"jsonrpc":"2.0"}"#.to_vec()),
        )
        .expect("response");

    assert_eq!(response.status_code, 201);
    assert_eq!(String::from_utf8_lossy(&response.body), r#"{"ok":true}"#);
}

#[test]
fn http_client_decodes_chunked_response_body() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_proxy_env();
    let server = TestServer::new(|raw| {
        assert_contains(&raw, "GET /chunked HTTP/1.1\r\n");
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"
            .to_vec()
    });
    let client = cli_http::new_http_client("").expect("client");
    let response = client
        .execute(HttpRequest::new("GET", server.url("/chunked")))
        .expect("response");

    assert_eq!(response.status_code, 200);
    assert_eq!(response.body, b"hello world");
}

#[test]
fn http_client_request_timeout_overrides_default_response_header_timeout() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unset_transport_env();
    clear_proxy_env();
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_RESPONSE_HEADER", "2500");

    let server = TestServer::new_allowing_write_failure(|raw| {
        assert_contains(&raw, "GET /slow HTTP/1.1\r\n");
        thread::sleep(Duration::from_millis(350));
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nslow".to_vec()
    });
    let client = cli_http::new_http_client("").expect("client");

    let started = Instant::now();
    let err = client
        .execute(HttpRequest::new("GET", server.url("/slow")).timeout(Duration::from_millis(75)))
        .expect_err("request-specific timeout should abort before delayed response");
    let elapsed = started.elapsed();

    assert_timeoutish_io_error(&err);
    assert!(
        elapsed < Duration::from_millis(1500),
        "request timeout should beat high default response header timeout; elapsed={elapsed:?}, err={err}"
    );
}

#[test]
fn http_client_request_timeout_does_not_extend_default_response_header_timeout() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unset_transport_env();
    clear_proxy_env();
    std::env::set_var("AWIKI_CLI_TIMEOUT_HTTP_RESPONSE_HEADER", "75");

    let server = TestServer::new_allowing_write_failure(|raw| {
        assert_contains(&raw, "GET /slow-default HTTP/1.1\r\n");
        thread::sleep(Duration::from_millis(350));
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nslow".to_vec()
    });
    let client = cli_http::new_http_client("").expect("client");

    let started = Instant::now();
    let err = client
        .execute(
            HttpRequest::new("GET", server.url("/slow-default"))
                .timeout(Duration::from_millis(2500)),
        )
        .expect_err("base response-header timeout should still abort delayed response");
    let elapsed = started.elapsed();

    assert_timeoutish_io_error(&err);
    assert!(
        elapsed < Duration::from_millis(1500),
        "base response-header timeout should beat longer request timeout; elapsed={elapsed:?}, err={err}"
    );
}

#[test]
fn http_client_default_ignores_proxy_env_like_go_transportcfg_client() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_proxy_env();
    std::env::set_var("HTTP_PROXY", dead_proxy_url());
    let origin = TestServer::new(|raw| {
        assert_contains(&raw, "GET /direct HTTP/1.1\r\n");
        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\norigin".to_vec()
    });

    let response = cli_http::new_http_client("")
        .expect("client")
        .execute(HttpRequest::new("GET", origin.url("/direct")))
        .expect("response");

    assert_eq!(response.body, b"origin");
}

#[test]
fn http_client_proxy_env_uses_absolute_form_for_http_proxy() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_proxy_env();
    let proxy = TestServer::new(|raw| {
        assert_contains(&raw, "GET http://example.test/latest HTTP/1.1\r\n");
        assert_contains(&raw, "Host: example.test\r\n");
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nproxy".to_vec()
    });
    std::env::set_var("HTTP_PROXY", proxy.url(""));

    let response = cli_http::new_http_client_with_proxy_env("")
        .expect("client")
        .execute(HttpRequest::new("GET", "http://example.test/latest"))
        .expect("response");

    assert_eq!(response.body, b"proxy");
}

#[test]
fn http_client_proxy_env_honors_no_proxy_bypass() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_proxy_env();
    std::env::set_var("HTTP_PROXY", dead_proxy_url());
    std::env::set_var("NO_PROXY", "127.0.0.1");
    let origin = TestServer::new(|raw| {
        assert_contains(&raw, "GET /bypass HTTP/1.1\r\n");
        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nbypass".to_vec()
    });

    let response = cli_http::new_http_client_with_proxy_env("")
        .expect("client")
        .execute(HttpRequest::new("GET", origin.url("/bypass")))
        .expect("response");

    assert_eq!(response.body, b"bypass");
}

fn unset_transport_env() {
    for key in cli_http::TRANSPORT_ENV_KEYS {
        std::env::remove_var(key);
    }
}

fn clear_proxy_env() {
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        std::env::remove_var(key);
    }
}

fn dead_proxy_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused proxy port");
    let address = listener.local_addr().expect("unused proxy addr");
    drop(listener);
    format!("http://{address}")
}

struct TestServer {
    address: String,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(handler: impl FnOnce(String) -> Vec<u8> + Send + 'static) -> Self {
        Self::new_with_write_failure(handler, false)
    }

    fn new_allowing_write_failure(
        handler: impl FnOnce(String) -> Vec<u8> + Send + 'static,
    ) -> Self {
        Self::new_with_write_failure(handler, true)
    }

    fn new_with_write_failure(
        handler: impl FnOnce(String) -> Vec<u8> + Send + 'static,
        allow_write_failure: bool,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let join = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept connection");
            handle_connection(stream, handler, allow_write_failure);
        });
        Self {
            address,
            join: Some(join),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.address, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            join.join().expect("server thread");
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    handler: impl FnOnce(String) -> Vec<u8>,
    allow_write_failure: bool,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            panic!("connection closed before headers");
        }
        raw.extend_from_slice(&buffer[..read]);
        if let Some(end) = find_header_end(&raw) {
            break end;
        }
    };
    let headers = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while raw.len() < header_end + 4 + content_length {
        let read = stream.read(&mut buffer).expect("read request body");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
    }
    let response = handler(String::from_utf8_lossy(&raw).to_string());
    let write_result = stream.write_all(&response);
    if !allow_write_failure {
        write_result.expect("write response");
    }
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected {needle:?} in:\n{haystack}"
    );
}

fn assert_timeoutish_io_error(err: &HttpClientError) {
    match err {
        HttpClientError::Io(io_err)
            if matches!(
                io_err.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) => {}
        _ => panic!("expected timeout-ish I/O error, got {err:?}"),
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
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
