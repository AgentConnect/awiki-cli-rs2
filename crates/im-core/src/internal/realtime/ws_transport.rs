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
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
const PING_TIMEOUT: Duration = Duration::from_secs(15);
const PING_CANCEL_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WsError {
    pub(crate) status_code: Option<u16>,
    pub(crate) message: String,
    timeout: bool,
    closed: bool,
}

impl WsError {
    pub(crate) fn timeout(message: impl Into<String>) -> Self {
        Self {
            status_code: None,
            message: message.into(),
            timeout: true,
            closed: false,
        }
    }

    pub(crate) fn closed(message: impl Into<String>) -> Self {
        Self {
            status_code: None,
            message: message.into(),
            timeout: false,
            closed: true,
        }
    }

    pub(crate) fn is_timeout(&self) -> bool {
        self.timeout
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }
}

impl fmt::Display for WsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for WsError {}

pub(crate) type WsResult<T> = Result<T, WsError>;

pub(crate) struct WsTransport {
    stream: Box<dyn ReadWrite>,
    default_read_timeout: Option<Duration>,
    ping_counter: i32,
}

trait ReadWrite: Read + Write + Send {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()>;
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
    pub(crate) fn connect_with_ca_bundle(
        websocket_url: &str,
        bearer_token: &str,
        ca_bundle: Option<&str>,
    ) -> WsResult<Self> {
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

    pub(crate) fn ping(&mut self) -> WsResult<()> {
        self.ping_with_timeout(PING_TIMEOUT)
    }

    pub(crate) fn ping_with_timeout(&mut self, timeout: Duration) -> WsResult<()> {
        self.ping_with_timeout_until(timeout, || false)
    }

    pub(crate) fn ping_with_timeout_until<F>(
        &mut self,
        timeout: Duration,
        should_cancel: F,
    ) -> WsResult<()>
    where
        F: Fn() -> bool,
    {
        self.ping_counter = self.ping_counter.wrapping_add(1);
        let payload = self.ping_counter.to_string().into_bytes();
        self.write_frame(0x9, &payload)?;
        let result = self.wait_for_pong_until(&payload, timeout, should_cancel);
        let restore_result = self
            .stream
            .set_read_timeout(self.default_read_timeout)
            .map_err(ws_error);
        match (result, restore_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(err), Ok(())) if err.is_timeout() => Err(WsError::timeout(format!(
                "websocket pong timed out after {timeout:?}"
            ))),
            (Err(err), Ok(())) => Err(err),
            (Ok(()), Err(restore_err)) => Err(restore_err),
            (Err(err), Err(restore_err)) if err.is_timeout() => Err(restore_err),
            (Err(err), Err(_)) => Err(err),
        }
    }

    pub(crate) fn read_json_message(&mut self) -> WsResult<Map<String, Value>> {
        loop {
            match self.read_frame()? {
                WsFrame::Text(raw) => return decode_json_object(raw.as_bytes()),
                WsFrame::Binary(raw) => return decode_json_object(&raw),
                WsFrame::Ping(payload) => {
                    self.write_frame(0xA, &payload)?;
                }
                WsFrame::Pong(_) => {}
                WsFrame::Close => {
                    return Err(WsError::closed("websocket notification loop closed"))
                }
            }
        }
    }

