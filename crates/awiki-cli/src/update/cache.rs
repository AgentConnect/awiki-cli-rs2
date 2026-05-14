use super::Metadata;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
struct CacheFile {
    #[serde(default)]
    latest_version: String,
    #[serde(default)]
    min_supported_version: String,
    #[serde(default)]
    retrieved_at: String,
}

#[derive(Debug, Clone)]
struct CacheRead {
    metadata: Metadata,
    fresh: bool,
}

#[derive(Debug)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path_and_query: String,
}

pub fn load_metadata(
    cache_dir: &str,
    ttl_seconds: i64,
    prefer_fresh: bool,
    cache_only: bool,
    registry_urls: &[String],
) -> Result<Metadata, String> {
    let mut cached = read_cache(&cache_path(cache_dir), ttl_seconds)
        .ok()
        .flatten()
        .filter(|cache| !cache.metadata.latest_version.trim().is_empty());

    if let Some(cache) = cached.as_mut() {
        if cache.fresh {
            cache.metadata.source = "cache".to_string();
            if !prefer_fresh || cache_only {
                return Ok(cache.metadata.clone());
            }
        }
    }

    if cache_only {
        return Err(
            "update cache-only mode is enabled but no cached metadata is available".to_string(),
        );
    }

    match fetch_from_registry(registry_urls) {
        Ok(network) => {
            let _ = write_cache(&cache_path(cache_dir), &network);
            Ok(network)
        }
        Err(err) => {
            if let Some(cache) = cached {
                let mut metadata = cache.metadata;
                metadata.source = "cache_stale".to_string();
                Ok(metadata)
            } else {
                Err(err)
            }
        }
    }
}

fn fetch_from_registry(registry_urls: &[String]) -> Result<Metadata, String> {
    fetch_from_registry_urls(registry_urls)
}

fn fetch_from_registry_urls(registry_urls: &[String]) -> Result<Metadata, String> {
    if registry_urls.is_empty() {
        return Err("no npm registry URLs configured".to_string());
    }

    let mut errors = Vec::new();
    for url in registry_urls {
        match fetch_from_registry_url(url) {
            Ok(metadata) => return Ok(metadata),
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }

    Err(format!(
        "failed to fetch awiki-cli metadata from npm registries: {}",
        errors.join("; ")
    ))
}

fn fetch_from_registry_url(url: &str) -> Result<Metadata, String> {
    let parsed = parse_url(url)?;
    let response = http_get(&parsed)?;
    if response.status_code != 200 {
        return Err(format!(
            "registry responded with status {}",
            response.status_code
        ));
    }

    let body: RegistryResponse =
        serde_json::from_str(&response.body).map_err(|err| err.to_string())?;
    let latest = body.version.trim().to_string();
    if latest.is_empty() {
        return Err("npm metadata missing version".to_string());
    }

    Ok(Metadata {
        latest_version: latest,
        min_supported_version: body.awiki_cli.min_supported_version.trim().to_string(),
        source: "network".to_string(),
    })
}

fn parse_url(raw: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| format!("registry URL missing scheme: {raw}"))?;
    let scheme = scheme.trim().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("unsupported registry URL scheme: {scheme}"));
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

