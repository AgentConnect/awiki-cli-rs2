#[cfg(feature = "blocking")]
use rustls::pki_types::ServerName;
use rustls::pki_types::{pem::PemObject, CertificateDer};
use rustls::{ClientConfig, RootCertStore};
#[cfg(feature = "blocking")]
use rustls::{ClientConnection, StreamOwned};
use std::collections::BTreeMap;
use std::fs;
#[cfg(feature = "blocking")]
use std::io::{Read, Write};
#[cfg(feature = "blocking")]
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
#[cfg(feature = "blocking")]
use std::sync::Arc;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub(crate) struct HttpClient {
    runtime: Option<std::sync::Arc<crate::network::NetworkRuntime>>,
    follow_redirects: bool,
    #[cfg(feature = "blocking")]
    tls_config: Arc<ClientConfig>,
    async_client: Result<reqwest::Client, crate::ImError>,
    attachment_async_client: Result<reqwest::Client, crate::ImError>,
    #[cfg(feature = "blocking")]
    init_error: Option<crate::ImError>,
}

#[derive(Clone)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct RedactedHeaders<'a>(&'a BTreeMap<String, String>);

        impl std::fmt::Debug for RedactedHeaders<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let mut map = f.debug_map();
                for (name, value) in self.0 {
                    if name.eq_ignore_ascii_case("authorization")
                        || name.eq_ignore_ascii_case("proxy-authorization")
                    {
                        map.entry(name, &"<redacted>");
                    } else {
                        map.entry(name, value);
                    }
                }
                map.finish()
            }
        }

        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &RedactedHeaders(&self.headers))
            .field("body", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpResponse {
    pub(crate) status_code: u16,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
}

struct HttpTimingGuard<'a> {
    runtime: &'a crate::network::NetworkRuntime,
    operation: String,
    stage: &'static str,
    started: std::time::Instant,
}

impl Drop for HttpTimingGuard<'_> {
    fn drop(&mut self) {
        self.runtime.trace(&self.operation, self.stage, self.started);
    }
}

impl HttpClient {
    pub(crate) fn for_core(core: &crate::core::ImCoreInner, follow_redirects: bool) -> Self {
        Self::with_runtime(
            core.sdk_config().ca_bundle_path(),
            follow_redirects,
            Some(core.network.clone()),
        )
    }
    pub(crate) fn from_config(config: &crate::ImCoreConfig) -> Self {
        Self::with_ca_bundle(config.ca_bundle_path(), true)
    }

    pub(crate) fn from_config_no_redirect(config: &crate::ImCoreConfig) -> Self {
        Self::with_ca_bundle(config.ca_bundle_path(), false)
    }

