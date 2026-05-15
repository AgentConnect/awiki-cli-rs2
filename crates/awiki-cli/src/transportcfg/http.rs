use super::{resolve, Config};
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::env;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HttpClient {
    config: Config,
    tls_config: Arc<ClientConfig>,
    trusted_root_count: usize,
    proxy_env: bool,
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout: Option<Duration>,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
            timeout: None,
        }
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = (!timeout.is_zero()).then_some(timeout);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum HttpClientError {
    InvalidUrl(String),
    ReadCABundle(std::io::Error),
    InvalidCABundle(String),
    Io(std::io::Error),
    Tls(String),
    Message(String),
}

impl fmt::Display for HttpClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(message) | Self::Tls(message) | Self::Message(message) => {
                f.write_str(message)
            }
            Self::ReadCABundle(err) => write!(f, "read ca bundle: {err}"),
            Self::InvalidCABundle(path) => write!(f, "invalid ca bundle: {path}"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for HttpClientError {}

impl From<std::io::Error> for HttpClientError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn new_http_client(ca_bundle: &str) -> Result<HttpClient, HttpClientError> {
    new_http_client_with_proxy(ca_bundle, false)
}

pub fn new_http_client_with_proxy_env(ca_bundle: &str) -> Result<HttpClient, HttpClientError> {
    new_http_client_with_proxy(ca_bundle, true)
}

fn new_http_client_with_proxy(
    ca_bundle: &str,
    proxy_env: bool,
) -> Result<HttpClient, HttpClientError> {
    let (tls_config, trusted_root_count) = rustls_client_config(ca_bundle)?;
    Ok(HttpClient {
        config: resolve(),
        tls_config: Arc::new(tls_config),
        trusted_root_count,
        proxy_env,
    })
}

impl HttpClient {
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn trusted_root_count(&self) -> usize {
        self.trusted_root_count
    }

    pub fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let parsed = parse_url(&request.url)?;
        let response_timeout =
            effective_timeout(self.config.http_response_header_timeout, request.timeout);
        let proxy = self.proxy_env.then(|| proxy_for(&parsed)).flatten();
        let connect_target = proxy.as_ref().unwrap_or(&parsed);
        let mut stream = connect_tcp(connect_target, &self.config)?;
        stream.set_write_timeout(Some(self.config.http_tls_handshake_timeout))?;
        stream.set_read_timeout(Some(self.config.http_tls_handshake_timeout))?;

        if parsed.scheme == "https" {
            if proxy.is_some() {
                establish_proxy_tunnel(&mut stream, &parsed)?;
            }
            let server_name = ServerName::try_from(parsed.host.clone())
                .map_err(|err| HttpClientError::Tls(format!("invalid TLS server name: {err}")))?;
            let conn = ClientConnection::new(self.tls_config.clone(), server_name)
                .map_err(|err| HttpClientError::Tls(err.to_string()))?;
            let mut tls = StreamOwned::new(conn, stream);
            write_http_request(&mut tls, &parsed, &request, RequestTarget::OriginForm)?;
            tls.sock.set_read_timeout(Some(response_timeout))?;
            read_http_response(&mut tls)
        } else {
            let target = if proxy.is_some() {
                RequestTarget::AbsoluteForm
            } else {
                RequestTarget::OriginForm
            };
            write_http_request(&mut stream, &parsed, &request, target)?;
            stream.set_read_timeout(Some(response_timeout))?;
            read_http_response(&mut stream)
        }
    }
}

fn effective_timeout(base: Duration, request: Option<Duration>) -> Duration {
    request
        .filter(|timeout| !timeout.is_zero())
        .map(|timeout| std::cmp::min(base, timeout))
        .unwrap_or(base)
}

fn rustls_client_config(ca_bundle: &str) -> Result<(ClientConfig, usize), HttpClientError> {
    let mut root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let ca_bundle = ca_bundle.trim();
    if !ca_bundle.is_empty() {
        let raw = fs::read(Path::new(ca_bundle)).map_err(HttpClientError::ReadCABundle)?;
        let certs = CertificateDer::pem_slice_iter(&raw)
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let (valid_count, _) = root_store.add_parsable_certificates(certs);
        if valid_count == 0 {
            return Err(HttpClientError::InvalidCABundle(ca_bundle.to_string()));
        }
    }
    let trusted_root_count = root_store.roots.len();
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok((config, trusted_root_count))
}

#[derive(Debug)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path_and_query: String,
}