fn split_host_port(authority: &str, scheme: &str) -> Result<(String, u16), String> {
    let trimmed = authority.trim();
    if trimmed.is_empty() {
        return Err("registry URL missing host".to_string());
    }
    if trimmed.starts_with('[') {
        let end = trimmed
            .find(']')
            .ok_or_else(|| format!("invalid bracketed host in registry URL: {authority}"))?;
        let host = trimmed[1..end].to_string();
        let remainder = &trimmed[end + 1..];
        let port = if let Some(raw_port) = remainder.strip_prefix(':') {
            raw_port
                .parse::<u16>()
                .map_err(|err| format!("invalid registry URL port: {err}"))?
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
                .map_err(|err| format!("invalid registry URL port: {err}"))?;
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

#[derive(Debug)]
struct HttpResponse {
    status_code: u16,
    body: String,
}

fn http_get(parsed: &ParsedUrl) -> Result<HttpResponse, String> {
    let proxy = proxy_for(parsed);
    let connect_target = proxy.as_ref().unwrap_or(parsed);
    let mut stream = TcpStream::connect((connect_target.host.as_str(), connect_target.port))
        .map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|err| err.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|err| err.to_string())?;

    if parsed.scheme == "https" {
        if proxy.is_some() {
            establish_proxy_tunnel(&mut stream, parsed)?;
        }
        let config = Arc::new(rustls_client_config());
        let server_name = ServerName::try_from(parsed.host.clone())
            .map_err(|err| format!("invalid TLS server name: {err}"))?;
        let conn = ClientConnection::new(config, server_name).map_err(|err| err.to_string())?;
        let mut tls = StreamOwned::new(conn, stream);
        write_http_request(&mut tls, parsed)?;
        read_http_response(&mut tls)
    } else {
        let mut plain = stream;
        if proxy.is_some() {
            write_http_proxy_request(&mut plain, parsed)?;
        } else {
            write_http_request(&mut plain, parsed)?;
        }
        read_http_response(&mut plain)
    }
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

fn establish_proxy_tunnel(stream: &mut TcpStream, parsed: &ParsedUrl) -> Result<(), String> {
    let authority = format!("{}:{}", parsed.host, parsed.port);
    let request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: awiki-cli-rs2\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let headers = read_http_headers(stream)?;
    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| "proxy response missing status line".to_string())?;
    let status = parse_status_code(status_line)?;
    if status != 200 {
        return Err(format!("proxy CONNECT responded with status {status}"));
    }
    Ok(())
}

fn rustls_client_config() -> ClientConfig {
    let root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth()
}

fn write_http_request(mut writer: impl Write, parsed: &ParsedUrl) -> Result<(), String> {
    write_http_request_target(&mut writer, parsed, &parsed.path_and_query)
}

fn write_http_proxy_request(mut writer: impl Write, parsed: &ParsedUrl) -> Result<(), String> {
    let target = format!(
        "{}://{}{}",
        parsed.scheme,
        host_header(parsed),
        parsed.path_and_query
    );
    write_http_request_target(&mut writer, parsed, &target)
}

fn write_http_request_target(
    mut writer: impl Write,
    parsed: &ParsedUrl,
    target: &str,
) -> Result<(), String> {
    let host = host_header(parsed);
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: awiki-cli-rs2\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
    );
    writer
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())
}

fn host_header(parsed: &ParsedUrl) -> String {
    let host = if parsed.port == default_port(&parsed.scheme) {
        parsed.host.clone()
    } else {
        format!("{}:{}", parsed.host, parsed.port)
    };
    host
}

fn read_http_response(mut reader: impl Read) -> Result<HttpResponse, String> {
    let mut raw = Vec::new();
    reader
        .read_to_end(&mut raw)
        .map_err(|err| err.to_string())?;
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "registry response missing HTTP headers".to_string())?;
    let headers_raw =
        std::str::from_utf8(&raw[..split]).map_err(|err| format!("invalid headers: {err}"))?;
    let body_bytes = &raw[split + 4..];
    let mut lines = headers_raw.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| "registry response missing status line".to_string())?;
    let status_code = parse_status_code(status_line)?;
    let chunked = lines.any(|line| {
        line.to_ascii_lowercase()
            .starts_with("transfer-encoding: chunked")
    });
    let body = if chunked {
        decode_chunked_body(body_bytes)?
    } else {
        String::from_utf8_lossy(body_bytes).to_string()
    };
    Ok(HttpResponse { status_code, body })
}

fn read_http_headers(reader: &mut impl Read) -> Result<String, String> {
    let mut raw = Vec::new();
    let mut byte = [0; 1];
    while raw.len() < 16 * 1024 {
        let read = reader.read(&mut byte).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            return String::from_utf8(raw).map_err(|err| err.to_string());
        }
    }
    Err("proxy response missing HTTP header terminator".to_string())
}

fn parse_status_code(status_line: &str) -> Result<u16, String> {
    let mut parts = status_line.split_whitespace();
    let _version = parts
        .next()
        .ok_or_else(|| "registry response missing HTTP version".to_string())?;
    parts
        .next()
        .ok_or_else(|| "registry response missing status code".to_string())?
        .parse::<u16>()
        .map_err(|err| format!("invalid HTTP status code: {err}"))
}