    fn with_ca_bundle(ca_bundle: Option<&str>, follow_redirects: bool) -> Self {
        Self::with_runtime(ca_bundle, follow_redirects, None)
    }
    fn with_runtime(
        ca_bundle: Option<&str>,
        follow_redirects: bool,
        runtime: Option<std::sync::Arc<crate::network::NetworkRuntime>>,
    ) -> Self {
        #[cfg(feature = "blocking")]
        let (roots, init_error) = match root_store(ca_bundle) {
            Ok(roots) => (roots, None),
            Err(err) => (
                RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                },
                Some(err),
            ),
        };
        Self {
            async_client: if let Some(runtime) = &runtime {
                runtime.client_for(&crate::network::Route::Legacy, follow_redirects, false)
            } else {
                async_client(ca_bundle, follow_redirects)
            },
            attachment_async_client: if let Some(runtime) = &runtime {
                runtime.client_for(&crate::network::Route::Legacy, follow_redirects, true)
            } else {
                build_async_client(ca_bundle, follow_redirects, None)
            },
            runtime,
            follow_redirects,
            #[cfg(feature = "blocking")]
            tls_config: Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            ),
            #[cfg(feature = "blocking")]
            init_error,
        }
    }

    #[cfg(feature = "blocking")]
    pub(crate) fn execute(&self, request: HttpRequest) -> crate::ImResult<HttpResponse> {
        #[cfg(all(test, feature = "sqlite", feature = "identity-native-anp"))]
        if let Some(response) =
            crate::internal::identity_root_import_completion::integration_tests::response_for(
                &request,
            )
        {
            return response;
        }
        if let Some(err) = &self.init_error {
            return Err(err.clone());
        }
        let parsed = ParsedUrl::parse(&request.url)?;
        let mut stream = connect_tcp(&parsed)?;
        stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
        stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;

        if parsed.scheme == "https" {
            let server_name = ServerName::try_from(parsed.host.clone()).map_err(|err| {
                crate::ImError::TransportUnavailable {
                    detail: format!("invalid TLS server name: {err}"),
                }
            })?;
            let conn =
                ClientConnection::new(self.tls_config.clone(), server_name).map_err(|err| {
                    crate::ImError::TransportUnavailable {
                        detail: err.to_string(),
                    }
                })?;
            let mut tls = StreamOwned::new(conn, stream);
            write_http_request(&mut tls, &parsed, &request)?;
            tls.sock.set_read_timeout(Some(RESPONSE_TIMEOUT))?;
            read_http_response(&mut tls)
        } else {
            write_http_request(&mut stream, &parsed, &request)?;
            stream.set_read_timeout(Some(RESPONSE_TIMEOUT))?;
            read_http_response(&mut stream)
        }
    }

    #[cfg(not(feature = "blocking"))]
    pub(crate) fn execute(&self, _request: HttpRequest) -> crate::ImResult<HttpResponse> {
        Err(crate::ImError::unsupported("sync-http"))
    }

    pub(crate) async fn execute_async(
        &self,
        request: HttpRequest,
    ) -> crate::ImResult<HttpResponse> {
        #[cfg(all(test, feature = "sqlite", feature = "identity-native-anp"))]
        if let Some(response) =
            crate::internal::identity_root_import_completion::integration_tests::response_for(
                &request,
            )
        {
            return response;
        }

        #[cfg(feature = "blocking")]
        if let Some(err) = &self.init_error {
            return Err(err.clone());
        }
        let trace_started = std::time::Instant::now();
        let trace_request = serde_json::from_slice::<serde_json::Value>(&request.body).ok();
        let http_stage = http_timing_stage(&request.url, trace_request.as_ref());
        let _http_timing = self
            .runtime
            .as_deref()
            .zip(http_stage)
            .map(|(runtime, stage)| {
                let operation = trace_request
                    .as_ref()
                    .and_then(|rpc| {
                        rpc.pointer("/params/meta/message_id")
                            .and_then(serde_json::Value::as_str)
                            .or_else(|| rpc.get("id").and_then(serde_json::Value::as_str))
                    })
                    .unwrap_or(stage)
                    .to_owned();
                HttpTimingGuard {
                    runtime,
                    operation,
                    stage,
                    started: trace_started,
                }
            });
        let trace = trace_request.as_ref().and_then(|rpc| {
            let stage = match rpc.get("method").and_then(serde_json::Value::as_str)? {
                "direct.send" => "send.http_delivery",
                "sync.delta" | "inbox.get" => "sync.http_receive",
                _ => return None,
            };
            let operation = rpc
                .pointer("/params/meta/message_id")
                .and_then(serde_json::Value::as_str)
                .or_else(|| rpc.get("id").and_then(serde_json::Value::as_str))
                .unwrap_or(stage);
            Some((operation.to_owned(), stage))
        });
        let discovery = request
            .url
            .split('?')
            .next()
            .is_some_and(|url| url.ends_with("/server-info"))
            || serde_json::from_slice::<serde_json::Value>(&request.body)
                .ok()
                .is_some_and(|body| body["method"] == "anp.get_capabilities");
        let connection_started = std::time::Instant::now();
        let route = if let Some(runtime) = &self.runtime {
            Some(runtime.route(&request.url).await?)
        } else {
            None
        };
        let client = if let (Some(runtime), Some(route)) = (&self.runtime, &route) {
            runtime.client_for_after(route, self.follow_redirects, false, connection_started)?
        } else {
            self.async_client.clone()?
        };
        let build = |client: &reqwest::Client| -> crate::ImResult<reqwest::RequestBuilder> {
            let mut builder = client
                .request(parse_method(&request.method)?, request.url.trim())
                .timeout(if discovery {
                    Duration::from_secs(10)
                } else {
                    RESPONSE_TIMEOUT
                });
            for (key, value) in &request.headers {
                builder = builder.header(key.trim(), value.trim());
            }
            if !request.body.is_empty() {
                builder = builder.body(request.body.clone());
            }
            Ok(builder)
        };
        let mut response = match build(&client)?.send().await {
            Ok(response) => response,
            Err(error) => {
                if error.is_builder() {
                    return Err(reqwest_transport_unavailable(error));
                }
                if let Some(runtime) = &self.runtime {
                    runtime.failed(&request.url);
                    // Only pre-submit connection failures, or explicitly read-only
                    // HTTP methods, can transparently try the other path once.
                    if runtime.diagnostics().policy
                        == crate::network::NetworkPolicy::SystemPreferred
                        && (error.is_connect() || matches!(request.method.as_str(), "GET" | "HEAD"))
                    {
                        let connection_started = std::time::Instant::now();
                        let alternate = runtime
                            .alternate(&request.url, route.as_ref().expect("runtime route"))
                            .await?;
                        let client = runtime.client_for_after(
                            &alternate,
                            self.follow_redirects,
                            false,
                            connection_started,
                        )?;
                        build(&client)?
                            .send()
                            .await
                            .map_err(reqwest_transport_unavailable)?
                    } else {
                        return Err(reqwest_transport_unavailable(error));
                    }
                } else {
                    return Err(reqwest_transport_unavailable(error));
                }
            }
        };
        let status_code = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body = if discovery {
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(reqwest_transport_unavailable)?
            {
                if body.len().saturating_add(chunk.len())
                    > crate::backend_capabilities::MAX_RESPONSE_BYTES
                {
                    return Err(crate::ImError::TransportUnavailable {
                        detail: "backend_capabilities_response_too_large".into(),
                    });
                }
                body.extend_from_slice(&chunk);
            }
            body
        } else {
            response
                .bytes()
                .await
                .map_err(reqwest_transport_unavailable)?
                .to_vec()
        };
        if let (Some(runtime), Some((operation, stage))) = (&self.runtime, trace) {
            runtime.trace(&operation, stage, trace_started);
        }
        Ok(HttpResponse {
            status_code,
            headers,
            body,
        })
    }

    pub(crate) fn async_client(&self) -> crate::ImResult<reqwest::Client> {
        self.async_client.clone()
    }

    pub(crate) async fn attachment_async_client(
        &self,
        url: &str,
    ) -> crate::ImResult<reqwest::Client> {
        if let Some(runtime) = &self.runtime {
            runtime.client(url, self.follow_redirects, true).await
        } else {
            self.attachment_async_client.clone()
        }
    }
}

