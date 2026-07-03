#![allow(dead_code)]

use rusqlite::{params, Connection, OptionalExtension};

use super::secret_store::{
    default_direct_secret_vault, direct_secret_key_id, open_direct_secret_blob,
    seal_direct_secret_blob, DirectSecretSealInput, DirectSecretVault,
};
use crate::vault::SecretKind;

const DIRECT_SESSION_METADATA_VERSION: &str = "im-core.direct-session.v1";
const DIRECT_PREKEY_METADATA_VERSION: &str = "im-core.direct-prekey.v1";
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectSessionRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) peer_did: String,
    pub(crate) session_id: String,
    pub(crate) state_blob: Vec<u8>,
    pub(crate) metadata_json: String,
    pub(crate) revision: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectSessionCasResult {
    Saved(DirectSessionRecord),
    Stale {
        current: Option<DirectSessionRecord>,
        expected_revision: i64,
    },
}

pub(crate) struct DirectInitSessionMaterial {
    pub(crate) existing_session: Option<DirectSessionRecord>,
    pub(crate) peer_session_revision: Option<i64>,
    pub(crate) signed_prekey_private: Option<anp::PrivateKeyMaterial>,
    pub(crate) one_time_prekey_private: Option<anp::PrivateKeyMaterial>,
}

pub(crate) struct DirectInitSendCommit {
    pub(crate) record: DirectSessionRecord,
    pub(crate) expected_peer_revision: Option<i64>,
}

pub(crate) struct DirectInitSessionCommit {
    pub(crate) record: DirectSessionRecord,
    pub(crate) expected_peer_revision: Option<i64>,
    pub(crate) consume_one_time_prekey_id: Option<String>,
    pub(crate) consumed_at: String,
}

pub(crate) enum DirectInitSessionCommitResult {
    Saved(DirectSessionRecord),
    Existing(DirectSessionRecord),
    Stale {
        current: Option<DirectSessionRecord>,
        expected_revision: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectSignedPrekeyRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) key_id: String,
    pub(crate) private_key_blob: Vec<u8>,
    pub(crate) public_key_blob: Vec<u8>,
    pub(crate) status: DirectPrekeyStatus,
    pub(crate) metadata_json: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectOneTimePrekeyRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) key_id: String,
    pub(crate) private_key_blob: Vec<u8>,
    pub(crate) public_key_blob: Vec<u8>,
    pub(crate) status: DirectPrekeyStatus,
    pub(crate) metadata_json: String,
    pub(crate) created_at: String,
    pub(crate) consumed_at: String,
}

#[derive(Debug)]
pub(crate) struct DirectOneTimePrekeyMaterialRecord {
    pub(crate) private_key: anp::PrivateKeyMaterial,
    pub(crate) metadata: anp::direct_e2ee::OneTimePrekey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectPrekeyStatus {
    Active,
    Retired,
    Available,
    Reserved,
    Consumed,
}

impl DirectPrekeyStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retired => "retired",
            Self::Available => "available",
            Self::Reserved => "reserved",
            Self::Consumed => "consumed",
        }
    }

    fn from_str(value: &str) -> Self {
        match value.trim() {
            "active" => Self::Active,
            "retired" => Self::Retired,
            "reserved" => Self::Reserved,
            "consumed" => Self::Consumed,
            _ => Self::Available,
        }
    }
}

pub(crate) struct SqliteDirectSecureStateStore<'a> {
    connection: &'a Connection,
    secret_vault: Option<DirectSecretVault>,
}

impl<'a> SqliteDirectSecureStateStore<'a> {
    pub(crate) fn new(connection: &'a Connection) -> crate::ImResult<Self> {
        crate::internal::local_state::schema::ensure_schema(connection)?;
        let secret_vault =
            default_direct_secret_vault(direct_secret_vault_dir_for_connection(connection)?)?;
        Ok(Self {
            connection,
            secret_vault,
        })
    }

