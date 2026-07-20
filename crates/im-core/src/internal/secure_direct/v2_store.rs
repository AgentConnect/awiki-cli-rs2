//! Product-owned persistence boundary for P5 v2 device sessions.
//!
//! P5 v2 records deliberately use separate tables and Vault kinds from the
//! legacy Direct runtime.  A row is always scoped by the local identity and
//! protocol device as well as the exact peer DID/device pair.

use anp::direct_e2ee::{
    deserialize_pending_outbound_v2, deserialize_session_state_v2, serialize_pending_outbound_v2,
    serialize_session_state_v2, V2DirectSessionState, V2OneTimePrekey, V2PendingOutboundRecord,
    V2PrekeyBundle, V2SessionBinding,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zeroize::Zeroizing;

use super::secret_store::{
    default_direct_secret_vault, direct_secret_key_id, direct_secret_key_id_prefix,
    direct_secret_ref_from_blob, open_direct_secret_blob_strict, seal_direct_secret_blob,
    validate_direct_secret_ref, DirectSecretOpenExpectation, DirectSecretSealInput,
    DirectSecretVault,
};
use crate::vault::{SecretKind, SecretRef};

const V2_SIGNED_PREKEY_MATERIAL_PREFIX: &[u8] = b"awiki-p5-v2-signed-prekey:v1\0";

pub(crate) const DIRECT_E2EE_V2_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS direct_e2ee_v2_owner_scopes (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, local_device_id),
    UNIQUE (owner_did, local_device_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_sessions (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    peer_did          TEXT NOT NULL,
    peer_device_id    TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    state_blob        BLOB NOT NULL,
    revision          INTEGER NOT NULL DEFAULT 0,
    disabled          INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (
        owner_identity_id, local_device_id, peer_did, peer_device_id, session_id
    )
);

CREATE INDEX IF NOT EXISTS idx_direct_e2ee_v2_session_selection
ON direct_e2ee_v2_sessions (
    owner_identity_id, local_device_id, peer_did, peer_device_id,
    disabled, updated_at DESC
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_pending (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    peer_did          TEXT NOT NULL,
    peer_device_id    TEXT NOT NULL,
    operation_id      TEXT NOT NULL,
    message_id        TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    session_revision  INTEGER NOT NULL DEFAULT 0,
    pending_blob      BLOB NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, local_device_id, operation_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_private_outbound (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    operation_id      TEXT NOT NULL,
    delivery_class    TEXT NOT NULL,
    context_json      TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    accepted_at       TEXT,
    failure_code      TEXT,
    completed_at      TEXT,
    PRIMARY KEY (owner_identity_id, local_device_id, operation_id),
    FOREIGN KEY (owner_identity_id, local_device_id, operation_id)
      REFERENCES direct_e2ee_v2_pending (
        owner_identity_id, local_device_id, operation_id
      ) ON DELETE CASCADE
);

-- A terminal control operation retains only secret-free status. Its exact
-- retry ciphertext and pending ratchet state are deleted from SQLite/Vault.
CREATE TABLE IF NOT EXISTS direct_e2ee_v2_private_outbound_tombstones (
    owner_identity_id   TEXT NOT NULL,
    owner_did           TEXT NOT NULL,
    local_device_id     TEXT NOT NULL,
    operation_id        TEXT NOT NULL,
    sender_device_id    TEXT NOT NULL,
    recipient_device_id TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    accepted_at         TEXT,
    completed_at        TEXT NOT NULL,
    terminal_phase      TEXT NOT NULL DEFAULT 'completed',
    failure_code        TEXT,
    PRIMARY KEY (owner_identity_id, local_device_id, operation_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_replay (
    owner_identity_id TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    sender_did        TEXT NOT NULL,
    sender_device_id  TEXT NOT NULL,
    message_id        TEXT NOT NULL,
    ciphertext_digest TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    processed_at      TEXT NOT NULL,
    PRIMARY KEY (
        owner_identity_id, local_device_id, sender_did, sender_device_id, message_id
    )
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_prekey_bundles (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    bundle_id         TEXT NOT NULL,
    bundle_json       TEXT NOT NULL,
    signed_prekey_private_blob BLOB NOT NULL,
    status            TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, local_device_id, bundle_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_one_time_prekeys (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    bundle_id         TEXT NOT NULL,
    key_id            TEXT NOT NULL,
    public_json       TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    status            TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    consumed_at       TEXT,
    PRIMARY KEY (owner_identity_id, local_device_id, key_id)
);

CREATE INDEX IF NOT EXISTS idx_direct_e2ee_v2_opk_available
ON direct_e2ee_v2_one_time_prekeys (
    owner_identity_id, local_device_id, bundle_id, status, created_at
);
"#;

pub(crate) fn ensure_v2_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(DIRECT_E2EE_V2_SCHEMA)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    ensure_pending_session_revision_column(connection)?;
    ensure_private_outbound_accepted_column(connection)?;
    ensure_private_outbound_status_columns(connection)?;
    ensure_private_outbound_tombstone_columns(connection)?;
    ensure_opk_bundle_id_column(connection)
}

fn ensure_pending_session_revision_column(connection: &Connection) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(direct_e2ee_v2_pending)")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if columns.iter().any(|column| column == "session_revision") {
        return Ok(());
    }
    connection
        .execute(
            "ALTER TABLE direct_e2ee_v2_pending ADD COLUMN session_revision INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

fn ensure_private_outbound_accepted_column(connection: &Connection) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(direct_e2ee_v2_private_outbound)")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if columns.iter().any(|column| column == "accepted_at") {
        return Ok(());
    }
    connection
        .execute(
            "ALTER TABLE direct_e2ee_v2_private_outbound ADD COLUMN accepted_at TEXT",
            [],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

fn ensure_private_outbound_status_columns(connection: &Connection) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(direct_e2ee_v2_private_outbound)")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    drop(statement);
    for (column, definition) in [
        ("failure_code", "failure_code TEXT"),
        ("completed_at", "completed_at TEXT"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE direct_e2ee_v2_private_outbound ADD COLUMN {definition}"),
                    [],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
        }
    }
    Ok(())
}

fn ensure_private_outbound_tombstone_columns(connection: &Connection) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(direct_e2ee_v2_private_outbound_tombstones)")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    drop(statement);
    for (column, definition) in [
        (
            "terminal_phase",
            "terminal_phase TEXT NOT NULL DEFAULT 'completed'",
        ),
        ("failure_code", "failure_code TEXT"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!(
                        "ALTER TABLE direct_e2ee_v2_private_outbound_tombstones ADD COLUMN {definition}"
                    ),
                    [],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
        }
    }
    Ok(())
}

fn ensure_opk_bundle_id_column(connection: &Connection) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(direct_e2ee_v2_one_time_prekeys)")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if columns.iter().any(|column| column == "bundle_id") {
        return Ok(());
    }
    connection
        .execute(
            "ALTER TABLE direct_e2ee_v2_one_time_prekeys ADD COLUMN bundle_id TEXT NOT NULL DEFAULT ''",
            [],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum V2InboundCommit {
    Applied,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V2SessionExpectation {
    Absent,
    Revision(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct V2StoredSession {
    pub(crate) state: V2DirectSessionState,
    pub(crate) revision: i64,
}

/// Closed, same-domain transport metadata paired with an exact persisted P5
/// v2 ciphertext. Only the explicitly modeled root-control fields can be
/// stored here; encrypted inner plaintext and private key material have no
/// representable field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct V2PrivateOutboundSidecar {
    operation_id: String,
    context: V2RootControlPrivateContext,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct V2RootControlPrivateContext {
    transport_context: crate::internal::identity_root_transfer::RootImportTransportContext,
    completion: Option<crate::internal::identity_root_transfer::RootKeyImportedCompletion>,
}

impl V2PrivateOutboundSidecar {
    pub(crate) fn root_control(
        operation_id: &str,
        transport_context: crate::internal::identity_root_transfer::RootImportTransportContext,
        completion: Option<crate::internal::identity_root_transfer::RootKeyImportedCompletion>,
    ) -> crate::ImResult<Self> {
        let sidecar = Self {
            operation_id: operation_id.to_owned(),
            context: V2RootControlPrivateContext {
                transport_context,
                completion,
            },
        };
        sidecar.validate()?;
        Ok(sidecar)
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn delivery_class(&self) -> &'static str {
        crate::internal::identity_root_transfer::ROOT_KEY_CONTROL_DELIVERY_CLASS
    }

    pub(crate) fn root_control_context(
        &self,
    ) -> (
        &crate::internal::identity_root_transfer::RootImportTransportContext,
        Option<&crate::internal::identity_root_transfer::RootKeyImportedCompletion>,
    ) {
        (
            &self.context.transport_context,
            self.context.completion.as_ref(),
        )
    }

    fn validate(&self) -> crate::ImResult<()> {
        if self.operation_id.trim().is_empty() || self.operation_id.len() > 2 * 1024 {
            return Err(crate::ImError::PermissionDenied);
        }
        self.context.transport_context.validate()
    }

    fn validate_binding(&self, binding: &V2SessionBinding) -> crate::ImResult<()> {
        self.validate()?;
        let context = &self.context.transport_context;
        let matches = if self.context.completion.is_some() {
            binding.local_device_id == context.recipient_device_id
                && binding.peer_device_id == context.sender_device_id
        } else {
            binding.local_device_id == context.sender_device_id
                && binding.peer_device_id == context.recipient_device_id
        };
        if !matches {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct V2OwnerScope {
    owner_identity_id: String,
    owner_did: String,
    local_device_id: String,
}

impl V2OwnerScope {
    /// Creates the P5 v2 storage scope only from the selected local identity's
    /// already-validated vNext authorization projection.
    pub(crate) fn from_identity_state(
        owner_identity_id: &crate::ids::IdentityId,
        owner_did: &crate::ids::Did,
        state: &crate::internal::identity_device_state::IdentityDeviceState,
    ) -> crate::ImResult<Self> {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationStatus, IdentityDeviceMode,
        };

        state.validate_for_did(owner_did)?;
        let authorization = state
            .authorization
            .as_ref()
            .filter(|_| state.mode == IdentityDeviceMode::VNext)
            .filter(|authorization| authorization.status == DeviceAuthorizationStatus::Active)
            .ok_or(crate::ImError::PermissionDenied)?;
        Ok(Self {
            owner_identity_id: owner_identity_id.as_str().to_owned(),
            owner_did: owner_did.as_str().to_owned(),
            local_device_id: authorization.protocol_device_id.as_str().to_owned(),
        })
    }

    fn validate_binding(&self, binding: &V2SessionBinding) -> crate::ImResult<()> {
        binding.validate().map_err(v2_error)?;
        if binding.local_did != self.owner_did || binding.local_device_id != self.local_device_id {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn validate_bundle(&self, bundle: &V2PrekeyBundle) -> crate::ImResult<()> {
        bundle.validate_structure().map_err(v2_error)?;
        if bundle.owner_did != self.owner_did || bundle.owner_device_id != self.local_device_id {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

pub(crate) struct V2LocalBundleMaterial {
    pub(crate) bundle: V2PrekeyBundle,
    pub(crate) signed_prekey_private: x25519_dalek::StaticSecret,
    /// The immutable public OPK batch used by the original publish request.
    /// It is retained with the signed-prekey material so retries remain byte
    /// identical after individual local OPKs have been consumed and deleted.
    pub(crate) published_one_time_prekeys: Option<Vec<V2OneTimePrekey>>,
}

pub(crate) struct V2LocalOneTimePrekeyMaterial {
    pub(crate) public: V2OneTimePrekey,
    pub(crate) private: x25519_dalek::StaticSecret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V2PrivateOutboundPhase {
    PendingDelivery,
    AwaitingImport,
    Importing,
    Failed,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct V2PrivateOutboundStatus {
    pub(crate) operation_id: String,
    pub(crate) sender_device_id: String,
    pub(crate) recipient_device_id: String,
    pub(crate) phase: V2PrivateOutboundPhase,
    pub(crate) created_at: String,
    pub(crate) accepted_at: Option<String>,
    pub(crate) completed_at: Option<String>,
    pub(crate) retryable: bool,
}

pub(crate) struct SqliteV2DirectStateStore<'a> {
    connection: &'a Connection,
    secret_vault: DirectSecretVault,
    scope: V2OwnerScope,
}

impl<'a> SqliteV2DirectStateStore<'a> {
    pub(crate) fn new(connection: &'a Connection, scope: V2OwnerScope) -> crate::ImResult<Self> {
        crate::internal::local_state::schema::ensure_schema(connection)?;
        ensure_v2_schema(connection)?;
        let secret_vault = default_direct_secret_vault(
            super::sqlite_store::direct_secret_vault_dir_for_connection(connection)?,
        )?
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "P5 v2 state requires the im-core secret vault".to_owned(),
        })?;
        let store = Self {
            connection,
            secret_vault,
            scope,
        };
        store.ensure_authoritative_scope()?;
        store.terminalize_expired_private_outbound(&chrono::Utc::now().to_rfc3339())?;
        store.gc_orphaned_v2_secrets()?;
        Ok(store)
    }

    pub(crate) fn new_with_secret_vault(
        connection: &'a Connection,
        secret_vault: DirectSecretVault,
        scope: V2OwnerScope,
    ) -> crate::ImResult<Self> {
        crate::internal::local_state::schema::ensure_schema(connection)?;
        ensure_v2_schema(connection)?;
        let store = Self {
            connection,
            secret_vault,
            scope,
        };
        store.ensure_authoritative_scope()?;
        store.terminalize_expired_private_outbound(&chrono::Utc::now().to_rfc3339())?;
        store.gc_orphaned_v2_secrets()?;
        Ok(store)
    }

    fn ensure_authoritative_scope(&self) -> crate::ImResult<()> {
        self.connection
            .execute(
                r#"INSERT OR IGNORE INTO direct_e2ee_v2_owner_scopes
 (owner_identity_id, owner_did, local_device_id, created_at)
VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))"#,
                params![
                    self.scope.owner_identity_id,
                    self.scope.owner_did,
                    self.scope.local_device_id,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        validate_owner_scope(
            self.connection,
            &self.scope.owner_identity_id,
            &self.scope.owner_did,
            &self.scope.local_device_id,
        )
    }

    pub(crate) fn publish_local_bundle(
        &self,
        bundle: &V2PrekeyBundle,
        signed_prekey_private: &x25519_dalek::StaticSecret,
        one_time_prekeys: &[(V2OneTimePrekey, x25519_dalek::StaticSecret)],
        now: &str,
    ) -> crate::ImResult<()> {
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let owner_did = self.scope.owner_did.as_str();
        let local_device_id = self.scope.local_device_id.as_str();
        let now = required("now", now)?;
        self.scope.validate_bundle(bundle)?;
        let signed_prekey_public = decode_x25519(&bundle.signed_prekey.public_key_b64u)?;
        if x25519_dalek::PublicKey::from(signed_prekey_private).to_bytes() != signed_prekey_public {
            return Err(crate::ImError::PermissionDenied);
        }
        let bundle_json = serde_json::to_string(bundle).map_err(serialization_error)?;
        let published_one_time_prekeys = one_time_prekeys
            .iter()
            .map(|(public, _)| public.clone())
            .collect::<Vec<_>>();
        validate_published_one_time_prekeys(&published_one_time_prekeys)?;
        let signed_material =
            encode_signed_prekey_material(signed_prekey_private, &published_one_time_prekeys)?;
        let signed_blob = self.seal(
            SecretKind::DirectE2eeV2SignedPrekeyPrivate,
            direct_secret_key_id(
                owner_identity_id,
                "v2-signed-prekey",
                &bundle.signed_prekey.key_id,
                local_device_id,
            ),
            signed_material.as_slice(),
            "P5 v2 signed prekey",
        )?;
        let sealed_opks = one_time_prekeys
            .iter()
            .map(|(public, private)| {
                public.validate().map_err(v2_error)?;
                let expected = x25519_dalek::PublicKey::from(private).to_bytes();
                let actual = decode_x25519(&public.public_key_b64u)?;
                if actual != expected {
                    return Err(crate::ImError::PermissionDenied);
                }
                let blob = self.seal(
                    SecretKind::DirectE2eeV2OneTimePrekeyPrivate,
                    direct_secret_key_id(
                        owner_identity_id,
                        "v2-one-time-prekey",
                        &public.key_id,
                        local_device_id,
                    ),
                    &private.to_bytes(),
                    "P5 v2 one-time prekey",
                )?;
                let public_json = serde_json::to_string(public).map_err(serialization_error)?;
                Ok((public, blob, public_json))
            })
            .collect::<crate::ImResult<Vec<_>>>()?;

        let transaction = self.transaction()?;
        let existing_bundle = transaction
            .query_row(
                r#"SELECT bundle_json FROM direct_e2ee_v2_prekey_bundles
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND bundle_id = ?3"#,
                params![owner_identity_id, local_device_id, bundle.bundle_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if existing_bundle
            .as_deref()
            .is_some_and(|existing| existing != bundle_json)
        {
            drop(transaction);
            self.gc_orphaned_v2_secrets()?;
            return Err(crate::ImError::PermissionDenied);
        }
        transaction
            .execute(
                r#"UPDATE direct_e2ee_v2_prekey_bundles SET status = 'retained'
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND status = 'active'
  AND bundle_id <> ?3"#,
                params![owner_identity_id, local_device_id, bundle.bundle_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        transaction
            .execute(
                r#"INSERT INTO direct_e2ee_v2_prekey_bundles
 (owner_identity_id, owner_did, local_device_id, bundle_id, bundle_json,
  signed_prekey_private_blob, status, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?7)
ON CONFLICT(owner_identity_id, local_device_id, bundle_id) DO UPDATE SET
 owner_did = excluded.owner_did,
 bundle_json = excluded.bundle_json,
 signed_prekey_private_blob = excluded.signed_prekey_private_blob,
 status = 'active', updated_at = excluded.updated_at"#,
                params![
                    owner_identity_id,
                    owner_did,
                    local_device_id,
                    bundle.bundle_id,
                    bundle_json,
                    signed_blob,
                    now,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        for (public, private_blob, public_json) in sealed_opks {
            let inserted = transaction
                .execute(
                    r#"INSERT INTO direct_e2ee_v2_one_time_prekeys
 (owner_identity_id, owner_did, local_device_id, bundle_id, key_id, public_json,
  private_key_blob, status, created_at, consumed_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'available', ?8, NULL)
ON CONFLICT(owner_identity_id, local_device_id, key_id) DO NOTHING"#,
                    params![
                        owner_identity_id,
                        owner_did,
                        local_device_id,
                        bundle.bundle_id,
                        public.key_id,
                        public_json,
                        private_blob,
                        now,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if inserted == 0 {
                let existing = transaction
                    .query_row(
                        r#"SELECT public_json, bundle_id FROM direct_e2ee_v2_one_time_prekeys
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND key_id = ?3"#,
                        params![owner_identity_id, local_device_id, public.key_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .map_err(crate::internal::local_state::local_state_unavailable)?;
                if existing != (public_json, bundle.bundle_id.clone()) {
                    drop(transaction);
                    self.gc_orphaned_v2_secrets()?;
                    return Err(crate::ImError::PermissionDenied);
                }
            }
        }
        transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_one_time_prekeys
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND bundle_id IN (
  SELECT bundle_id FROM direct_e2ee_v2_prekey_bundles
  WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND status = 'retained'
  ORDER BY updated_at DESC, bundle_id DESC LIMIT -1 OFFSET 2
)"#,
                params![owner_identity_id, local_device_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_prekey_bundles
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND status = 'retained'
  AND bundle_id IN (
    SELECT bundle_id FROM direct_e2ee_v2_prekey_bundles
    WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND status = 'retained'
    ORDER BY updated_at DESC, bundle_id DESC LIMIT -1 OFFSET 2
  )"#,
                params![owner_identity_id, local_device_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(())
    }

    pub(crate) fn load_active_bundle(&self) -> crate::ImResult<Option<V2LocalBundleMaterial>> {
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let local_device_id = self.scope.local_device_id.as_str();
        let row = self
            .connection
            .query_row(
                r#"SELECT bundle_json, signed_prekey_private_blob
FROM direct_e2ee_v2_prekey_bundles
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND status = 'active'
ORDER BY updated_at DESC LIMIT 1"#,
                params![
                    required("owner_identity_id", owner_identity_id)?,
                    required("local_device_id", local_device_id)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        row.map(|(bundle_json, private_blob)| {
            self.open_local_bundle_material(bundle_json, private_blob)
        })
        .transpose()
    }

    pub(crate) fn load_accepted_bundle(
        &self,
        bundle_id: &str,
        now: &str,
    ) -> crate::ImResult<Option<V2LocalBundleMaterial>> {
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let local_device_id = self.scope.local_device_id.as_str();
        let now = required("now", now)?;
        let now = chrono::DateTime::parse_from_rfc3339(&now)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let row = self
            .connection
            .query_row(
                r#"SELECT bundle_json, signed_prekey_private_blob
FROM direct_e2ee_v2_prekey_bundles
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND bundle_id = ?3
  AND status IN ('active', 'retained')"#,
                params![
                    required("owner_identity_id", owner_identity_id)?,
                    required("local_device_id", local_device_id)?,
                    required("bundle_id", bundle_id)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        row.map(|(bundle_json, private_blob)| {
            let material = self.open_local_bundle_material(bundle_json, private_blob)?;
            let expires_at =
                chrono::DateTime::parse_from_rfc3339(&material.bundle.signed_prekey.expires_at)
                    .map_err(|_| crate::ImError::PermissionDenied)?;
            if expires_at <= now {
                return Err(crate::ImError::PermissionDenied);
            }
            Ok(material)
        })
        .transpose()
    }

    fn open_local_bundle_material(
        &self,
        bundle_json: String,
        private_blob: Vec<u8>,
    ) -> crate::ImResult<V2LocalBundleMaterial> {
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let owner_did = self.scope.owner_did.as_str();
        let local_device_id = self.scope.local_device_id.as_str();
        let bundle: V2PrekeyBundle =
            serde_json::from_str(&bundle_json).map_err(serialization_error)?;
        bundle.validate_structure().map_err(v2_error)?;
        let (private, published_one_time_prekeys) = self.open_signed_prekey_material(
            private_blob,
            DirectSecretOpenExpectation {
                owner_identity_id,
                owner_did,
                device_id: local_device_id,
                kind: SecretKind::DirectE2eeV2SignedPrekeyPrivate,
                key_id_prefix: direct_secret_key_id_prefix(
                    owner_identity_id,
                    "v2-signed-prekey",
                    &bundle.signed_prekey.key_id,
                    local_device_id,
                ),
            },
        )?;
        if x25519_dalek::PublicKey::from(&private).to_bytes()
            != decode_x25519(&bundle.signed_prekey.public_key_b64u)?
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(V2LocalBundleMaterial {
            bundle,
            signed_prekey_private: private,
            published_one_time_prekeys,
        })
    }

    pub(crate) fn load_available_opk(
        &self,
        bundle_id: &str,
        key_id: &str,
    ) -> crate::ImResult<Option<V2LocalOneTimePrekeyMaterial>> {
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let owner_did = self.scope.owner_did.as_str();
        let local_device_id = self.scope.local_device_id.as_str();
        let row = self
            .connection
            .query_row(
                r#"SELECT public_json, private_key_blob
FROM direct_e2ee_v2_one_time_prekeys
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND bundle_id = ?3 AND key_id = ?4
  AND status = 'available'"#,
                params![
                    required("owner_identity_id", owner_identity_id)?,
                    required("local_device_id", local_device_id)?,
                    required("bundle_id", bundle_id)?,
                    required("key_id", key_id)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        row.map(|(public_json, private_blob)| {
            let public: V2OneTimePrekey =
                serde_json::from_str(&public_json).map_err(serialization_error)?;
            public.validate().map_err(v2_error)?;
            let private = self.open_fixed_private(
                private_blob,
                DirectSecretOpenExpectation {
                    owner_identity_id,
                    owner_did,
                    device_id: local_device_id,
                    kind: SecretKind::DirectE2eeV2OneTimePrekeyPrivate,
                    key_id_prefix: direct_secret_key_id_prefix(
                        owner_identity_id,
                        "v2-one-time-prekey",
                        &public.key_id,
                        local_device_id,
                    ),
                },
            )?;
            if x25519_dalek::PublicKey::from(&private).to_bytes()
                != decode_x25519(&public.public_key_b64u)?
            {
                return Err(crate::ImError::PermissionDenied);
            }
            Ok(V2LocalOneTimePrekeyMaterial { public, private })
        })
        .transpose()
    }

    pub(crate) fn load_available_opk_publics(
        &self,
        bundle_id: &str,
    ) -> crate::ImResult<Vec<V2OneTimePrekey>> {
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let local_device_id = self.scope.local_device_id.as_str();
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT public_json FROM direct_e2ee_v2_one_time_prekeys
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND bundle_id = ?3
  AND status = 'available'
ORDER BY created_at, key_id"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                params![
                    owner_identity_id,
                    local_device_id,
                    required("bundle_id", bundle_id)?,
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(|row| {
                let public: V2OneTimePrekey = serde_json::from_str(
                    &row.map_err(crate::internal::local_state::local_state_unavailable)?,
                )
                .map_err(serialization_error)?;
                public.validate().map_err(v2_error)?;
                Ok(public)
            })
            .collect();
        rows
    }

    /// Persists the advanced ratchet and exact retry body before network send.
    pub(crate) fn commit_outbound(
        &self,
        state: &V2DirectSessionState,
        pending: &V2PendingOutboundRecord,
        expected_session: V2SessionExpectation,
        now: &str,
    ) -> crate::ImResult<()> {
        self.commit_outbound_inner(state, pending, expected_session, now, None)
    }

    /// Persists the ratchet, exact retry ciphertext, and AWiki-private
    /// transport sidecar in one SQLite transaction. This is used only for
    /// same-domain control delivery and does not alter the P5 v2 wire model.
    pub(crate) fn commit_outbound_with_private_sidecar(
        &self,
        state: &V2DirectSessionState,
        pending: &V2PendingOutboundRecord,
        expected_session: V2SessionExpectation,
        now: &str,
        sidecar: &V2PrivateOutboundSidecar,
    ) -> crate::ImResult<()> {
        sidecar.validate_binding(&state.binding)?;
        if sidecar.operation_id != pending.operation_id {
            return Err(crate::ImError::PermissionDenied);
        }
        self.commit_outbound_inner(state, pending, expected_session, now, Some(sidecar))
    }

    fn commit_outbound_inner(
        &self,
        state: &V2DirectSessionState,
        pending: &V2PendingOutboundRecord,
        expected_session: V2SessionExpectation,
        now: &str,
        sidecar: Option<&V2PrivateOutboundSidecar>,
    ) -> crate::ImResult<()> {
        state.validate().map_err(v2_error)?;
        self.scope.validate_binding(&state.binding)?;
        pending.validate().map_err(v2_error)?;
        if pending.binding != state.binding || pending.session_id != state.session_id {
            return Err(crate::ImError::PermissionDenied);
        }
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let now = required("now", now)?;
        let transaction = self.transaction()?;
        if sidecar.is_some()
            && transaction
                .query_row(
                    r#"SELECT 1 FROM direct_e2ee_v2_private_outbound_tombstones
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                    params![
                        owner_identity_id,
                        state.binding.local_device_id,
                        pending.operation_id,
                    ],
                    |_| Ok(()),
                )
                .optional()
                .map_err(crate::internal::local_state::local_state_unavailable)?
                .is_some()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some((existing_blob, stored_revision, stored_session_id)) = transaction
            .query_row(
                r#"SELECT pending_blob, session_revision, session_id
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    owner_identity_id,
                    state.binding.local_device_id,
                    pending.operation_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
        {
            let existing =
                self.open_pending(existing_blob, &state.binding, &pending.operation_id)?;
            if existing != *pending || stored_session_id != pending.session_id {
                return Err(crate::ImError::PermissionDenied);
            }
            self.validate_pending_session_checkpoint(
                &transaction,
                &state.binding,
                &stored_session_id,
                stored_revision,
            )?;
            validate_existing_private_sidecar(
                &transaction,
                owner_identity_id,
                &state.binding.local_device_id,
                &pending.operation_id,
                sidecar,
            )?;
            transaction
                .rollback()
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            return Ok(());
        }
        let state_blob = self.seal_state(state)?;
        let pending_blob = self.seal_pending(pending)?;
        let session_revision = write_state_for_owner_cas(
            &transaction,
            owner_identity_id,
            state,
            &state_blob,
            expected_session,
            &now,
        )?;
        let inserted = transaction
            .execute(
                r#"INSERT INTO direct_e2ee_v2_pending
 (owner_identity_id, owner_did, local_device_id, peer_did, peer_device_id,
  operation_id, message_id, session_id, session_revision, pending_blob,
  created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
ON CONFLICT(owner_identity_id, local_device_id, operation_id) DO NOTHING"#,
                params![
                    owner_identity_id,
                    state.binding.local_did,
                    state.binding.local_device_id,
                    state.binding.peer_did,
                    state.binding.peer_device_id,
                    pending.operation_id,
                    pending.message_id,
                    pending.session_id,
                    session_revision,
                    pending_blob,
                    now,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if inserted != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(sidecar) = sidecar {
            let context_json =
                serde_json::to_string(&sidecar.context).map_err(serialization_error)?;
            let inserted = transaction
                .execute(
                    r#"INSERT INTO direct_e2ee_v2_private_outbound
 (owner_identity_id, owner_did, local_device_id, operation_id,
  delivery_class, context_json, created_at, accepted_at, failure_code, completed_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL)"#,
                    params![
                        owner_identity_id,
                        state.binding.local_did,
                        state.binding.local_device_id,
                        sidecar.operation_id(),
                        sidecar.delivery_class(),
                        context_json,
                        now,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if inserted != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(())
    }

    pub(crate) fn load_private_outbound_sidecar(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<Option<V2PrivateOutboundSidecar>> {
        self.scope.validate_binding(binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let operation_id = required("operation_id", operation_id)?;
        let row = self
            .connection
            .query_row(
                r#"SELECT delivery_class, context_json
FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        row.map(|(delivery_class, context_json)| {
            if delivery_class
                != crate::internal::identity_root_transfer::ROOT_KEY_CONTROL_DELIVERY_CLASS
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let sidecar = V2PrivateOutboundSidecar {
                operation_id: operation_id.to_owned(),
                context: serde_json::from_str(&context_json).map_err(serialization_error)?,
            };
            sidecar.validate_binding(binding)?;
            Ok(sidecar)
        })
        .transpose()
    }

    /// Converts expired private controls into secret-free terminal records.
    /// The retry ciphertext, ratchet pending record, and their Vault secret
    /// are removed while the non-secret Failed status remains inspectable.
    fn terminalize_expired_private_outbound(&self, now: &str) -> crate::ImResult<usize> {
        let now_text = required("now", now)?;
        let now = chrono::DateTime::parse_from_rfc3339(&now_text)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT operation_id, delivery_class, context_json, created_at, accepted_at
FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                params![
                    self.scope.owner_identity_id,
                    self.scope.owner_did,
                    self.scope.local_device_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        drop(statement);

        let mut expired = Vec::new();
        for (operation_id, delivery_class, context_json, created_at, accepted_at) in rows {
            if delivery_class
                != crate::internal::identity_root_transfer::ROOT_KEY_CONTROL_DELIVERY_CLASS
                || operation_id.trim().is_empty()
                || created_at.trim().is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let context: V2RootControlPrivateContext =
                serde_json::from_str(&context_json).map_err(serialization_error)?;
            context.transport_context.validate()?;
            if operation_id != context.transport_context.message_id {
                return Err(crate::ImError::PermissionDenied);
            }
            let is_ack = context.completion.is_some();
            let owned_by_local_device = if is_ack {
                context.transport_context.recipient_device_id == self.scope.local_device_id
            } else {
                context.transport_context.sender_device_id == self.scope.local_device_id
            };
            if !owned_by_local_device {
                return Err(crate::ImError::PermissionDenied);
            }
            let expires_at =
                chrono::DateTime::parse_from_rfc3339(&context.transport_context.expires_at)
                    .map_err(|_| crate::ImError::PermissionDenied)?;
            if expires_at <= now {
                expired.push((
                    operation_id,
                    context.transport_context.sender_device_id,
                    context.transport_context.recipient_device_id,
                    created_at,
                    accepted_at,
                ));
            }
        }
        if expired.is_empty() {
            return Ok(0);
        }

        let transaction = self.transaction()?;
        for (operation_id, sender_device_id, recipient_device_id, created_at, accepted_at) in
            &expired
        {
            let inserted = transaction
                .execute(
                    r#"INSERT INTO direct_e2ee_v2_private_outbound_tombstones
 (owner_identity_id, owner_did, local_device_id, operation_id,
  sender_device_id, recipient_device_id, created_at, accepted_at, completed_at,
  terminal_phase, failure_code)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'failed', 'expired')"#,
                    params![
                        self.scope.owner_identity_id,
                        self.scope.owner_did,
                        self.scope.local_device_id,
                        operation_id,
                        sender_device_id,
                        recipient_device_id,
                        created_at,
                        accepted_at,
                        now_text,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if inserted != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
            let deleted_sidecar = transaction
                .execute(
                    r#"DELETE FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                    params![
                        self.scope.owner_identity_id,
                        self.scope.local_device_id,
                        operation_id,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            let deleted_pending = transaction
                .execute(
                    r#"DELETE FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                    params![
                        self.scope.owner_identity_id,
                        self.scope.local_device_id,
                        operation_id,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if deleted_sidecar != 1 || deleted_pending != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(expired.len())
    }

    pub(crate) fn list_private_outbound_statuses(
        &self,
        now: &str,
    ) -> crate::ImResult<Vec<V2PrivateOutboundStatus>> {
        let now_text = required("now", now)?;
        self.terminalize_expired_private_outbound(&now_text)?;
        let now = chrono::DateTime::parse_from_rfc3339(&now_text)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT operation_id, delivery_class, context_json, created_at,
       accepted_at, failure_code, completed_at
FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3
ORDER BY created_at DESC, operation_id DESC"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                params![
                    self.scope.owner_identity_id,
                    self.scope.owner_did,
                    self.scope.local_device_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let mut statuses = rows
            .into_iter()
            .map(
                |(
                    operation_id,
                    delivery_class,
                    context_json,
                    created_at,
                    accepted_at,
                    failure_code,
                    completed_at,
                )| {
                    if delivery_class
                        != crate::internal::identity_root_transfer::ROOT_KEY_CONTROL_DELIVERY_CLASS
                        || created_at.trim().is_empty()
                        || failure_code
                            .as_deref()
                            .is_some_and(|value| value != "delivery_failed")
                    {
                        return Err(crate::ImError::PermissionDenied);
                    }
                    let context: V2RootControlPrivateContext =
                        serde_json::from_str(&context_json).map_err(serialization_error)?;
                    context.transport_context.validate()?;
                    if operation_id != context.transport_context.message_id {
                        return Err(crate::ImError::PermissionDenied);
                    }
                    let expires_at =
                        chrono::DateTime::parse_from_rfc3339(&context.transport_context.expires_at)
                            .map_err(|_| crate::ImError::PermissionDenied)?;
                    let expired = expires_at <= now;
                    if expired {
                        return Err(crate::ImError::PermissionDenied);
                    }
                    let is_ack = context.completion.is_some();
                    let owned_by_local_device = if is_ack {
                        context.transport_context.recipient_device_id == self.scope.local_device_id
                    } else {
                        context.transport_context.sender_device_id == self.scope.local_device_id
                    };
                    if !owned_by_local_device {
                        return Err(crate::ImError::PermissionDenied);
                    }
                    let phase = if completed_at.is_some() {
                        V2PrivateOutboundPhase::Completed
                    } else if failure_code.is_some() || expired {
                        V2PrivateOutboundPhase::Failed
                    } else if is_ack {
                        V2PrivateOutboundPhase::Importing
                    } else if accepted_at.is_some() {
                        V2PrivateOutboundPhase::AwaitingImport
                    } else {
                        V2PrivateOutboundPhase::PendingDelivery
                    };
                    Ok(V2PrivateOutboundStatus {
                        operation_id,
                        sender_device_id: context.transport_context.sender_device_id,
                        recipient_device_id: context.transport_context.recipient_device_id,
                        phase,
                        created_at,
                        accepted_at,
                        completed_at,
                        retryable: !expired && !matches!(phase, V2PrivateOutboundPhase::Completed),
                    })
                },
            )
            .collect::<crate::ImResult<Vec<_>>>()?;
        drop(statement);

        let mut tombstone_statement = self
            .connection
            .prepare(
                r#"SELECT operation_id, sender_device_id, recipient_device_id,
       created_at, accepted_at, completed_at, terminal_phase, failure_code
FROM direct_e2ee_v2_private_outbound_tombstones
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let tombstone_rows = tombstone_statement
            .query_map(
                params![
                    self.scope.owner_identity_id,
                    self.scope.owner_did,
                    self.scope.local_device_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?,
                    ))
                },
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        drop(tombstone_statement);
        for (
            operation_id,
            sender_device_id,
            recipient_device_id,
            created_at,
            accepted_at,
            terminal_at,
            terminal_phase,
            failure_code,
        ) in tombstone_rows
        {
            if terminal_at.trim().is_empty() {
                return Err(crate::ImError::PermissionDenied);
            }
            let (phase, completed_at) = match (terminal_phase.as_str(), failure_code.as_deref()) {
                ("completed", None) => (V2PrivateOutboundPhase::Completed, Some(terminal_at)),
                ("failed", Some("expired")) => (V2PrivateOutboundPhase::Failed, None),
                _ => return Err(crate::ImError::PermissionDenied),
            };
            let tombstone = V2PrivateOutboundStatus {
                operation_id,
                sender_device_id,
                recipient_device_id,
                phase,
                created_at,
                accepted_at,
                completed_at,
                retryable: false,
            };
            if tombstone.operation_id.trim().is_empty()
                || tombstone.sender_device_id.trim().is_empty()
                || tombstone.recipient_device_id.trim().is_empty()
                || tombstone.sender_device_id == tombstone.recipient_device_id
                || tombstone.created_at.trim().is_empty()
                || (matches!(tombstone.phase, V2PrivateOutboundPhase::Completed)
                    && tombstone.completed_at.as_deref().is_none_or(str::is_empty))
                || (tombstone.sender_device_id != self.scope.local_device_id
                    && tombstone.recipient_device_id != self.scope.local_device_id)
                || statuses
                    .iter()
                    .any(|status| status.operation_id == tombstone.operation_id)
            {
                return Err(crate::ImError::PermissionDenied);
            }
            statuses.push(tombstone);
        }
        statuses.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.operation_id.cmp(&left.operation_id))
        });
        Ok(statuses)
    }

    pub(crate) fn private_outbound_status(
        &self,
        operation_id: &str,
        now: &str,
    ) -> crate::ImResult<Option<V2PrivateOutboundStatus>> {
        let operation_id = required("operation_id", operation_id)?;
        let mut matches = self
            .list_private_outbound_statuses(now)?
            .into_iter()
            .filter(|status| status.operation_id == operation_id);
        let status = matches.next();
        if matches.next().is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(status)
    }

    pub(crate) fn mark_private_outbound_failed(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<()> {
        self.update_private_outbound_failed(binding, operation_id)
    }

    pub(crate) fn mark_private_outbound_completed(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<()> {
        self.scope.validate_binding(binding)?;
        let operation_id = required("operation_id", operation_id)?;
        if let Some(phase) = self.terminal_tombstone_phase(binding, &operation_id)? {
            return if phase == "completed" {
                Ok(())
            } else {
                Err(crate::ImError::PermissionDenied)
            };
        }
        let sidecar = self
            .load_private_outbound_sidecar(binding, &operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        sidecar.validate_binding(binding)?;
        let (context, _) = sidecar.root_control_context();
        let transaction = self.transaction()?;
        let (created_at, accepted_at) = transaction
            .query_row(
                r#"SELECT created_at, accepted_at
FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    operation_id,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if created_at.trim().is_empty() {
            return Err(crate::ImError::PermissionDenied);
        }
        let completed_at = chrono::Utc::now().to_rfc3339();
        let inserted = transaction
            .execute(
                r#"INSERT INTO direct_e2ee_v2_private_outbound_tombstones
 (owner_identity_id, owner_did, local_device_id, operation_id,
  sender_device_id, recipient_device_id, created_at, accepted_at, completed_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
                params![
                    self.scope.owner_identity_id,
                    self.scope.owner_did,
                    binding.local_device_id,
                    operation_id,
                    context.sender_device_id,
                    context.recipient_device_id,
                    created_at,
                    accepted_at,
                    completed_at,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if inserted != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let deleted_sidecar = transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    operation_id,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if deleted_sidecar != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let deleted = transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    operation_id,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if deleted != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(())
    }

    fn update_private_outbound_failed(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<()> {
        self.scope.validate_binding(binding)?;
        let operation_id = required("operation_id", operation_id)?;
        let sidecar = self
            .load_private_outbound_sidecar(binding, &operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        sidecar.validate_binding(binding)?;
        let changed = self
            .connection
            .execute(
                r#"UPDATE direct_e2ee_v2_private_outbound
SET failure_code = 'delivery_failed'
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3
  AND completed_at IS NULL"#,
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    operation_id,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if changed != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn terminal_tombstone_phase(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<Option<String>> {
        let row = self
            .connection
            .query_row(
                r#"SELECT sender_device_id, recipient_device_id, terminal_phase
FROM direct_e2ee_v2_private_outbound_tombstones
WHERE owner_identity_id = ?1 AND owner_did = ?2
  AND local_device_id = ?3 AND operation_id = ?4"#,
                params![
                    self.scope.owner_identity_id,
                    self.scope.owner_did,
                    binding.local_device_id,
                    operation_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let Some((sender_device_id, recipient_device_id, terminal_phase)) = row else {
            return Ok(None);
        };
        let matches = (binding.local_device_id == sender_device_id
            && binding.peer_device_id == recipient_device_id)
            || (binding.local_device_id == recipient_device_id
                && binding.peer_device_id == sender_device_id);
        if !matches {
            return Err(crate::ImError::PermissionDenied);
        }
        if !matches!(terminal_phase.as_str(), "completed" | "failed") {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Some(terminal_phase))
    }

    pub(crate) fn load_pending(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<Option<V2PendingOutboundRecord>> {
        self.scope.validate_binding(binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let row = self
            .connection
            .query_row(
                r#"SELECT pending_blob, session_revision, session_id
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    owner_identity_id,
                    binding.local_device_id,
                    required("operation_id", operation_id)?,
                ],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        row.map(|(blob, session_revision, stored_session_id)| {
            let pending = self.open_pending(blob, binding, operation_id)?;
            if pending.binding != *binding || pending.session_id != stored_session_id {
                return Err(crate::ImError::PermissionDenied);
            }
            self.validate_pending_session_checkpoint(
                self.connection,
                binding,
                &stored_session_id,
                session_revision,
            )?;
            Ok(pending)
        })
        .transpose()
    }

    pub(crate) fn mark_pending_accepted(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<bool> {
        self.scope.validate_binding(binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let operation_id = required("operation_id", operation_id)?;
        let transaction = self.transaction()?;
        let row = transaction
            .query_row(
                r#"SELECT pending_blob, session_revision, session_id
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let Some((blob, session_revision, stored_session_id)) = row else {
            transaction
                .rollback()
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            return Ok(false);
        };
        let pending = self.open_pending(blob, binding, &operation_id)?;
        if pending.session_id != stored_session_id {
            return Err(crate::ImError::PermissionDenied);
        }
        self.validate_pending_session_checkpoint(
            &transaction,
            binding,
            &stored_session_id,
            session_revision,
        )?;
        let private_exists = transaction
            .query_row(
                r#"SELECT 1 FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .is_some();
        if private_exists
            || pending.wire_content_type == anp::direct_e2ee::CONTENT_TYPE_DIRECT_INIT_V2
            || pending
                .operation_id
                .starts_with(super::v2_runtime::SESSION_REPLY_OPERATION_PREFIX)
        {
            if !private_exists {
                transaction
                    .commit()
                    .map_err(crate::internal::local_state::local_state_unavailable)?;
                return Ok(true);
            }
            let changed = transaction
                .execute(
                    r#"UPDATE direct_e2ee_v2_private_outbound
SET accepted_at = COALESCE(accepted_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    failure_code = NULL
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                    params![owner_identity_id, binding.local_device_id, operation_id],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if changed != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
            transaction
                .commit()
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            return Ok(true);
        }
        transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let deleted = transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if deleted != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(true)
    }

    /// Removes the retained exact Init retry only after its authenticated
    /// first reply has established the same session. Replaying that first
    /// reply after a crash remains idempotent because a missing Init is
    /// accepted as already completed.
    pub(crate) fn complete_session_init(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        self.scope.validate_binding(binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let operation_id = required("operation_id", operation_id)?;
        let session_id = required("session_id", session_id)?;
        let transaction = self.transaction()?;
        let row = transaction
            .query_row(
                r#"SELECT pending_blob, session_revision, session_id
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let Some((blob, session_revision, stored_session_id)) = row else {
            transaction
                .rollback()
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            return Ok(false);
        };
        let pending = self.open_pending(blob, binding, &operation_id)?;
        if stored_session_id != session_id
            || pending.session_id != session_id
            || pending.wire_content_type != anp::direct_e2ee::CONTENT_TYPE_DIRECT_INIT_V2
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.validate_pending_session_checkpoint(
            &transaction,
            binding,
            &stored_session_id,
            session_revision,
        )?;
        let private_exists = transaction
            .query_row(
                r#"SELECT 1 FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .is_some();
        if private_exists {
            return Err(crate::ImError::PermissionDenied);
        }
        let deleted = transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![owner_identity_id, binding.local_device_id, operation_id],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if deleted != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(true)
    }

    pub(crate) fn complete_session_init_for_session(
        &self,
        binding: &V2SessionBinding,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        self.scope.validate_binding(binding)?;
        let session_id = required("session_id", session_id)?;
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT operation_id, pending_blob
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND peer_did = ?3
  AND peer_device_id = ?4 AND session_id = ?5"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    binding.peer_did,
                    binding.peer_device_id,
                    session_id,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        drop(statement);
        let mut matching = rows
            .into_iter()
            .map(|(operation_id, blob)| {
                let pending = self.open_pending(blob, binding, &operation_id)?;
                Ok((operation_id, pending))
            })
            .collect::<crate::ImResult<Vec<_>>>()?
            .into_iter()
            .filter(|(_, pending)| {
                pending.wire_content_type == anp::direct_e2ee::CONTENT_TYPE_DIRECT_INIT_V2
            });
        let Some((operation_id, _)) = matching.next() else {
            return Ok(false);
        };
        if matching.next().is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        self.complete_session_init(binding, &operation_id, &session_id)
    }

    /// The first authenticated Cipher on a responder-created session proves
    /// that the peer received its Session Reply. Remove the exact Reply retry
    /// at that point; an exact inbound replay makes the cleanup idempotent.
    pub(crate) fn complete_session_reply_for_session(
        &self,
        binding: &V2SessionBinding,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        self.scope.validate_binding(binding)?;
        let session_id = required("session_id", session_id)?;
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT operation_id, pending_blob
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND peer_did = ?3
  AND peer_device_id = ?4 AND session_id = ?5"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    binding.peer_did,
                    binding.peer_device_id,
                    session_id,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        drop(statement);
        let mut matching = rows
            .into_iter()
            .map(|(operation_id, blob)| {
                let pending = self.open_pending(blob, binding, &operation_id)?;
                Ok((operation_id, pending))
            })
            .collect::<crate::ImResult<Vec<_>>>()?
            .into_iter()
            .filter(|(operation_id, pending)| {
                super::v2_runtime::is_session_reply_operation_id(operation_id)
                    && pending.wire_content_type == anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2
            });
        let Some((operation_id, pending)) = matching.next() else {
            return Ok(false);
        };
        if matching.next().is_some() || pending.session_id != session_id {
            return Err(crate::ImError::PermissionDenied);
        }

        let transaction = self.transaction()?;
        self.validate_pending_session_checkpoint(
            &transaction,
            binding,
            &session_id,
            self.pending_session_revision(&transaction, binding, &operation_id, &session_id)?,
        )?;
        let private_exists = transaction
            .query_row(
                r#"SELECT 1 FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    operation_id,
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .is_some();
        if private_exists {
            return Err(crate::ImError::PermissionDenied);
        }
        let deleted = transaction
            .execute(
                r#"DELETE FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    operation_id,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if deleted != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(true)
    }

    fn pending_session_revision(
        &self,
        connection: &Connection,
        binding: &V2SessionBinding,
        operation_id: &str,
        session_id: &str,
    ) -> crate::ImResult<i64> {
        connection
            .query_row(
                r#"SELECT session_revision, session_id
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![
                    self.scope.owner_identity_id,
                    binding.local_device_id,
                    operation_id,
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)
            .and_then(|(revision, stored_session_id)| {
                if stored_session_id != session_id {
                    Err(crate::ImError::PermissionDenied)
                } else {
                    Ok(revision)
                }
            })
    }

    pub(crate) fn load_session(
        &self,
        binding: &V2SessionBinding,
        session_id: &str,
    ) -> crate::ImResult<Option<V2StoredSession>> {
        self.scope.validate_binding(binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let row = self
            .connection
            .query_row(
                r#"SELECT state_blob, revision FROM direct_e2ee_v2_sessions
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND peer_did = ?3
  AND peer_device_id = ?4 AND session_id = ?5"#,
                params![
                    owner_identity_id,
                    binding.local_device_id,
                    binding.peer_did,
                    binding.peer_device_id,
                    required("session_id", session_id)?,
                ],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        row.map(|(blob, revision)| {
            validate_storage_revision(revision)?;
            Ok(V2StoredSession {
                state: self.open_state(blob, binding, session_id)?,
                revision,
            })
        })
        .transpose()
    }

    pub(crate) fn select_established_session(
        &self,
        binding: &V2SessionBinding,
    ) -> crate::ImResult<Option<V2StoredSession>> {
        self.scope.validate_binding(binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT session_id, state_blob, revision FROM direct_e2ee_v2_sessions
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND peer_did = ?3
  AND peer_device_id = ?4 AND disabled = 0
ORDER BY updated_at DESC"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let blobs = statement
            .query_map(
                params![
                    owner_identity_id,
                    binding.local_device_id,
                    binding.peer_did,
                    binding.peer_device_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let stored = blobs
            .into_iter()
            .map(|(session_id, blob, revision)| {
                validate_storage_revision(revision)?;
                Ok(V2StoredSession {
                    state: self.open_state(blob, binding, &session_id)?,
                    revision,
                })
            })
            .collect::<crate::ImResult<Vec<_>>>()?;
        let states = stored
            .iter()
            .map(|record| record.state.clone())
            .collect::<Vec<_>>();
        let selected = anp::direct_e2ee::select_default_outbound_session_v2(binding, &states)
            .map_err(v2_error)?;
        let selected_session_id = selected.map(|state| state.session_id.clone());
        Ok(selected_session_id.and_then(|session_id| {
            stored
                .into_iter()
                .find(|record| record.state.session_id == session_id)
        }))
    }

    /// Checks replay before the SDK attempts an inbound decrypt. Exact replay
    /// is safe to drop; reuse of the same message ID with any other cipher or
    /// session is rejected.
    pub(crate) fn is_exact_inbound_replay(
        &self,
        binding: &V2SessionBinding,
        message_id: &str,
        ciphertext_digest: &str,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        self.scope.validate_binding(binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let ciphertext_digest = required("ciphertext_digest", ciphertext_digest)?;
        let session_id = required("session_id", session_id)?;
        let existing = self
            .connection
            .query_row(
                r#"SELECT ciphertext_digest, session_id FROM direct_e2ee_v2_replay
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND sender_did = ?3
  AND sender_device_id = ?4 AND message_id = ?5"#,
                params![
                    owner_identity_id,
                    binding.local_device_id,
                    binding.peer_did,
                    binding.peer_device_id,
                    required("message_id", message_id)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        match existing {
            None => Ok(false),
            Some((digest, stored_session))
                if digest == ciphertext_digest && stored_session == session_id =>
            {
                Ok(true)
            }
            Some(_) => Err(crate::ImError::PermissionDenied),
        }
    }

    /// Commits replay, ratchet state, and optional OPK consumption together.
    pub(crate) fn commit_inbound(
        &self,
        state: &V2DirectSessionState,
        message_id: &str,
        ciphertext_digest: &str,
        consume_opk_id: Option<&str>,
        expected_session: V2SessionExpectation,
        now: &str,
    ) -> crate::ImResult<V2InboundCommit> {
        state.validate().map_err(v2_error)?;
        self.scope.validate_binding(&state.binding)?;
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let message_id = required("message_id", message_id)?;
        let ciphertext_digest = required("ciphertext_digest", ciphertext_digest)?;
        let now = required("now", now)?;
        let transaction = self.transaction()?;
        let replay = transaction
            .query_row(
                r#"SELECT ciphertext_digest, session_id FROM direct_e2ee_v2_replay
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND sender_did = ?3
  AND sender_device_id = ?4 AND message_id = ?5"#,
                params![
                    owner_identity_id,
                    state.binding.local_device_id,
                    state.binding.peer_did,
                    state.binding.peer_device_id,
                    message_id,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if let Some((stored_digest, stored_session)) = replay {
            if stored_digest == ciphertext_digest && stored_session == state.session_id {
                transaction
                    .rollback()
                    .map_err(crate::internal::local_state::local_state_unavailable)?;
                return Ok(V2InboundCommit::Replay);
            }
            return Err(crate::ImError::PermissionDenied);
        }
        let state_blob = self.seal_state(state)?;
        transaction
            .execute(
                r#"INSERT INTO direct_e2ee_v2_replay
 (owner_identity_id, local_device_id, sender_did, sender_device_id, message_id,
  ciphertext_digest, session_id, processed_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
                params![
                    owner_identity_id,
                    state.binding.local_device_id,
                    state.binding.peer_did,
                    state.binding.peer_device_id,
                    message_id,
                    ciphertext_digest,
                    state.session_id,
                    now,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let _ = write_state_for_owner_cas(
            &transaction,
            owner_identity_id,
            state,
            &state_blob,
            expected_session,
            &now,
        )?;
        if let Some(opk_id) = consume_opk_id {
            let changed = transaction
                .execute(
                    r#"DELETE FROM direct_e2ee_v2_one_time_prekeys
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND key_id = ?3
  AND status = 'available'"#,
                    params![
                        owner_identity_id,
                        state.binding.local_device_id,
                        required("consume_opk_id", opk_id)?,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if changed != 1 {
                drop(transaction);
                return Err(crate::ImError::PermissionDenied);
            }
        }
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        self.gc_orphaned_v2_secrets()?;
        Ok(V2InboundCommit::Applied)
    }

    /// Deletes only unreferenced P5 v2 Vault records. It runs after each
    /// successful mutation and when the store is reopened, so a crash between
    /// DB commit and Vault deletion converges without deleting the current
    /// ratchet or retry record first.
    pub(crate) fn gc_orphaned_v2_secrets(&self) -> crate::ImResult<usize> {
        let mut live = Vec::<SecretRef>::new();
        let owner_identity_id = self.scope.owner_identity_id.as_str();
        let owner_did = self.scope.owner_did.as_str();
        let device_id = self.scope.local_device_id.as_str();

        let session_rows = query_scope_rows(
            self.connection,
            &self.scope,
            r#"SELECT session_id, state_blob FROM direct_e2ee_v2_sessions
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3"#,
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        for (session_id, blob) in session_rows {
            push_live_ref(
                &mut live,
                &blob,
                DirectSecretOpenExpectation {
                    owner_identity_id,
                    owner_did,
                    device_id,
                    kind: SecretKind::DirectE2eeV2SessionState,
                    key_id_prefix: direct_secret_key_id_prefix(
                        owner_identity_id,
                        "v2-session",
                        &session_id,
                        device_id,
                    ),
                },
            )?;
        }

        let pending_rows = query_scope_rows(
            self.connection,
            &self.scope,
            r#"SELECT operation_id, pending_blob FROM direct_e2ee_v2_pending
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3"#,
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        for (operation_id, blob) in pending_rows {
            push_live_ref(
                &mut live,
                &blob,
                DirectSecretOpenExpectation {
                    owner_identity_id,
                    owner_did,
                    device_id,
                    kind: SecretKind::DirectE2eeV2PendingOutbound,
                    key_id_prefix: direct_secret_key_id_prefix(
                        owner_identity_id,
                        "v2-pending",
                        &operation_id,
                        device_id,
                    ),
                },
            )?;
        }

        let bundle_rows = query_scope_rows(
            self.connection,
            &self.scope,
            r#"SELECT bundle_json, signed_prekey_private_blob
FROM direct_e2ee_v2_prekey_bundles
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3"#,
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        for (bundle_json, blob) in bundle_rows {
            let bundle: V2PrekeyBundle =
                serde_json::from_str(&bundle_json).map_err(serialization_error)?;
            bundle.validate_structure().map_err(v2_error)?;
            push_live_ref(
                &mut live,
                &blob,
                DirectSecretOpenExpectation {
                    owner_identity_id,
                    owner_did,
                    device_id,
                    kind: SecretKind::DirectE2eeV2SignedPrekeyPrivate,
                    key_id_prefix: direct_secret_key_id_prefix(
                        owner_identity_id,
                        "v2-signed-prekey",
                        &bundle.signed_prekey.key_id,
                        device_id,
                    ),
                },
            )?;
        }

        let opk_rows = query_scope_rows(
            self.connection,
            &self.scope,
            r#"SELECT key_id, private_key_blob FROM direct_e2ee_v2_one_time_prekeys
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3"#,
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        for (key_id, blob) in opk_rows {
            push_live_ref(
                &mut live,
                &blob,
                DirectSecretOpenExpectation {
                    owner_identity_id,
                    owner_did,
                    device_id,
                    kind: SecretKind::DirectE2eeV2OneTimePrekeyPrivate,
                    key_id_prefix: direct_secret_key_id_prefix(
                        owner_identity_id,
                        "v2-one-time-prekey",
                        &key_id,
                        device_id,
                    ),
                },
            )?;
        }

        let mut deleted = 0;
        for secret_ref in self.secret_vault.list()? {
            if is_current_scope_v2_secret(&secret_ref, &self.scope) && !live.contains(&secret_ref) {
                self.secret_vault.delete(&secret_ref)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    fn seal_state(&self, state: &V2DirectSessionState) -> crate::ImResult<Vec<u8>> {
        self.scope.validate_binding(&state.binding)?;
        let raw = serialize_session_state_v2(state).map_err(v2_error)?;
        self.seal(
            SecretKind::DirectE2eeV2SessionState,
            direct_secret_key_id(
                &self.scope.owner_identity_id,
                "v2-session",
                &state.session_id,
                &self.scope.local_device_id,
            ),
            &raw,
            "P5 v2 session state",
        )
    }

    fn seal_pending(&self, pending: &V2PendingOutboundRecord) -> crate::ImResult<Vec<u8>> {
        pending.validate().map_err(v2_error)?;
        self.scope.validate_binding(&pending.binding)?;
        let raw = serialize_pending_outbound_v2(pending).map_err(v2_error)?;
        self.seal(
            SecretKind::DirectE2eeV2PendingOutbound,
            direct_secret_key_id(
                &self.scope.owner_identity_id,
                "v2-pending",
                &pending.operation_id,
                &self.scope.local_device_id,
            ),
            &raw,
            "P5 v2 pending outbound",
        )
    }

    fn seal(
        &self,
        kind: SecretKind,
        key_id: String,
        plaintext: &[u8],
        field: &'static str,
    ) -> crate::ImResult<Vec<u8>> {
        seal_direct_secret_blob(
            Some(&self.secret_vault),
            DirectSecretSealInput {
                owner_identity_id: &self.scope.owner_identity_id,
                owner_did: &self.scope.owner_did,
                device_id: Some(&self.scope.local_device_id),
                kind,
                key_id,
                plaintext,
                field,
            },
        )
    }

    fn open_state(
        &self,
        blob: Vec<u8>,
        expected_binding: &V2SessionBinding,
        expected_session_id: &str,
    ) -> crate::ImResult<V2DirectSessionState> {
        self.scope.validate_binding(expected_binding)?;
        let raw = open_direct_secret_blob_strict(
            &self.secret_vault,
            blob,
            &DirectSecretOpenExpectation {
                owner_identity_id: &self.scope.owner_identity_id,
                owner_did: &expected_binding.local_did,
                device_id: &expected_binding.local_device_id,
                kind: SecretKind::DirectE2eeV2SessionState,
                key_id_prefix: direct_secret_key_id_prefix(
                    &self.scope.owner_identity_id,
                    "v2-session",
                    expected_session_id,
                    &expected_binding.local_device_id,
                ),
            },
        )?;
        let state = deserialize_session_state_v2(&raw).map_err(v2_error)?;
        if state.binding != *expected_binding || state.session_id != expected_session_id {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(state)
    }

    fn open_pending(
        &self,
        blob: Vec<u8>,
        expected_binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<V2PendingOutboundRecord> {
        self.scope.validate_binding(expected_binding)?;
        let raw = open_direct_secret_blob_strict(
            &self.secret_vault,
            blob,
            &DirectSecretOpenExpectation {
                owner_identity_id: &self.scope.owner_identity_id,
                owner_did: &expected_binding.local_did,
                device_id: &expected_binding.local_device_id,
                kind: SecretKind::DirectE2eeV2PendingOutbound,
                key_id_prefix: direct_secret_key_id_prefix(
                    &self.scope.owner_identity_id,
                    "v2-pending",
                    operation_id,
                    &expected_binding.local_device_id,
                ),
            },
        )?;
        let pending = deserialize_pending_outbound_v2(&raw).map_err(v2_error)?;
        if pending.binding != *expected_binding || pending.operation_id != operation_id {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(pending)
    }

    fn validate_pending_session_checkpoint(
        &self,
        connection: &Connection,
        binding: &V2SessionBinding,
        session_id: &str,
        minimum_revision: i64,
    ) -> crate::ImResult<()> {
        self.scope.validate_binding(binding)?;
        if minimum_revision < 0 {
            return Err(crate::ImError::PermissionDenied);
        }
        let row = connection
            .query_row(
                r#"SELECT state_blob, revision FROM direct_e2ee_v2_sessions
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3
  AND peer_did = ?4 AND peer_device_id = ?5 AND session_id = ?6"#,
                params![
                    self.scope.owner_identity_id,
                    self.scope.owner_did,
                    binding.local_device_id,
                    binding.peer_did,
                    binding.peer_device_id,
                    required("session_id", session_id)?,
                ],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let Some((state_blob, current_revision)) = row else {
            return Err(crate::ImError::PermissionDenied);
        };
        if current_revision < minimum_revision {
            return Err(crate::ImError::PermissionDenied);
        }
        self.open_state(state_blob, binding, session_id)?;
        Ok(())
    }

    fn open_fixed_private(
        &self,
        blob: Vec<u8>,
        expected: DirectSecretOpenExpectation<'_>,
    ) -> crate::ImResult<x25519_dalek::StaticSecret> {
        let raw = open_direct_secret_blob_strict(&self.secret_vault, blob, &expected)?;
        let bytes: [u8; 32] = raw
            .try_into()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        Ok(x25519_dalek::StaticSecret::from(bytes))
    }

    fn open_signed_prekey_material(
        &self,
        blob: Vec<u8>,
        expected: DirectSecretOpenExpectation<'_>,
    ) -> crate::ImResult<(x25519_dalek::StaticSecret, Option<Vec<V2OneTimePrekey>>)> {
        let raw = Zeroizing::new(open_direct_secret_blob_strict(
            &self.secret_vault,
            blob,
            &expected,
        )?);
        if raw.len() == 32 {
            let bytes: Zeroizing<[u8; 32]> = Zeroizing::new(
                raw.as_slice()
                    .try_into()
                    .map_err(|_| crate::ImError::PermissionDenied)?,
            );
            return Ok((x25519_dalek::StaticSecret::from(*bytes), None));
        }
        let private_start = V2_SIGNED_PREKEY_MATERIAL_PREFIX.len();
        let public_start = private_start
            .checked_add(32)
            .ok_or(crate::ImError::PermissionDenied)?;
        if !raw.starts_with(V2_SIGNED_PREKEY_MATERIAL_PREFIX) || raw.len() <= public_start {
            return Err(crate::ImError::PermissionDenied);
        }
        let private_bytes: Zeroizing<[u8; 32]> = Zeroizing::new(
            raw[private_start..public_start]
                .try_into()
                .map_err(|_| crate::ImError::PermissionDenied)?,
        );
        let public_batch: Vec<V2OneTimePrekey> =
            serde_json::from_slice(&raw[public_start..]).map_err(serialization_error)?;
        validate_published_one_time_prekeys(&public_batch)?;
        Ok((
            x25519_dalek::StaticSecret::from(*private_bytes),
            Some(public_batch),
        ))
    }

    fn transaction(&self) -> crate::ImResult<Transaction<'_>> {
        self.connection
            .unchecked_transaction()
            .map_err(crate::internal::local_state::local_state_unavailable)
    }
}

fn query_scope_rows<T, F>(
    connection: &Connection,
    scope: &V2OwnerScope,
    sql: &str,
    map: F,
) -> crate::ImResult<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut statement = connection
        .prepare(sql)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![
                scope.owner_identity_id,
                scope.owner_did,
                scope.local_device_id,
            ],
            map,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(rows)
}

fn push_live_ref(
    live: &mut Vec<SecretRef>,
    blob: &[u8],
    expected: DirectSecretOpenExpectation<'_>,
) -> crate::ImResult<()> {
    let secret_ref = direct_secret_ref_from_blob(blob)?;
    validate_direct_secret_ref(&secret_ref, &expected)?;
    live.push(secret_ref);
    Ok(())
}

fn is_v2_secret_kind(kind: &SecretKind) -> bool {
    matches!(
        kind,
        SecretKind::DirectE2eeV2SignedPrekeyPrivate
            | SecretKind::DirectE2eeV2OneTimePrekeyPrivate
            | SecretKind::DirectE2eeV2SessionState
            | SecretKind::DirectE2eeV2PendingOutbound
    )
}

fn is_current_scope_v2_secret(secret_ref: &SecretRef, scope: &V2OwnerScope) -> bool {
    secret_ref.workspace_id == "awiki-im-core"
        && secret_ref.identity_id.as_deref() == Some(scope.owner_identity_id.as_str())
        && secret_ref.did.as_deref() == Some(scope.owner_did.as_str())
        && secret_ref.device_id == scope.local_device_id
        && is_v2_secret_kind(&secret_ref.kind)
}

fn write_state_for_owner_cas(
    transaction: &Transaction<'_>,
    owner_identity_id: &str,
    state: &V2DirectSessionState,
    state_blob: &[u8],
    expected: V2SessionExpectation,
    now: &str,
) -> crate::ImResult<i64> {
    state.validate().map_err(v2_error)?;
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let now = required("now", now)?;
    let revision = match expected {
        V2SessionExpectation::Absent => {
            let inserted = transaction
                .execute(
                    r#"INSERT INTO direct_e2ee_v2_sessions
 (owner_identity_id, owner_did, local_device_id, peer_did, peer_device_id,
  session_id, state_blob, revision, disabled, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?9)
ON CONFLICT(owner_identity_id, local_device_id, peer_did, peer_device_id, session_id)
DO NOTHING"#,
                    params![
                        owner_identity_id,
                        state.binding.local_did,
                        state.binding.local_device_id,
                        state.binding.peer_did,
                        state.binding.peer_device_id,
                        state.session_id,
                        state_blob,
                        i64::from(state.disabled),
                        now,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if inserted != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
            0
        }
        V2SessionExpectation::Revision(expected_revision) => {
            validate_storage_revision(expected_revision)?;
            let next_revision = expected_revision
                .checked_add(1)
                .ok_or(crate::ImError::PermissionDenied)?;
            let updated = transaction
                .execute(
                    r#"UPDATE direct_e2ee_v2_sessions
SET state_blob = ?1, revision = ?2, disabled = ?3, updated_at = ?4
WHERE owner_identity_id = ?5 AND owner_did = ?6 AND local_device_id = ?7
  AND peer_did = ?8 AND peer_device_id = ?9 AND session_id = ?10
  AND revision = ?11"#,
                    params![
                        state_blob,
                        next_revision,
                        i64::from(state.disabled),
                        now,
                        owner_identity_id,
                        state.binding.local_did,
                        state.binding.local_device_id,
                        state.binding.peer_did,
                        state.binding.peer_device_id,
                        state.session_id,
                        expected_revision,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            if updated != 1 {
                return Err(crate::ImError::PermissionDenied);
            }
            next_revision
        }
    };
    Ok(revision)
}

fn validate_storage_revision(revision: i64) -> crate::ImResult<()> {
    if revision < 0 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_owner_scope(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    local_device_id: &str,
) -> crate::ImResult<()> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let owner_did = required("owner_did", owner_did)?;
    let local_device_id = required("local_device_id", local_device_id)?;
    let stored_did = connection
        .query_row(
            r#"SELECT owner_did FROM direct_e2ee_v2_owner_scopes
WHERE owner_identity_id = ?1 AND local_device_id = ?2"#,
            params![owner_identity_id, local_device_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if stored_did.as_deref() != Some(owner_did.as_str()) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn required(field: &str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn decode_x25519(value: &str) -> crate::ImResult<[u8; 32]> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .try_into()
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn encode_signed_prekey_material(
    private: &x25519_dalek::StaticSecret,
    public_batch: &[V2OneTimePrekey],
) -> crate::ImResult<Zeroizing<Vec<u8>>> {
    validate_published_one_time_prekeys(public_batch)?;
    let public_json = serde_json::to_vec(public_batch).map_err(serialization_error)?;
    let mut material = Zeroizing::new(Vec::with_capacity(
        V2_SIGNED_PREKEY_MATERIAL_PREFIX.len() + 32 + public_json.len(),
    ));
    material.extend_from_slice(V2_SIGNED_PREKEY_MATERIAL_PREFIX);
    let private_bytes = Zeroizing::new(private.to_bytes());
    material.extend_from_slice(private_bytes.as_slice());
    material.extend_from_slice(&public_json);
    Ok(material)
}

fn validate_published_one_time_prekeys(public_batch: &[V2OneTimePrekey]) -> crate::ImResult<()> {
    let mut previous_key_id: Option<&str> = None;
    for public in public_batch {
        public.validate().map_err(v2_error)?;
        if previous_key_id.is_some_and(|previous| previous >= public.key_id.as_str()) {
            return Err(crate::ImError::PermissionDenied);
        }
        previous_key_id = Some(public.key_id.as_str());
    }
    Ok(())
}

fn serialization_error(error: serde_json::Error) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

fn validate_existing_private_sidecar(
    connection: &Connection,
    owner_identity_id: &str,
    local_device_id: &str,
    operation_id: &str,
    expected: Option<&V2PrivateOutboundSidecar>,
) -> crate::ImResult<()> {
    let row = connection
        .query_row(
            r#"SELECT delivery_class, context_json
FROM direct_e2ee_v2_private_outbound
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
            params![owner_identity_id, local_device_id, operation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    match (row, expected) {
        (None, None) => Ok(()),
        (Some((delivery_class, context_json)), Some(expected)) => {
            expected.validate()?;
            let context: V2RootControlPrivateContext =
                serde_json::from_str(&context_json).map_err(serialization_error)?;
            if delivery_class == expected.delivery_class() && context == expected.context {
                Ok(())
            } else {
                Err(crate::ImError::PermissionDenied)
            }
        }
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn v2_error(_: anp::direct_e2ee::DirectE2eeV2Error) -> crate::ImError {
    crate::ImError::PermissionDenied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
        IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };
    use crate::vault::{DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore};
    use anp::direct_e2ee::{
        V2DirectCipherBody, V2DirectSessionState, V2PendingOutboundRecord, V2PrekeyBundle,
        V2RatchetHeader, V2SessionBinding, V2SignedPrekey, CONTENT_TYPE_DIRECT_CIPHER_V2,
        DIRECT_E2EE_V2_PENDING_STATE_FORMAT, DIRECT_E2EE_V2_SESSION_STATE_FORMAT,
        MTI_DIRECT_E2EE_SUITE_V2, V2_SESSION_STATUS_PENDING_CONFIRMATION,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::sync::Arc;

    fn vault(dir: &std::path::Path) -> DirectSecretVault {
        Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([43; 32]),
            FileSecretVaultStore::new(dir),
        ))
    }

    fn store<'a>(db: &'a Connection, dir: &std::path::Path) -> SqliteV2DirectStateStore<'a> {
        SqliteV2DirectStateStore::new_with_secret_vault(db, vault(dir), owner_scope()).unwrap()
    }

    fn owner_scope() -> V2OwnerScope {
        owner_scope_for("identity-alice", "did:example:alice", "alice-phone")
    }

    fn owner_scope_for(identity_id: &str, did: &str, device_id: &str) -> V2OwnerScope {
        let identity_id = crate::ids::IdentityId::parse(identity_id).unwrap();
        let did = crate::ids::Did::parse(did).unwrap();
        let state = IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse(device_id).unwrap(),
                signing_key_id: format!("{}#device-sign", did.as_str()),
                e2ee_key_id: format!("{}#device-e2ee", did.as_str()),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        };
        V2OwnerScope::from_identity_state(&identity_id, &did, &state).unwrap()
    }

    fn pending_state_for_scope(did: &str, device_id: &str) -> V2DirectSessionState {
        let mut state = pending_state();
        state.binding.local_did = did.to_owned();
        state.binding.local_device_id = device_id.to_owned();
        state.binding.local_e2ee_key_id = format!("{did}#device-e2ee");
        state
    }

    fn count_kind(vault: &DirectSecretVault, kind: SecretKind) -> usize {
        vault
            .list()
            .unwrap()
            .into_iter()
            .filter(|secret_ref| secret_ref.kind == kind)
            .count()
    }

    fn pending_state() -> V2DirectSessionState {
        let ratchet = x25519_dalek::StaticSecret::from([7; 32]);
        V2DirectSessionState {
            state_format: DIRECT_E2EE_V2_SESSION_STATE_FORMAT.to_owned(),
            binding: V2SessionBinding {
                local_did: "did:example:alice".to_owned(),
                local_device_id: "alice-phone".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                peer_device_id: "bob-phone".to_owned(),
                suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
                local_e2ee_key_id: "did:example:alice#phone-e2ee".to_owned(),
                peer_e2ee_key_id: "did:example:bob#phone-e2ee".to_owned(),
            },
            session_id: URL_SAFE_NO_PAD.encode([1; 16]),
            root_key_b64u: URL_SAFE_NO_PAD.encode([2; 32]),
            send_chain_key_b64u: Some(URL_SAFE_NO_PAD.encode([3; 32])),
            recv_chain_key_b64u: None,
            ratchet_private_key_b64u: URL_SAFE_NO_PAD.encode(ratchet.to_bytes()),
            ratchet_public_key_b64u: URL_SAFE_NO_PAD
                .encode(x25519_dalek::PublicKey::from(&ratchet).to_bytes()),
            peer_ratchet_public_key_b64u: None,
            send_n: 1,
            recv_n: 0,
            previous_send_chain_length: 0,
            skipped_message_keys: vec![],
            is_initiator: true,
            status: V2_SESSION_STATUS_PENDING_CONFIRMATION.to_owned(),
            disabled: false,
        }
    }

    fn pending_record(state: &V2DirectSessionState, ciphertext: &[u8]) -> V2PendingOutboundRecord {
        V2PendingOutboundRecord {
            state_format: DIRECT_E2EE_V2_PENDING_STATE_FORMAT.to_owned(),
            binding: state.binding.clone(),
            session_id: state.session_id.clone(),
            operation_id: "message-1".to_owned(),
            message_id: "message-1".to_owned(),
            wire_content_type: CONTENT_TYPE_DIRECT_CIPHER_V2.to_owned(),
            body: serde_json::to_value(V2DirectCipherBody {
                session_id: state.session_id.clone(),
                suite: Some(MTI_DIRECT_E2EE_SUITE_V2.to_owned()),
                ratchet_header: V2RatchetHeader {
                    dh_pub_b64u: URL_SAFE_NO_PAD.encode([8; 32]),
                    pn: "0".to_owned(),
                    n: "0".to_owned(),
                },
                ciphertext_b64u: URL_SAFE_NO_PAD.encode(ciphertext),
            })
            .unwrap(),
        }
    }

    fn local_bundle(signed_prekey_private: &x25519_dalek::StaticSecret) -> V2PrekeyBundle {
        V2PrekeyBundle {
            bundle_id: "bundle-1".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            owner_device_id: "alice-phone".to_owned(),
            suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
            static_key_agreement_id: "did:example:alice#phone-e2ee".to_owned(),
            signed_prekey: V2SignedPrekey {
                key_id: "spk-1".to_owned(),
                public_key_b64u: URL_SAFE_NO_PAD
                    .encode(x25519_dalek::PublicKey::from(signed_prekey_private).to_bytes()),
                expires_at: "2026-07-20T00:00:00Z".to_owned(),
            },
            proof: serde_json::json!({
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "verificationMethod": "did:example:alice#phone-sign",
                "proofPurpose": "assertionMethod",
                "created": "2026-07-19T00:00:00Z",
                "proofValue": "zProof"
            }),
        }
    }

    #[test]
    fn rotated_prekey_bundles_keep_only_two_delayed_init_windows() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let store = store(&db, root.path());
        for index in 1_u8..=4 {
            let private = x25519_dalek::StaticSecret::from([index; 32]);
            let mut bundle = local_bundle(&private);
            bundle.bundle_id = format!("bundle-{index}");
            bundle.signed_prekey.key_id = format!("spk-{index}");
            bundle.signed_prekey.expires_at = "2030-01-01T00:00:00Z".to_owned();
            store
                .publish_local_bundle(
                    &bundle,
                    &private,
                    &[],
                    &format!("2026-07-20T00:00:0{index}Z"),
                )
                .unwrap();
        }
        assert!(store
            .load_accepted_bundle("bundle-1", "2026-07-20T00:01:00Z")
            .unwrap()
            .is_none());
        for bundle_id in ["bundle-2", "bundle-3", "bundle-4"] {
            assert!(store
                .load_accepted_bundle(bundle_id, "2026-07-20T00:01:00Z")
                .unwrap()
                .is_some());
        }
    }

    #[test]
    fn authoritative_scope_rejects_first_wrong_local_binding() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let store = store(&db, root.path());

        let mut wrong_did = pending_state();
        wrong_did.binding.local_did = "did:example:mallory".to_owned();
        wrong_did.binding.local_e2ee_key_id = "did:example:mallory#phone-e2ee".to_owned();
        assert!(store
            .commit_inbound(
                &wrong_did,
                "wrong-did",
                "sha256:wrong-did",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .is_err());

        let mut wrong_device = pending_state();
        wrong_device.binding.local_device_id = "alice-tablet".to_owned();
        assert!(store
            .commit_inbound(
                &wrong_device,
                "wrong-device",
                "sha256:wrong-device",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:01Z",
            )
            .is_err());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM direct_e2ee_v2_sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }

    #[test]
    fn v2_tables_and_vault_kinds_are_separate_from_v1() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let store = store(&db, root.path());
        let state = pending_state();

        store
            .commit_inbound(
                &state,
                "setup-message",
                "sha256:setup-cipher",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .unwrap();
        let loaded = store
            .load_session(&state.binding, &state.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.state, state);
        assert_eq!(loaded.revision, 0);
        let mut wrong_owner_did_state = state.clone();
        wrong_owner_did_state.binding.local_did = "did:example:mallory".to_owned();
        wrong_owner_did_state.binding.local_e2ee_key_id =
            "did:example:mallory#phone-e2ee".to_owned();
        assert!(store
            .commit_inbound(
                &wrong_owner_did_state,
                "wrong-owner-message",
                "sha256:wrong-owner-cipher",
                None,
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:01Z",
            )
            .is_err());
        let mut other_device = state.binding.clone();
        other_device.peer_device_id = "bob-tablet".to_owned();
        assert!(store
            .load_session(&other_device, &state.session_id)
            .unwrap()
            .is_none());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM direct_e2ee_sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert!(std::fs::read_dir(root.path().join("records"))
            .unwrap()
            .next()
            .is_some());
        let state_ref = store
            .secret_vault
            .list()
            .unwrap()
            .into_iter()
            .find(|secret_ref| secret_ref.kind == SecretKind::DirectE2eeV2SessionState)
            .unwrap();
        assert_eq!(state_ref.identity_id.as_deref(), Some("identity-alice"));
        assert_eq!(state_ref.did.as_deref(), Some("did:example:alice"));
        assert_eq!(state_ref.device_id, "alice-phone");
    }

    #[test]
    fn replay_and_session_commit_is_idempotent_and_conflict_closed() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let store = store(&db, root.path());
        let state = pending_state();

        assert_eq!(
            store
                .commit_inbound(
                    &state,
                    "message-1",
                    "sha256:cipher-a",
                    None,
                    V2SessionExpectation::Absent,
                    "2026-07-19T00:00:00Z",
                )
                .unwrap(),
            V2InboundCommit::Applied
        );
        assert!(store
            .is_exact_inbound_replay(
                &state.binding,
                "message-1",
                "sha256:cipher-a",
                &state.session_id,
            )
            .unwrap());
        assert!(store
            .is_exact_inbound_replay(
                &state.binding,
                "message-1",
                "sha256:cipher-b",
                &state.session_id,
            )
            .is_err());
        assert_eq!(
            store
                .commit_inbound(
                    &state,
                    "message-1",
                    "sha256:cipher-a",
                    None,
                    V2SessionExpectation::Absent,
                    "2026-07-19T00:00:01Z",
                )
                .unwrap(),
            V2InboundCommit::Replay
        );
        assert!(store
            .commit_inbound(
                &state,
                "message-1",
                "sha256:cipher-b",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:02Z",
            )
            .is_err());
    }

    #[test]
    fn outbound_pending_is_exactly_idempotent_and_conflict_closed() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let store = store(&db, root.path());
        let state = pending_state();
        let pending = pending_record(&state, b"cipher-a");

        store
            .commit_inbound(
                &state,
                "prior-message",
                "sha256:prior-cipher",
                None,
                V2SessionExpectation::Absent,
                "2026-07-18T23:59:59Z",
            )
            .unwrap();

        store
            .commit_outbound(
                &state,
                &pending,
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:00Z",
            )
            .unwrap();
        store
            .commit_outbound(
                &state,
                &pending,
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:01Z",
            )
            .unwrap();
        assert_eq!(
            store.load_pending(&state.binding, "message-1").unwrap(),
            Some(pending.clone())
        );
        assert_eq!(
            db.query_row("SELECT revision FROM direct_e2ee_v2_sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );

        assert!(store
            .commit_outbound(
                &state,
                &pending_record(&state, b"cipher-b"),
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:02Z",
            )
            .is_err());

        db.execute("UPDATE direct_e2ee_v2_sessions SET revision = 0", [])
            .unwrap();
        assert!(store
            .commit_outbound(
                &state,
                &pending,
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:03Z",
            )
            .is_err());
        assert!(store.load_pending(&state.binding, "message-1").is_err());
        db.execute("DELETE FROM direct_e2ee_v2_sessions", [])
            .unwrap();
        assert!(store
            .commit_outbound(
                &state,
                &pending,
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:04Z",
            )
            .is_err());
    }

    #[test]
    fn inbound_opk_consumption_rolls_back_with_replay_and_session() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let store = store(&db, root.path());
        let state = pending_state();

        assert!(store
            .commit_inbound(
                &state,
                "message-missing-opk",
                "sha256:cipher-a",
                Some("opk-1"),
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .is_err());
        for table in ["direct_e2ee_v2_sessions", "direct_e2ee_v2_replay"] {
            assert_eq!(
                db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                0
            );
        }

        let signed_prekey_private = x25519_dalek::StaticSecret::from([9; 32]);
        let opk_private = x25519_dalek::StaticSecret::from([10; 32]);
        let opk = V2OneTimePrekey {
            key_id: "opk-1".to_owned(),
            public_key_b64u: URL_SAFE_NO_PAD
                .encode(x25519_dalek::PublicKey::from(&opk_private).to_bytes()),
        };
        store
            .publish_local_bundle(
                &local_bundle(&signed_prekey_private),
                &signed_prekey_private,
                &[(opk, opk_private)],
                "2026-07-19T00:00:01Z",
            )
            .unwrap();
        assert_eq!(
            store
                .commit_inbound(
                    &state,
                    "message-1",
                    "sha256:cipher-a",
                    Some("opk-1"),
                    V2SessionExpectation::Absent,
                    "2026-07-19T00:00:02Z",
                )
                .unwrap(),
            V2InboundCommit::Applied
        );
        assert_eq!(
            store
                .commit_inbound(
                    &state,
                    "message-1",
                    "sha256:cipher-a",
                    Some("opk-1"),
                    V2SessionExpectation::Absent,
                    "2026-07-19T00:00:03Z",
                )
                .unwrap(),
            V2InboundCommit::Replay
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM direct_e2ee_v2_one_time_prekeys WHERE key_id = 'opk-1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn inbound_opk_rollback_keeps_old_state_and_restart_cleans_orphan() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let vault = vault(root.path());
        let store =
            SqliteV2DirectStateStore::new_with_secret_vault(&db, vault.clone(), owner_scope())
                .unwrap();
        let state = pending_state();
        store
            .commit_inbound(
                &state,
                "message-1",
                "sha256:cipher-1",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .unwrap();
        let old_blob = db
            .query_row(
                "SELECT state_blob FROM direct_e2ee_v2_sessions",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap();
        let old_record = store
            .load_session(&state.binding, &state.session_id)
            .unwrap()
            .unwrap();

        let mut advanced = state.clone();
        advanced.root_key_b64u = URL_SAFE_NO_PAD.encode([21; 32]);
        assert!(store
            .commit_inbound(
                &advanced,
                "message-missing-opk",
                "sha256:cipher-missing-opk",
                Some("opk-missing"),
                V2SessionExpectation::Revision(old_record.revision),
                "2026-07-19T00:00:01Z",
            )
            .is_err());

        let current_blob = db
            .query_row(
                "SELECT state_blob FROM direct_e2ee_v2_sessions",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap();
        assert_eq!(current_blob, old_blob);
        assert_eq!(
            store
                .load_session(&state.binding, &state.session_id)
                .unwrap(),
            Some(old_record.clone())
        );
        assert_eq!(count_kind(&vault, SecretKind::DirectE2eeV2SessionState), 2);

        drop(store);
        let restarted =
            SqliteV2DirectStateStore::new_with_secret_vault(&db, vault.clone(), owner_scope())
                .unwrap();
        assert_eq!(count_kind(&vault, SecretKind::DirectE2eeV2SessionState), 1);
        assert_eq!(
            restarted
                .load_session(&state.binding, &state.session_id)
                .unwrap(),
            Some(old_record)
        );
    }

    #[test]
    fn stale_session_writer_cannot_overwrite_committed_ratchet() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let vault = vault(root.path());
        let store =
            SqliteV2DirectStateStore::new_with_secret_vault(&db, vault.clone(), owner_scope())
                .unwrap();
        let state = pending_state();
        store
            .commit_inbound(
                &state,
                "message-1",
                "sha256:cipher-1",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .unwrap();
        let loaded = store
            .load_session(&state.binding, &state.session_id)
            .unwrap()
            .unwrap();

        let mut winner = loaded.state.clone();
        winner.root_key_b64u = URL_SAFE_NO_PAD.encode([22; 32]);
        store
            .commit_inbound(
                &winner,
                "message-2",
                "sha256:cipher-2",
                None,
                V2SessionExpectation::Revision(loaded.revision),
                "2026-07-19T00:00:01Z",
            )
            .unwrap();

        let mut stale = loaded.state;
        stale.root_key_b64u = URL_SAFE_NO_PAD.encode([23; 32]);
        assert!(store
            .commit_inbound(
                &stale,
                "message-3",
                "sha256:cipher-3",
                None,
                V2SessionExpectation::Revision(loaded.revision),
                "2026-07-19T00:00:02Z",
            )
            .is_err());
        let committed = store
            .load_session(&state.binding, &state.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(committed.state, winner);
        assert_eq!(committed.revision, 1);
        assert_eq!(count_kind(&vault, SecretKind::DirectE2eeV2SessionState), 2);

        drop(store);
        let restarted =
            SqliteV2DirectStateStore::new_with_secret_vault(&db, vault.clone(), owner_scope())
                .unwrap();
        assert_eq!(count_kind(&vault, SecretKind::DirectE2eeV2SessionState), 1);
        assert_eq!(
            restarted
                .load_session(&state.binding, &state.session_id)
                .unwrap(),
            Some(committed)
        );
    }

    #[test]
    fn gc_does_not_delete_another_scope_in_shared_vault() {
        let root = tempfile::tempdir().unwrap();
        let shared_vault = vault(root.path());
        let db_alice = Connection::open_in_memory().unwrap();
        let db_carol = Connection::open_in_memory().unwrap();
        let alice = SqliteV2DirectStateStore::new_with_secret_vault(
            &db_alice,
            shared_vault.clone(),
            owner_scope(),
        )
        .unwrap();
        let alice_state = pending_state();
        alice
            .commit_inbound(
                &alice_state,
                "alice-message-1",
                "sha256:alice-cipher-1",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .unwrap();

        let carol_scope = owner_scope_for("identity-carol", "did:example:carol", "carol-phone");
        let carol = SqliteV2DirectStateStore::new_with_secret_vault(
            &db_carol,
            shared_vault.clone(),
            carol_scope.clone(),
        )
        .unwrap();
        let carol_state = pending_state_for_scope("did:example:carol", "carol-phone");
        carol
            .commit_inbound(
                &carol_state,
                "carol-message-1",
                "sha256:carol-cipher-1",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:01Z",
            )
            .unwrap();
        let carol_ref = shared_vault
            .list()
            .unwrap()
            .into_iter()
            .find(|secret_ref| {
                secret_ref.kind == SecretKind::DirectE2eeV2SessionState
                    && secret_ref.identity_id.as_deref() == Some("identity-carol")
            })
            .unwrap();
        assert_eq!(
            count_kind(&shared_vault, SecretKind::DirectE2eeV2SessionState),
            2
        );

        assert_eq!(alice.gc_orphaned_v2_secrets().unwrap(), 0);
        assert!(shared_vault.open(&carol_ref).is_ok());
        assert_eq!(
            count_kind(&shared_vault, SecretKind::DirectE2eeV2SessionState),
            2
        );

        drop(carol);
        let restarted_carol = SqliteV2DirectStateStore::new_with_secret_vault(
            &db_carol,
            shared_vault.clone(),
            carol_scope,
        )
        .unwrap();
        let loaded = restarted_carol
            .load_session(&carol_state.binding, &carol_state.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.state, carol_state);
        assert_eq!(loaded.revision, 0);
        assert!(shared_vault.open(&carol_ref).is_ok());
    }

    #[test]
    fn v2_secret_columns_reject_plaintext_fallback() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let store = store(&db, root.path());
        let state = pending_state();
        store
            .commit_inbound(
                &state,
                "setup-message",
                "sha256:setup-cipher",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .unwrap();

        let sealed_state = db
            .query_row(
                "SELECT state_blob FROM direct_e2ee_v2_sessions",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap();
        db.execute(
            "UPDATE direct_e2ee_v2_sessions SET state_blob = ?1",
            params![serialize_session_state_v2(&state).unwrap()],
        )
        .unwrap();
        assert!(store
            .load_session(&state.binding, &state.session_id)
            .is_err());
        assert!(store.gc_orphaned_v2_secrets().is_err());
        db.execute(
            "UPDATE direct_e2ee_v2_sessions SET state_blob = ?1",
            params![sealed_state],
        )
        .unwrap();

        let pending = pending_record(&state, b"cipher-a");
        store
            .commit_outbound(
                &state,
                &pending,
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:01Z",
            )
            .unwrap();
        let sealed_pending = db
            .query_row(
                "SELECT pending_blob FROM direct_e2ee_v2_pending",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap();
        db.execute(
            "UPDATE direct_e2ee_v2_pending SET pending_blob = ?1",
            params![serialize_pending_outbound_v2(&pending).unwrap()],
        )
        .unwrap();
        assert!(store.load_pending(&state.binding, "message-1").is_err());
        db.execute(
            "UPDATE direct_e2ee_v2_pending SET pending_blob = ?1",
            params![sealed_pending],
        )
        .unwrap();

        let signed_prekey_private = x25519_dalek::StaticSecret::from([9; 32]);
        let opk_private = x25519_dalek::StaticSecret::from([10; 32]);
        let opk = V2OneTimePrekey {
            key_id: "opk-raw".to_owned(),
            public_key_b64u: URL_SAFE_NO_PAD
                .encode(x25519_dalek::PublicKey::from(&opk_private).to_bytes()),
        };
        store
            .publish_local_bundle(
                &local_bundle(&signed_prekey_private),
                &signed_prekey_private,
                &[(opk, opk_private)],
                "2026-07-19T00:00:02Z",
            )
            .unwrap();
        let sealed_spk = db
            .query_row(
                "SELECT signed_prekey_private_blob FROM direct_e2ee_v2_prekey_bundles",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap();
        db.execute(
            "UPDATE direct_e2ee_v2_prekey_bundles SET signed_prekey_private_blob = ?1",
            params![vec![9_u8; 32]],
        )
        .unwrap();
        assert!(store.load_active_bundle().is_err());
        db.execute(
            "UPDATE direct_e2ee_v2_prekey_bundles SET signed_prekey_private_blob = ?1",
            params![sealed_spk],
        )
        .unwrap();

        let sealed_opk = db
            .query_row(
                "SELECT private_key_blob FROM direct_e2ee_v2_one_time_prekeys WHERE key_id = 'opk-raw'",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap();
        db.execute(
            "UPDATE direct_e2ee_v2_one_time_prekeys SET private_key_blob = ?1 WHERE key_id = 'opk-raw'",
            params![vec![10_u8; 32]],
        )
        .unwrap();
        assert!(store.load_available_opk("bundle-1", "opk-raw").is_err());
        db.execute(
            "UPDATE direct_e2ee_v2_one_time_prekeys SET private_key_blob = ?1 WHERE key_id = 'opk-raw'",
            params![sealed_opk],
        )
        .unwrap();
    }

    #[test]
    fn v2_gc_removes_replaced_acked_and_restart_orphan_secrets() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        let vault = vault(root.path());
        let store =
            SqliteV2DirectStateStore::new_with_secret_vault(&db, vault.clone(), owner_scope())
                .unwrap();
        let state = pending_state();

        store
            .commit_inbound(
                &state,
                "message-1",
                "sha256:cipher-1",
                None,
                V2SessionExpectation::Absent,
                "2026-07-19T00:00:00Z",
            )
            .unwrap();
        store
            .commit_inbound(
                &state,
                "message-2",
                "sha256:cipher-2",
                None,
                V2SessionExpectation::Revision(0),
                "2026-07-19T00:00:01Z",
            )
            .unwrap();
        assert_eq!(count_kind(&vault, SecretKind::DirectE2eeV2SessionState), 1);

        let pending = pending_record(&state, b"cipher-a");
        store
            .commit_outbound(
                &state,
                &pending,
                V2SessionExpectation::Revision(1),
                "2026-07-19T00:00:02Z",
            )
            .unwrap();
        assert_eq!(
            count_kind(&vault, SecretKind::DirectE2eeV2PendingOutbound),
            1
        );
        assert!(store
            .mark_pending_accepted(&state.binding, "message-1")
            .unwrap());
        assert_eq!(
            count_kind(&vault, SecretKind::DirectE2eeV2PendingOutbound),
            0
        );

        let _orphan = store.seal_state(&state).unwrap();
        assert_eq!(count_kind(&vault, SecretKind::DirectE2eeV2SessionState), 2);
        drop(store);
        let restarted =
            SqliteV2DirectStateStore::new_with_secret_vault(&db, vault.clone(), owner_scope())
                .unwrap();
        assert_eq!(count_kind(&vault, SecretKind::DirectE2eeV2SessionState), 1);
        let loaded = restarted
            .load_session(&state.binding, &state.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.state, state);
        assert_eq!(loaded.revision, 2);
    }
}