/// Closed diagnostic labels: never copy URLs, headers or arbitrary RPC names.
fn http_timing_stage(url: &str, rpc: Option<&serde_json::Value>) -> Option<&'static str> {
    let method = rpc
        .and_then(|value| value.get("method"))
        .and_then(serde_json::Value::as_str);
    match method {
        Some("direct.send") => Some("http.direct.send"),
        Some("direct.e2ee.publish_prekey_bundle") => Some("http.direct.e2ee.publish_prekey_bundle"),
        Some("direct.e2ee.get_prekey_bundle") => Some("http.direct.e2ee.get_prekey_bundle"),
        Some("group.get") => Some("http.group.get"),
        Some("group.get_info") => Some("http.group.get_info"),
        Some("group.list_members") => Some("http.group.list_members"),
        Some("group.e2ee.send") => Some("http.group.e2ee.send"),
        Some("group.e2ee.publish_key_package") => Some("http.group.e2ee.publish_key_package"),
        Some("group.e2ee.get_key_package") => Some("http.group.e2ee.get_key_package"),
        Some("group.e2ee.create") => Some("http.group.e2ee.create"),
        Some("group.e2ee.add") => Some("http.group.e2ee.add"),
        Some("group.e2ee.remove") => Some("http.group.e2ee.remove"),
        Some("sync.delta") => Some("http.sync.delta"),
        Some("inbox.get") => Some("http.inbox.get"),
        Some("anp.get_capabilities") => Some("http.anp.get_capabilities"),
        _ if url
            .split('?')
            .next()
            .is_some_and(|value| value.ends_with("/did.json")) =>
        {
            Some("http.did.resolve")
        }
        _ => Some("http.other"),
    }
}

#[cfg(test)]
#[path = "http_timing_tests.rs"]
mod timing_tests;

fn async_client(
    ca_bundle: Option<&str>,
    follow_redirects: bool,
) -> Result<reqwest::Client, crate::ImError> {
    build_async_client(ca_bundle, follow_redirects, Some(RESPONSE_TIMEOUT))
}

