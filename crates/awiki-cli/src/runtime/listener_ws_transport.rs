use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rand::RngCore;
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::{Map, Value};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const READ_HEADER_LIMIT: usize = 16 * 1024;
const MAX_FRAME_SIZE: u64 = 16 * 1024 * 1024;
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsDialError {
    pub status_code: Option<u16>,
    pub message: String,
}

impl fmt::Display for WsDialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for WsDialError {}

pub struct WsTransport {
    stream: Box<dyn ReadWrite>,
}

trait ReadWrite: Read + Write + Send {}

impl<T> ReadWrite for T where T: Read + Write + Send {}

impl WsTransport {
    pub fn connect(
        websocket_url: &str,
        bearer_token: &str,
        ca_bundle: &str,
    ) -> Result<Self, WsDialError> {
        let parsed = ParsedWsUrl::parse(websocket_url)?;
        let mut stream = connect_stream(&parsed, ca_bundle)?;
        let key = websocket_key();
        write_handshake_request(&mut stream, &parsed, bearer_token, &key)?;
        let headers = read_http_headers(&mut stream)?;
        validate_handshake_response(&headers, &key)?;
        Ok(Self { stream })
    }

    pub fn send_json(&mut self, payload: &Map<String, Value>) -> anyhow::Result<()> {
        let raw = serde_json::to_vec(payload)?;
        self.write_frame(0x1, &raw)
    }

    pub fn ping(&mut self) -> anyhow::Result<()> {
        self.write_frame(0x9, b"")
    }

    pub fn close(&mut self) -> anyhow::Result<()> {
        self.write_frame(0x8, b"")
    }

