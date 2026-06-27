use super::*;
use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

#[tokio::test]
async fn sync_delta_reads_checkpoint_calls_wire_and_advances_checkpoint() {
    let fixture = Fixture::new("sync-delta-basic");
    let client = fixture.client();
    fixture.store_checkpoint("4");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![delta_page(
                vec![message_created_event("sev-5", "5", "msg-delta-5", 5)],
                "5",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest {
                limit: Some(50),
                device_id: Some("device-a".to_owned()),
                reason: Some("app_resumed".to_owned()),
            },
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 1);
    assert_eq!(result.pages_fetched, 1);
    assert_eq!(result.last_applied_event_seq.as_deref(), Some("5"));
    assert_eq!(fixture.checkpoint(), Some("5".to_owned()));
    assert_eq!(fixture.message_server_seq("msg-delta-5"), Some(5));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "sync.delta");
    assert_eq!(calls[0].params["body"]["since_event_seq"], "4");
    assert_eq!(calls[0].params["body"]["limit"], 50);
    assert_eq!(calls[0].params["body"]["device_id"], "device-a");
    assert_eq!(calls[0].params["body"]["reason"], "app_resumed");
}

#[tokio::test]
async fn sync_delta_success_emits_committed_invalidation_after_apply() {
    let fixture = Fixture::new("sync-delta-invalidation");
    let client = fixture.client();
    fixture.store_checkpoint("776");
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event(
                    "sev-777",
                    "777",
                    "msg-delta-777",
                    777,
                )],
                "777",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.last_applied_event_seq.as_deref(), Some("777"));
    assert_eq!(fixture.checkpoint(), Some("777".to_owned()));
    let invalidations = committed_sync_invalidations_for_test();
    let invalidation = invalidations
        .iter()
        .find(|item| item.checkpoint_event_seq == "777")
        .expect("committed invalidation for checkpoint 777");
    assert_eq!(invalidation.owner_identity_id, "alice-id");
    assert_eq!(invalidation.owner_did, "did:example:alice");
    assert_eq!(invalidation.reason, "sync_delta");
    assert_eq!(
        invalidation.conversation_ids,
        vec!["dm:did:example:bob".to_owned()]
    );
    assert_eq!(
        invalidation.thread_ids,
        vec!["dm:did:example:bob".to_owned()]
    );
}

#[tokio::test]
async fn sync_delta_has_more_reads_committed_checkpoint_for_next_page() {
    let fixture = Fixture::new("sync-delta-has-more");
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![
                delta_page(
                    vec![message_created_event("sev-1", "1", "msg-delta-1", 1)],
                    "1",
                    true,
                ),
                delta_page(
                    vec![message_created_event("sev-2", "2", "msg-delta-2", 2)],
                    "2",
                    false,
                ),
            ],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest {
                limit: Some(1),
                device_id: None,
                reason: Some("gap".to_owned()),
            },
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 2);
    assert_eq!(result.pages_fetched, 2);
    assert_eq!(fixture.checkpoint(), Some("2".to_owned()));
    assert_eq!(fixture.message_server_seq("msg-delta-1"), Some(1));
    assert_eq!(fixture.message_server_seq("msg-delta-2"), Some(2));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].params["body"]["since_event_seq"], "0");
    assert_eq!(calls[1].params["body"]["since_event_seq"], "1");
}