fn build_async_client(
    ca_bundle: Option<&str>,
    follow_redirects: bool,
    request_timeout: Option<Duration>,
) -> Result<reqwest::Client, crate::ImError> {
    async_client_builder(ca_bundle, follow_redirects, request_timeout)?
        .build()
        .map_err(reqwest_transport_unavailable)
}

pub(crate) fn async_client_builder(
    ca_bundle: Option<&str>,
    follow_redirects: bool,
    request_timeout: Option<Duration>,
) -> crate::ImResult<reqwest::ClientBuilder> {
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(if follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });
    if let Some(request_timeout) = request_timeout {
        builder = builder.timeout(request_timeout);
    }
    if let Some(ca_bundle) = ca_bundle {
        let raw =
            fs::read(Path::new(ca_bundle)).map_err(|err| crate::ImError::TransportUnavailable {
                detail: format!("read ca bundle: {err}"),
            })?;
        let certs = reqwest::Certificate::from_pem_bundle(&raw).map_err(|err| {
            crate::ImError::TransportUnavailable {
                detail: format!("parse ca bundle: {err}"),
            }
        })?;
        if certs.is_empty() {
            return Err(crate::ImError::TransportUnavailable {
                detail: format!("invalid ca bundle: {ca_bundle}"),
            });
        }
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
    }
    Ok(builder)
}

fn parse_method(method: &str) -> crate::ImResult<reqwest::Method> {
    method
        .trim()
        .parse::<reqwest::Method>()
        .map_err(|err| crate::ImError::TransportUnavailable {
            detail: format!("invalid HTTP method: {err}"),
        })
}

fn response_headers(headers: &reqwest::header::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(key, value)| {
            Some((
                key.as_str().to_string(),
                value.to_str().ok()?.trim().to_string(),
            ))
        })
        .collect()
}

fn reqwest_transport_unavailable(err: reqwest::Error) -> crate::ImError {
    crate::ImError::TransportUnavailable {
        detail: err.to_string(),
    }
}

fn root_store(ca_bundle: Option<&str>) -> crate::ImResult<RootCertStore> {
    let mut root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let Some(ca_bundle) = ca_bundle else {
        return Ok(root_store);
    };
    let raw =
        fs::read(Path::new(ca_bundle)).map_err(|err| crate::ImError::TransportUnavailable {
            detail: format!("read ca bundle: {err}"),
        })?;
    let certs = CertificateDer::pem_slice_iter(&raw)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| crate::ImError::TransportUnavailable {
            detail: format!("parse ca bundle: {err}"),
        })?;
    let (valid_count, _) = root_store.add_parsable_certificates(certs);
    if valid_count == 0 {
        return Err(crate::ImError::TransportUnavailable {
            detail: format!("invalid ca bundle: {ca_bundle}"),
        });
    }
    Ok(root_store)
}

#[derive(Debug)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path_and_query: String,
}

#[cfg(feature = "blocking")]
impl ParsedUrl {
    fn parse(raw: &str) -> crate::ImResult<Self> {
        let (scheme, rest) =
            raw.split_once("://")
                .ok_or_else(|| crate::ImError::TransportUnavailable {
                    detail: format!("URL missing scheme: {raw}"),
                })?;
        let scheme = scheme.trim().to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(crate::ImError::TransportUnavailable {
                detail: format!("unsupported URL scheme: {scheme}"),
            });
        }
        let (authority, path) = match rest.find(['/', '?']) {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        let (host, port) = split_host_port(authority, &scheme)?;
        let path_and_query = if path.starts_with('?') {
            format!("/{path}")
        } else {
            path.to_string()
        };
        Ok(Self {
            scheme,
            host,
            port,
            path_and_query,
        })
    }
}

#[cfg(feature = "blocking")]
fn split_host_port(authority: &str, scheme: &str) -> crate::ImResult<(String, u16)> {
    let trimmed = authority.trim();
    if trimmed.is_empty() {
        return Err(crate::ImError::TransportUnavailable {
            detail: "URL missing host".to_string(),
        });
    }
    let mut parts = trimmed.rsplitn(2, ':');
    let last = parts.next().unwrap_or_default();
    let maybe_host = parts.next();
    if let Some(host) = maybe_host {
        if !last.is_empty() && last.chars().all(|ch| ch.is_ascii_digit()) {
            let port = last
                .parse::<u16>()
                .map_err(|err| crate::ImError::TransportUnavailable {
                    detail: format!("invalid URL port: {err}"),
                })?;
            return Ok((host.to_string(), port));
        }
    }
    Ok((trimmed.to_string(), default_port(scheme)))
}