    pub(crate) fn new_with_secret_vault(
        connection: &'a Connection,
        secret_vault: DirectSecretVault,
    ) -> crate::ImResult<Self> {
        crate::internal::local_state::schema::ensure_schema(connection)?;
        Ok(Self {
            connection,
            secret_vault: Some(secret_vault),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_without_secret_vault_for_legacy(
        connection: &'a Connection,
    ) -> crate::ImResult<Self> {
        crate::internal::local_state::schema::ensure_schema(connection)?;
        Ok(Self {
            connection,
            secret_vault: None,
        })
    }

    pub(crate) fn upsert_session(&self, record: &DirectSessionRecord) -> crate::ImResult<()> {
        let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
        let peer_did = required("peer_did", &record.peer_did)?;
        let session_id = required("session_id", &record.session_id)?;
        let state_blob = seal_direct_secret_blob(
            self.secret_vault.as_ref(),
            DirectSecretSealInput {
                owner_identity_id: &owner_identity_id,
                owner_did: record.owner_did.trim(),
                kind: SecretKind::DirectE2eeSessionState,
                key_id: direct_secret_key_id(&owner_identity_id, "session", &peer_did, &session_id),
                plaintext: &record.state_blob,
                field: "direct E2EE session state",
            },
        )?;
        self.connection
            .execute(
                r#"
	INSERT INTO direct_e2ee_sessions
	    (owner_identity_id, owner_did, peer_did, session_id, state_blob, metadata_json, revision, created_at, updated_at)
	VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)
	ON CONFLICT(owner_identity_id, peer_did)
	DO UPDATE SET
	    owner_did = excluded.owner_did,
	    session_id = excluded.session_id,
	    state_blob = excluded.state_blob,
	    metadata_json = excluded.metadata_json,
	    revision = direct_e2ee_sessions.revision + 1,
	    updated_at = excluded.updated_at"#,
                params![
                    owner_identity_id,
                    record.owner_did.trim(),
                    peer_did,
                    session_id,
                    state_blob,
                    nullable_text(&record.metadata_json),
                    required("created_at", &record.created_at)?,
                    required("updated_at", &record.updated_at)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(())
    }

    pub(crate) fn save_session_if_revision(
        &self,
        record: &DirectSessionRecord,
        expected_revision: i64,
    ) -> crate::ImResult<DirectSessionCasResult> {
        let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
        let peer_did = required("peer_did", &record.peer_did)?;
        let session_id = required("session_id", &record.session_id)?;
        let existing = self.get_session(&owner_identity_id, &peer_did)?;
        let Some(existing) = existing else {
            if expected_revision != 0 {
                return Ok(DirectSessionCasResult::Stale {
                    current: None,
                    expected_revision,
                });
            }
            let state_blob = seal_direct_secret_blob(
                self.secret_vault.as_ref(),
                DirectSecretSealInput {
                    owner_identity_id: &owner_identity_id,
                    owner_did: record.owner_did.trim(),
                    kind: SecretKind::DirectE2eeSessionState,
                    key_id: direct_secret_key_id(
                        &owner_identity_id,
                        "session",
                        &peer_did,
                        &format!("{session_id}:rev-0"),
                    ),
                    plaintext: &record.state_blob,
                    field: "direct E2EE session state",
                },
            )?;
            self.connection
                .execute(
                    r#"
INSERT INTO direct_e2ee_sessions
    (owner_identity_id, owner_did, peer_did, session_id, state_blob, metadata_json, revision, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)"#,
                    params![
                        owner_identity_id,
                        record.owner_did.trim(),
                        peer_did,
                        session_id,
                        state_blob,
                        nullable_text(&record.metadata_json),
                        required("created_at", &record.created_at)?,
                        required("updated_at", &record.updated_at)?,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            let saved = self
                .get_session(&owner_identity_id, &peer_did)?
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "direct E2EE session save did not persist a row".to_string(),
                })?;
            return Ok(DirectSessionCasResult::Saved(saved));
        };
        if existing.revision != expected_revision {
            return Ok(DirectSessionCasResult::Stale {
                current: Some(existing),
                expected_revision,
            });
        }
        let changed = self
            .connection
            .execute(
                r#"
UPDATE direct_e2ee_sessions
SET owner_did = ?4,
    session_id = ?5,
    state_blob = ?6,
    metadata_json = ?7,
    revision = ?8,
    updated_at = ?9
WHERE owner_identity_id = ?1
  AND peer_did = ?2
  AND revision = ?3"#,
                params![
                    owner_identity_id,
                    peer_did,
                    expected_revision,
                    record.owner_did.trim(),
                    session_id,
                    seal_direct_secret_blob(
                        self.secret_vault.as_ref(),
                        DirectSecretSealInput {
                            owner_identity_id: &owner_identity_id,
                            owner_did: record.owner_did.trim(),
                            kind: SecretKind::DirectE2eeSessionState,
                            key_id: direct_secret_key_id(
                                &owner_identity_id,
                                "session",
                                &peer_did,
                                &format!("{}:rev-{}", session_id, expected_revision + 1),
                            ),
                            plaintext: &record.state_blob,
                            field: "direct E2EE session state",
                        }
                    )?,
                    nullable_text(&record.metadata_json),
                    expected_revision + 1,
                    required("updated_at", &record.updated_at)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if changed == 0 {
            return Ok(DirectSessionCasResult::Stale {
                current: self.get_session(&owner_identity_id, &peer_did)?,
                expected_revision,
            });
        }
        let saved = self
            .get_session(&owner_identity_id, &peer_did)?
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "direct E2EE session save did not persist a row".to_string(),
            })?;
        Ok(DirectSessionCasResult::Saved(saved))
    }

    pub(crate) fn get_session(
        &self,
        owner_identity_id: &str,
        peer_did: &str,
    ) -> crate::ImResult<Option<DirectSessionRecord>> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let peer_did = required("peer_did", peer_did)?;
        self.connection
            .query_row(
                r#"
	SELECT owner_identity_id, owner_did, peer_did, session_id, state_blob, metadata_json, revision, created_at, updated_at
	FROM direct_e2ee_sessions
	WHERE owner_identity_id = ?1 AND peer_did = ?2"#,
                params![owner_identity_id, peer_did],
                session_from_row,
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(|record| self.open_session_record(record))
            .transpose()
    }

    pub(crate) fn get_session_by_id(
        &self,
        owner_identity_id: &str,
        session_id: &str,
    ) -> crate::ImResult<Option<DirectSessionRecord>> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let session_id = required("session_id", session_id)?;
        self.connection
            .query_row(
                r#"
	SELECT owner_identity_id, owner_did, peer_did, session_id, state_blob, metadata_json, revision, created_at, updated_at
	FROM direct_e2ee_sessions
	WHERE owner_identity_id = ?1 AND session_id = ?2"#,
                params![owner_identity_id, session_id],
                session_from_row,
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(|record| self.open_session_record(record))
            .transpose()
    }

    pub(crate) fn direct_init_session_material(
        &self,
        owner_identity_id: &str,
        peer_did: &str,
        session_id: &str,
        signed_prekey_id: &str,
        one_time_prekey_id: Option<&str>,
    ) -> crate::ImResult<DirectInitSessionMaterial> {
        let existing_session = if session_id.trim().is_empty() {
            None
        } else {
            self.get_session_by_id(owner_identity_id, session_id)?
        };
        let peer_session_revision = self
            .get_session(owner_identity_id, peer_did)?
            .map(|record| record.revision);
        let signed_prekey_private =
            self.load_signed_prekey_material(owner_identity_id, signed_prekey_id)?;
        let one_time_prekey_private = one_time_prekey_id
            .filter(|key_id| !key_id.trim().is_empty())
            .map(|key_id| self.load_one_time_prekey_material(owner_identity_id, key_id))
            .transpose()?
            .flatten()
            .map(|record| record.private_key);
        Ok(DirectInitSessionMaterial {
            existing_session,
            peer_session_revision,
            signed_prekey_private,
            one_time_prekey_private,
        })
    }

    pub(crate) fn delete_session(
        &self,
        owner_identity_id: &str,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let session_id = required("session_id", session_id)?;
        let changed = self
            .connection
            .execute(
                r#"
DELETE FROM direct_e2ee_sessions
WHERE owner_identity_id = ?1 AND session_id = ?2"#,
                params![owner_identity_id, session_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(changed > 0)
    }

    pub(crate) fn delete_session_by_peer(
        &self,
        owner_identity_id: &str,
        peer_did: &str,
    ) -> crate::ImResult<bool> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let peer_did = required("peer_did", peer_did)?;
        let changed = self
            .connection
            .execute(
                r#"
DELETE FROM direct_e2ee_sessions
WHERE owner_identity_id = ?1 AND peer_did = ?2"#,
                params![owner_identity_id, peer_did],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(changed > 0)
    }

    pub(crate) fn upsert_signed_prekey(
        &self,
        record: &DirectSignedPrekeyRecord,
    ) -> crate::ImResult<()> {
        let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
        let key_id = required("key_id", &record.key_id)?;
        let private_key_blob = seal_direct_secret_blob(
            self.secret_vault.as_ref(),
            DirectSecretSealInput {
                owner_identity_id: &owner_identity_id,
                owner_did: record.owner_did.trim(),
                kind: SecretKind::DirectE2eeSignedPrekeyPrivate,
                key_id: direct_secret_key_id(
                    &owner_identity_id,
                    "signed-prekey",
                    &key_id,
                    "private",
                ),
                plaintext: &record.private_key_blob,
                field: "direct E2EE signed prekey private key",
            },
        )?;
        self.connection
            .execute(
                r#"
INSERT INTO direct_e2ee_signed_prekeys
    (owner_identity_id, owner_did, key_id, private_key_blob, public_key_blob, status, metadata_json, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
ON CONFLICT(owner_identity_id, key_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    private_key_blob = excluded.private_key_blob,
    public_key_blob = excluded.public_key_blob,
    status = excluded.status,
    metadata_json = excluded.metadata_json,
    updated_at = excluded.updated_at"#,
                params![
                    owner_identity_id,
                    record.owner_did.trim(),
                    key_id,
                    private_key_blob,
                    record.public_key_blob,
                    record.status.as_str(),
                    nullable_text(&record.metadata_json),
                    required("created_at", &record.created_at)?,
                    required("updated_at", &record.updated_at)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(())
    }

    pub(crate) fn get_signed_prekey(
        &self,
        owner_identity_id: &str,
        key_id: &str,
    ) -> crate::ImResult<Option<DirectSignedPrekeyRecord>> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let key_id = required("key_id", key_id)?;
        self.connection
            .query_row(
                r#"
SELECT owner_identity_id, owner_did, key_id, private_key_blob, public_key_blob, status, metadata_json, created_at, updated_at
FROM direct_e2ee_signed_prekeys
WHERE owner_identity_id = ?1 AND key_id = ?2"#,
                params![owner_identity_id, key_id],
                signed_prekey_from_row,
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(|record| self.open_signed_prekey_record(record))
            .transpose()
    }

    pub(crate) fn active_signed_prekey(
        &self,
        owner_identity_id: &str,
    ) -> crate::ImResult<Option<DirectSignedPrekeyRecord>> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        self.connection
            .query_row(
                r#"
SELECT owner_identity_id, owner_did, key_id, private_key_blob, public_key_blob, status, metadata_json, created_at, updated_at
FROM direct_e2ee_signed_prekeys
WHERE owner_identity_id = ?1 AND status = 'active'
ORDER BY updated_at DESC, key_id DESC
LIMIT 1"#,
                params![owner_identity_id],
                signed_prekey_from_row,
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(|record| self.open_signed_prekey_record(record))
            .transpose()
    }

    pub(crate) fn load_signed_prekey_material(
        &self,
        owner_identity_id: &str,
        key_id: &str,
    ) -> crate::ImResult<Option<anp::PrivateKeyMaterial>> {
        let Some(record) = self.get_signed_prekey(owner_identity_id, key_id)? else {
            return Ok(None);
        };
        Ok(Some(private_key_from_blob(
            &record.private_key_blob,
            "signed prekey",
        )?))
    }

    pub(crate) fn upsert_one_time_prekey(
        &self,
        record: &DirectOneTimePrekeyRecord,
    ) -> crate::ImResult<()> {
        let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
        let key_id = required("key_id", &record.key_id)?;
        let private_key_blob = seal_direct_secret_blob(
            self.secret_vault.as_ref(),
            DirectSecretSealInput {
                owner_identity_id: &owner_identity_id,
                owner_did: record.owner_did.trim(),
                kind: SecretKind::DirectE2eeOneTimePrekeyPrivate,
                key_id: direct_secret_key_id(
                    &owner_identity_id,
                    "one-time-prekey",
                    &key_id,
                    "private",
                ),
                plaintext: &record.private_key_blob,
                field: "direct E2EE one-time prekey private key",
            },
        )?;
        self.connection
            .execute(
                r#"
INSERT INTO direct_e2ee_one_time_prekeys
    (owner_identity_id, owner_did, key_id, private_key_blob, public_key_blob, status, metadata_json, created_at, consumed_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
ON CONFLICT(owner_identity_id, key_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    private_key_blob = excluded.private_key_blob,
    public_key_blob = excluded.public_key_blob,
    status = excluded.status,
    metadata_json = excluded.metadata_json,
    consumed_at = excluded.consumed_at"#,
                params![
                    owner_identity_id,
                    record.owner_did.trim(),
                    key_id,
                    private_key_blob,
                    record.public_key_blob,
                    record.status.as_str(),
                    nullable_text(&record.metadata_json),
                    required("created_at", &record.created_at)?,
                    nullable_text(&record.consumed_at),
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(())
    }

    pub(crate) fn get_one_time_prekey(
        &self,
        owner_identity_id: &str,
        key_id: &str,
    ) -> crate::ImResult<Option<DirectOneTimePrekeyRecord>> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let key_id = required("key_id", key_id)?;
        self.connection
            .query_row(
                r#"
SELECT owner_identity_id, owner_did, key_id, private_key_blob, public_key_blob, status, metadata_json, created_at, consumed_at
FROM direct_e2ee_one_time_prekeys
WHERE owner_identity_id = ?1 AND key_id = ?2"#,
                params![owner_identity_id, key_id],
                one_time_prekey_from_row,
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(|record| self.open_one_time_prekey_record(record))
            .transpose()
    }

    pub(crate) fn list_available_one_time_prekeys(
        &self,
        owner_identity_id: &str,
    ) -> crate::ImResult<Vec<anp::direct_e2ee::OneTimePrekey>> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let mut statement = self
            .connection
            .prepare(
                r#"
SELECT key_id, metadata_json
FROM direct_e2ee_one_time_prekeys
WHERE owner_identity_id = ?1 AND status = 'available'
ORDER BY created_at ASC, key_id ASC"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(params![owner_identity_id], |row| {
                let key_id = row.get::<_, String>("key_id")?;
                let metadata_json = row
                    .get::<_, Option<String>>("metadata_json")?
                    .unwrap_or_default();
                Ok((key_id, metadata_json))
            })
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let mut result = Vec::new();
        for row in rows {
            let (key_id, metadata_json) =
                row.map_err(crate::internal::local_state::local_state_unavailable)?;
            result.push(one_time_prekey_metadata(&key_id, &metadata_json)?);
        }
        Ok(result)
    }

    pub(crate) fn load_one_time_prekey_material(
        &self,
        owner_identity_id: &str,
        key_id: &str,
    ) -> crate::ImResult<Option<DirectOneTimePrekeyMaterialRecord>> {
        let Some(record) = self.get_one_time_prekey(owner_identity_id, key_id)? else {
            return Ok(None);
        };
        Ok(Some(DirectOneTimePrekeyMaterialRecord {
            private_key: private_key_from_blob(&record.private_key_blob, "one-time prekey")?,
            metadata: one_time_prekey_metadata(&record.key_id, &record.metadata_json)?,
        }))
    }

    pub(crate) fn reserve_next_one_time_prekey(
        &self,
        owner_identity_id: &str,
        consumed_at: &str,
    ) -> crate::ImResult<Option<DirectOneTimePrekeyRecord>> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let consumed_at = required("consumed_at", consumed_at)?;
        let Some(record) = self
            .connection
            .query_row(
                r#"
SELECT owner_identity_id, owner_did, key_id, private_key_blob, public_key_blob, status, metadata_json, created_at, consumed_at
FROM direct_e2ee_one_time_prekeys
WHERE owner_identity_id = ?1 AND status = 'available'
ORDER BY created_at ASC, key_id ASC
LIMIT 1"#,
                params![owner_identity_id],
                one_time_prekey_from_row,
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(|record| self.open_one_time_prekey_record(record))
            .transpose()?
        else {
            return Ok(None);
        };
        self.connection
            .execute(
                r#"
UPDATE direct_e2ee_one_time_prekeys
SET status = 'reserved', consumed_at = ?3
WHERE owner_identity_id = ?1 AND key_id = ?2 AND status = 'available'"#,
                params![owner_identity_id, record.key_id, consumed_at],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.get_one_time_prekey(&owner_identity_id, &record.key_id)
    }

    pub(crate) fn mark_one_time_prekey_consumed(
        &self,
        owner_identity_id: &str,
        key_id: &str,
        consumed_at: &str,
    ) -> crate::ImResult<bool> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let key_id = required("key_id", key_id)?;
        let consumed_at = required("consumed_at", consumed_at)?;
        let changed = self
            .connection
            .execute(
                r#"
UPDATE direct_e2ee_one_time_prekeys
SET status = 'consumed', consumed_at = ?3
WHERE owner_identity_id = ?1 AND key_id = ?2"#,
                params![owner_identity_id, key_id, consumed_at],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(changed > 0)
    }

    fn open_session_record(
        &self,
        mut record: DirectSessionRecord,
    ) -> crate::ImResult<DirectSessionRecord> {
        record.state_blob = open_direct_secret_blob(
            self.secret_vault.as_ref(),
            record.state_blob,
            "direct E2EE session state",
        )?;
        Ok(record)
    }

    fn open_signed_prekey_record(
        &self,
        mut record: DirectSignedPrekeyRecord,
    ) -> crate::ImResult<DirectSignedPrekeyRecord> {
        record.private_key_blob = open_direct_secret_blob(
            self.secret_vault.as_ref(),
            record.private_key_blob,
            "direct E2EE signed prekey private key",
        )?;
        Ok(record)
    }

    fn open_one_time_prekey_record(
        &self,
        mut record: DirectOneTimePrekeyRecord,
    ) -> crate::ImResult<DirectOneTimePrekeyRecord> {
        record.private_key_blob = open_direct_secret_blob(
            self.secret_vault.as_ref(),
            record.private_key_blob,
            "direct E2EE one-time prekey private key",
        )?;
        Ok(record)
    }
}

fn direct_secret_vault_dir_for_connection(
    connection: &Connection,
) -> crate::ImResult<std::path::PathBuf> {
    let database_path = connection.path().unwrap_or_default();
    if database_path.trim().is_empty() || database_path == ":memory:" {
        return Ok(std::env::temp_dir().join(format!(
            "awiki-im-core-direct-sqlite-vault-{}",
            std::process::id()
        )));
    }
    let parent = std::path::Path::new(database_path)
        .parent()
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "direct E2EE local state database path has no parent".to_owned(),
        })?;
    Ok(parent.join("secrets").join("vault"))
}

pub(crate) fn save_incoming_init_session(
    connection: &mut Connection,
    commit: DirectInitSessionCommit,
) -> crate::ImResult<DirectInitSessionCommitResult> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let result = {
        let store = SqliteDirectSecureStateStore::new(&transaction)?;
        let owner_identity_id = required("owner_identity_id", &commit.record.owner_identity_id)?;
        let peer_did = required("peer_did", &commit.record.peer_did)?;
        let session_id = required("session_id", &commit.record.session_id)?;
        if let Some(existing) = store.get_session_by_id(&owner_identity_id, &session_id)? {
            if existing.peer_did != peer_did {
                return Err(crate::ImError::Serialization {
                    detail: "direct E2EE init session id is already bound to another peer"
                        .to_owned(),
                });
            }
            DirectInitSessionCommitResult::Existing(existing)
        } else {
            let current_peer = store.get_session(&owner_identity_id, &peer_did)?;
            let revision_matches = match (&current_peer, commit.expected_peer_revision) {
                (None, None) => true,
                (Some(current), Some(expected)) => current.revision == expected,
                _ => false,
            };
            if !revision_matches {
                DirectInitSessionCommitResult::Stale {
                    current: current_peer,
                    expected_revision: commit.expected_peer_revision,
                }
            } else {
                let expected_revision = commit.expected_peer_revision.unwrap_or(0);
                let saved = store.save_session_if_revision(&commit.record, expected_revision)?;
                match saved {
                    DirectSessionCasResult::Saved(record) => {
                        if let Some(key_id) = commit
                            .consume_one_time_prekey_id
                            .as_deref()
                            .filter(|value| !value.trim().is_empty())
                        {
                            store.mark_one_time_prekey_consumed(
                                &owner_identity_id,
                                key_id,
                                &commit.consumed_at,
                            )?;
                        }
                        DirectInitSessionCommitResult::Saved(record)
                    }
                    DirectSessionCasResult::Stale { current, .. } => {
                        DirectInitSessionCommitResult::Stale {
                            current,
                            expected_revision: commit.expected_peer_revision,
                        }
                    }
                }
            }
        }
    };
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(result)
}

pub(crate) fn save_outgoing_init_session(
    connection: &mut Connection,
    commit: DirectInitSendCommit,
) -> crate::ImResult<DirectInitSessionCommitResult> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let result = {
        let store = SqliteDirectSecureStateStore::new(&transaction)?;
        let owner_identity_id = required("owner_identity_id", &commit.record.owner_identity_id)?;
        let peer_did = required("peer_did", &commit.record.peer_did)?;
        let current_peer = store.get_session(&owner_identity_id, &peer_did)?;
        let revision_matches = match (&current_peer, commit.expected_peer_revision) {
            (None, None) => true,
            (Some(current), Some(expected)) => current.revision == expected,
            _ => false,
        };
        if !revision_matches {
            DirectInitSessionCommitResult::Stale {
                current: current_peer,
                expected_revision: commit.expected_peer_revision,
            }
        } else {
            let expected_revision = commit.expected_peer_revision.unwrap_or(0);
            match store.save_session_if_revision(&commit.record, expected_revision)? {
                DirectSessionCasResult::Saved(record) => {
                    DirectInitSessionCommitResult::Saved(record)
                }
                DirectSessionCasResult::Stale { current, .. } => {
                    DirectInitSessionCommitResult::Stale {
                        current,
                        expected_revision: commit.expected_peer_revision,
                    }
                }
            }
        }
    };
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(result)
}

pub(crate) struct AnpDirectSessionStore<'a> {
    owner_identity_id: String,
    owner_did: String,
    store: SqliteDirectSecureStateStore<'a>,
}

impl<'a> AnpDirectSessionStore<'a> {
    pub(crate) fn new(
        connection: &'a Connection,
        owner_identity_id: &str,
        owner_did: &str,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            owner_identity_id: required("owner_identity_id", owner_identity_id)?,
            owner_did: owner_did.trim().to_owned(),
            store: SqliteDirectSecureStateStore::new(connection)?,
        })
    }

    pub(crate) fn find_by_peer_did(
        &self,
        peer_did: &str,
    ) -> crate::ImResult<Option<anp::direct_e2ee::DirectSessionState>> {
        let Some(record) = self.store.get_session(&self.owner_identity_id, peer_did)? else {
            return Ok(None);
        };
        Ok(Some(
            direct_session_from_blob(&record.state_blob).map_err(direct_store_error_from_anp)?,
        ))
    }
}

impl anp::direct_e2ee::SessionStore for AnpDirectSessionStore<'_> {
    fn save_session(
        &mut self,
        session: &anp::direct_e2ee::DirectSessionState,
    ) -> Result<(), anp::direct_e2ee::DirectE2eeError> {
        let now = now_utc_like();
        let existing = self
            .store
            .get_session_by_id(&self.owner_identity_id, &session.session_id)
            .map_err(direct_store_error)?;
        self.store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: self.owner_identity_id.clone(),
                owner_did: self.owner_did.clone(),
                peer_did: session.peer_did.clone(),
                session_id: session.session_id.clone(),
                state_blob: direct_session_to_blob(session)?,
                metadata_json: direct_session_metadata_json(session)?,
                revision: existing.as_ref().map(|record| record.revision).unwrap_or(0),
                created_at: existing
                    .map(|record| record.created_at)
                    .unwrap_or_else(|| now.clone()),
                updated_at: now,
            })
            .map_err(direct_store_error)
    }

    fn load_session(
        &self,
        session_id: &str,
    ) -> Result<anp::direct_e2ee::DirectSessionState, anp::direct_e2ee::DirectE2eeError> {
        let record = self
            .store
            .get_session_by_id(&self.owner_identity_id, session_id)
            .map_err(direct_store_error)?
            .ok_or_else(|| {
                anp::direct_e2ee::DirectE2eeError::SessionNotFound(session_id.to_owned())
            })?;
        direct_session_from_blob(&record.state_blob)
    }

    fn delete_session(
        &mut self,
        session_id: &str,
    ) -> Result<(), anp::direct_e2ee::DirectE2eeError> {
        self.store
            .delete_session(&self.owner_identity_id, session_id)
            .map(|_| ())
            .map_err(direct_store_error)
    }
}

pub(crate) struct AnpDirectSignedPrekeyStore<'a> {
    owner_identity_id: String,
    owner_did: String,
    store: SqliteDirectSecureStateStore<'a>,
}

impl<'a> AnpDirectSignedPrekeyStore<'a> {
    pub(crate) fn new(
        connection: &'a Connection,
        owner_identity_id: &str,
        owner_did: &str,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            owner_identity_id: required("owner_identity_id", owner_identity_id)?,
            owner_did: owner_did.trim().to_owned(),
            store: SqliteDirectSecureStateStore::new(connection)?,
        })
    }