    pub(crate) fn read_json_message_timeout(
        &mut self,
        timeout: Duration,
    ) -> WsResult<Option<Map<String, Value>>> {
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(ws_error)?;
        let result = self.read_json_message();
        let restore_result = self
            .stream
            .set_read_timeout(self.default_read_timeout)
            .map_err(ws_error);
        match (result, restore_result) {
            (Ok(message), Ok(())) => Ok(Some(message)),
            (Err(err), Ok(())) if err.is_timeout() => Ok(None),
            (Err(err), Ok(())) => Err(err),
            (Ok(_), Err(restore_err)) => Err(restore_err),
            (Err(err), Err(restore_err)) if err.is_timeout() => Err(restore_err),
            (Err(err), Err(_)) => Err(err),
        }
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> WsResult<()> {
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
        self.stream.write_all(&header).map_err(ws_error)?;
        for (idx, byte) in payload.iter().enumerate() {
            self.stream
                .write_all(&[*byte ^ mask[idx % 4]])
                .map_err(ws_error)?;
        }
        self.stream.flush().map_err(ws_error)?;
        Ok(())
    }

    fn wait_for_pong_until<F>(
        &mut self,
        expected_payload: &[u8],
        timeout: Duration,
        should_cancel: F,
    ) -> WsResult<()>
    where
        F: Fn() -> bool,
    {
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            WsError::timeout(format!("websocket ping timeout is too large: {timeout:?}"))
        })?;
        let mut deferred_frames = VecDeque::new();
        let result = loop {
            if should_cancel() {
                break Err(ws_message("context canceled"));
            }
            let remaining = match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) if !remaining.is_zero() => remaining,
                _ => break Err(WsError::timeout("websocket pong timed out")),
            };
            let read_timeout = bounded_ping_read_timeout(remaining);
            if let Err(err) = self.stream.set_read_timeout(Some(read_timeout)) {
                break Err(ws_error(err));
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
                    break Err(WsError::closed("websocket notification loop closed"));
                }
                Err(err) if err.is_timeout() && Instant::now() < deadline => {}
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

    fn defer_frames(&mut self, frames: VecDeque<WsFrame>) -> WsResult<()> {
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

    fn read_frame(&mut self) -> WsResult<WsFrame> {
        let mut head = [0_u8; 2];
        self.stream.read_exact(&mut head).map_err(ws_error)?;
        let opcode = head[0] & 0x0F;
        let masked = head[1] & 0x80 != 0;
        let mut len = u64::from(head[1] & 0x7F);
        if len == 126 {
            let mut bytes = [0_u8; 2];
            self.stream.read_exact(&mut bytes).map_err(ws_error)?;
            len = u64::from(u16::from_be_bytes(bytes));
        } else if len == 127 {
            let mut bytes = [0_u8; 8];
            self.stream.read_exact(&mut bytes).map_err(ws_error)?;
            len = u64::from_be_bytes(bytes);
        }
        if len > MAX_FRAME_SIZE {
            return Err(ws_message(format!("websocket frame is too large: {len}")));
        }
        let mut mask = [0_u8; 4];
        if masked {
            self.stream.read_exact(&mut mask).map_err(ws_error)?;
        }
        let mut payload = vec![0_u8; len as usize];
        self.stream.read_exact(&mut payload).map_err(ws_error)?;
        if masked {
            for (idx, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[idx % 4];
            }
        }
        match opcode {
            0x1 => String::from_utf8(payload)
                .map(WsFrame::Text)
                .map_err(|err| ws_message(err.to_string())),
            0x2 => Ok(WsFrame::Binary(payload)),
            0x8 => Ok(WsFrame::Close),
            0x9 => Ok(WsFrame::Ping(payload)),
            0xA => Ok(WsFrame::Pong(payload)),
            _ => Err(ws_message(format!(
                "unsupported websocket frame opcode: {opcode}"
            ))),
        }
    }
}

