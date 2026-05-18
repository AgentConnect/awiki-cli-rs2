use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rand::RngCore;
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const READ_HEADER_LIMIT: usize = 16 * 1024;
const MAX_FRAME_SIZE: u64 = 16 * 1024 * 1024;
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(90);
const PING_TIMEOUT: Duration = Duration::from_secs(15);

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
    default_read_timeout: Option<Duration>,
    ping_counter: i32,
}

trait ReadWrite: Read + Write + Send {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()>;

    #[cfg(test)]
    fn timeout_events(&self) -> Vec<Option<Duration>> {
        Vec::new()
    }

    #[cfg(test)]
    fn written_bytes(&self) -> Vec<u8> {
        Vec::new()
    }
}

impl ReadWrite for TcpStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }
}

impl ReadWrite for StreamOwned<ClientConnection, TcpStream> {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.sock.set_read_timeout(timeout)
    }
}

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
        Ok(Self {
            stream,
            default_read_timeout: Some(DEFAULT_READ_TIMEOUT),
            ping_counter: 0,
        })
    }

    pub fn send_json(&mut self, payload: &Map<String, Value>) -> anyhow::Result<()> {
        let raw = serde_json::to_vec(payload)?;
        self.write_frame(0x1, &raw)
    }

    pub fn ping(&mut self) -> anyhow::Result<()> {
        self.ping_with_timeout(PING_TIMEOUT)
    }

    pub fn ping_with_timeout(&mut self, timeout: Duration) -> anyhow::Result<()> {
        self.ping_counter = self.ping_counter.wrapping_add(1);
        let payload = self.ping_counter.to_string().into_bytes();
        self.write_frame(0x9, &payload)?;
        let result = self.wait_for_pong(&payload, timeout);
        let restore_result = self.stream.set_read_timeout(self.default_read_timeout);
        match (result, restore_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(err), Ok(())) if is_timeout_error(&err) => {
                anyhow::bail!("websocket pong timed out after {timeout:?}")
            }
            (Err(err), Ok(())) => Err(err),
            (Ok(()), Err(restore_err)) => Err(restore_err.into()),
            (Err(err), Err(restore_err)) if is_timeout_error(&err) => Err(restore_err.into()),
            (Err(err), Err(_)) => Err(err),
        }
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
                WsFrame::Pong(_) => {}
                WsFrame::Close => anyhow::bail!("websocket notification loop closed"),
            }
        }
    }

    pub fn read_json_message_timeout(
        &mut self,
        timeout: Duration,
    ) -> anyhow::Result<Option<Map<String, Value>>> {
        self.stream.set_read_timeout(Some(timeout))?;
        let result = self.read_json_message();
        let restore_result = self.stream.set_read_timeout(self.default_read_timeout);
        match (result, restore_result) {
            (Ok(message), Ok(())) => Ok(Some(message)),
            (Err(err), Ok(())) if is_timeout_error(&err) => Ok(None),
            (Err(err), Ok(())) => Err(err),
            (Ok(_), Err(restore_err)) => Err(restore_err.into()),
            (Err(err), Err(restore_err)) if is_timeout_error(&err) => Err(restore_err.into()),
            (Err(err), Err(_)) => Err(err),
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

    fn wait_for_pong(&mut self, expected_payload: &[u8], timeout: Duration) -> anyhow::Result<()> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| anyhow::anyhow!("websocket ping timeout is too large: {timeout:?}"))?;
        let mut deferred_frames = VecDeque::new();
        let result = loop {
            let read_timeout = match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) if !remaining.is_zero() => remaining,
                _ => break Err(timeout_error("websocket pong timed out")),
            };
            if let Err(err) = self.stream.set_read_timeout(Some(read_timeout)) {
                break Err(err.into());
            }
            match self.read_frame() {
                Ok(WsFrame::Ping(payload)) => {
                    if let Err(err) = self.write_frame(0xA, &payload) {
                        break Err(err);
                    }
                }
                Ok(WsFrame::Pong(payload)) if payload == expected_payload => break Ok(()),
                Ok(WsFrame::Pong(_)) => {}
                Ok(frame @ (WsFrame::Text(_) | WsFrame::Binary(_))) => {
                    deferred_frames.push_back(frame);
                }
                Ok(WsFrame::Close) => {
                    break Err(anyhow::anyhow!("websocket notification loop closed"));
                }
                Err(err) => break Err(err),
            }
        };
        let defer_result = self.defer_frames(deferred_frames);
        match (result, defer_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(err), Ok(())) => Err(err),
            (Ok(()), Err(err)) => Err(err),
            (Err(err), Err(_)) => Err(err),
        }
    }

    fn defer_frames(&mut self, frames: VecDeque<WsFrame>) -> anyhow::Result<()> {
        if frames.is_empty() {
            return Ok(());
        }
        let mut prefix = Vec::new();
        for frame in frames {
            append_unmasked_frame(&mut prefix, frame)?;
        }
        let inner = std::mem::replace(&mut self.stream, Box::new(EmptyReadWrite));
        self.stream = Box::new(PrefixedReadWrite {
            prefix: VecDeque::from(prefix),
            inner,
        });
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
            0xA => Ok(WsFrame::Pong(payload)),
            _ => anyhow::bail!("unsupported websocket frame opcode: {opcode}"),
        }
    }
}