    pub(crate) fn load_latest_signed_prekey(
        &self,
    ) -> crate::ImResult<Option<(anp::PrivateKeyMaterial, anp::direct_e2ee::SignedPrekey)>> {
        let Some(record) = self.store.active_signed_prekey(&self.owner_identity_id)? else {
            return Ok(None);
        };
        Ok(Some((
            private_key_from_blob(&record.private_key_blob, "signed prekey")?,
            signed_prekey_metadata(&record.key_id, &record.metadata_json)?,
        )))
    }
}

impl anp::direct_e2ee::SignedPrekeyStore for AnpDirectSignedPrekeyStore<'_> {
    fn save_signed_prekey(
        &mut self,
        key_id: &str,
        private_key: &anp::PrivateKeyMaterial,
        metadata: &anp::direct_e2ee::SignedPrekey,
    ) -> Result<(), anp::direct_e2ee::DirectE2eeError> {
        let now = now_utc_like();
        let existing = self
            .store
            .get_signed_prekey(&self.owner_identity_id, key_id)
            .map_err(direct_store_error)?;
        self.store
            .upsert_signed_prekey(&DirectSignedPrekeyRecord {
                owner_identity_id: self.owner_identity_id.clone(),
                owner_did: self.owner_did.clone(),
                key_id: key_id.trim().to_owned(),
                private_key_blob: private_key.to_pem().into_bytes(),
                public_key_blob: metadata.public_key_b64u.as_bytes().to_vec(),
                status: DirectPrekeyStatus::Active,
                metadata_json: signed_prekey_metadata_json(metadata)?,
                created_at: existing
                    .map(|record| record.created_at)
                    .unwrap_or_else(|| now.clone()),
                updated_at: now,
            })
            .map_err(direct_store_error)
    }

    fn load_signed_prekey(
        &self,
        key_id: &str,
    ) -> Result<anp::PrivateKeyMaterial, anp::direct_e2ee::DirectE2eeError> {
        self.store
            .load_signed_prekey_material(&self.owner_identity_id, key_id)
            .map_err(direct_store_error)?
            .ok_or_else(|| {
                anp::direct_e2ee::DirectE2eeError::invalid_field(format!(
                    "signed prekey not found: {key_id}"
                ))
            })
    }
}

