use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OwnerDeleteTable {
    pub(crate) table: &'static str,
    pub(crate) delete_owner_dids: bool,
}

pub(crate) const OWNER_BUSINESS_DELETE_TABLES: &[OwnerDeleteTable] = &[
    OwnerDeleteTable {
        table: "attachment_manifest_cache",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "contact_handle_bindings",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "contacts",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "conversation_aliases",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "conversation_registry",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "conversation_summaries",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "did_transition_edges",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "direct_e2ee_one_time_prekeys",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "direct_e2ee_sessions",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "direct_e2ee_signed_prekeys",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "direct_peer_routes",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "e2ee_outbox",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "group_members",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "group_rebind_outbox",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "group_rebind_p6_jobs",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "groups",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "identity_root_import_completion_v1",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "identity_root_transfer_sender_v1",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "inbound_resolution_backlog",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "inbound_resolution_thread_bindings",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "lane_sync_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "local_mutation_outbox",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "message_identity_aliases",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "message_sync_run_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "message_sync_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "messages",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "p6_lane_blockers",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "peer_identifiers",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "peer_personas",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "peer_profiles",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "relationship_events",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "sync_applied_events",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_installation_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_lane_applied_events",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_lane_capability_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_lane_inbox",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_lane_transport_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_p5_did_cutovers",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_p5_input_outcomes",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_p6_input_outcomes",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_p6_legacy_migration_repairs",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_recovery_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_remote_read_states",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_state",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "sync_thread_bindings",
        delete_owner_dids: false,
    },
    OwnerDeleteTable {
        table: "system_notification_join_state",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "system_notification_receipts",
        delete_owner_dids: true,
    },
    OwnerDeleteTable {
        table: "thread_read_state",
        delete_owner_dids: true,
    },
];

pub(crate) const OWNER_DEDICATED_DELETE_TABLES: &[&str] =
    &["identity_did_history", "identity_account_bindings"];

pub(crate) const OWNER_CONTROL_PRESERVE_TABLES: &[&str] = &[
    "handle_recovery_operations_v4",
    "identity_transition_pending",
    "registration_retired_join_rollovers",
    "local_identity_deletions",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerScope {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) device_id: Option<String>,
    pub(crate) credential_name: Option<String>,
}

impl OwnerScope {
    pub(crate) fn new(
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            owner_identity_id: Self::require_identity_id(owner_identity_id)?,
            owner_did: require_non_empty("owner_did", owner_did.into())?,
            device_id: None,
            credential_name: None,
        })
    }

    pub(crate) fn for_client(client: &crate::core::ImClient) -> crate::ImResult<Self> {
        Self::for_identity(client.current_identity())
    }

    pub(crate) fn for_identity(
        identity: &crate::identity::IdentitySummary,
    ) -> crate::ImResult<Self> {
        let mut scope = Self::new(identity.id.as_str(), identity.did.as_str())?;
        scope.device_id = optional_trimmed(identity.device_id.clone());
        scope.credential_name = optional_trimmed(identity.local_alias.clone());
        Ok(scope)
    }

    pub(crate) fn require_identity_id(value: impl Into<String>) -> crate::ImResult<String> {
        require_non_empty("owner_identity_id", value.into())
    }

    pub(crate) fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = optional_trimmed(Some(device_id.into()));
        self
    }

    pub(crate) fn with_credential_name(mut self, credential_name: impl Into<String>) -> Self {
        self.credential_name = optional_trimmed(Some(credential_name.into()));
        self
    }
}

/// Removes every local-state row owned by one stable identity while leaving
/// rows for other identities in the shared SQLite scope untouched.
pub(crate) fn delete_owner_data(
    sqlite_path: &Path,
    owner_identity_id: &str,
    current_did: &str,
) -> crate::ImResult<usize> {
    let owner_identity_id = OwnerScope::require_identity_id(owner_identity_id.to_owned())?;
    let current_did = require_non_empty("owner_did", current_did.to_owned())?;
    let mut connection = super::open_writable(sqlite_path)?;
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    let deleted = delete_owner_data_in_transaction(&transaction, &owner_identity_id, &current_did)?;
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(deleted)
}