    pub fn read_json_message(&mut self) -> anyhow::Result<Map<String, Value>> {
        loop {
            match self.read_frame()? {
                WsFrame::Text(raw) => return decode_json_object(raw.as_bytes()),
                WsFrame::Binary(raw) => return decode_json_object(&raw),
                WsFrame::Ping(payload) => {
                    self.write_frame(0xA, &payload)?;
                }
                WsFrame::Pong => {}
                WsFrame::Close => anyhow::bail!("websocket notification loop closed"),
            }
        }
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> anyhow::Result<()> {
        let mut header = Vec::with_capacity(14);
        header.push(0x80 | (opcode & 0x0F));
        if payload.len() < 126 {
            header.push(0x80 | payload.len() as u8);
        } else if payload.len() <= u16::MAX as usize {
            header.push(0x80 | 126);
            header.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        } else {
            header.push(0x80 | 127);
            header.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
        let mut mask = [0_u8; 4];
        rand::thread_rng().fill_bytes(&mut mask);
        header.extend_from_slice(&mask);
        self.stream.write_all(&header)?;
        for (idx, byte) in payload.iter().enumerate() {
            self.stream.write_all(&[*byte ^ mask[idx % 4]])?;
        }
        self.stream.flush()?;
        Ok(())
    }

    fn read_frame(&mut self) -> anyhow::Result<WsFrame> {
        let mut head = [0_u8; 2];
        self.stream.read_exact(&mut head)?;
        let opcode = head[0] & 0x0F;
        let masked = head[1] & 0x80 != 0;
        let mut len = u64::from(head[1] & 0x7F);
        if len == 126 {
            let mut bytes = [0_u8; 2];
            self.stream.read_exact(&mut bytes)?;
            len = u64::from(u16::from_be_bytes(bytes));
        } else if len == 127 {
            let mut bytes = [0_u8; 8];
            self.stream.read_exact(&mut bytes)?;
            len = u64::from_be_bytes(bytes);
        }
        if len > MAX_FRAME_SIZE {
            anyhow::bail!("websocket frame is too large: {len}");
        }
        let mut mask = [0_u8; 4];
        if masked {
            self.stream.read_exact(&mut mask)?;
        }
        let mut payload = vec![0_u8; len as usize];
        self.stream.read_exact(&mut payload)?;
        if masked {
            for (idx, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[idx % 4];
            }
        }
        match opcode {
            0x1 => Ok(WsFrame::Text(String::from_utf8(payload)?)),
            0x2 => Ok(WsFrame::Binary(payload)),
            0x8 => Ok(WsFrame::Close),
            0x9 => Ok(WsFrame::Ping(payload)),
            0xA => Ok(WsFrame::Pong),
            _ => anyhow::bail!("unsupported websocket frame opcode: {opcode}"),
        }
    }
}

enum WsFrame {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close,
}

fn decode_json_object(raw: &[u8]) -> anyhow::Result<Map<String, Value>> {
    match serde_json::from_slice::<Value>(raw)? {
        Value::Object(map) => Ok(map),
        _ => anyhow::bail!("websocket JSON message must be an object"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedWsUrl {
    scheme: String,
    host: String,
    port: u16,
    path_and_query: String,
}

impl ParsedWsUrl {
    fn parse(raw: &str) -> Result<Self, WsDialError> {
        let (scheme, rest) = raw
            .split_once("://")
            .ok_or_else(|| dial_error(None, format!("websocket URL missing scheme: {raw}")))?;
        let scheme = scheme.trim().to_ascii_lowercase();
        if scheme != "ws" && scheme != "wss" {
            return Err(dial_error(
                None,
                format!("unsupported websocket URL scheme: {scheme}"),
            ));
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

    fn host_header(&self) -> String {
        if self.port == default_port(&self.scheme) {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn split_host_port(authority: &str, scheme: &str) -> Result<(String, u16), WsDialError> {
    let authority = authority.trim();
    if authority.is_empty() {
        return Err(dial_error(None, "websocket URL missing host"));
    }
    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| dial_error(None, format!("invalid bracketed host: {authority}")))?;
        let host = authority[1..end].to_string();
        let port = if let Some(raw) = authority[end + 1..].strip_prefix(':') {
            raw.parse::<u16>()
                .map_err(|err| dial_error(None, format!("invalid websocket URL port: {err}")))?
        } else {
            default_port(scheme)
        };
        return Ok((host, port));
    }
    let mut parts = authority.rsplitn(2, ':');
    let last = parts.next().unwrap_or_default();
    if let Some(host) = parts.next() {
        if !last.is_empty() && last.chars().all(|ch| ch.is_ascii_digit()) {
            let port = last
                .parse::<u16>()
                .map_err(|err| dial_error(None, format!("invalid websocket URL port: {err}")))?;
            return Ok((host.to_string(), port));
        }
    }
    Ok((authority.to_string(), default_port(scheme)))
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "wss" {
        443
    } else {
        80
    }
}

fn connect_stream(
    parsed: &ParsedWsUrl,
    ca_bundle: &str,
) -> Result<Box<dyn ReadWrite>, WsDialError> {
    let addrs = (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(|err| dial_error(None, err.to_string()))?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(8)) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(90)))
                    .map_err(|err| dial_error(None, err.to_string()))?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(15)))
                    .map_err(|err| dial_error(None, err.to_string()))?;
                if parsed.scheme == "wss" {
                    let config = Arc::new(rustls_config(ca_bundle)?);
                    let server_name = ServerName::try_from(parsed.host.clone()).map_err(|err| {
                        dial_error(None, format!("invalid TLS server name: {err}"))
                    })?;
                    let conn = ClientConnection::new(config, server_name)
                        .map_err(|err| dial_error(None, err.to_string()))?;
                    return Ok(Box::new(StreamOwned::new(conn, stream)));
                }
                return Ok(Box::new(stream));
            }
            Err(err) => last_error = Some(err),
        }
    }
    Err(dial_error(
        None,
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "no resolved websocket socket addresses".to_string()),
    ))
}

fn rustls_config(ca_bundle: &str) -> Result<ClientConfig, WsDialError> {
    let mut root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let ca_bundle = ca_bundle.trim();
    if !ca_bundle.is_empty() {
        let raw = fs::read(Path::new(ca_bundle))
            .map_err(|err| dial_error(None, format!("read ca bundle: {err}")))?;
        let certs = CertificateDer::pem_slice_iter(&raw)
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let (valid_count, _) = root_store.add_parsable_certificates(certs);
        if valid_count == 0 {
            return Err(dial_error(None, format!("invalid ca bundle: {ca_bundle}")));
        }
    }
    Ok(ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth())
}

