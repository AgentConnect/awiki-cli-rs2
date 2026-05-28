#[cfg(feature = "blocking")]
use std::io::{Read, Write};
#[cfg(feature = "blocking")]
use std::net::TcpListener;
use std::path::PathBuf;
#[cfg(feature = "blocking")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "blocking")]
use std::time::Duration;

use im_core::prelude::*;
#[cfg(feature = "blocking")]
use im_core::realtime::{connect_realtime_with_transport, RealtimeAuthProvider, RealtimeTransport};
use im_core::realtime::{
    realtime_client_construction_plan, realtime_client_endpoints, simulate_realtime_connect,
    RealtimeConnectAction, RealtimeDialOutcome, RealtimeRefreshOutcome,
};

#[test]
fn realtime_connect_endpoints_match_legacy_listener_derivation() {
    let endpoints =
        realtime_client_endpoints("http://127.0.0.1:18080").expect("listener endpoints");

    assert_eq!(endpoints.request_url, "http://127.0.0.1:18080/im/ws");
    assert_eq!(endpoints.websocket_url, "ws://127.0.0.1:18080/im/ws");
    assert_eq!(
        endpoints.did_auth_url,
        "http://127.0.0.1:18080/user-service/did-auth/rpc"
    );
}

#[test]
fn realtime_connect_construction_plan_preserves_scope_side_effects() {
    let plan =
        realtime_client_construction_plan("http://127.0.0.1:18080/").expect("construction plan");

    assert_eq!(plan.endpoints.request_url, "http://127.0.0.1:18080/im/ws");
    assert_eq!(
        plan.remembered_scope_inputs,
        vec![
            "http://127.0.0.1:18080/".to_string(),
            "http://127.0.0.1:18080/user-service/did-auth/rpc".to_string(),
            "http://127.0.0.1:18080/im/ws".to_string(),
        ]
    );
}

#[test]
fn realtime_connect_simulation_refreshes_expired_bearer_once() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = simulate_realtime_connect(
        "expired-token",
        |token| {
            dialed.push(token.to_string());
            if token == "expired-token" {
                RealtimeDialOutcome::Failed {
                    status_code: Some(401),
                    error: "websocket dial failed".to_string(),
                    response_body: Some(
                        br#"{"jsonrpc":"2.0","error":{"code":1401,"message":"expired session"}}"#
                            .to_vec(),
                    ),
                }
            } else {
                RealtimeDialOutcome::Connected
            }
        },
        || {
            refreshes += 1;
            RealtimeRefreshOutcome::Refreshed {
                current_jwt: " refreshed-token ".to_string(),
            }
        },
    );

    assert_eq!(
        result.actions,
        vec![
            dial_action("expired-token"),
            RealtimeConnectAction::RefreshBearer,
            dial_action("refreshed-token"),
            RealtimeConnectAction::Attach,
        ]
    );
    assert_eq!(result.error, None);
    assert_eq!(dialed, vec!["expired-token", "refreshed-token"]);
    assert_eq!(refreshes, 1);
}

#[test]
fn realtime_connect_simulation_returns_first_non_401_dial_error_without_refresh() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = simulate_realtime_connect(
        "stale-token",
        |token| {
            dialed.push(token.to_string());
            RealtimeDialOutcome::Failed {
                status_code: Some(500),
                error: "websocket dial failed".to_string(),
                response_body: Some(b" upstream down\n".to_vec()),
            }
        },
        || {
            refreshes += 1;
            RealtimeRefreshOutcome::Refreshed {
                current_jwt: "must-not-use".to_string(),
            }
        },
    );

    assert_eq!(result.actions, vec![dial_action("stale-token")]);
    assert_eq!(
        result.error,
        Some("websocket dial failed: upstream down".to_string())
    );
    assert_eq!(dialed, vec!["stale-token"]);
    assert_eq!(refreshes, 0);
}