pub(crate) fn delete_owner_data_in_transaction(
    transaction: &rusqlite::Connection,
    owner_identity_id: &str,
    current_did: &str,
) -> crate::ImResult<usize> {
    let owner_identity_id = OwnerScope::require_identity_id(owner_identity_id.to_owned())?;
    let current_did = require_non_empty("owner_did", current_did.to_owned())?;
    transaction
        .pragma_update(None, "defer_foreign_keys", "ON")
        .map_err(super::local_state_unavailable)?;

    let mut owner_dids = BTreeSet::from([current_did]);
    if table_has_column(&transaction, "identity_did_history", "owner_identity_id")? {
        let mut statement = transaction
            .prepare("SELECT did FROM identity_did_history WHERE owner_identity_id=?1")
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([owner_identity_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(super::local_state_unavailable)?;
        for row in rows {
            let did = row.map_err(super::local_state_unavailable)?;
            if !did.trim().is_empty() {
                owner_dids.insert(did);
            }
        }
    }

    let mut deleted = 0usize;
    for spec in OWNER_BUSINESS_DELETE_TABLES {
        deleted += transaction
            .execute(
                &format!(
                    "DELETE FROM {} WHERE owner_identity_id=?1",
                    quote_identifier(spec.table)
                ),
                [owner_identity_id.as_str()],
            )
            .map_err(super::local_state_unavailable)?;
        if spec.delete_owner_dids {
            for did in &owner_dids {
                deleted += transaction
                    .execute(
                        &format!(
                            "DELETE FROM {} WHERE owner_did=?1 AND (owner_identity_id IS NULL OR owner_identity_id=?2)",
                            quote_identifier(spec.table)
                        ),
                        rusqlite::params![did, owner_identity_id],
                    )
                    .map_err(super::local_state_unavailable)?;
            }
        }
    }
    deleted += transaction
        .execute(
            "DELETE FROM identity_did_history WHERE owner_identity_id=?1",
            [owner_identity_id.as_str()],
        )
        .map_err(super::local_state_unavailable)?;
    deleted += transaction
        .execute(
            "DELETE FROM identity_account_bindings WHERE owner_identity_id=?1",
            [owner_identity_id.as_str()],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(deleted)
}

#[cfg(test)]
fn user_tables(connection: &rusqlite::Connection) -> crate::ImResult<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_schema \
             WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(super::local_state_unavailable)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(super::local_state_unavailable)
}

fn table_has_column(
    connection: &rusqlite::Connection,
    table: &str,
    expected: &str,
) -> crate::ImResult<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({})", quote_identifier(table)))
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(super::local_state_unavailable)?;
    for row in rows {
        if row.map_err(super::local_state_unavailable)? == expected {
            return Ok(true);
        }
    }
    Ok(false)
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectPeerScope {
    pub(crate) user_id: String,
    pub(crate) full_handle: String,
}

impl DirectPeerScope {
    pub(crate) fn new(
        user_id: impl Into<String>,
        full_handle: impl Into<String>,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            user_id: require_non_empty("peer_user_id", user_id.into())?,
            full_handle: normalize_full_handle(full_handle.into())?,
        })
    }
}

pub(crate) fn direct_conversation_id(peer_did: &str) -> String {
    let peer_did = peer_did.trim();
    if peer_did.is_empty() {
        "dm:unknown".to_owned()
    } else {
        format!("dm:{peer_did}")
    }
}

pub(crate) fn direct_conversation_id_for_peer_scope(scope: &DirectPeerScope) -> String {
    let input = format!("user:{}\nhandle:{}", scope.user_id, scope.full_handle);
    format!("dm:peer-scope:v1:{}", sha256_hex(input.as_bytes()))
}

pub(crate) fn direct_conversation_id_from_thread_alias(
    thread_id: &str,
    owner_did: &str,
) -> Option<String> {
    let alias = thread_id.trim().strip_prefix("dm:")?.trim();
    let owner_did = owner_did.trim();
    if alias.is_empty() {
        return Some(direct_conversation_id(""));
    }
    if owner_did.is_empty() {
        return Some(direct_conversation_id(alias));
    }
    if let Some(peer) = alias
        .strip_prefix(owner_did)
        .and_then(|rest| rest.strip_prefix(':'))
        .filter(|peer| !peer.trim().is_empty())
    {
        return Some(direct_conversation_id(peer));
    }
    if let Some(peer) = alias
        .strip_suffix(owner_did)
        .and_then(|rest| rest.strip_suffix(':'))
        .filter(|peer| !peer.trim().is_empty())
    {
        return Some(direct_conversation_id(peer));
    }
    Some(direct_conversation_id(alias))
}

pub(crate) fn group_conversation_id(group_id_or_did: &str) -> String {
    let group_id_or_did = group_id_or_did.trim();
    if group_id_or_did.is_empty() {
        "group:unknown".to_owned()
    } else {
        format!("group:{group_id_or_did}")
    }
}

pub(crate) fn mail_conversation_id(source: &str) -> String {
    let source = source.trim();
    if source.is_empty() {
        "mail:unknown".to_owned()
    } else {
        format!("mail:{source}")
    }
}