pub(crate) struct AnpDirectOneTimePrekeyStore<'a> {
    owner_identity_id: String,
    owner_did: String,
    store: SqliteDirectSecureStateStore<'a>,
}

impl<'a> AnpDirectOneTimePrekeyStore<'a> {
    pub(crate) fn new(
        connection: &'a Connection,
        owner_identity_id: &str,
        owner_did: &str,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            owner_identity_id: required("owner_identity_id", owner_identity_id)?,
            owner_did: owner_did.trim().to_owned(),
            store: SqliteDirectSecureStateStore::new(connection)?,
        })
    }

    pub(crate) fn save_one_time_prekey(
        &mut self,
        key_id: &str,
        private_key: &anp::PrivateKeyMaterial,
        metadata: &anp::direct_e2ee::OneTimePrekey,
    ) -> crate::ImResult<()> {
        let now = now_utc_like();
        self.store
            .upsert_one_time_prekey(&DirectOneTimePrekeyRecord {
                owner_identity_id: self.owner_identity_id.clone(),
                owner_did: self.owner_did.clone(),
                key_id: required("key_id", key_id)?,
                private_key_blob: private_key.to_pem().into_bytes(),
                public_key_blob: metadata.public_key_b64u.as_bytes().to_vec(),
                status: DirectPrekeyStatus::Available,
                metadata_json: one_time_prekey_metadata_json(metadata)?,
                created_at: now,
                consumed_at: String::new(),
            })
    }

    pub(crate) fn load_one_time_prekey(
        &self,
        key_id: &str,
    ) -> crate::ImResult<Option<DirectOneTimePrekeyMaterialRecord>> {
        self.store
            .load_one_time_prekey_material(&self.owner_identity_id, key_id)
    }

    pub(crate) fn list_one_time_prekeys(
        &self,
    ) -> crate::ImResult<Vec<anp::direct_e2ee::OneTimePrekey>> {
        self.store
            .list_available_one_time_prekeys(&self.owner_identity_id)
    }

    pub(crate) fn mark_consumed(
        &mut self,
        key_id: &str,
        consumed_at: &str,
    ) -> crate::ImResult<bool> {
        self.store
            .mark_one_time_prekey_consumed(&self.owner_identity_id, key_id, consumed_at)
    }
}

