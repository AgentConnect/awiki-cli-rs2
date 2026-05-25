#[cfg(feature = "sqlite")]
use rusqlite::{params, OptionalExtension};

#[cfg(feature = "sqlite")]
use super::sqlite_store::SqliteDirectSecureStateStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectSecureLocalStatus {
    pub(crate) peer: crate::ids::PeerRef,
    pub(crate) resolved_peer: Option<crate::ids::Did>,
    pub(crate) state: crate::secure::DirectSecureState,
    pub(crate) can_send_secure: bool,
    pub(crate) pending_outbox_count: u32,
    pub(crate) problem: Option<crate::secure::SecureProblem>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectSecureRepairPlan {
    pub(crate) status: DirectSecureLocalStatus,
    pub(crate) removed_session: bool,
    pub(crate) requeued_outbox_count: u32,
}

#[cfg(feature = "sqlite")]
pub(crate) fn direct_status_for_client(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
) -> crate::ImResult<DirectSecureLocalStatus> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    direct_status_for_connection(client, &connection, peer)
}

#[cfg(feature = "sqlite")]
pub(crate) fn direct_status_for_connection(
    client: &crate::core::ImClient,
    connection: &rusqlite::Connection,
    peer: crate::ids::PeerRef,
) -> crate::ImResult<DirectSecureLocalStatus> {
    let owner_identity_id = client.current_identity().id.as_str();
    let owner_did = client.did().as_str();
    let resolved_peer = resolve_peer_did(connection, owner_identity_id, owner_did, &peer)?;
    let store = SqliteDirectSecureStateStore::new(connection)?;
    let pending_outbox_count =
        pending_outbox_count(connection, owner_did, resolved_peer.as_ref(), &peer)?;

    let Some(peer_did) = resolved_peer.as_ref() else {
        return Ok(DirectSecureLocalStatus {
            peer,
            resolved_peer: None,
            state: crate::secure::DirectSecureState::WaitingForPeer,
            can_send_secure: false,
            pending_outbox_count,
            problem: Some(crate::secure::SecureProblem::peer_not_found()),
            warnings: Vec::new(),
        });
    };

    if store
        .get_session(owner_identity_id, peer_did.as_str())?
        .is_some()
    {
        return Ok(DirectSecureLocalStatus {
            peer,
            resolved_peer: Some(peer_did.clone()),
            state: crate::secure::DirectSecureState::Ready,
            can_send_secure: true,
            pending_outbox_count,
            problem: None,
            warnings: Vec::new(),
        });
    }

    let has_signed_prekey = store.active_signed_prekey(owner_identity_id)?.is_some();
    let has_one_time_prekeys = !store
        .list_available_one_time_prekeys(owner_identity_id)?
        .is_empty();
    let (state, problem, warnings) = match (has_signed_prekey, has_one_time_prekeys) {
        (true, true) => (
            crate::secure::DirectSecureState::WaitingForPeer,
            None,
            Vec::new(),
        ),
        (true, false) => (
            crate::secure::DirectSecureState::Preparing,
            Some(crate::secure::SecureProblem::peer_keys_unavailable(
                "direct E2EE one-time prekeys are not available locally",
                true,
            )),
            vec!["direct E2EE one-time prekeys need to be replenished".to_owned()],
        ),
        (false, _) => (
            crate::secure::DirectSecureState::Preparing,
            Some(crate::secure::SecureProblem::peer_keys_unavailable(
                "direct E2EE signed prekey is not available locally",
                true,
            )),
            vec!["direct E2EE prekey bundle has not been prepared".to_owned()],
        ),
    };
    Ok(DirectSecureLocalStatus {
        peer,
        resolved_peer: Some(peer_did.clone()),
        state,
        can_send_secure: false,
        pending_outbox_count,
        problem,
        warnings,
    })
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn direct_status_for_client(
    _client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
) -> crate::ImResult<DirectSecureLocalStatus> {
    Ok(DirectSecureLocalStatus {
        peer,
        resolved_peer: None,
        state: crate::secure::DirectSecureState::Unavailable,
        can_send_secure: false,
        pending_outbox_count: 0,
        problem: Some(crate::secure::SecureProblem {
            code: crate::secure::SecureProblemCode::LocalStateUnavailable,
            message: "sqlite local state is disabled".to_owned(),
            retryable: true,
        }),
        warnings: Vec::new(),
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn repair_direct_for_client(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
) -> crate::ImResult<DirectSecureRepairPlan> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let owner_identity_id = client.current_identity().id.as_str();
    let owner_did = client.did().as_str();
    let resolved_peer = resolve_peer_did(&connection, owner_identity_id, owner_did, &peer)?;
    let mut removed_session = false;
    let mut requeued_outbox_count = 0;

    if let Some(peer_did) = resolved_peer.as_ref() {
        let store = SqliteDirectSecureStateStore::new(&connection)?;
        removed_session = store.delete_session_by_peer(owner_identity_id, peer_did.as_str())?;
        requeued_outbox_count = requeue_failed_outbox(&connection, owner_did, peer_did.as_str())?;
    }

    let status = direct_status_for_connection(client, &connection, peer)?;
    Ok(DirectSecureRepairPlan {
        status,
        removed_session,
        requeued_outbox_count,
    })
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn repair_direct_for_client(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
) -> crate::ImResult<DirectSecureRepairPlan> {
    Ok(DirectSecureRepairPlan {
        status: direct_status_for_client(client, peer)?,
        removed_session: false,
        requeued_outbox_count: 0,
    })
}

#[cfg(feature = "sqlite")]
fn resolve_peer_did(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<Option<crate::ids::Did>> {
    let value = peer.as_str().trim();
    if value.starts_with("did:") {
        return crate::ids::Did::parse(value).map(Some);
    }
    match crate::internal::contact_store::records::get_current_contact_by_handle(
        connection,
        owner_identity_id,
        owner_did,
        value,
    ) {
        Ok(record) => crate::ids::Did::parse(record.did).map(Some),
        Err(crate::ImError::LocalStateUnavailable { .. }) => Ok(None),
        Err(crate::ImError::InvalidInput { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(feature = "sqlite")]
fn pending_outbox_count(
    connection: &rusqlite::Connection,
    owner_did: &str,
    resolved_peer: Option<&crate::ids::Did>,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<u32> {
    let peer_did = resolved_peer
        .map(crate::ids::Did::as_str)
        .unwrap_or(peer.as_str());
    let count = connection
        .query_row(
            r#"
SELECT COUNT(*)
FROM e2ee_outbox
WHERE owner_did = ?1
  AND peer_did = ?2
  AND local_status IN ('queued', 'sending', 'failed')"#,
            params![owner_did.trim(), peer_did.trim()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .unwrap_or(0);
    Ok(count.max(0) as u32)
}

#[cfg(feature = "sqlite")]
fn requeue_failed_outbox(
    connection: &rusqlite::Connection,
    owner_did: &str,
    peer_did: &str,
) -> crate::ImResult<u32> {
    let changed = connection
        .execute(
            r#"
UPDATE e2ee_outbox
SET local_status = 'queued',
    retry_hint = NULL,
    updated_at = ?3
WHERE owner_did = ?1
  AND peer_did = ?2
  AND local_status = 'failed'"#,
            params![owner_did.trim(), peer_did.trim(), now_utc_like()],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(changed as u32)
}

#[cfg(feature = "sqlite")]
fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::internal::secure_direct::sqlite_store::{
        DirectSessionRecord, SqliteDirectSecureStateStore,
    };

    #[test]
    fn secure_direct_status_reports_ready_without_leaking_session_details() {
        let (client, db) = test_client_and_db("direct-status-ready");
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();
        store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: "secret-session-id".to_owned(),
                state_blob: serde_json::to_vec(&test_session()).unwrap(),
                metadata_json: "{}".to_owned(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            })
            .unwrap();

        let status = direct_status_for_connection(
            &client,
            &db,
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        )
        .unwrap();

        assert_eq!(status.state, crate::secure::DirectSecureState::Ready);
        assert!(status.can_send_secure);
        assert_eq!(
            status.resolved_peer.as_ref().map(crate::ids::Did::as_str),
            Some("did:example:bob")
        );
        assert!(status.problem.is_none());
        assert!(!format!("{status:?}").contains("secret-session-id"));
    }

    #[test]
    fn secure_direct_repair_deletes_peer_session_and_requeues_failed_outbox() {
        let root = unique_temp_root("direct-repair");
        let core = crate::ImCore::new(test_config(), test_paths(&root)).unwrap();
        write_identity_fixture(&root, "alice", "did:example:alice");
        let client = core
            .client(crate::IdentitySelector::LocalAlias("alice".to_owned()))
            .unwrap();
        let db = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();
        store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: "session-to-reset".to_owned(),
                state_blob: serde_json::to_vec(&test_session()).unwrap(),
                metadata_json: "{}".to_owned(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            })
            .unwrap();
        db.execute(
            r#"
INSERT INTO e2ee_outbox
    (outbox_id, owner_did, peer_did, plaintext, local_status, created_at, updated_at)
VALUES ('outbox-1', 'did:example:alice', 'did:example:bob', 'redacted', 'failed', '2026-05-24T00:00:00Z', '2026-05-24T00:00:00Z')"#,
            [],
        )
        .unwrap();
        drop(db);

        let plan = repair_direct_for_client(
            &client,
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        )
        .unwrap();

        assert!(plan.removed_session);
        assert_eq!(plan.requeued_outbox_count, 1);
        assert_eq!(
            plan.status.state,
            crate::secure::DirectSecureState::Preparing
        );
        let db = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        assert!(SqliteDirectSecureStateStore::new(&db)
            .unwrap()
            .get_session("alice-id", "did:example:bob")
            .unwrap()
            .is_none());
        let status = db
            .query_row(
                "SELECT local_status FROM e2ee_outbox WHERE outbox_id = 'outbox-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(status, "queued");
    }

    fn test_client_and_db(prefix: &str) -> (crate::core::ImClient, Connection) {
        let root = unique_temp_root(prefix);
        write_identity_fixture(&root, "alice", "did:example:alice");
        let core = crate::ImCore::new(test_config(), test_paths(&root)).unwrap();
        let client = core
            .client(crate::IdentitySelector::LocalAlias("alice".to_owned()))
            .unwrap();
        let db = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        (client, db)
    }

    fn test_session() -> anp::direct_e2ee::DirectSessionState {
        anp::direct_e2ee::DirectSessionState {
            session_id: "secret-session-id".to_owned(),
            suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            local_key_agreement_id: "did:example:alice#key-3".to_owned(),
            peer_key_agreement_id: "did:example:bob#key-3".to_owned(),
            root_key_b64u: "root-key".to_owned(),
            send_chain_key_b64u: Some("send-chain".to_owned()),
            recv_chain_key_b64u: Some("recv-chain".to_owned()),
            ratchet_private_key_b64u: "ratchet-private".to_owned(),
            ratchet_public_key_b64u: "ratchet-public".to_owned(),
            peer_ratchet_public_key_b64u: Some("peer-ratchet".to_owned()),
            send_n: 0,
            recv_n: 0,
            previous_send_chain_length: 0,
            skipped_message_keys: Vec::new(),
            is_initiator: true,
            status: "established".to_owned(),
        }
    }

    fn test_config() -> crate::ImCoreConfig {
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "awiki.test".to_owned(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        }
    }

    fn test_paths(root: &std::path::Path) -> crate::ImCorePaths {
        crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        }
    }

    fn write_identity_fixture(root: &std::path::Path, alias: &str, did: &str) {
        let identity_root = root.join("identities");
        let identity_dir = identity_root.join(alias);
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(identity_root.join("default"), format!("{alias}\n")).unwrap();
        std::fs::write(
            identity_root.join("registry.json"),
            serde_json::json!({
                "default_identity": alias,
                "identities": [{
                    "id": "alice-id",
                    "did": did,
                    "local_alias": alias,
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(identity_dir.join("did.json"), "{}").unwrap();
    }

    fn unique_temp_root(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