struct PrefixedReadWrite {
    prefix: VecDeque<u8>,
    inner: Box<dyn ReadWrite>,
}

impl Read for PrefixedReadWrite {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.prefix.is_empty() {
            return self.inner.read(buf);
        }
        let mut count = 0;
        while count < buf.len() {
            let Some(byte) = self.prefix.pop_front() else {
                break;
            };
            buf[count] = byte;
            count += 1;
        }
        Ok(count)
    }
}

impl Write for PrefixedReadWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl ReadWrite for PrefixedReadWrite {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.inner.set_read_timeout(timeout)
    }

    #[cfg(test)]
    fn timeout_events(&self) -> Vec<Option<Duration>> {
        self.inner.timeout_events()
    }

    #[cfg(test)]
    fn written_bytes(&self) -> Vec<u8> {
        self.inner.written_bytes()
    }
}

struct EmptyReadWrite;

impl Read for EmptyReadWrite {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl Write for EmptyReadWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl ReadWrite for EmptyReadWrite {
    fn set_read_timeout(&mut self, _timeout: Option<Duration>) -> std::io::Result<()> {
        Ok(())
    }
}

enum WsFrame {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

fn decode_json_object(raw: &[u8]) -> anyhow::Result<Map<String, Value>> {
    match serde_json::from_slice::<Value>(raw)? {
        Value::Object(map) => Ok(map),
        _ => anyhow::bail!("websocket JSON message must be an object"),
    }
}

fn is_timeout_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<std::io::Error>()
        .map(|err| matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
        .unwrap_or(false)
}

fn timeout_error(message: &'static str) -> anyhow::Error {
    std::io::Error::new(ErrorKind::TimedOut, message).into()
}

fn append_unmasked_frame(buffer: &mut Vec<u8>, frame: WsFrame) -> anyhow::Result<()> {
    match frame {
        WsFrame::Text(raw) => append_unmasked_frame_parts(buffer, 0x1, raw.as_bytes()),
        WsFrame::Binary(raw) => append_unmasked_frame_parts(buffer, 0x2, &raw),
        WsFrame::Ping(_) | WsFrame::Pong(_) | WsFrame::Close => {
            anyhow::bail!("cannot defer websocket control frame")
        }
    }
    Ok(())
}

fn append_unmasked_frame_parts(buffer: &mut Vec<u8>, opcode: u8, payload: &[u8]) {
    buffer.push(0x80 | (opcode & 0x0F));
    if payload.len() < 126 {
        buffer.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        buffer.push(126);
        buffer.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        buffer.push(127);
        buffer.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    buffer.extend_from_slice(payload);
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
    use super::{sha1_digest, websocket_accept, ReadWrite, WsTransport};
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use std::collections::VecDeque;
    use std::io::{Error, ErrorKind, Read, Write};
    use std::time::Duration;

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

    #[test]
    fn timeout_read_returns_none_and_restores_default_timeout() {
        let stream = ScriptedStream {
            read_error: Some(ErrorKind::WouldBlock),
            ..ScriptedStream::default()
        };
        let mut transport = test_transport(stream, Some(Duration::from_secs(90)));

        let result = transport
            .read_json_message_timeout(Duration::from_millis(5))
            .expect("timeout poll");

        assert_eq!(result, None);
        assert_eq!(
            transport.stream.timeout_events(),
            vec![
                Some(Duration::from_millis(5)),
                Some(Duration::from_secs(90))
            ]
        );
    }

    #[test]
    fn timeout_read_decodes_text_message_and_restores_default_timeout() {
        let mut stream = ScriptedStream::default();
        stream.reads.push_back(server_text_frame(br#"{"ok":true}"#));
        let mut transport = test_transport(stream, Some(Duration::from_secs(90)));

        let result = transport
            .read_json_message_timeout(Duration::from_millis(5))
            .expect("timeout poll")
            .expect("message");

        assert_eq!(result["ok"], true);
        assert_eq!(
            transport.stream.timeout_events(),
            vec![
                Some(Duration::from_millis(5)),
                Some(Duration::from_secs(90))
            ]
        );
    }

    #[test]
    fn timeout_read_still_auto_pongs_ping_before_message() {
        let mut stream = ScriptedStream::default();
        stream.reads.push_back(server_ping_frame(b"hi"));
        stream
            .reads
            .push_back(server_text_frame(br#"{"after_ping":true}"#));
        let mut transport = test_transport(stream, Some(Duration::from_secs(90)));

        let result = transport
            .read_json_message_timeout(Duration::from_millis(5))
            .expect("timeout poll")
            .expect("message");

        assert_eq!(result["after_ping"], true);
        assert!(
            !transport.stream.written_bytes().is_empty(),
            "ping should cause an automatic pong frame write"
        );
        assert_eq!(transport.stream.written_bytes()[0] & 0x0F, 0xA);
    }

    #[test]
    fn ping_with_timeout_returns_timeout_and_restores_default_timeout() {
        let stream = ScriptedStream {
            read_error: Some(ErrorKind::TimedOut),
            ..ScriptedStream::default()
        };
        let mut transport = test_transport(stream, Some(Duration::from_secs(90)));

        let err = transport
            .ping_with_timeout(Duration::from_millis(50))
            .expect_err("ping should time out without a pong");

        assert_eq!(err.to_string(), "websocket pong timed out after 50ms");
        let timeout_events = transport.stream.timeout_events();
        assert_eq!(timeout_events.last(), Some(&Some(Duration::from_secs(90))));
        assert!(
            timeout_events
                .first()
                .and_then(|event| *event)
                .is_some_and(
                    |timeout| timeout <= Duration::from_millis(50) && timeout > Duration::ZERO
                ),
            "first timeout should be bounded by the requested ping timeout: {timeout_events:?}"
        );
        assert!(
            !transport.stream.written_bytes().is_empty(),
            "ping should write a websocket ping frame before waiting"
        );
        assert_eq!(
            client_frame(&transport.stream.written_bytes()),
            (0x9, b"1".to_vec())
        );
    }

    #[test]
    fn wait_for_pong_auto_pongs_and_preserves_deferred_message_frame() {
        let mut stream = ScriptedStream::default();
        stream.reads.push_back(server_ping_frame(b"server-ping"));
        stream
            .reads
            .push_back(server_text_frame(br#"{"queued":true}"#));
        stream.reads.push_back(server_pong_frame(b"expected"));
        let mut transport = test_transport(stream, Some(Duration::from_secs(90)));

        transport
            .wait_for_pong(b"expected", Duration::from_secs(1))
            .expect("wait for expected pong");

        assert!(
            !transport.stream.written_bytes().is_empty(),
            "server ping during ping wait should cause an automatic pong write"
        );
        assert_eq!(transport.stream.written_bytes()[0] & 0x0F, 0xA);

        let message = transport
            .read_json_message()
            .expect("deferred data frame should be readable after pong");
        assert_eq!(message["queued"], true);
    }

    fn test_transport(
        stream: ScriptedStream,
        default_read_timeout: Option<Duration>,
    ) -> WsTransport {
        WsTransport {
            stream: Box::new(stream),
            default_read_timeout,
            ping_counter: 0,
        }
    }

    fn server_text_frame(payload: &[u8]) -> Vec<u8> {
        server_frame(0x1, payload)
    }

    fn server_ping_frame(payload: &[u8]) -> Vec<u8> {
        server_frame(0x9, payload)
    }

    fn server_pong_frame(payload: &[u8]) -> Vec<u8> {
        server_frame(0xA, payload)
    }

    fn server_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.push(0x80 | (opcode & 0x0F));
        frame.push(payload.len() as u8);
        frame.extend_from_slice(payload);
        frame
    }

    fn client_frame(raw: &[u8]) -> (u8, Vec<u8>) {
        let opcode = raw[0] & 0x0F;
        let masked = raw[1] & 0x80 != 0;
        assert!(masked, "client websocket frames must be masked");
        let len = usize::from(raw[1] & 0x7F);
        assert!(len < 126, "test helper only decodes small frames");
        let mask = &raw[2..6];
        let mut payload = raw[6..6 + len].to_vec();
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
        (opcode, payload)
    }

    #[derive(Default)]
    struct ScriptedStream {
        reads: VecDeque<Vec<u8>>,
        current: VecDeque<u8>,
        writes: Vec<u8>,
        timeout_events: Vec<Option<Duration>>,
        read_error: Option<ErrorKind>,
    }

    impl Read for ScriptedStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if let Some(kind) = self.read_error {
                return Err(Error::new(kind, "scripted read timeout"));
            }
            if self.current.is_empty() {
                if let Some(next) = self.reads.pop_front() {
                    self.current = VecDeque::from(next);
                }
            }
            if self.current.is_empty() {
                return Ok(0);
            }
            let mut count = 0;
            while count < buf.len() {
                let Some(byte) = self.current.pop_front() else {
                    break;
                };
                buf[count] = byte;
                count += 1;
            }
            Ok(count)
        }
    }

    impl Write for ScriptedStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.writes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl ReadWrite for ScriptedStream {
        fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
            self.timeout_events.push(timeout);
            Ok(())
        }

        fn timeout_events(&self) -> Vec<Option<Duration>> {
            self.timeout_events.clone()
        }

        fn written_bytes(&self) -> Vec<u8> {
            self.writes.clone()
        }
    }
}