fn direct_store_error(error: crate::ImError) -> anp::direct_e2ee::DirectE2eeError {
    anp::direct_e2ee::DirectE2eeError::invalid_field(format!("sqlite direct e2ee store: {error}"))
}

fn direct_store_error_from_anp(error: anp::direct_e2ee::DirectE2eeError) -> crate::ImError {
    crate::ImError::Serialization {
        detail: format!("sqlite direct e2ee store: {error}"),
    }
}

pub(crate) fn direct_session_to_blob(
    session: &anp::direct_e2ee::DirectSessionState,
) -> Result<Vec<u8>, anp::direct_e2ee::DirectE2eeError> {
    serde_json::to_vec(session).map_err(|err| {
        anp::direct_e2ee::DirectE2eeError::invalid_field(format!("serialize direct session: {err}"))
    })
}

pub(crate) fn direct_session_from_blob(
    blob: &[u8],
) -> Result<anp::direct_e2ee::DirectSessionState, anp::direct_e2ee::DirectE2eeError> {
    serde_json::from_slice(blob).map_err(|err| {
        anp::direct_e2ee::DirectE2eeError::invalid_field(format!("parse direct session: {err}"))
    })
}

pub(crate) fn direct_session_metadata_json(
    session: &anp::direct_e2ee::DirectSessionState,
) -> Result<String, anp::direct_e2ee::DirectE2eeError> {
    serde_json::to_string(&serde_json::json!({
        "version": DIRECT_SESSION_METADATA_VERSION,
        "suite": session.suite,
        "status": session.status,
        "is_initiator": session.is_initiator,
    }))
    .map_err(|err| {
        anp::direct_e2ee::DirectE2eeError::invalid_field(format!(
            "serialize direct session metadata: {err}"
        ))
    })
}

fn signed_prekey_metadata_json(
    metadata: &anp::direct_e2ee::SignedPrekey,
) -> Result<String, anp::direct_e2ee::DirectE2eeError> {
    prekey_metadata_json(metadata)
}