#[test]
#[cfg(feature = "blocking")]
fn realtime_connect_with_fake_transport_emits_initial_connection_events() {
    let endpoints = realtime_client_endpoints("https://example.test").unwrap();
    let mut transport = FakeRealtimeTransport::new(vec![RealtimeDialOutcome::Connected]);
    let mut auth = FakeRealtimeAuthProvider::new(vec![]);

    let handle =
        connect_realtime_with_transport(&endpoints, "live-token", &mut transport, &mut auth)
            .expect("connected");

    assert_eq!(
        transport.dialed,
        vec![(
            "wss://example.test/im/ws".to_string(),
            "live-token".to_string()
        )]
    );
    assert!(auth.refresh_urls.is_empty());
    assert_eq!(
        handle.events.into_iter().collect::<Vec<_>>(),
        vec![
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connecting,
                reason: None,
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                reason: None,
            }),
        ]
    );
    assert!(!handle.control.is_closed());
}

#[test]
#[cfg(feature = "blocking")]
fn realtime_connect_with_fake_transport_refreshes_after_401_and_retries() {
    let endpoints = realtime_client_endpoints("https://example.test").unwrap();
    let mut transport = FakeRealtimeTransport::new(vec![
        RealtimeDialOutcome::Failed {
            status_code: Some(401),
            error: "unauthorized".to_string(),
            response_body: None,
        },
        RealtimeDialOutcome::Connected,
    ]);
    let mut auth = FakeRealtimeAuthProvider::new(vec![RealtimeRefreshOutcome::Refreshed {
        current_jwt: "new-token".to_string(),
    }]);

    let handle =
        connect_realtime_with_transport(&endpoints, "old-token", &mut transport, &mut auth)
            .expect("connected after refresh");

    assert_eq!(
        transport.dialed,
        vec![
            (
                "wss://example.test/im/ws".to_string(),
                "old-token".to_string()
            ),
            (
                "wss://example.test/im/ws".to_string(),
                "new-token".to_string()
            ),
        ]
    );
    assert_eq!(
        auth.refresh_urls,
        vec!["https://example.test/user-service/did-auth/rpc".to_string()]
    );
    let events = handle.events.into_iter().collect::<Vec<_>>();
    assert!(matches!(
        events.as_slice(),
        [
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connecting,
                reason: None,
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                reason: None,
            }),
        ]
    ));
}

#[test]
#[cfg(feature = "blocking")]
fn realtime_service_connect_starts_native_transport_worker_without_exposing_websocket() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock websocket listener");
    let addr = listener.local_addr().expect("listener addr");
    let request = Arc::new(Mutex::new(String::new()));
    let server_request = Arc::clone(&request);
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept websocket dial");
        let mut raw = Vec::new();
        let mut byte = [0_u8; 1];
        while raw.len() < 16 * 1024 {
            let read = stream.read(&mut byte).expect("read handshake byte");
            if read == 0 {
                break;
            }
            raw.push(byte[0]);
            if raw.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        *server_request.lock().expect("request lock") = String::from_utf8(raw).unwrap();
        stream
            .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n")
            .expect("write failure response");
    });

    let fixture = RuntimeFixture::new();
    fixture.write_ready_identity("alice", Some("live-token"));
    let client = fixture.client_with_service_base_url("alice", &format!("http://{addr}"));

    let handle = client
        .realtime()
        .connect(RealtimeOptions::default())
        .expect("connect should return event handle while worker owns native transport");

    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(2)),
        Ok(ImEvent::ConnectionStateChanged(ConnectionStateChanged {
            state: RealtimeConnectionState::Connecting,
            reason: None,
        }))
    ));
    let disconnected = handle
        .events
        .recv_timeout(Duration::from_secs(2))
        .expect("disconnected event after mock server rejects upgrade");
    assert!(matches!(
        disconnected,
        ImEvent::ConnectionStateChanged(ConnectionStateChanged {
            state: RealtimeConnectionState::Disconnected,
            reason: None,
        })
    ));
    handle.control.shutdown();
    server.join().expect("mock server thread");
    let request = request.lock().expect("request lock").clone();
    assert!(request.starts_with("GET /im/ws HTTP/1.1\r\n"));
    assert!(request.contains("Authorization: Bearer live-token\r\n"));
}