#[cfg(feature = "blocking")]
fn default_port(scheme: &str) -> u16 {
    if scheme == "https" {
        443
    } else {
        80
    }
}

#[cfg(feature = "blocking")]
fn connect_tcp(parsed: &ParsedUrl) -> crate::ImResult<TcpStream> {
    let addrs = (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(|err| crate::ImError::TransportUnavailable {
            detail: err.to_string(),
        })?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err),
        }
    }
    Err(crate::ImError::TransportUnavailable {
        detail: last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "no resolved socket addresses".to_string()),
    })
}

#[cfg(feature = "blocking")]
fn write_http_request(
    mut writer: impl Write,
    parsed: &ParsedUrl,
    request: &HttpRequest,
) -> crate::ImResult<()> {
    write!(
        writer,
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: im-core\r\nConnection: close\r\n",
        request.method.trim(),
        parsed.path_and_query,
        host_header(parsed)
    )
    .map_err(crate::ImError::from)?;
    for (key, value) in &request.headers {
        write!(writer, "{}: {}\r\n", key.trim(), value.trim()).map_err(crate::ImError::from)?;
    }
    if !request.body.is_empty() {
        write!(writer, "Content-Length: {}\r\n", request.body.len())
            .map_err(crate::ImError::from)?;
    }
    writer.write_all(b"\r\n").map_err(crate::ImError::from)?;
    if !request.body.is_empty() {
        writer
            .write_all(&request.body)
            .map_err(crate::ImError::from)?;
    }
    Ok(())
}

#[cfg(feature = "blocking")]
fn host_header(parsed: &ParsedUrl) -> String {
    if parsed.port == default_port(&parsed.scheme) {
        parsed.host.clone()
    } else {
        format!("{}:{}", parsed.host, parsed.port)
    }
}

#[cfg(feature = "blocking")]
fn read_http_response(mut reader: impl Read) -> crate::ImResult<HttpResponse> {
    let raw = read_http_response_bytes(&mut reader)?;
    parse_http_response(&raw)
}

#[cfg(feature = "blocking")]
fn read_http_response_bytes(reader: &mut impl Read) -> crate::ImResult<Vec<u8>> {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => raw.extend_from_slice(&buffer[..count]),
            Err(err) if is_tolerable_response_eof(&err, &raw) => break,
            Err(err) => return Err(crate::ImError::from(err)),
        }
    }
    Ok(raw)
}

#[cfg(feature = "blocking")]
fn parse_http_response(raw: &[u8]) -> crate::ImResult<HttpResponse> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| crate::ImError::TransportUnavailable {
            detail: "response missing HTTP headers".to_string(),
        })?;
    let headers_raw =
        std::str::from_utf8(&raw[..split]).map_err(|err| crate::ImError::TransportUnavailable {
            detail: format!("invalid response headers: {err}"),
        })?;
    let body_bytes = &raw[split + 4..];
    let mut lines = headers_raw.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| crate::ImError::TransportUnavailable {
            detail: "response missing status line".to_string(),
        })?;
    let status_code = parse_status_code(status_line)?;
    let headers = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let body = if is_chunked(&headers) {
        decode_chunked_body(body_bytes)?
    } else {
        body_bytes.to_vec()
    };
    Ok(HttpResponse {
        status_code,
        headers,
        body,
    })
}

#[cfg(feature = "blocking")]
fn parse_status_code(status_line: &str) -> crate::ImResult<u16> {
    let mut parts = status_line.split_whitespace();
    let _version = parts
        .next()
        .ok_or_else(|| crate::ImError::TransportUnavailable {
            detail: "response missing HTTP version".to_string(),
        })?;
    parts
        .next()
        .ok_or_else(|| crate::ImError::TransportUnavailable {
            detail: "response missing status code".to_string(),
        })?
        .parse::<u16>()
        .map_err(|err| crate::ImError::TransportUnavailable {
            detail: format!("invalid HTTP status code: {err}"),
        })
}

#[cfg(feature = "blocking")]
fn is_chunked(headers: &BTreeMap<String, String>) -> bool {
    headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
    })
}