fn one_time_prekey_metadata_json(
    metadata: &anp::direct_e2ee::OneTimePrekey,
) -> crate::ImResult<String> {
    prekey_metadata_json(metadata).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn prekey_metadata_json<T: serde::Serialize>(
    metadata: &T,
) -> Result<String, anp::direct_e2ee::DirectE2eeError> {
    serde_json::to_string(&serde_json::json!({
        "version": DIRECT_PREKEY_METADATA_VERSION,
        "metadata": metadata,
    }))
    .map_err(|err| {
        anp::direct_e2ee::DirectE2eeError::invalid_field(format!(
            "serialize direct prekey metadata: {err}"
        ))
    })
}

fn signed_prekey_metadata(
    key_id: &str,
    metadata_json: &str,
) -> crate::ImResult<anp::direct_e2ee::SignedPrekey> {
    if let Some(metadata) = parse_prekey_metadata::<anp::direct_e2ee::SignedPrekey>(metadata_json)?
    {
        return Ok(metadata);
    }
    Ok(anp::direct_e2ee::SignedPrekey {
        key_id: key_id.to_owned(),
        public_key_b64u: String::new(),
        expires_at: String::new(),
    })
}

fn one_time_prekey_metadata(
    key_id: &str,
    metadata_json: &str,
) -> crate::ImResult<anp::direct_e2ee::OneTimePrekey> {
    if let Some(metadata) = parse_prekey_metadata::<anp::direct_e2ee::OneTimePrekey>(metadata_json)?
    {
        return Ok(metadata);
    }
    Ok(anp::direct_e2ee::OneTimePrekey {
        key_id: key_id.to_owned(),
        public_key_b64u: String::new(),
    })
}

fn parse_prekey_metadata<T: serde::de::DeserializeOwned>(
    metadata_json: &str,
) -> crate::ImResult<Option<T>> {
    if metadata_json.trim().is_empty() {
        return Ok(None);
    }
    let value = serde_json::from_str::<serde_json::Value>(metadata_json).map_err(|err| {
        crate::ImError::Serialization {
            detail: format!("parse direct prekey metadata: {err}"),
        }
    })?;
    let metadata = value.get("metadata").cloned().unwrap_or(value);
    serde_json::from_value(metadata)
        .map(Some)
        .map_err(|err| crate::ImError::Serialization {
            detail: format!("parse direct prekey metadata object: {err}"),
        })
}

fn private_key_from_blob(blob: &[u8], field: &str) -> crate::ImResult<anp::PrivateKeyMaterial> {
    let pem = std::str::from_utf8(blob).map_err(|err| crate::ImError::Serialization {
        detail: format!("parse {field} private key utf8: {err}"),
    })?;
    anp::PrivateKeyMaterial::from_pem(pem).map_err(|err| crate::ImError::Serialization {
        detail: format!("parse {field} private key: {err}"),
    })
}

fn now_utc_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}

fn required(field: &str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_owned())
}