#[test]
#[cfg(feature = "blocking")]
fn realtime_service_connect_reads_native_websocket_notification_into_im_event() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock websocket listener");
    let addr = listener.local_addr().expect("listener addr");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept websocket dial");
        let request = read_handshake_request(&mut stream);
        let key = request
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("Sec-WebSocket-Key")
                    .then(|| value.trim().to_string())
            })
            .expect("websocket key header");
        let accept = websocket_accept(&key);
        write!(
            stream,
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        )
        .expect("write upgrade response");
        write_unmasked_text_frame(
            &mut stream,
            r#"{"method":"direct.incoming","params":{"meta":{"message_id":"msg-native-1","sender_did":"did:example:bob","target":{"did":"did:example:alice"},"content_type":"text/plain"},"body":{"text":"hello native ws"}}}"#,
        );
        stream.write_all(&[0x88, 0x00]).expect("write close frame");
    });

    let fixture = RuntimeFixture::new();
    fixture.write_ready_identity("alice", Some("live-token"));
    let client = fixture.client_with_service_base_url("alice", &format!("http://{addr}"));

    let handle = client
        .realtime()
        .connect(RealtimeOptions::default())
        .expect("native websocket session");

    let events = recv_events(&handle.events, 4);
    handle.control.shutdown();
    server.join().expect("mock websocket server");

    assert!(matches!(
        events.as_slice(),
        [
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connecting,
                ..
            }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                ..
            }),
            ImEvent::MessageReceived(MessageReceivedEvent { message, .. }),
            ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Closed,
                ..
            }),
        ] if message.id.as_str() == "msg-native-1"
            && message.body == (MessageBodyView::Text {
                text: "hello native ws".to_string(),
                kind: MessageKind::Text,
            })
    ));
}

#[tokio::test]
async fn realtime_service_start_async_requires_realtime_bearer_before_default_transport() {
    let fixture = RuntimeFixture::new();
    fixture.write_ready_identity("alice", None);
    let client = fixture.client("alice");

    let error = match client
        .realtime()
        .start_async(RealtimeOptions::default())
        .await
    {
        Ok(_) => panic!("missing auth should block transport dial"),
        Err(error) => error,
    };

    assert_eq!(error, ImError::AuthRequired);
}

#[cfg(feature = "blocking")]
fn recv_events(receiver: &std::sync::mpsc::Receiver<ImEvent>, count: usize) -> Vec<ImEvent> {
    (0..count)
        .map(|_| {
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("realtime event")
        })
        .collect()
}

