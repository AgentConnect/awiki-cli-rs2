use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use awiki_im_core::prelude::*;
use serde_json::{json, Value};

#[cfg(not(feature = "blocking"))]
#[test]
fn directory_relationship_sync_methods_fail_closed_by_default() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");

    let follow = client.directory().follow(FollowRequest {
        peer: PeerRef::parse("bob.awiki.test", "").unwrap(),
    });
    assert!(matches!(
        follow,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-directory-follow"
    ));

    let unfollow = client.directory().unfollow(UnfollowRequest {
        peer: PeerRef::parse("did:example:bob", "").unwrap(),
    });
    assert!(matches!(
        unfollow,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-directory-unfollow"
    ));

    let status = client
        .directory()
        .relationship_status(PeerRef::parse("did:example:bob", "").unwrap());
    assert!(matches!(
        status,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-directory-relationship-status"
    ));

    let followers = client.directory().followers(RelationshipListQuery {
        limit: Some(PageLimit(2)),
        ..RelationshipListQuery::default()
    });
    assert!(matches!(
        followers,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-directory-followers"
    ));

    let following = client.directory().following(RelationshipListQuery {
        limit: Some(PageLimit(2)),
        ..RelationshipListQuery::default()
    });
    assert!(matches!(
        following,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-directory-following"
    ));
}