#[tokio::test]
async fn sync_delta_snapshot_required_is_fail_closed() {
    let fixture = Fixture::new("sync-delta-snapshot");
    let client = fixture.client();
    fixture.store_checkpoint("7");
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![json!({
                "events": [message_created_event("sev-8", "8", "msg-delta-8", 8)],
                "next_event_seq": "8",
                "has_more": false,
                "snapshot_required": true,
                "retention_floor_event_seq": "10",
                "warnings": ["delta_retention_gap"],
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert!(result.snapshot_required);
    assert_eq!(result.retention_floor_event_seq.as_deref(), Some("10"));
    assert_eq!(result.events_applied, 0);
    assert_eq!(fixture.checkpoint(), Some("7".to_owned()));
    assert_eq!(fixture.message_server_seq("msg-delta-8"), None);
    assert!(!committed_sync_invalidations_for_test()
        .iter()
        .any(|item| item.checkpoint_event_seq == "8"));
}

#[tokio::test]
async fn sync_delta_invalid_page_rolls_back_message_and_checkpoint() {
    let fixture = Fixture::new("sync-delta-rollback");
    let client = fixture.client();
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![
                    message_created_event("sev-1", "1", "msg-before-gap", 1),
                    message_created_event("sev-3", "3", "msg-after-gap", 3),
                ],
                "3",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let err = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap_err();

    assert!(err.to_string().contains("event_seq gap"));
    assert_eq!(fixture.checkpoint(), None);
    assert_eq!(fixture.message_server_seq("msg-before-gap"), None);
    assert_eq!(fixture.message_server_seq("msg-after-gap"), None);
    assert!(!committed_sync_invalidations_for_test()
        .iter()
        .any(|item| item.checkpoint_event_seq == "3"));
}

#[tokio::test]
async fn sync_delta_duplicate_page_is_idempotent() {
    let fixture = Fixture::new("sync-delta-duplicate");
    let client = fixture.client();
    fixture.store_checkpoint("1");
    fixture.seed_message(
        "msg-delta-1",
        "dm:did:example:bob",
        "",
        Some(1),
        "did:example:bob",
        "did:example:alice",
    );
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![delta_page(
                vec![message_created_event("sev-1", "1", "msg-delta-1", 1)],
                "1",
                false,
            )],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap();

    assert_eq!(result.events_applied, 0);
    assert_eq!(fixture.checkpoint(), Some("1".to_owned()));
    assert_eq!(fixture.message_count("msg-delta-1"), 1);
}

#[tokio::test]
async fn sync_delta_has_more_duplicate_page_without_progress_is_invalid() {
    let fixture = Fixture::new("sync-delta-has-more-no-progress");
    let client = fixture.client();
    fixture.store_checkpoint("1");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![delta_page(
                vec![message_created_event("sev-1", "1", "msg-delta-1", 1)],
                "1",
                true,
            )],
        ),
        NoopDirectoryTransport,
    );

    let err = runtime
        .sync_delta_async(SyncDeltaInput {
            request: crate::messages::SyncDeltaRequest::default(),
        })
        .await
        .unwrap_err();

    assert!(err.to_string().contains("without checkpoint progress"));
    assert_eq!(fixture.checkpoint(), Some("1".to_owned()));
    assert_eq!(calls.borrow().len(), 1);
}

#[tokio::test]
async fn sync_thread_after_uses_local_max_seq_and_filters_numeric_ascending() {
    let fixture = Fixture::new("thread-after-direct");
    let client = fixture.client();
    fixture.seed_message(
        "local-direct-newest",
        "dm:did:example:bob",
        "",
        Some(42),
        "did:example:bob",
        "did:example:alice",
    );
    fixture.seed_message(
        "local-other-thread",
        "dm:did:example:carol",
        "",
        Some(100),
        "did:example:carol",
        "did:example:alice",
    );
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [
                    {
                        "id": "remote-old",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "old",
                        "content_type": "text/plain",
                        "server_seq": 41
                    },
                    {
                        "id": "remote-new-44",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "new 44",
                        "content_type": "text/plain",
                        "server_seq": "44"
                    },
                    {
                        "id": "remote-new-43",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "new 43",
                        "content_type": "text/plain",
                        "server_seq": 43
                    },
                    {
                        "id": "remote-no-seq",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "no seq",
                        "content_type": "text/plain"
                    }
                ],
                "has_more": false
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_thread_after_async(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                after_server_seq: None,
                limit: Some(10),
            },
            resolved_peer_did: Some("did:example:bob".to_owned()),
            peer_scope: None,
        })
        .await
        .unwrap();

    assert_eq!(
        result
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["remote-new-43", "remote-new-44"]
    );
    assert_eq!(result.next_after_server_seq.as_deref(), Some("44"));
    assert!(!result.has_more);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "direct.get_history");
    assert_eq!(calls[0].params["body"]["since_seq"], "42");
    assert_eq!(calls[0].params["body"]["limit"], 10);
}