#[cfg(feature = "blocking")]
fn decode_chunked_body(bytes: &[u8]) -> crate::ImResult<Vec<u8>> {
    let mut offset = 0usize;
    let mut decoded = Vec::new();
    loop {
        let line_end = find_crlf(bytes, offset).ok_or_else(|| crate::ImError::Serialization {
            detail: "chunked response missing chunk size".to_string(),
        })?;
        let size_line = std::str::from_utf8(&bytes[offset..line_end]).map_err(|err| {
            crate::ImError::Serialization {
                detail: format!("invalid chunk size: {err}"),
            }
        })?;
        let size_hex = size_line.split(';').next().unwrap_or_default().trim();
        let size =
            usize::from_str_radix(size_hex, 16).map_err(|err| crate::ImError::Serialization {
                detail: format!("invalid chunk size: {err}"),
            })?;
        offset = line_end + 2;
        if size == 0 {
            break;
        }
        if bytes.len() < offset + size + 2 {
            return Err(crate::ImError::Serialization {
                detail: "chunked response ended early".to_string(),
            });
        }
        decoded.extend_from_slice(&bytes[offset..offset + size]);
        offset += size + 2;
    }
    Ok(decoded)
}

#[cfg(feature = "blocking")]
fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

#[cfg(feature = "blocking")]
fn is_tolerable_response_eof(err: &std::io::Error, raw: &[u8]) -> bool {
    if err.kind() != std::io::ErrorKind::UnexpectedEof {
        return false;
    }
    let lower = err.to_string().to_ascii_lowercase();
    if !lower.contains("close_notify") && !lower.contains("unexpected eof") {
        return false;
    }
    response_is_complete(raw)
}

#[cfg(feature = "blocking")]
fn response_is_complete(raw: &[u8]) -> bool {
    let Some(split) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let Ok(headers_raw) = std::str::from_utf8(&raw[..split]) else {
        return false;
    };
    let body = &raw[split + 4..];
    let headers = headers_raw
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    if is_chunked(&headers) {
        return decode_chunked_body(body).is_ok();
    }
    if let Some(content_length) = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
    {
        return body.len() == content_length;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_request_debug_redacts_authorization_and_body() {
        let request = HttpRequest {
            method: "POST".to_owned(),
            url: "https://example.test/rpc".to_owned(),
            headers: BTreeMap::from([
                (
                    "Authorization".to_owned(),
                    "Bearer access-secret".to_owned(),
                ),
                ("Content-Type".to_owned(), "application/json".to_owned()),
            ]),
            body: br#"{"refresh_token":"refresh-secret"}"#.to_vec(),
        };

        let debug = format!("{request:?}");
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("refresh-secret"));
        assert!(debug.contains("Authorization"));
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("Content-Type"));
    }

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn skill_onboarding_no_redirect_does_not_contact_redirect_origin() {
        let redirect_target = TcpListener::bind("127.0.0.1:0").unwrap();
        redirect_target.set_nonblocking(true).unwrap();
        let target_url = format!("http://{}/token", redirect_target.local_addr().unwrap());

        let redirect_source = TcpListener::bind("127.0.0.1:0").unwrap();
        let source_url = format!("http://{}/claim", redirect_source.local_addr().unwrap());
        let source = thread::spawn(move || {
            let (mut stream, _) = redirect_source.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });

        let (contacted_tx, contacted_rx) = mpsc::channel();
        let target = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline {
                match redirect_target.accept() {
                    Ok(_) => {
                        contacted_tx.send(true).unwrap();
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("redirect target accept failed: {error}"),
                }
            }
            contacted_tx.send(false).unwrap();
        });

        let response = HttpClient::with_ca_bundle(None, false)
            .execute_async(HttpRequest {
                method: "POST".to_owned(),
                url: source_url,
                headers: BTreeMap::new(),
                body: b"secret-token".to_vec(),
            })
            .await
            .unwrap();

        assert_eq!(response.status_code, 302);
        assert!(!contacted_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        source.join().unwrap();
        target.join().unwrap();
    }
}

pub(crate) fn network_tls_config(
    ca_bundle: Option<&str>,
) -> crate::ImResult<std::sync::Arc<ClientConfig>> {
    Ok(std::sync::Arc::new(
        ClientConfig::builder()
            .with_root_certificates(root_store(ca_bundle)?)
            .with_no_client_auth(),
    ))
}