fn parse_url(raw: &str) -> Result<ParsedUrl, HttpClientError> {
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| HttpClientError::InvalidUrl(format!("URL missing scheme: {raw}")))?;
    let scheme = scheme.trim().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(HttpClientError::InvalidUrl(format!(
            "unsupported URL scheme: {scheme}"
        )));
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
    Ok(ParsedUrl {
        scheme,
        host,
        port,
        path_and_query,
    })
}

fn split_host_port(authority: &str, scheme: &str) -> Result<(String, u16), HttpClientError> {
    let trimmed = authority.trim();
    if trimmed.is_empty() {
        return Err(HttpClientError::InvalidUrl("URL missing host".to_string()));
    }
    if trimmed.starts_with('[') {
        let end = trimmed.find(']').ok_or_else(|| {
            HttpClientError::InvalidUrl(format!("invalid bracketed host in URL: {authority}"))
        })?;
        let host = trimmed[1..end].to_string();
        let remainder = &trimmed[end + 1..];
        let port = if let Some(raw_port) = remainder.strip_prefix(':') {
            raw_port
                .parse::<u16>()
                .map_err(|err| HttpClientError::InvalidUrl(format!("invalid URL port: {err}")))?
        } else {
            default_port(scheme)
        };
        return Ok((host, port));
    }

    let mut parts = trimmed.rsplitn(2, ':');
    let last = parts.next().unwrap_or_default();
    let maybe_host = parts.next();
    if let Some(host) = maybe_host {
        if !last.is_empty() && last.chars().all(|ch| ch.is_ascii_digit()) {
            let port = last
                .parse::<u16>()
                .map_err(|err| HttpClientError::InvalidUrl(format!("invalid URL port: {err}")))?;
            return Ok((host.to_string(), port));
        }
    }
    Ok((trimmed.to_string(), default_port(scheme)))
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "https" {
        443
    } else {
        80
    }
}

fn connect_tcp(parsed: &ParsedUrl, config: &Config) -> Result<TcpStream, HttpClientError> {
    let addrs = (parsed.host.as_str(), parsed.port).to_socket_addrs()?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, config.http_dial_timeout) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error
        .map(HttpClientError::Io)
        .unwrap_or_else(|| HttpClientError::Message("no resolved socket addresses".to_string())))
}

fn proxy_for(parsed: &ParsedUrl) -> Option<ParsedUrl> {
    if no_proxy_matches(&parsed.host) {
        return None;
    }
    let raw = if parsed.scheme == "https" {
        first_env(&["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"])
    } else {
        first_env(&["HTTP_PROXY", "http_proxy"])
    }?;
    let proxy = parse_url(&raw).ok()?;
    if proxy.scheme == "http" {
        Some(proxy)
    } else {
        None
    }
}

fn first_env(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn no_proxy_matches(host: &str) -> bool {
    let Some(raw) = first_env(&["NO_PROXY", "no_proxy"]) else {
        return false;
    };
    let host = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            let entry = entry
                .trim_matches(['[', ']'])
                .split(':')
                .next()
                .unwrap_or(entry)
                .trim()
                .to_ascii_lowercase();
            if entry.is_empty() {
                return false;
            }
            host == entry
                || entry
                    .strip_prefix('.')
                    .map(|suffix| host.ends_with(suffix))
                    .unwrap_or(false)
                || host.ends_with(&format!(".{entry}"))
        })
}

fn establish_proxy_tunnel(
    stream: &mut TcpStream,
    parsed: &ParsedUrl,
) -> Result<(), HttpClientError> {
    let authority = format!("{}:{}", parsed.host, parsed.port);
    write!(
        stream,
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: awiki-cli-rs2\r\n\r\n"
    )?;
    let headers = read_http_headers(stream)?;
    let status_line = headers.lines().next().ok_or_else(|| {
        HttpClientError::Message("proxy response missing status line".to_string())
    })?;
    let status = parse_status_code(status_line)?;
    if status != 200 {
        return Err(HttpClientError::Message(format!(
            "proxy CONNECT responded with status {status}"
        )));
    }
    Ok(())
}