#[tokio::test]
async fn sync_thread_after_explicit_after_seq_does_not_return_old_history_page_items() {
    let fixture = Fixture::new("thread-after-explicit");
    let client = fixture.client();
    fixture.seed_message(
        "local-direct-would-merge",
        "dm:did:example:bob",
        "",
        Some(99),
        "did:example:bob",
        "did:example:alice",
    );
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [
                    {
                        "id": "remote-old-1",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "old 1",
                        "content_type": "text/plain",
                        "server_seq": 1
                    },
                    {
                        "id": "remote-new-8",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "new 8",
                        "content_type": "text/plain",
                        "server_seq": 8
                    }
                ],
                "has_more": false
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_thread_after_async(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                after_server_seq: Some("7".to_owned()),
                limit: None,
            },
            resolved_peer_did: Some("did:example:bob".to_owned()),
            peer_scope: None,
        })
        .await
        .unwrap();

    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].id.as_str(), "remote-new-8");
    assert_eq!(result.next_after_server_seq.as_deref(), Some("8"));
    let calls = calls.borrow();
    assert_eq!(calls[0].method, "direct.get_history");
    assert_eq!(calls[0].params["body"]["since_seq"], "7");
    assert_eq!(calls[0].params["body"]["limit"], 100);
}

#[tokio::test]
async fn sync_thread_after_group_uses_raw_group_messages_since_seq() {
    let fixture = Fixture::new("thread-after-group");
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageSyncRuntime::new(
        &client,
        ReadyAnySessionProvider,
        RecordingTransport::queued(
            Rc::clone(&calls),
            vec![json!({
                "messages": [
                    {
                        "id": "group-old",
                        "group_did": "did:example:group",
                        "sender_did": "did:example:bob",
                        "content": "old",
                        "content_type": "text/plain",
                        "group_event_seq": 5
                    },
                    {
                        "id": "group-new",
                        "group_did": "did:example:group",
                        "sender_did": "did:example:bob",
                        "content": "new",
                        "content_type": "text/plain",
                        "group_event_seq": 6
                    }
                ],
                "has_more": true,
                "warnings": ["partial"]
            })],
        ),
        NoopDirectoryTransport,
    );

    let result = runtime
        .sync_thread_after_async(SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse("did:example:group").unwrap(),
                ),
                after_server_seq: Some("5".to_owned()),
                limit: Some(50),
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .await
        .unwrap();

    assert_eq!(
        result
            .messages
            .iter()
            .map(|message| message.metadata.server_sequence)
            .collect::<Vec<_>>(),
        vec![Some(6)]
    );
    assert_eq!(result.messages[0].metadata.server_sequence, Some(6));
    assert_eq!(result.next_after_server_seq.as_deref(), Some("6"));
    assert!(result.has_more);
    assert_eq!(result.warnings, vec!["partial"]);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "group.list_messages");
    assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
    assert_eq!(calls[0].params["body"]["since_seq"], "5");
}

#[test]
fn sync_thread_after_rejects_invalid_after_seq_and_limit() {
    assert!(explicit_after_server_seq(Some("01")).is_err());
    assert!(explicit_after_server_seq(Some("-1")).is_err());
    assert_eq!(explicit_after_server_seq(Some("0")).unwrap(), Some(0));
    assert!(sync_thread_after_limit(Some(0)).is_err());
    assert!(sync_thread_after_limit(Some(501)).is_err());
    assert_eq!(sync_thread_after_limit(None).unwrap(), 100);
}

struct ReadyAnySessionProvider;

impl SessionProvider for ReadyAnySessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        assert!(matches!(
            scope,
            crate::auth::AuthScope::Messaging | crate::auth::AuthScope::GroupMessaging
        ));
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("sync runtime should not refresh through the session provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("sync runtime should not read status")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for ReadyAnySessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        SessionProvider::ensure_session(self, scope)
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        SessionProvider::refresh_session(self)
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        SessionProvider::status(self)
    }
}

struct RecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    responses: RefCell<VecDeque<Value>>,
}

impl RecordingTransport {
    fn queued(calls: Rc<RefCell<Vec<RecordedCall>>>, responses: Vec<Value>) -> Self {
        Self {
            calls,
            responses: RefCell::new(responses.into()),
        }
    }
}