fn decode_chunked_body(bytes: &[u8]) -> Result<String, String> {
    let mut offset = 0usize;
    let mut decoded = Vec::new();
    loop {
        let line_end = find_crlf(bytes, offset)
            .ok_or_else(|| "chunked registry response missing chunk size".to_string())?;
        let size_line = std::str::from_utf8(&bytes[offset..line_end])
            .map_err(|err| format!("invalid chunk size: {err}"))?;
        let size_hex = size_line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|err| format!("invalid chunk size: {err}"))?;
        offset = line_end + 2;
        if size == 0 {
            break;
        }
        if bytes.len() < offset + size + 2 {
            return Err("chunked registry response ended early".to_string());
        }
        decoded.extend_from_slice(&bytes[offset..offset + size]);
        offset += size + 2;
    }
    String::from_utf8(decoded).map_err(|err| err.to_string())
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

#[derive(Debug, Deserialize)]
struct RegistryResponse {
    #[serde(default)]
    version: String,
    #[serde(default, rename = "awikiCli")]
    awiki_cli: RegistryAwikiCli,
}

#[derive(Debug, Default, Deserialize)]
struct RegistryAwikiCli {
    #[serde(default, rename = "minSupportedVersion")]
    min_supported_version: String,
}

fn write_cache(path: &Path, metadata: &Metadata) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        set_dir_permissions(parent)?;
    }

    let payload = CacheWrite {
        latest_version: metadata.latest_version.as_str(),
        min_supported_version: metadata.min_supported_version.as_str(),
        retrieved_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|err| err.to_string())?,
        source: metadata.source.as_str(),
    };
    let raw = serde_json::to_string_pretty(&payload).map_err(|err| err.to_string())?;
    write_restricted_file(path, format!("{raw}\n").as_bytes())
}

#[derive(Debug, Serialize)]
struct CacheWrite<'a> {
    latest_version: &'a str,
    min_supported_version: &'a str,
    retrieved_at: String,
    source: &'a str,
}

fn write_restricted_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())
    }
}

fn set_dir_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|err| err.to_string())?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn cache_path(cache_dir: &str) -> PathBuf {
    Path::new(cache_dir).join("update").join("metadata.json")
}

fn read_cache(path: &Path, ttl_seconds: i64) -> Result<Option<CacheRead>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    let file: CacheFile = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    let retrieved_at = parse_retrieved_at(&file.retrieved_at)?;
    let fresh = match retrieved_at {
        Some(retrieved_at) if ttl_seconds > 0 => {
            OffsetDateTime::now_utc() - retrieved_at <= time::Duration::seconds(ttl_seconds)
        }
        _ => true,
    };
    if !fresh {
        return Ok(None);
    }
    Ok(Some(CacheRead {
        metadata: Metadata {
            latest_version: file.latest_version.trim().to_string(),
            min_supported_version: file.min_supported_version.trim().to_string(),
            source: "cache".to_string(),
        },
        fresh,
    }))
}

fn parse_retrieved_at(raw: &str) -> Result<Option<OffsetDateTime>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    OffsetDateTime::parse(trimmed, &Rfc3339)
        .map(Some)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_from_registry_rejects_missing_version() {
        let server =
            crate::update::tests::TestServer::new(vec![crate::update::tests::TestResponse::ok(
                r#"{"awikiCli":{"minSupportedVersion":"1.0.8"}}"#,
            )]);

        let err = fetch_from_registry_urls(&[server.url("/latest")]).expect_err("error");

        assert!(
            err.contains("npm metadata missing version"),
            "error should report missing version: {err}"
        );
    }

    #[test]
    fn fetch_from_registry_allows_missing_min_supported_version() {
        let server =
            crate::update::tests::TestServer::new(vec![crate::update::tests::TestResponse::ok(
                r#"{"version":"1.0.9"}"#,
            )]);

        let metadata = fetch_from_registry_urls(&[server.url("/latest")]).expect("metadata");

        assert_eq!(metadata.latest_version, "1.0.9");
        assert_eq!(metadata.min_supported_version, "");
        assert_eq!(metadata.source, "network");
    }
}
