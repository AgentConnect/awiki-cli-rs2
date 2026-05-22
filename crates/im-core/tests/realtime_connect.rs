use std::path::PathBuf;

use im_core::prelude::*;
use im_core::realtime::{
    connect_realtime_with_transport, realtime_client_construction_plan, realtime_client_endpoints,
    simulate_realtime_connect, RealtimeAuthProvider, RealtimeConnectAction, RealtimeDialOutcome,
    RealtimeRefreshOutcome, RealtimeTransport,
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
fn realtime_service_connect_uses_auth_before_default_unavailable_transport() {
    let fixture = RuntimeFixture::new();
    fixture.write_ready_identity("alice", Some("live-token"));
    let client = fixture.client("alice");

    let error = match client.realtime().connect(RealtimeOptions::default()) {
        Ok(_) => panic!("default transport should be unavailable in 5E tests"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ImError::TransportUnavailable { detail } if detail == "websocket transport is not configured for wss://example.test/im/ws"
    ));
}

#[test]
fn realtime_service_connect_requires_realtime_bearer_before_default_transport() {
    let fixture = RuntimeFixture::new();
    fixture.write_ready_identity("alice", None);
    let client = fixture.client("alice");

    let error = match client.realtime().connect(RealtimeOptions::default()) {
        Ok(_) => panic!("missing auth should block transport dial"),
        Err(error) => error,
    };

    assert_eq!(error, ImError::AuthRequired);
}

fn dial_action(token: &str) -> RealtimeConnectAction {
    RealtimeConnectAction::DialBearer {
        token: token.to_string(),
        authorization: format!("Bearer {token}"),
    }
}

struct FakeRealtimeTransport {
    outcomes: Vec<RealtimeDialOutcome>,
    dialed: Vec<(String, String)>,
}

impl FakeRealtimeTransport {
    fn new(outcomes: Vec<RealtimeDialOutcome>) -> Self {
        Self {
            outcomes,
            dialed: Vec::new(),
        }
    }
}

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

struct FakeRealtimeAuthProvider {
    outcomes: Vec<RealtimeRefreshOutcome>,
    refresh_urls: Vec<String>,
}

impl FakeRealtimeAuthProvider {
    fn new(outcomes: Vec<RealtimeRefreshOutcome>) -> Self {
        Self {
            outcomes,
            refresh_urls: Vec::new(),
        }
    }
}

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
        self.core()
            .client(IdentitySelector::LocalAlias(alias.to_string()))
            .unwrap()
    }

    fn core(&self) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
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