fn nullable_text(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn optional_string(value: Option<String>) -> String {
    value.unwrap_or_default()
}

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DirectSessionRecord> {
    Ok(DirectSessionRecord {
        owner_identity_id: row.get("owner_identity_id")?,
        owner_did: row.get("owner_did")?,
        peer_did: row.get("peer_did")?,
        session_id: row.get("session_id")?,
        state_blob: row.get("state_blob")?,
        metadata_json: optional_string(row.get("metadata_json")?),
        revision: row.get::<_, Option<i64>>("revision")?.unwrap_or(0),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn signed_prekey_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DirectSignedPrekeyRecord> {
    Ok(DirectSignedPrekeyRecord {
        owner_identity_id: row.get("owner_identity_id")?,
        owner_did: row.get("owner_did")?,
        key_id: row.get("key_id")?,
        private_key_blob: row.get("private_key_blob")?,
        public_key_blob: row
            .get::<_, Option<Vec<u8>>>("public_key_blob")?
            .unwrap_or_default(),
        status: DirectPrekeyStatus::from_str(&row.get::<_, String>("status")?),
        metadata_json: optional_string(row.get("metadata_json")?),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn one_time_prekey_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<DirectOneTimePrekeyRecord> {
    Ok(DirectOneTimePrekeyRecord {
        owner_identity_id: row.get("owner_identity_id")?,
        owner_did: row.get("owner_did")?,
        key_id: row.get("key_id")?,
        private_key_blob: row.get("private_key_blob")?,
        public_key_blob: row
            .get::<_, Option<Vec<u8>>>("public_key_blob")?
            .unwrap_or_default(),
        status: DirectPrekeyStatus::from_str(&row.get::<_, String>("status")?),
        metadata_json: optional_string(row.get("metadata_json")?),
        created_at: row.get("created_at")?,
        consumed_at: optional_string(row.get("consumed_at")?),
    })
}

#[cfg(test)]
mod tests {
    use super::super::secret_store::is_direct_secret_envelope;
    use super::*;
    use anp::direct_e2ee::{SessionStore as _, SignedPrekeyStore as _};

    #[test]
    fn sqlite_store_upserts_and_reads_sessions_by_owner_identity() {
        let db = Connection::open_in_memory().unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();

        store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:alice".to_owned(),
                peer_did: "did:bob".to_owned(),
                session_id: "session-1".to_owned(),
                state_blob: b"state-v1".to_vec(),
                metadata_json: r#"{"version":1}"#.to_owned(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:01Z".to_owned(),
            })
            .unwrap();
        store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "charlie-id".to_owned(),
                owner_did: "did:charlie".to_owned(),
                peer_did: "did:bob".to_owned(),
                session_id: "session-2".to_owned(),
                state_blob: b"other-owner-state".to_vec(),
                metadata_json: String::new(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:02Z".to_owned(),
            })
            .unwrap();

        let alice = store
            .get_session("alice-id", "did:bob")
            .unwrap()
            .expect("alice session");
        assert_eq!(alice.session_id, "session-1");
        assert_eq!(alice.state_blob, b"state-v1");
        assert_eq!(alice.metadata_json, r#"{"version":1}"#);
        assert_eq!(alice.revision, 0);
        assert_eq!(
            store.get_session_by_id("charlie-id", "session-1").unwrap(),
            None
        );
        assert_eq!(
            store
                .get_session_by_id("alice-id", "session-1")
                .unwrap()
                .expect("session by id")
                .peer_did,
            "did:bob"
        );
    }

    #[test]
    fn sqlite_store_encrypts_direct_secret_blobs_at_rest() {
        let db = Connection::open_in_memory().unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();
        let session_secret = b"session-plaintext-secret".to_vec();
        let signed_secret = b"signed-prekey-plaintext-secret".to_vec();
        let one_time_secret = b"one-time-prekey-plaintext-secret".to_vec();

        store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:alice".to_owned(),
                peer_did: "did:bob".to_owned(),
                session_id: "session-1".to_owned(),
                state_blob: session_secret.clone(),
                metadata_json: "{}".to_owned(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:01Z".to_owned(),
            })
            .unwrap();
        store
            .upsert_signed_prekey(&DirectSignedPrekeyRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:alice".to_owned(),
                key_id: "spk-1".to_owned(),
                private_key_blob: signed_secret.clone(),
                public_key_blob: b"spk-public".to_vec(),
                status: DirectPrekeyStatus::Active,
                metadata_json: "{}".to_owned(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:01Z".to_owned(),
            })
            .unwrap();
        store
            .upsert_one_time_prekey(&DirectOneTimePrekeyRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:alice".to_owned(),
                key_id: "otk-1".to_owned(),
                private_key_blob: one_time_secret.clone(),
                public_key_blob: b"otk-public".to_vec(),
                status: DirectPrekeyStatus::Available,
                metadata_json: "{}".to_owned(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                consumed_at: String::new(),
            })
            .unwrap();

        let raw_session: Vec<u8> = db
            .query_row(
                "SELECT state_blob FROM direct_e2ee_sessions WHERE owner_identity_id = 'alice-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let raw_signed: Vec<u8> = db
            .query_row(
                "SELECT private_key_blob FROM direct_e2ee_signed_prekeys WHERE owner_identity_id = 'alice-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let raw_one_time: Vec<u8> = db
            .query_row(
                "SELECT private_key_blob FROM direct_e2ee_one_time_prekeys WHERE owner_identity_id = 'alice-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(is_direct_secret_envelope(&raw_session));
        assert!(is_direct_secret_envelope(&raw_signed));
        assert!(is_direct_secret_envelope(&raw_one_time));
        assert_ne!(raw_session, session_secret);
        assert_ne!(raw_signed, signed_secret);
        assert_ne!(raw_one_time, one_time_secret);
        assert!(!String::from_utf8_lossy(&raw_session).contains("session-plaintext-secret"));
        assert!(!String::from_utf8_lossy(&raw_signed).contains("signed-prekey-plaintext-secret"));
        assert!(
            !String::from_utf8_lossy(&raw_one_time).contains("one-time-prekey-plaintext-secret")
        );

        assert_eq!(
            store
                .get_session("alice-id", "did:bob")
                .unwrap()
                .expect("session")
                .state_blob,
            session_secret
        );
        assert_eq!(
            store
                .get_signed_prekey("alice-id", "spk-1")
                .unwrap()
                .expect("signed prekey")
                .private_key_blob,
            signed_secret
        );
        assert_eq!(
            store
                .get_one_time_prekey("alice-id", "otk-1")
                .unwrap()
                .expect("one-time prekey")
                .private_key_blob,
            one_time_secret
        );
    }

    #[test]
    fn sqlite_store_without_secret_vault_rejects_new_direct_secret_writes() {
        let db = Connection::open_in_memory().unwrap();
        let store = SqliteDirectSecureStateStore::new_without_secret_vault_for_legacy(&db).unwrap();

        let err = store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:alice".to_owned(),
                peer_did: "did:bob".to_owned(),
                session_id: "session-1".to_owned(),
                state_blob: b"session-secret".to_vec(),
                metadata_json: "{}".to_owned(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:01Z".to_owned(),
            })
            .expect_err("new direct secret write must require vault");

        assert!(err.to_string().contains("refusing plaintext fallback"));
    }

    #[test]
    fn sqlite_store_without_secret_vault_reads_legacy_plaintext_blobs() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO direct_e2ee_sessions
    (owner_identity_id, owner_did, peer_did, session_id, state_blob, metadata_json, revision, created_at, updated_at)
VALUES ('alice-id', 'did:alice', 'did:bob', 'session-1', ?1, '{}', 0, '2026-05-24T00:00:00Z', '2026-05-24T00:00:01Z')
"#,
            params![b"legacy-session-secret".to_vec()],
        )
        .unwrap();

        let store = SqliteDirectSecureStateStore::new_without_secret_vault_for_legacy(&db).unwrap();
        let loaded = store
            .get_session("alice-id", "did:bob")
            .unwrap()
            .expect("legacy session");

        assert_eq!(loaded.state_blob, b"legacy-session-secret");
    }

    #[test]
    fn sqlite_store_without_secret_vault_rejects_vault_envelope_reads() {
        let db = Connection::open_in_memory().unwrap();
        let vault_store = SqliteDirectSecureStateStore::new(&db).unwrap();
        vault_store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:alice".to_owned(),
                peer_did: "did:bob".to_owned(),
                session_id: "session-1".to_owned(),
                state_blob: b"session-secret".to_vec(),
                metadata_json: "{}".to_owned(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:01Z".to_owned(),
            })
            .unwrap();
        let no_vault_store =
            SqliteDirectSecureStateStore::new_without_secret_vault_for_legacy(&db).unwrap();

        let err = no_vault_store
            .get_session("alice-id", "did:bob")
            .expect_err("envelope read must require vault");

        assert!(err
            .to_string()
            .contains("requires AWIKI_IM_CORE_VAULT_ROOT_KEY_B64"));
    }

    #[test]
    fn sqlite_store_cas_rejects_stale_direct_session_updates() {
        let db = Connection::open_in_memory().unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();
        store
            .upsert_session(&DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:alice".to_owned(),
                peer_did: "did:bob".to_owned(),
                session_id: "session-1".to_owned(),
                state_blob: b"state-v1".to_vec(),
                metadata_json: "{}".to_owned(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:01Z".to_owned(),
            })
            .unwrap();

        let loaded = store
            .get_session("alice-id", "did:bob")
            .unwrap()
            .expect("session");
        let mut first_update = loaded.clone();
        first_update.state_blob = b"state-v2".to_vec();
        first_update.updated_at = "2026-05-24T00:00:02Z".to_owned();
        let first = store
            .save_session_if_revision(&first_update, loaded.revision)
            .unwrap();
        let DirectSessionCasResult::Saved(saved) = first else {
            panic!("first update should save");
        };
        assert_eq!(saved.revision, loaded.revision + 1);
        assert_eq!(saved.state_blob, b"state-v2");

        let mut stale_update = loaded;
        stale_update.state_blob = b"stale-state".to_vec();
        stale_update.updated_at = "2026-05-24T00:00:03Z".to_owned();
        let stale = store
            .save_session_if_revision(&stale_update, stale_update.revision)
            .unwrap();
        let DirectSessionCasResult::Stale {
            current,
            expected_revision,
        } = stale
        else {
            panic!("second update should be stale");
        };
        assert_eq!(expected_revision, 0);
        let current = current.expect("current row");
        assert_eq!(current.revision, 1);
        assert_eq!(current.state_blob, b"state-v2");
        assert_ne!(current.state_blob, b"stale-state");
    }

    #[test]
    fn sqlite_store_keeps_signed_prekeys_owner_scoped() {
        let db = Connection::open_in_memory().unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();

        store
            .upsert_signed_prekey(&signed_prekey(
                "alice-id",
                "did:alice",
                "spk-1",
                DirectPrekeyStatus::Active,
                "2026-05-24T00:00:01Z",
            ))
            .unwrap();
        store
            .upsert_signed_prekey(&signed_prekey(
                "alice-id",
                "did:alice",
                "spk-old",
                DirectPrekeyStatus::Retired,
                "2026-05-23T00:00:01Z",
            ))
            .unwrap();
        store
            .upsert_signed_prekey(&signed_prekey(
                "charlie-id",
                "did:charlie",
                "spk-1",
                DirectPrekeyStatus::Active,
                "2026-05-24T00:00:02Z",
            ))
            .unwrap();

        assert_eq!(
            store
                .get_signed_prekey("alice-id", "spk-1")
                .unwrap()
                .expect("alice key")
                .owner_did,
            "did:alice"
        );
        assert_eq!(
            store
                .active_signed_prekey("alice-id")
                .unwrap()
                .expect("active key")
                .key_id,
            "spk-1"
        );
        assert_eq!(
            store.get_signed_prekey("charlie-id", "spk-old").unwrap(),
            None
        );
    }

    #[test]
    fn sqlite_store_reserves_and_consumes_one_time_prekeys_by_owner() {
        let db = Connection::open_in_memory().unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();

        store
            .upsert_one_time_prekey(&one_time_prekey(
                "alice-id",
                "did:alice",
                "otk-2",
                DirectPrekeyStatus::Available,
                "2026-05-24T00:00:02Z",
            ))
            .unwrap();
        store
            .upsert_one_time_prekey(&one_time_prekey(
                "alice-id",
                "did:alice",
                "otk-1",
                DirectPrekeyStatus::Available,
                "2026-05-24T00:00:01Z",
            ))
            .unwrap();
        store
            .upsert_one_time_prekey(&one_time_prekey(
                "charlie-id",
                "did:charlie",
                "otk-0",
                DirectPrekeyStatus::Available,
                "2026-05-23T00:00:01Z",
            ))
            .unwrap();

        let reserved = store
            .reserve_next_one_time_prekey("alice-id", "2026-05-24T00:01:00Z")
            .unwrap()
            .expect("reserved key");
        assert_eq!(reserved.key_id, "otk-1");
        assert_eq!(reserved.status, DirectPrekeyStatus::Reserved);
        assert_eq!(reserved.consumed_at, "2026-05-24T00:01:00Z");
        assert_eq!(
            store
                .get_one_time_prekey("charlie-id", "otk-0")
                .unwrap()
                .expect("charlie key")
                .status,
            DirectPrekeyStatus::Available
        );

        assert!(store
            .mark_one_time_prekey_consumed("alice-id", "otk-1", "2026-05-24T00:02:00Z")
            .unwrap());
        let consumed = store
            .get_one_time_prekey("alice-id", "otk-1")
            .unwrap()
            .expect("consumed key");
        assert_eq!(consumed.status, DirectPrekeyStatus::Consumed);
        assert_eq!(consumed.consumed_at, "2026-05-24T00:02:00Z");
    }

    #[test]
    fn sqlite_store_rejects_empty_owner_identity() {
        let db = Connection::open_in_memory().unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();

        let err = store
            .get_session(" ", "did:bob")
            .expect_err("empty owner must fail");
        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(field),
                ..
            } if field == "owner_identity_id"
        ));
    }

    #[test]
    fn anp_session_store_persists_direct_session_state_in_sqlite() {
        let db = Connection::open_in_memory().unwrap();
        let session = direct_session("session-1", "did:bob");

        let mut store = AnpDirectSessionStore::new(&db, "alice-id", "did:alice").unwrap();
        store.save_session(&session).unwrap();

        assert_eq!(store.load_session("session-1").unwrap(), session);
        assert_eq!(
            store.find_by_peer_did("did:bob").unwrap().expect("by peer"),
            session
        );
        store.delete_session("session-1").unwrap();
        assert!(matches!(
            store.load_session("session-1"),
            Err(anp::direct_e2ee::DirectE2eeError::SessionNotFound(value)) if value == "session-1"
        ));
    }

    #[test]
    fn anp_signed_prekey_store_persists_private_material_and_metadata() {
        let db = Connection::open_in_memory().unwrap();
        let private_key = x25519_private_key();
        let metadata = anp::direct_e2ee::SignedPrekey {
            key_id: "spk-1".to_owned(),
            public_key_b64u: "signed-public".to_owned(),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
        };

        let mut store = AnpDirectSignedPrekeyStore::new(&db, "alice-id", "did:alice").unwrap();
        store
            .save_signed_prekey("spk-1", &private_key, &metadata)
            .unwrap();

        assert_eq!(
            store.load_signed_prekey("spk-1").unwrap().to_pem(),
            private_key.to_pem()
        );
        let (latest_private, latest_metadata) = store
            .load_latest_signed_prekey()
            .unwrap()
            .expect("latest key");
        assert_eq!(latest_private.to_pem(), private_key.to_pem());
        assert_eq!(latest_metadata, metadata);
    }

    #[test]
    fn anp_one_time_prekey_store_lists_and_consumes_available_keys() {
        let db = Connection::open_in_memory().unwrap();
        let private_key = x25519_private_key();
        let metadata = anp::direct_e2ee::OneTimePrekey {
            key_id: "otk-1".to_owned(),
            public_key_b64u: "otk-public".to_owned(),
        };

        let mut store = AnpDirectOneTimePrekeyStore::new(&db, "alice-id", "did:alice").unwrap();
        store
            .save_one_time_prekey("otk-1", &private_key, &metadata)
            .unwrap();

        assert_eq!(
            store.list_one_time_prekeys().unwrap(),
            vec![metadata.clone()]
        );
        let loaded = store
            .load_one_time_prekey("otk-1")
            .unwrap()
            .expect("otk material");
        assert_eq!(loaded.private_key.to_pem(), private_key.to_pem());
        assert_eq!(loaded.metadata, metadata);
        assert!(store
            .mark_consumed("otk-1", "2026-05-24T00:00:00Z")
            .unwrap());
        assert!(store.list_one_time_prekeys().unwrap().is_empty());
    }

    fn signed_prekey(
        owner_identity_id: &str,
        owner_did: &str,
        key_id: &str,
        status: DirectPrekeyStatus,
        updated_at: &str,
    ) -> DirectSignedPrekeyRecord {
        DirectSignedPrekeyRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            key_id: key_id.to_owned(),
            private_key_blob: format!("private-{key_id}").into_bytes(),
            public_key_blob: format!("public-{key_id}").into_bytes(),
            status,
            metadata_json: String::new(),
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: updated_at.to_owned(),
        }
    }

    fn one_time_prekey(
        owner_identity_id: &str,
        owner_did: &str,
        key_id: &str,
        status: DirectPrekeyStatus,
        created_at: &str,
    ) -> DirectOneTimePrekeyRecord {
        DirectOneTimePrekeyRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            key_id: key_id.to_owned(),
            private_key_blob: format!("private-{key_id}").into_bytes(),
            public_key_blob: format!("public-{key_id}").into_bytes(),
            status,
            metadata_json: String::new(),
            created_at: created_at.to_owned(),
            consumed_at: String::new(),
        }
    }

    fn direct_session(session_id: &str, peer_did: &str) -> anp::direct_e2ee::DirectSessionState {
        anp::direct_e2ee::DirectSessionState {
            session_id: session_id.to_owned(),
            suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_owned(),
            peer_did: peer_did.to_owned(),
            local_key_agreement_id: "did:alice#key-3".to_owned(),
            peer_key_agreement_id: "did:bob#key-3".to_owned(),
            root_key_b64u: "root".to_owned(),
            send_chain_key_b64u: Some("send-chain".to_owned()),
            recv_chain_key_b64u: Some("recv-chain".to_owned()),
            ratchet_private_key_b64u: "ratchet-private".to_owned(),
            ratchet_public_key_b64u: "ratchet-public".to_owned(),
            peer_ratchet_public_key_b64u: Some("peer-ratchet".to_owned()),
            send_n: 1,
            recv_n: 2,
            previous_send_chain_length: 0,
            skipped_message_keys: Vec::new(),
            is_initiator: true,
            status: "established".to_owned(),
        }
    }

    fn x25519_private_key() -> anp::PrivateKeyMaterial {
        let bundle = anp::authentication::create_did_wba_document(
            "awiki.ai",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        bundle.load_private_key("key-3").unwrap()
    }
}