fn write_handshake_request(
    stream: &mut Box<dyn ReadWrite>,
    parsed: &ParsedWsUrl,
    bearer_token: &str,
    key: &str,
) -> Result<(), WsDialError> {
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: awiki-cli-rs2\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\n",
        parsed.path_and_query,
        parsed.host_header(),
        key,
    )
    .map_err(|err| dial_error(None, err.to_string()))?;
    let bearer_token = bearer_token.trim();
    if !bearer_token.is_empty() {
        write!(stream, "Authorization: Bearer {bearer_token}\r\n")
            .map_err(|err| dial_error(None, err.to_string()))?;
    }
    stream
        .write_all(b"\r\n")
        .map_err(|err| dial_error(None, err.to_string()))?;
    stream
        .flush()
        .map_err(|err| dial_error(None, err.to_string()))
}

fn read_http_headers(stream: &mut Box<dyn ReadWrite>) -> Result<String, WsDialError> {
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while raw.len() < READ_HEADER_LIMIT {
        let read = stream
            .read(&mut byte)
            .map_err(|err| dial_error(None, err.to_string()))?;
        if read == 0 {
            break;
        }
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            return String::from_utf8(raw).map_err(|err| dial_error(None, err.to_string()));
        }
    }
    Err(dial_error(
        None,
        "websocket upgrade response missing HTTP header terminator",
    ))
}

fn validate_handshake_response(headers: &str, key: &str) -> Result<(), WsDialError> {
    let mut lines = headers.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| dial_error(None, "websocket upgrade response missing status line"))?;
    let status = parse_status_code(status_line)?;
    if status != 101 {
        return Err(dial_error(
            Some(status),
            format!("websocket upgrade failed with status {status}"),
        ));
    }
    let header_pairs = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<Vec<_>>();
    let accept = header_pairs
        .iter()
        .find(|(name, _)| name == "sec-websocket-accept")
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    let expected = websocket_accept(key);
    if accept != expected {
        return Err(dial_error(
            None,
            "websocket upgrade response has invalid Sec-WebSocket-Accept",
        ));
    }
    Ok(())
}

fn parse_status_code(status_line: &str) -> Result<u16, WsDialError> {
    let mut parts = status_line.split_whitespace();
    let _version = parts
        .next()
        .ok_or_else(|| dial_error(None, "websocket upgrade response missing HTTP version"))?;
    parts
        .next()
        .ok_or_else(|| dial_error(None, "websocket upgrade response missing status code"))?
        .parse::<u16>()
        .map_err(|err| dial_error(None, format!("invalid HTTP status code: {err}")))
}

fn websocket_key() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    BASE64_STANDARD.encode(bytes)
}

fn websocket_accept(key: &str) -> String {
    let mut raw = Vec::with_capacity(key.len() + WS_GUID.len());
    raw.extend_from_slice(key.as_bytes());
    raw.extend_from_slice(WS_GUID.as_bytes());
    BASE64_STANDARD.encode(sha1_digest(&raw))
}

fn sha1_digest(input: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (input.len() as u64) * 8;
    let mut data = input.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in data.chunks_exact(64) {
        let mut w = [0_u32; 80];
        for (idx, word) in w.iter_mut().take(16).enumerate() {
            let offset = idx * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for idx in 16..80 {
            w[idx] = (w[idx - 3] ^ w[idx - 8] ^ w[idx - 14] ^ w[idx - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (idx, word) in w.iter().enumerate() {
            let (f, k) = match idx {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0_u8; 20];
    out[..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn dial_error(status_code: Option<u16>, message: impl Into<String>) -> WsDialError {
    WsDialError {
        status_code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{sha1_digest, websocket_accept};
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

    #[test]
    fn sha1_digest_matches_known_websocket_accept_vector() {
        assert_eq!(
            BASE64_STANDARD.encode(sha1_digest(b"hello")),
            "qvTGHdzF6KLavt4PO0gs2a6pQ00="
        );
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
}