impl AuthenticatedRpcTransport for RecordingTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall {
            endpoint: endpoint.to_owned(),
            method: method.to_owned(),
            params,
        });
        Ok(self
            .responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| json!({ "messages": [], "has_more": false })))
    }
}

impl AsyncAuthenticatedRpcTransport for RecordingTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

struct RecordedCall {
    endpoint: String,
    method: String,
    params: Value,
}

struct NoopDirectoryTransport;

impl RpcTransport for NoopDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, _method: &str, _params: Value) -> crate::ImResult<Value> {
        Err(crate::ImError::PeerNotFound {
            peer: "noop-directory".to_owned(),
        })
    }
}

impl AsyncRpcTransport for NoopDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = unique_temp_root(name);
        let identities = root.join("identities");
        fs::create_dir_all(identities.join("alice")).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
              }]
            }"#,
        )
        .unwrap();
        Self { root }
    }

    fn client(&self) -> crate::core::ImClient {
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_owned(),
        ))
        .unwrap()
    }

    fn sqlite_path(&self) -> PathBuf {
        self.root.join("local").join("im.sqlite")
    }

    fn seed_message(
        &self,
        msg_id: &str,
        conversation_id: &str,
        group_did: &str,
        server_seq: Option<i64>,
        sender_did: &str,
        receiver_did: &str,
    ) {
        let db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::messages::upsert_message(
            &db,
            &crate::internal::local_state::messages::MessageRecord {
                msg_id: msg_id.to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: conversation_id.to_owned(),
                thread_id: conversation_id.to_owned(),
                direction: 0,
                sender_did: sender_did.to_owned(),
                receiver_did: receiver_did.to_owned(),
                group_id: String::new(),
                group_did: group_did.to_owned(),
                content_type: "text/plain".to_owned(),
                content: "local".to_owned(),
                server_seq,
                sent_at: "2026-06-27T00:00:00Z".to_owned(),
                stored_at: "2026-06-27T00:00:00Z".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn store_checkpoint(&self, event_seq: &str) {
        let mut db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        let tx = db.transaction().unwrap();
        crate::internal::local_state::sync_state::store_global_checkpoint_tx(
            &tx,
            "alice-id",
            "did:example:alice",
            event_seq,
            None,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn checkpoint(&self) -> Option<String> {
        let db = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        crate::internal::local_state::sync_state::load_global_checkpoint(&db, "alice-id")
            .unwrap()
            .map(|checkpoint| checkpoint.event_seq)
    }

    fn message_server_seq(&self, msg_id: &str) -> Option<i64> {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT server_seq FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
    }

    fn message_count(&self, msg_id: &str) -> i64 {
        let db = rusqlite::Connection::open(self.sqlite_path()).unwrap();
        db.query_row(
            "SELECT COUNT(*) FROM messages WHERE owner_identity_id = 'alice-id' AND msg_id = ?1",
            rusqlite::params![msg_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    }
}

fn unique_temp_root(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "im-core-sync-runtime-{name}-{}-{nanos}-{counter}",
        std::process::id()
    ))
}

fn delta_page(events: Vec<Value>, next_event_seq: &str, has_more: bool) -> Value {
    json!({
        "events": events,
        "next_event_seq": next_event_seq,
        "has_more": has_more,
        "snapshot_required": false,
        "retention_floor_event_seq": "1",
        "warnings": [],
    })
}

fn message_created_event(
    event_id: &str,
    event_seq: &str,
    message_id: &str,
    server_seq: i64,
) -> Value {
    json!({
        "event_id": event_id,
        "event_seq": event_seq,
        "event_type": "message.created",
        "aggregate_kind": "direct_message",
        "aggregate_id": message_id,
        "owner_subject_id": "did:example:alice",
        "created_at": "2026-06-27T00:00:00Z",
        "payload": {
            "thread_kind": "direct",
            "thread": {
                "kind": "direct",
                "peer_did": "did:example:bob"
            },
            "message": {
                "id": message_id,
                "server_seq": server_seq.to_string(),
                "sender_did": "did:example:bob",
                "receiver_did": "did:example:alice",
                "content_type": "text/plain",
                "content": "hello from sync.delta",
                "sent_at": "2026-06-27T00:00:00Z"
            }
        }
    })
}