fn read_http_headers(reader: &mut impl Read) -> Result<String, HttpClientError> {
    let mut raw = Vec::new();
    let mut byte = [0; 1];
    while raw.len() < 16 * 1024 {
        let read = reader.read(&mut byte)?;
        if read == 0 {
            break;
        }
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            return String::from_utf8(raw).map_err(|err| HttpClientError::Message(err.to_string()));
        }
    }
    Err(HttpClientError::Message(
        "proxy response missing HTTP header terminator".to_string(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestTarget {
    OriginForm,
    AbsoluteForm,
}

fn write_http_request(
    mut writer: impl Write,
    parsed: &ParsedUrl,
    request: &HttpRequest,
    target: RequestTarget,
) -> Result<(), HttpClientError> {
    let target = match target {
        RequestTarget::OriginForm => parsed.path_and_query.clone(),
        RequestTarget::AbsoluteForm => format!(
            "{}://{}{}",
            parsed.scheme,
            host_header(parsed),
            parsed.path_and_query
        ),
    };
    write!(
        writer,
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: awiki-cli-rs2\r\nConnection: close\r\n",
        request.method.trim(),
        target,
        host_header(parsed)
    )?;
    for (key, value) in &request.headers {
        write!(writer, "{}: {}\r\n", key.trim(), value.trim())?;
    }
    if !request.body.is_empty() {
        write!(writer, "Content-Length: {}\r\n", request.body.len())?;
    }
    writer.write_all(b"\r\n")?;
    if !request.body.is_empty() {
        writer.write_all(&request.body)?;
    }
    Ok(())
}

fn host_header(parsed: &ParsedUrl) -> String {
    if parsed.port == default_port(&parsed.scheme) {
        parsed.host.clone()
    } else {
        format!("{}:{}", parsed.host, parsed.port)
    }
}

fn read_http_response(mut reader: impl Read) -> Result<HttpResponse, HttpClientError> {
    let raw = read_http_response_bytes(&mut reader)?;
    parse_http_response(&raw)
}

fn read_http_response_bytes(reader: &mut impl Read) -> Result<Vec<u8>, HttpClientError> {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => raw.extend_from_slice(&buffer[..count]),
            Err(err) if is_tolerable_response_eof(&err, &raw) => break,
            Err(err) => return Err(HttpClientError::Io(err)),
        }
    }
    Ok(raw)
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, HttpClientError> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| HttpClientError::Message("response missing HTTP headers".to_string()))?;
    let headers_raw = std::str::from_utf8(&raw[..split])
        .map_err(|err| HttpClientError::Message(format!("invalid headers: {err}")))?;
    let body_bytes = &raw[split + 4..];
    let mut lines = headers_raw.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| HttpClientError::Message("response missing status line".to_string()))?;
    let status_code = parse_status_code(status_line)?;
    let headers = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect::<Vec<_>>();
    let chunked = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
    });
    let body = if chunked {
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

fn is_tolerable_response_eof(err: &std::io::Error, raw: &[u8]) -> bool {
    if err.kind() != std::io::ErrorKind::UnexpectedEof {
        return false;
    }
    let message = err.to_string();
    let lower = message.to_ascii_lowercase();
    if !lower.contains("close_notify") && !lower.contains("unexpected eof") {
        return false;
    }
    response_is_complete(raw)
}

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
            Some((key.trim(), value.trim()))
        })
        .collect::<Vec<_>>();
    if headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
    }) {
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

fn parse_status_code(status_line: &str) -> Result<u16, HttpClientError> {
    let mut parts = status_line.split_whitespace();
    let _version = parts
        .next()
        .ok_or_else(|| HttpClientError::Message("response missing HTTP version".to_string()))?;
    parts
        .next()
        .ok_or_else(|| HttpClientError::Message("response missing status code".to_string()))?
        .parse::<u16>()
        .map_err(|err| HttpClientError::Message(format!("invalid HTTP status code: {err}")))
}

fn decode_chunked_body(bytes: &[u8]) -> Result<Vec<u8>, HttpClientError> {
    let mut offset = 0usize;
    let mut decoded = Vec::new();
    loop {
        let line_end = find_crlf(bytes, offset).ok_or_else(|| {
            HttpClientError::Message("chunked response missing chunk size".to_string())
        })?;
        let size_line = std::str::from_utf8(&bytes[offset..line_end])
            .map_err(|err| HttpClientError::Message(format!("invalid chunk size: {err}")))?;
        let size_hex = size_line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|err| HttpClientError::Message(format!("invalid chunk size: {err}")))?;
        offset = line_end + 2;
        if size == 0 {
            break;
        }
        if bytes.len() < offset + size + 2 {
            return Err(HttpClientError::Message(
                "chunked response ended early".to_string(),
            ));
        }
        decoded.extend_from_slice(&bytes[offset..offset + size]);
        offset += size + 2;
    }
    Ok(decoded)
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

#[cfg(test)]
mod tests {
    use super::response_is_complete;

    #[test]
    fn close_delimited_response_is_complete_after_headers_like_go_net_http() {
        assert!(response_is_complete(b"HTTP/1.1 204 No Content\r\n\r\n"));
        assert!(response_is_complete(b"HTTP/1.1 200 OK\r\n\r\nbody"));
    }
}