fn require_non_empty(field: &'static str, value: String) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_owned())
}

fn normalize_full_handle(value: String) -> crate::ImResult<String> {
    let value = require_non_empty("peer_full_handle", value)?;
    Ok(value.to_ascii_lowercase())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_scope_rejects_empty_identity_id() {
        let err = OwnerScope::new("  ", "did:example:alice").unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "owner_identity_id"
        ));
    }

    #[test]
    fn owner_scope_trims_metadata_without_using_it_as_owner_fallback() {
        let scope = OwnerScope::new(" alice-id ", " did:example:alice ")
            .unwrap()
            .with_device_id(" device-a ")
            .with_credential_name(" alice ");

        assert_eq!(scope.owner_identity_id, "alice-id");
        assert_eq!(scope.owner_did, "did:example:alice");
        assert_eq!(scope.device_id.as_deref(), Some("device-a"));
        assert_eq!(scope.credential_name.as_deref(), Some("alice"));
    }

    #[test]
    fn conversation_ids_are_stable_without_local_owner_did() {
        assert_eq!(
            direct_conversation_id(" did:example:bob "),
            "dm:did:example:bob"
        );
        assert_eq!(
            group_conversation_id(" did:example:group "),
            "group:did:example:group"
        );
        assert_eq!(mail_conversation_id(" inbox "), "mail:inbox");
    }

    #[test]
    fn direct_aliases_drop_local_owner_did() {
        assert_eq!(
            direct_conversation_id_from_thread_alias(
                "dm:did:example:alice:did:example:bob",
                "did:example:alice",
            ),
            Some("dm:did:example:bob".to_owned())
        );
        assert_eq!(
            direct_conversation_id_from_thread_alias(
                "dm:did:example:bob:did:example:alice",
                "did:example:alice",
            ),
            Some("dm:did:example:bob".to_owned())
        );
        assert_eq!(
            direct_conversation_id_from_thread_alias("dm:did:example:bob", "did:example:alice"),
            Some("dm:did:example:bob".to_owned())
        );
    }

    #[test]
    fn direct_peer_scope_conversation_ids_ignore_did_rotation() {
        let scope = DirectPeerScope::new("user-1", " Alice.AnPClaw.com ").expect("valid scope");
        let same_scope = DirectPeerScope::new("user-1", "alice.anpclaw.com").expect("valid scope");

        assert_eq!(
            direct_conversation_id_for_peer_scope(&scope),
            direct_conversation_id_for_peer_scope(&same_scope)
        );
        assert!(direct_conversation_id_for_peer_scope(&scope).starts_with("dm:peer-scope:v1:"));
    }

    #[test]
    fn direct_peer_scope_separates_handle_reuse_between_users() {
        let old_owner = DirectPeerScope::new("user-1", "alice.anpclaw.com").expect("valid scope");
        let new_owner = DirectPeerScope::new("user-2", "alice.anpclaw.com").expect("valid scope");

        assert_ne!(
            direct_conversation_id_for_peer_scope(&old_owner),
            direct_conversation_id_for_peer_scope(&new_owner)
        );
    }

    #[test]
    fn deleting_owner_data_preserves_other_owner_rows() {
        let directory = tempfile::tempdir().unwrap();
        let sqlite_path = directory.path().join("local-state.sqlite");
        let connection = super::super::open_writable(&sqlite_path).unwrap();
        connection
            .execute(
                "INSERT INTO messages \
                 (msg_id,owner_identity_id,owner_did,thread_id,stored_at) \
                 VALUES ('alice-message','alice-owner','did:example:alice-old','dm:bob','now'), \
                        ('bob-message','bob-owner','did:example:bob','dm:alice','now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO identity_did_history \
                 (owner_identity_id,did,status,first_seen_at,last_seen_at) \
                 VALUES ('alice-owner','did:example:alice-old','previous','now','now'), \
                        ('alice-owner','did:example:alice-new','current','now','now'), \
                        ('bob-owner','did:example:bob','current','now','now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "CREATE TABLE legacy_owner_did_state (owner_did TEXT NOT NULL, value TEXT NOT NULL)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO legacy_owner_did_state (owner_did,value) \
                 VALUES ('did:example:alice-old','alice-session'), \
                        ('did:example:bob','bob-session')",
                [],
            )
            .unwrap();
        drop(connection);

        let deleted =
            delete_owner_data(&sqlite_path, "alice-owner", "did:example:alice-new").unwrap();

        assert!(deleted >= 3);
        let connection = super::super::open_writable(&sqlite_path).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE owner_identity_id='alice-owner'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0,
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE owner_identity_id='bob-owner'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM legacy_owner_did_state WHERE owner_did='did:example:alice-old'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM legacy_owner_did_state WHERE owner_did='did:example:bob'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
        );
    }

    #[test]
    fn owner_table_classification_is_complete_and_disjoint() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        super::super::schema::ensure_schema(&connection).unwrap();
        let mut classified = std::collections::BTreeMap::<&str, usize>::new();
        for spec in OWNER_BUSINESS_DELETE_TABLES {
            *classified.entry(spec.table).or_default() += 1;
        }
        for table in OWNER_DEDICATED_DELETE_TABLES {
            *classified.entry(table).or_default() += 1;
        }
        for table in OWNER_CONTROL_PRESERVE_TABLES {
            *classified.entry(table).or_default() += 1;
        }
        let mut discovered = std::collections::BTreeSet::new();
        for table in user_tables(&connection).unwrap() {
            let has_owner_id = table_has_column(&connection, &table, "owner_identity_id").unwrap();
            let has_owner_did = table_has_column(&connection, &table, "owner_did").unwrap();
            if has_owner_id || has_owner_did {
                discovered.insert(table);
            }
        }
        assert_eq!(
            discovered,
            classified.keys().map(|table| (*table).to_owned()).collect()
        );
        assert!(classified.values().all(|count| *count == 1));
    }

    #[test]
    fn owner_data_delete_never_deletes_control_tables() {
        let directory = tempfile::tempdir().unwrap();
        let sqlite_path = directory.path().join("local-state.sqlite");
        let connection = super::super::open_writable(&sqlite_path).unwrap();
        connection
            .execute(
                "INSERT INTO handle_recovery_operations_v4(operation_id,owner_identity_id,full_handle,lifecycle_class,commit_attempted,key_state,vault_key_id,created_at,updated_at) VALUES ('op-a','alice-owner','alice.example.invalid','discarded_pre_attempt',0,'destroyed_pre_attempt','vault-a','now','now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO identity_transition_pending(recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,current_device_id,device_auth_generation,registry_version,applied_at,metadata_json,phase,created_at,updated_at) VALUES ('transition-a',1,'4.0','sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA','initiator','op-a','sha256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB','account-a','alice-owner','alice.example.invalid','did:example:alice-old','did:example:alice','2','device-a','2','2','now','{}','completed','now','now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO registration_retired_join_rollovers(join_session_id,schema_version,account_user_id,owner_identity_id,handle,retired_did,retired_device_id,retired_binding_generation,current_did,current_binding_generation,new_device_id,join_expires_at,completed_auth_generation,phase,created_at,updated_at,completed_at) VALUES ('join-a',1,'account-a','alice-owner','alice.example.invalid','did:example:alice-old','device-old','1','did:example:alice','2','device-new','2099-01-01T00:00:00Z','2','completed','now','now','now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO local_identity_deletions(deletion_id,schema_version,mode,owner_identity_id,current_did,full_handle,local_alias,phase,created_at,updated_at) VALUES ('delete-a',1,'full_data_app','alice-owner','did:example:alice','alice.example.invalid','alice','prepared','2026-08-29T00:00:00Z','2026-08-29T00:00:00Z')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "CREATE TABLE future_owner_state(owner_identity_id TEXT NOT NULL,value TEXT NOT NULL)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO future_owner_state(owner_identity_id,value) VALUES ('alice-owner','preserve')",
                [],
            )
            .unwrap();
        drop(connection);

        delete_owner_data(&sqlite_path, "alice-owner", "did:example:alice").unwrap();
        let connection = super::super::open_writable(&sqlite_path).unwrap();
        for table in OWNER_CONTROL_PRESERVE_TABLES {
            assert_eq!(
                connection
                    .query_row(
                        &format!(
                            "SELECT COUNT(*) FROM {} WHERE owner_identity_id='alice-owner'",
                            quote_identifier(table)
                        ),
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1,
                "{table}",
            );
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM future_owner_state WHERE owner_identity_id='alice-owner'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn binding_delete_has_no_control_table_cascade() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        super::super::schema::ensure_schema(&connection).unwrap();
        for table in OWNER_CONTROL_PRESERVE_TABLES {
            let mut statement = connection
                .prepare(&format!(
                    "PRAGMA foreign_key_list({})",
                    quote_identifier(table)
                ))
                .unwrap();
            let foreign_keys = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(2)?, row.get::<_, String>(6)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                foreign_keys.iter().all(|(target, on_delete)| {
                    target != "identity_account_bindings"
                        || !on_delete.eq_ignore_ascii_case("cascade")
                }),
                "{table}",
            );
        }
    }
}