#[cfg(feature = "blocking")]
#[test]
fn directory_relationship_follow_persists_projection_and_status() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "follow",
            json!({ "target_did": "did:example:bob" }),
            json!({ "ok": true, "is_friend": true }),
        ),
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "get_status",
            json!({ "target_did": "did:example:bob" }),
            json!({
                "is_following": true,
                "is_follower": true,
                "is_friend": true,
                "is_blocked": false,
                "is_blocked_by": false,
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let result = client
        .directory()
        .follow(FollowRequest {
            peer: PeerRef::parse("bob.awiki.test", "").unwrap(),
        })
        .unwrap();
    assert_eq!(result.did.as_str(), "did:example:bob");
    assert!(result.is_friend);
    assert!(result.relation.is_following);
    assert!(result.relation.is_follower);
    assert!(result.relation.is_friend);
    assert!(result.relation.is_contact);
    assert_eq!(result.relation.relationship.as_deref(), Some("following"));
    assert!(result.warnings.is_empty());

    let requests = server.join();
    assert_eq!(requests.len(), 5);
    assert!(requests
        .iter()
        .all(|request| request.authorization.as_deref() == Some("Bearer test-token-for-alice")));

    let connection = rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite"))
        .expect("open im-core local state");
    let (followed, relationship, handle): (i64, String, String) = connection
        .query_row(
            "SELECT followed, relationship, handle FROM contacts WHERE owner_identity_id = ?1 AND did = ?2",
            ("alice-id", "did:example:bob"),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(followed, 1);
    assert_eq!(relationship, "following");
    assert_eq!(handle, "bob.awiki.test");

    let (event_type, status, target_handle): (String, String, String) = connection
        .query_row(
            "SELECT event_type, status, target_handle FROM relationship_events WHERE owner_identity_id = ?1 AND target_did = ?2",
            ("alice-id", "did:example:bob"),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(event_type, "followed");
    assert_eq!(status, "applied");
    assert_eq!(target_handle, "bob.awiki.test");
}

#[tokio::test]
async fn directory_relationship_follow_async_persists_projection_and_status() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "follow",
            json!({ "target_did": "did:example:bob" }),
            json!({ "ok": true, "is_friend": true }),
        ),
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "get_status",
            json!({ "target_did": "did:example:bob" }),
            json!({
                "is_following": true,
                "is_follower": true,
                "is_friend": true,
                "is_blocked": false,
                "is_blocked_by": false,
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let result = client
        .directory()
        .follow_async(FollowRequest {
            peer: PeerRef::parse("bob.awiki.test", "").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(result.did.as_str(), "did:example:bob");
    assert!(result.is_friend);
    assert!(result.relation.is_following);
    assert!(result.relation.is_follower);
    assert!(result.relation.is_friend);
    assert!(result.relation.is_contact);
    assert_eq!(result.relation.relationship.as_deref(), Some("following"));
    assert!(result.warnings.is_empty());

    let requests = server.join();
    assert_eq!(requests.len(), 5);
    assert!(requests
        .iter()
        .all(|request| request.authorization.as_deref() == Some("Bearer test-token-for-alice")));

    let connection = rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite"))
        .expect("open im-core local state");
    let (followed, relationship, handle): (i64, String, String) = connection
        .query_row(
            "SELECT followed, relationship, handle FROM contacts WHERE owner_identity_id = ?1 AND did = ?2",
            ("alice-id", "did:example:bob"),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(followed, 1);
    assert_eq!(relationship, "following");
    assert_eq!(handle, "bob.awiki.test");

    let (event_type, status, target_handle): (String, String, String) = connection
        .query_row(
            "SELECT event_type, status, target_handle FROM relationship_events WHERE owner_identity_id = ?1 AND target_did = ?2",
            ("alice-id", "did:example:bob"),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(event_type, "followed");
    assert_eq!(status, "applied");
    assert_eq!(target_handle, "bob.awiki.test");
}

#[tokio::test]
async fn directory_relationship_unfollow_async_did_updates_projection() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "unfollow",
            json!({ "target_did": "did:example:bob" }),
            json!({ "ok": true }),
        ),
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "get_status",
            json!({ "target_did": "did:example:bob" }),
            json!({
                "is_following": false,
                "is_follower": false,
                "is_friend": false,
                "is_blocked": false,
                "is_blocked_by": false,
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let result = client
        .directory()
        .unfollow_async(UnfollowRequest {
            peer: PeerRef::parse("did:example:bob", "").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(result.did.as_str(), "did:example:bob");
    assert!(result.ok);
    assert!(!result.relation.is_following);
    assert!(result.relation.is_contact);
    assert_eq!(result.relation.relationship.as_deref(), Some("none"));
    assert!(result.warnings.is_empty());

    let requests = server.join();
    assert_eq!(requests.len(), 2);

    let connection = rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite"))
        .expect("open im-core local state");
    let (followed, relationship): (i64, String) = connection
        .query_row(
            "SELECT followed, relationship FROM contacts WHERE owner_identity_id = ?1 AND did = ?2",
            ("alice-id", "did:example:bob"),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(followed, 0);
    assert_eq!(relationship, "none");

    let event_type: String = connection
        .query_row(
            "SELECT event_type FROM relationship_events WHERE owner_identity_id = ?1 AND target_did = ?2",
            ("alice-id", "did:example:bob"),
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_type, "unfollowed");
}

#[cfg(feature = "blocking")]
#[test]
fn directory_relationship_lists_hide_service_user_ids() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "get_followers",
            json!({ "limit": 2, "offset": 4 }),
            json!({
                "items": [{
                    "from_did": "did:example:carol",
                    "to_did": "did:example:alice",
                    "from_user_id": "internal-carol",
                    "to_user_id": "internal-alice",
                    "created_at": "2026-05-22T00:00:00Z"
                }]
            }),
        ),
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "get_following",
            json!({ "limit": 3, "offset": 0 }),
            json!({
                "items": [{
                    "from_did": "did:example:alice",
                    "to_did": "did:example:dave",
                    "from_user_id": "internal-alice",
                    "to_user_id": "internal-dave",
                    "created_at": "2026-05-22T01:00:00Z"
                }]
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let followers = client
        .directory()
        .followers(RelationshipListQuery {
            limit: Some(PageLimit(2)),
            offset: Some(4),
            hydrate_profiles: false,
        })
        .unwrap();
    assert_eq!(followers.items.len(), 1);
    assert_eq!(
        followers.items[0].did.as_ref().unwrap().as_str(),
        "did:example:carol"
    );
    assert_eq!(
        followers.items[0].created_at.as_deref(),
        Some("2026-05-22T00:00:00Z")
    );

    let following = client
        .directory()
        .following(RelationshipListQuery {
            limit: Some(PageLimit(3)),
            offset: None,
            hydrate_profiles: false,
        })
        .unwrap();
    assert_eq!(following.items.len(), 1);
    assert_eq!(
        following.items[0].did.as_ref().unwrap().as_str(),
        "did:example:dave"
    );
    assert_eq!(
        following.items[0].created_at.as_deref(),
        Some("2026-05-22T01:00:00Z")
    );

    let output = serde_json::to_value(&following.items[0]).unwrap();
    assert!(output.get("from_user_id").is_none());
    assert!(output.get("to_user_id").is_none());

    let requests = server.join();
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn directory_relationship_async_lists_hide_service_user_ids() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "get_followers",
            json!({ "limit": 2, "offset": 4 }),
            json!({
                "items": [{
                    "from_did": "did:example:carol",
                    "to_did": "did:example:alice",
                    "from_user_id": "internal-carol",
                    "to_user_id": "internal-alice",
                    "created_at": "2026-05-22T00:00:00Z"
                }]
            }),
        ),
        ExpectedRpc::new(
            "/user-service/did/relationships/rpc",
            "get_following",
            json!({ "limit": 3, "offset": 0 }),
            json!({
                "items": [{
                    "from_did": "did:example:alice",
                    "to_did": "did:example:dave",
                    "from_user_id": "internal-alice",
                    "to_user_id": "internal-dave",
                    "created_at": "2026-05-22T01:00:00Z"
                }]
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let followers = client
        .directory()
        .followers_async(RelationshipListQuery {
            limit: Some(PageLimit(2)),
            offset: Some(4),
            hydrate_profiles: false,
        })
        .await
        .unwrap();
    assert_eq!(followers.items.len(), 1);
    assert_eq!(
        followers.items[0].did.as_ref().unwrap().as_str(),
        "did:example:carol"
    );

    let following = client
        .directory()
        .following_async(RelationshipListQuery {
            limit: Some(PageLimit(3)),
            offset: None,
            hydrate_profiles: false,
        })
        .await
        .unwrap();
    assert_eq!(following.items.len(), 1);
    assert_eq!(
        following.items[0].did.as_ref().unwrap().as_str(),
        "did:example:dave"
    );
    let output = serde_json::to_value(&following.items[0]).unwrap();
    assert!(output.get("from_user_id").is_none());
    assert!(output.get("to_user_id").is_none());

    let requests = server.join();
    assert_eq!(requests.len(), 2);
}

#[test]
fn directory_relationship_api_validates_inputs_before_transport() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");

    let self_follow = client.directory().follow(FollowRequest {
        peer: PeerRef::parse("did:example:alice", "").unwrap(),
    });
    assert!(matches!(
        self_follow,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "peer"
    ));

    let self_unfollow = client.directory().unfollow(UnfollowRequest {
        peer: PeerRef::parse("did:example:alice", "").unwrap(),
    });
    assert!(matches!(
        self_unfollow,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "peer"
    ));

    let followers = client.directory().followers(RelationshipListQuery {
        limit: Some(PageLimit(0)),
        ..RelationshipListQuery::default()
    });
    assert!(matches!(
        followers,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "limit"
    ));
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(&identities).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "display_name": "Alice",
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
              }]
            }"#,
        )
        .unwrap();
        write_identity_runtime(&identities, "alice");
        Self { root }
    }

    fn client(&self, alias: &str) -> ImClient {
        self.core()
            .client(IdentitySelector::LocalAlias(alias.to_string()))
            .unwrap()
    }

    fn client_with_base_url(&self, alias: &str, base_url: &str) -> ImClient {
        self.core_with_base_url(base_url)
            .client(IdentitySelector::LocalAlias(alias.to_string()))
            .unwrap()
    }

    fn core(&self) -> ImCore {
        self.core_with_base_url("https://example.test")
    }

    fn core_with_base_url(&self, base_url: &str) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
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

fn write_identity_runtime(identities: &std::path::Path, alias: &str) {
    let identity_dir = identities.join(alias);
    fs::create_dir_all(&identity_dir).unwrap();
    let bundle = anp::authentication::create_did_wba_document(
        "awiki.test",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["user".to_string()],
            domain: Some("awiki.test".to_string()),
            challenge: Some(format!("phase2-relationship-{alias}")),
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
        format!(r#"{{"jwt_token":"test-token-for-{alias}"}}"#),
    )
    .unwrap();
}

fn handle_lookup_value() -> Value {
    json!({
        "handle": "bob",
        "full_handle": "bob.awiki.test",
        "did": "did:example:bob",
        "user_id": "user-bob",
        "domain": "awiki.test",
        "status": "active",
    })
}

fn public_profile_value() -> Value {
    json!({
        "did": "did:example:bob",
        "handle": "bob.awiki.test",
        "nick_name": "Bob",
        "bio": "Directory profile",
        "tags": ["directory"],
    })
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-phase2-relationship-{}-{nanos}",
        std::process::id()
    ))
}

struct RpcTestServer {
    base_url: String,
    handle: thread::JoinHandle<Vec<CapturedRpc>>,
}

impl RpcTestServer {
    fn spawn(expected: Vec<ExpectedRpc>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut captured = Vec::new();
            for expected in expected {
                let mut stream = accept_before_deadline(&listener, deadline);
                let request = read_rpc_request(&mut stream);
                assert_eq!(request.path, expected.path);
                assert_eq!(request.rpc_method, expected.rpc_method);
                assert_eq!(request.params, expected.params);
                write_rpc_response(&mut stream, request.id.clone(), expected.result);
                captured.push(request);
            }
            captured
        });
        Self { base_url, handle }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn join(self) -> Vec<CapturedRpc> {
        self.handle.join().unwrap()
    }
}

struct ExpectedRpc {
    path: String,
    rpc_method: String,
    params: Value,
    result: Value,
}

impl ExpectedRpc {
    fn new(path: &str, rpc_method: &str, params: Value, result: Value) -> Self {
        Self {
            path: path.to_string(),
            rpc_method: rpc_method.to_string(),
            params,
            result,
        }
    }
}

#[derive(Debug)]
struct CapturedRpc {
    path: String,
    rpc_method: String,
    params: Value,
    id: Value,
    authorization: Option<String>,
}

fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("set test stream blocking");
                return stream;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for RPC request"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("accept RPC request: {err}"),
        }
    }
}

fn read_rpc_request(stream: &mut TcpStream) -> CapturedRpc {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "RPC request closed before headers");
        raw.extend_from_slice(&buffer[..count]);
        if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
    };
    let headers_text = std::str::from_utf8(&raw[..header_end]).unwrap();
    let mut lines = headers_text.lines();
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    assert_eq!(request_parts.next(), Some("POST"));
    let path = request_parts.next().unwrap().to_string();
    let headers = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while raw.len() < body_start + content_length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "RPC request closed before body");
        raw.extend_from_slice(&buffer[..count]);
    }
    let body = &raw[body_start..body_start + content_length];
    let payload: Value = serde_json::from_slice(body).unwrap();
    CapturedRpc {
        path,
        rpc_method: payload["method"].as_str().unwrap().to_string(),
        params: payload["params"].clone(),
        id: payload["id"].clone(),
        authorization: headers.get("authorization").cloned(),
    }
}

fn write_rpc_response(stream: &mut TcpStream, id: Value, result: Value) {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
    .to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}