#[cfg(feature = "blocking")]
fn read_handshake_request(stream: &mut std::net::TcpStream) -> String {
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while raw.len() < 16 * 1024 {
        let read = stream.read(&mut byte).expect("read handshake byte");
        if read == 0 {
            break;
        }
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(raw).expect("handshake utf8")
}

#[cfg(feature = "blocking")]
fn write_unmasked_text_frame(stream: &mut std::net::TcpStream, text: &str) {
    let bytes = text.as_bytes();
    if bytes.len() < 126 {
        stream
            .write_all(&[0x81, bytes.len() as u8])
            .expect("write frame header");
    } else {
        assert!(bytes.len() <= u16::MAX as usize, "test frame too large");
        stream.write_all(&[0x81, 126]).expect("write frame tag");
        stream
            .write_all(&(bytes.len() as u16).to_be_bytes())
            .expect("write frame len");
    }
    stream.write_all(bytes).expect("write frame payload");
}

#[cfg(feature = "blocking")]
fn websocket_accept(key: &str) -> String {
    const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    let mut raw = Vec::with_capacity(key.len() + WS_GUID.len());
    raw.extend_from_slice(key.as_bytes());
    raw.extend_from_slice(WS_GUID.as_bytes());
    BASE64_STANDARD.encode(sha1_digest(&raw))
}

#[cfg(feature = "blocking")]
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

fn dial_action(token: &str) -> RealtimeConnectAction {
    RealtimeConnectAction::DialBearer {
        token: token.to_string(),
        authorization: format!("Bearer {token}"),
    }
}

#[cfg(feature = "blocking")]
struct FakeRealtimeTransport {
    outcomes: Vec<RealtimeDialOutcome>,
    dialed: Vec<(String, String)>,
}

#[cfg(feature = "blocking")]
impl FakeRealtimeTransport {
    fn new(outcomes: Vec<RealtimeDialOutcome>) -> Self {
        Self {
            outcomes,
            dialed: Vec::new(),
        }
    }
}

#[cfg(feature = "blocking")]
impl RealtimeTransport for FakeRealtimeTransport {
    fn dial_bearer(&mut self, websocket_url: &str, bearer_token: &str) -> RealtimeDialOutcome {
        self.dialed
            .push((websocket_url.to_string(), bearer_token.to_string()));
        if self.outcomes.is_empty() {
            return RealtimeDialOutcome::Failed {
                status_code: None,
                error: "unexpected dial".to_string(),
                response_body: None,
            };
        }
        self.outcomes.remove(0)
    }
}

#[cfg(feature = "blocking")]
struct FakeRealtimeAuthProvider {
    outcomes: Vec<RealtimeRefreshOutcome>,
    refresh_urls: Vec<String>,
}

#[cfg(feature = "blocking")]
impl FakeRealtimeAuthProvider {
    fn new(outcomes: Vec<RealtimeRefreshOutcome>) -> Self {
        Self {
            outcomes,
            refresh_urls: Vec::new(),
        }
    }
}

#[cfg(feature = "blocking")]
impl RealtimeAuthProvider for FakeRealtimeAuthProvider {
    fn refresh_realtime_bearer(&mut self, did_auth_url: &str) -> ImResult<RealtimeRefreshOutcome> {
        self.refresh_urls.push(did_auth_url.to_string());
        if self.outcomes.is_empty() {
            return Err(ImError::AuthRequired);
        }
        Ok(self.outcomes.remove(0))
    }
}

struct RuntimeFixture {
    root: PathBuf,
}

impl RuntimeFixture {
    fn new() -> Self {
        let root = unique_temp_root();
        std::fs::create_dir_all(root.join("identities")).unwrap();
        Self { root }
    }

    fn write_ready_identity(&self, alias: &str, token: Option<&str>) {
        let identities = self.root.join("identities");
        std::fs::write(identities.join("default"), format!("{alias}\n")).unwrap();
        std::fs::write(
            identities.join("registry.json"),
            format!(
                r#"{{
                  "default_identity": "{alias}",
                  "identities": [{{
                    "id": "{alias}-id",
                    "did": "did:example:{alias}",
                    "handle": "{alias}.awiki.test",
                    "display_name": "{alias}",
                    "local_alias": "{alias}",
                    "is_default": true,
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }}]
                }}"#
            ),
        )
        .unwrap();
        let dir = identities.join(alias);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("did.json"), r#"{"id":"did:example:test"}"#).unwrap();
        std::fs::write(dir.join("private.key"), "test-private-key").unwrap();
        if let Some(token) = token {
            std::fs::write(
                dir.join("auth.json"),
                format!(r#"{{"jwt_token":"{token}"}}"#),
            )
            .unwrap();
        }
    }

    fn client(&self, alias: &str) -> ImClient {
        self.client_with_service_base_url(alias, "https://example.test")
    }

    fn client_with_service_base_url(&self, alias: &str, service_base_url: &str) -> ImClient {
        self.core()
            .with_service_base_url(service_base_url)
            .client(IdentitySelector::LocalAlias(alias.to_string()))
            .unwrap()
    }

    fn core(&self) -> RuntimeCoreBuilder<'_> {
        RuntimeCoreBuilder {
            root: &self.root,
            service_base_url: "https://example.test",
        }
    }
}

struct RuntimeCoreBuilder<'a> {
    root: &'a std::path::Path,
    service_base_url: &'a str,
}

impl<'a> RuntimeCoreBuilder<'a> {
    fn with_service_base_url(mut self, service_base_url: &'a str) -> Self {
        self.service_base_url = service_base_url;
        self
    }

    fn client(&self, selector: IdentitySelector) -> ImResult<ImClient> {
        self.build().client(selector)
    }

    fn build(&self) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(self.service_base_url).unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: MessageTransportPolicy::Auto,
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

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-realtime-connect-{}-{nanos}",
        std::process::id()
    ))
}