fn bounded_ping_read_timeout(remaining: Duration) -> Duration {
    if remaining > PING_CANCEL_POLL {
        PING_CANCEL_POLL
    } else {
        remaining
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

fn decode_json_object(raw: &[u8]) -> WsResult<Map<String, Value>> {
    match serde_json::from_slice::<Value>(raw).map_err(|err| ws_message(err.to_string()))? {
        Value::Object(map) => Ok(map),
        _ => Err(ws_message("websocket JSON message must be an object")),
    }
}

fn append_unmasked_frame(buffer: &mut Vec<u8>, frame: WsFrame) -> WsResult<()> {
    match frame {
        WsFrame::Text(raw) => append_unmasked_frame_parts(buffer, 0x1, raw.as_bytes()),
        WsFrame::Binary(raw) => append_unmasked_frame_parts(buffer, 0x2, &raw),
        WsFrame::Ping(_) | WsFrame::Pong(_) | WsFrame::Close => {
            return Err(ws_message("cannot defer websocket control frame"));
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
    fn parse(raw: &str) -> WsResult<Self> {
        let (scheme, rest) = raw
            .split_once("://")
            .ok_or_else(|| ws_message(format!("websocket URL missing scheme: {raw}")))?;
        let scheme = scheme.trim().to_ascii_lowercase();
        if scheme != "ws" && scheme != "wss" {
            return Err(ws_message(format!(
                "unsupported websocket URL scheme: {scheme}"
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

fn split_host_port(authority: &str, scheme: &str) -> WsResult<(String, u16)> {
    let authority = authority.trim();
    if authority.is_empty() {
        return Err(ws_message("websocket URL missing host"));
    }
    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| ws_message(format!("invalid bracketed host: {authority}")))?;
        let host = authority[1..end].to_string();
        let port = if let Some(raw) = authority[end + 1..].strip_prefix(':') {
            raw.parse::<u16>()
                .map_err(|err| ws_message(format!("invalid websocket URL port: {err}")))?
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
                .map_err(|err| ws_message(format!("invalid websocket URL port: {err}")))?;
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

fn connect_stream(parsed: &ParsedWsUrl, ca_bundle: Option<&str>) -> WsResult<Box<dyn ReadWrite>> {
    let addrs = (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(ws_error)?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(DEFAULT_READ_TIMEOUT))
                    .map_err(ws_error)?;
                stream
                    .set_write_timeout(Some(WRITE_TIMEOUT))
                    .map_err(ws_error)?;
                if parsed.scheme == "wss" {
                    let config = Arc::new(rustls_config(ca_bundle)?);
                    let server_name = ServerName::try_from(parsed.host.clone())
                        .map_err(|err| ws_message(format!("invalid TLS server name: {err}")))?;
                    let conn = ClientConnection::new(config, server_name)
                        .map_err(|err| ws_message(err.to_string()))?;
                    return Ok(Box::new(StreamOwned::new(conn, stream)));
                }
                return Ok(Box::new(stream));
            }
            Err(err) => last_error = Some(err),
        }
    }
    Err(ws_message(
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "no resolved websocket socket addresses".to_string()),
    ))
}

fn rustls_config(ca_bundle: Option<&str>) -> WsResult<ClientConfig> {
    let mut root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(ca_bundle) = ca_bundle.map(str::trim).filter(|value| !value.is_empty()) {
        let raw = fs::read(Path::new(ca_bundle))
            .map_err(|err| ws_message(format!("read ca bundle: {err}")))?;
        let certs = CertificateDer::pem_slice_iter(&raw)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| ws_message(format!("parse ca bundle: {err}")))?;
        let (valid_count, _) = root_store.add_parsable_certificates(certs);
        if valid_count == 0 {
            return Err(ws_message(format!("invalid ca bundle: {ca_bundle}")));
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
) -> WsResult<()> {
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: awiki-im-core\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\n",
        parsed.path_and_query,
        parsed.host_header(),
        key,
    )
    .map_err(ws_error)?;
    let bearer_token = bearer_token.trim();
    if !bearer_token.is_empty() {
        write!(stream, "Authorization: Bearer {bearer_token}\r\n").map_err(ws_error)?;
    }
    stream.write_all(b"\r\n").map_err(ws_error)?;
    stream.flush().map_err(ws_error)
}

fn read_http_headers(stream: &mut Box<dyn ReadWrite>) -> WsResult<String> {
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while raw.len() < READ_HEADER_LIMIT {
        let read = stream.read(&mut byte).map_err(ws_error)?;
        if read == 0 {
            break;
        }
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            return String::from_utf8(raw).map_err(|err| ws_message(err.to_string()));
        }
    }
    Err(ws_message(
        "websocket upgrade response missing HTTP header terminator",
    ))
}

fn validate_handshake_response(headers: &str, key: &str) -> WsResult<()> {
    let mut lines = headers.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| ws_message("websocket upgrade response missing status line"))?;
    let status = parse_status_code(status_line)?;
    if status != 101 {
        return Err(ws_status(
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
        return Err(ws_message(
            "websocket upgrade response has invalid Sec-WebSocket-Accept",
        ));
    }
    Ok(())
}

fn parse_status_code(status_line: &str) -> WsResult<u16> {
    let mut parts = status_line.split_whitespace();
    let _version = parts
        .next()
        .ok_or_else(|| ws_message("websocket upgrade response missing HTTP version"))?;
    parts
        .next()
        .ok_or_else(|| ws_message("websocket upgrade response missing status code"))?
        .parse::<u16>()
        .map_err(|err| ws_message(format!("invalid HTTP status code: {err}")))
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

fn ws_status(status_code: Option<u16>, message: impl Into<String>) -> WsError {
    WsError {
        status_code,
        message: message.into(),
        timeout: false,
        closed: false,
    }
}

fn ws_message(message: impl Into<String>) -> WsError {
    ws_status(None, message)
}

fn ws_error(err: std::io::Error) -> WsError {
    let timeout = matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut);
    WsError {
        status_code: None,
        message: err.to_string(),
        timeout,
        closed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{websocket_accept, ParsedWsUrl};

    #[test]
    fn websocket_accept_matches_rfc_vector() {
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn parsed_ws_url_handles_default_and_explicit_ports() {
        let parsed = ParsedWsUrl::parse("wss://example.test/im/ws?x=1").unwrap();
        assert_eq!(parsed.scheme, "wss");
        assert_eq!(parsed.host, "example.test");
        assert_eq!(parsed.port, 443);
        assert_eq!(parsed.path_and_query, "/im/ws?x=1");
        assert_eq!(parsed.host_header(), "example.test");

        let parsed = ParsedWsUrl::parse("ws://127.0.0.1:18080").unwrap();
        assert_eq!(parsed.scheme, "ws");
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 18080);
        assert_eq!(parsed.path_and_query, "/");
        assert_eq!(parsed.host_header(), "127.0.0.1:18080");
    }
}
