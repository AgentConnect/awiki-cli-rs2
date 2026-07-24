//! Receiver-only RootKeyEnvelope validation and crash coordinator.
//!
//! This module is deliberately below the message projection boundary. It
//! stores canonical root DER only in the encrypted pending Vault kind and
//! keeps SQLite state secret-free.

use anp::direct_e2ee::{
    V2DirectBody, V2DirectMetadata, V2DirectSessionState, V2SecretJsonPayload,
    DIRECT_E2EE_PROFILE_V2, V2_SESSION_STATUS_ESTABLISHED,
};
use base64::engine::general_purpose::STANDARD;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::internal::identity_device_join_runtime::{
    DeviceJoinAdminHttpAdapter, DeviceJoinAdminRemote, DeviceJoinRemoteDeviceSummary,
    DeviceJoinRemoteRegistry,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityDeviceMode,
};
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::{
    policy::SecretAccessPolicy,
    record::{SecretKind, SecretMetadata, SecretRef},
    SealIfAbsentResult, SealSecretRequest,
};
use crate::internal::secure_direct::v2_runtime::{
    V2EstablishedDirectRuntime, V2ValidatedSecretInboundOutcome,
};
use crate::internal::secure_direct::v2_store::{SqliteV2DirectStateStore, V2OwnerScope};
use crate::internal::transport::AsyncAuthenticatedRpcTransport;

pub(crate) const ROOT_KEY_ENVELOPE_SYSTEM_TYPE: &str = "awiki.device.root-key-envelope.v1";
const ROOT_ENVELOPE_MAX_WINDOW_SECONDS: i64 = 600;
const ED25519_PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrustedDirectDeliverySource {
    Mailbox,
    ReliableSync,
    Realtime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrustedDirectDeliveryContext {
    pub(crate) message_id: String,
    pub(crate) operation_id: String,
    pub(crate) accepted_at: Option<String>,
    pub(crate) sender_did: String,
    pub(crate) sender_device_id: String,
    pub(crate) recipient_did: String,
    pub(crate) recipient_device_id: String,
    pub(crate) method: String,
    pub(crate) target_kind: String,
    pub(crate) profile: String,
    pub(crate) security_profile: String,
    pub(crate) content_type: String,
    pub(crate) source: TrustedDirectDeliverySource,
}

impl TrustedDirectDeliveryContext {
    pub(crate) fn from_stored_message(
        metadata: &V2DirectMetadata,
        accepted_at: Option<String>,
        source: TrustedDirectDeliverySource,
    ) -> crate::ImResult<Self> {
        if source == TrustedDirectDeliverySource::Realtime {
            return Err(crate::ImError::PermissionDenied);
        }
        metadata
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let context = Self {
            message_id: metadata.message_id.clone(),
            operation_id: metadata.operation_id.clone(),
            accepted_at,
            sender_did: metadata.sender_did.clone(),
            sender_device_id: metadata.sender_device_id.clone(),
            recipient_did: metadata.target.did.clone(),
            recipient_device_id: metadata.recipient_device_id.clone(),
            method: "direct.send".to_owned(),
            target_kind: metadata.target.kind.clone(),
            profile: metadata.profile.clone(),
            security_profile: metadata.security_profile.clone(),
            content_type: metadata.content_type.clone(),
            source,
        };
        context.validate()?;
        Ok(context)
    }

    pub(crate) fn realtime_hint(metadata: &V2DirectMetadata) -> crate::ImResult<Self> {
        metadata
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let context = Self {
            message_id: metadata.message_id.clone(),
            operation_id: metadata.operation_id.clone(),
            accepted_at: None,
            sender_did: metadata.sender_did.clone(),
            sender_device_id: metadata.sender_device_id.clone(),
            recipient_did: metadata.target.did.clone(),
            recipient_device_id: metadata.recipient_device_id.clone(),
            method: "direct.send".to_owned(),
            target_kind: metadata.target.kind.clone(),
            profile: metadata.profile.clone(),
            security_profile: metadata.security_profile.clone(),
            content_type: metadata.content_type.clone(),
            source: TrustedDirectDeliverySource::Realtime,
        };
        context.validate()?;
        Ok(context)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.message_id.trim().is_empty()
            || self.operation_id != self.message_id
            || self.method != "direct.send"
            || self.target_kind != "agent"
            || self.profile != DIRECT_E2EE_PROFILE_V2
            || self.security_profile != "direct-e2ee"
            || self.sender_did.trim().is_empty()
            || self.recipient_did.trim().is_empty()
            || self.sender_device_id.trim().is_empty()
            || self.recipient_device_id.trim().is_empty()
            || self.sender_device_id == self.recipient_device_id
            || !matches!(
                self.content_type.as_str(),
                "application/anp-direct-init+json" | "application/anp-direct-cipher+json"
            )
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if self.source == TrustedDirectDeliverySource::Realtime && self.accepted_at.is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RootImportCompletionPhase {
    ImportSealed,
    ProofPrepared,
    CompletionPending,
    CompletionAccepted,
    TokenRefreshed,
    RegistryConfirmed,
    Promoted,
    TerminalFailed,
}

impl RootImportCompletionPhase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ImportSealed => "import_sealed",
            Self::ProofPrepared => "proof_prepared",
            Self::CompletionPending => "completion_pending",
            Self::CompletionAccepted => "completion_accepted",
            Self::TokenRefreshed => "token_refreshed",
            Self::RegistryConfirmed => "registry_confirmed",
            Self::Promoted => "promoted",
            Self::TerminalFailed => "terminal_failed",
        }
    }
}

#[derive(Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootKeyEnvelope {
    pub(crate) system_type: String,
    pub(crate) message_id: String,
    pub(crate) did: String,
    pub(crate) root_key_id: String,
    pub(crate) root_public_key_fingerprint: String,
    pub(crate) root_private_key_pkcs8_b64u: String,
    pub(crate) sender_device_id: String,
    pub(crate) sender_e2ee_key_id: String,
    pub(crate) recipient_device_id: String,
    pub(crate) recipient_e2ee_key_id: String,
    pub(crate) document_version: u64,
    pub(crate) document_hash: String,
    pub(crate) registry_version: u64,
    pub(crate) issued_at: String,
    pub(crate) expires_at: String,
}

impl std::fmt::Debug for RootKeyEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RootKeyEnvelope")
            .field("value", &"<redacted-root-key-envelope>")
            .finish()
    }
}

pub(crate) enum RootSecretPayload {
    NotRoot,
    Root(Zeroizing<RootKeyEnvelope>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootInboundInterceptOutcome {
    NotRoot,
    Consumed,
    Replay,
    SuppressedForHydration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootProbeOutcome {
    NotRoot,
    Root,
    Replay,
}

enum RootInboundValidation {
    NotRoot,
    Root(RootImportSealedPlan),
    Terminal,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RootImportSealedPlan {
    owner_identity_id: String,
    owner_did: String,
    local_device_id: String,
    message_id: String,
    sender_device_id: String,
    recipient_device_id: String,
    sender_e2ee_key_id: String,
    recipient_e2ee_key_id: String,
    accepted_at: String,
    imported_at: String,
    envelope_expires_at: String,
    pending_root_ref_json: String,
    root_key_id: String,
    root_fingerprint: String,
    document_version: u64,
    document_hash: String,
    registry_version: u64,
    now: String,
}

#[derive(Debug, Deserialize)]
struct RootTypeProbe {
    system_type: Option<String>,
}

pub(crate) fn decode_root_secret_payload(
    plaintext: &V2SecretJsonPayload,
) -> crate::ImResult<RootSecretPayload> {
    // This probe intentionally ignores every member except `system_type`, so
    // it never materializes the private-key string in an ordinary JSON tree.
    let probe: RootTypeProbe =
        serde_json::from_slice(plaintext.expose_secret()).map_err(redacted_serialization)?;
    let Some(system_type) = probe.system_type.as_deref() else {
        return Ok(RootSecretPayload::NotRoot);
    };
    if system_type != ROOT_KEY_ENVELOPE_SYSTEM_TYPE {
        return Ok(RootSecretPayload::NotRoot);
    }
    let envelope: RootKeyEnvelope =
        serde_json::from_slice(plaintext.expose_secret()).map_err(redacted_serialization)?;
    let canonical = Zeroizing::new(
        serde_json_canonicalizer::to_vec(&envelope).map_err(redacted_serialization)?,
    );
    if canonical.as_slice() != plaintext.expose_secret() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(RootSecretPayload::Root(Zeroizing::new(envelope)))
}

fn root_system_type(plaintext: &V2SecretJsonPayload) -> crate::ImResult<bool> {
    let probe: RootTypeProbe =
        serde_json::from_slice(plaintext.expose_secret()).map_err(redacted_serialization)?;
    Ok(probe.system_type.as_deref() == Some(ROOT_KEY_ENVELOPE_SYSTEM_TYPE))
}

pub(crate) async fn receive_root_envelope_candidate(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    metadata: &V2DirectMetadata,
    body: &V2DirectBody,
    delivery: &TrustedDirectDeliveryContext,
) -> crate::ImResult<RootInboundInterceptOutcome> {
    if metadata.profile != DIRECT_E2EE_PROFILE_V2
        || metadata.sender_did != client.did().as_str()
        || metadata.target.did != client.did().as_str()
        || metadata.sender_device_id == metadata.recipient_device_id
    {
        return Ok(RootInboundInterceptOutcome::NotRoot);
    }
    delivery.validate()?;
    let local_entry = local_device_entry(core, client)?;
    let local_state = local_entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let local_authorization = local_state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if local_authorization.protocol_device_id.as_str() != metadata.recipient_device_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
    let document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await?;
    let sender_manifest = anp::authentication::find_eligible_device(
        &document,
        &metadata.sender_device_id,
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    let binding = anp::direct_e2ee::V2SessionBinding {
        local_did: client.did().as_str().to_owned(),
        local_device_id: metadata.recipient_device_id.clone(),
        peer_did: client.did().as_str().to_owned(),
        peer_device_id: metadata.sender_device_id.clone(),
        suite: anp::direct_e2ee::MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
        local_e2ee_key_id: local_authorization.e2ee_key_id.clone(),
        peer_e2ee_key_id: sender_manifest.e2ee_key_id.clone(),
    };
    binding
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let scope = V2OwnerScope::from_identity_state(
        &client.current_identity().id,
        client.did(),
        local_state,
    )?;
    let now = OffsetDateTime::now_utc();
    let now_text = format_time(now)?;
    let local_static =
        crate::internal::secure_direct::v2_prekey_runtime::local_static_private(client)?;
    let sender_static =
        crate::internal::secure_direct::v2_prekey_runtime::static_public_from_document(
            &document,
            &sender_manifest.e2ee_key_id,
        )?;

    let probe = probe_root_candidate(
        core,
        &scope,
        &binding,
        metadata,
        body,
        &local_static,
        &sender_static,
        &now_text,
    )?;
    match probe {
        RootProbeOutcome::NotRoot => return Ok(RootInboundInterceptOutcome::NotRoot),
        RootProbeOutcome::Replay => {
            return if import_coordinator_exists(core, client, &metadata.message_id)? {
                drive_root_import_completion(core, client, &metadata.message_id).await?;
                Ok(RootInboundInterceptOutcome::Replay)
            } else {
                Ok(RootInboundInterceptOutcome::NotRoot)
            };
        }
        RootProbeOutcome::Root => {}
    }

    // Registry/checkpoint is Root-specific. Ordinary same-DID P5 messages
    // never depend on this network read.
    let mut remote = DeviceJoinAdminHttpAdapter::production(client);
    let registry = remote.registry(client.did(), false).await?;
    let rejected_non_root = std::cell::Cell::new(false);

    let result = with_v2_runtime(core, &scope, |direct| match body {
        V2DirectBody::Init(init) => direct.decrypt_inbound_init_secret_json_validated_with_commit(
            &binding,
            metadata,
            init,
            &local_static,
            &sender_static,
            &now_text,
            |plaintext, session| {
                classify_and_seal_root(
                    core, client, metadata, body, session, delivery, plaintext, &document,
                    &registry, now,
                )
            },
            |transaction, validated| match validated {
                RootInboundValidation::NotRoot => {
                    rejected_non_root.set(true);
                    Err(crate::ImError::unsupported("p5-not-root-envelope"))
                }
                RootInboundValidation::Root(plan) => persist_import_sealed_tx(transaction, plan),
                RootInboundValidation::Terminal => Ok(()),
            },
        ),
        V2DirectBody::Cipher(cipher) => direct.decrypt_inbound_secret_json_validated_with_commit(
            &binding,
            metadata,
            cipher,
            &now_text,
            |plaintext, pre_session, session| {
                if pre_session.status != V2_SESSION_STATUS_ESTABLISHED {
                    Ok(RootInboundValidation::Terminal)
                } else {
                    classify_and_seal_root(
                        core, client, metadata, body, session, delivery, plaintext, &document,
                        &registry, now,
                    )
                }
            },
            |transaction, validated| match validated {
                RootInboundValidation::NotRoot => {
                    rejected_non_root.set(true);
                    Err(crate::ImError::unsupported("p5-not-root-envelope"))
                }
                RootInboundValidation::Root(plan) => persist_import_sealed_tx(transaction, plan),
                RootInboundValidation::Terminal => Ok(()),
            },
        ),
    });

    match result {
        Err(_) if rejected_non_root.get() => Ok(RootInboundInterceptOutcome::NotRoot),
        Err(error) => Err(error),
        Ok(V2ValidatedSecretInboundOutcome::Decrypted {
            validated: RootInboundValidation::Root(_),
            ..
        }) => {
            drive_root_import_completion(core, client, &metadata.message_id).await?;
            Ok(RootInboundInterceptOutcome::Consumed)
        }
        Ok(V2ValidatedSecretInboundOutcome::Decrypted {
            validated: RootInboundValidation::NotRoot,
            ..
        }) => Err(crate::ImError::PermissionDenied),
        Ok(V2ValidatedSecretInboundOutcome::Decrypted {
            validated: RootInboundValidation::Terminal,
            ..
        }) => Ok(RootInboundInterceptOutcome::Consumed),
        Ok(V2ValidatedSecretInboundOutcome::Replay { .. }) => {
            if import_coordinator_exists(core, client, &metadata.message_id)? {
                drive_root_import_completion(core, client, &metadata.message_id).await?;
                Ok(RootInboundInterceptOutcome::Replay)
            } else {
                Ok(RootInboundInterceptOutcome::NotRoot)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn probe_root_candidate(
    core: &crate::core::ImCore,
    scope: &V2OwnerScope,
    binding: &anp::direct_e2ee::V2SessionBinding,
    metadata: &V2DirectMetadata,
    body: &V2DirectBody,
    local_static: &x25519_dalek::StaticSecret,
    sender_static: &[u8; 32],
    now: &str,
) -> crate::ImResult<RootProbeOutcome> {
    let classification = std::cell::Cell::new(None);
    let result = with_v2_runtime(core, scope, |direct| match body {
        V2DirectBody::Init(init) => direct.decrypt_inbound_init_secret_json_validated_with_commit(
            binding,
            metadata,
            init,
            local_static,
            sender_static,
            now,
            |plaintext, _| root_system_type(plaintext),
            |_, is_root| {
                classification.set(Some(*is_root));
                Err(crate::ImError::unsupported("p5-root-probe-rollback"))
            },
        ),
        V2DirectBody::Cipher(cipher) => direct.decrypt_inbound_secret_json_validated_with_commit(
            binding,
            metadata,
            cipher,
            now,
            |plaintext, _, _| root_system_type(plaintext),
            |_, is_root| {
                classification.set(Some(*is_root));
                Err(crate::ImError::unsupported("p5-root-probe-rollback"))
            },
        ),
    });
    match (result, classification.get()) {
        (Err(_), Some(false)) => Ok(RootProbeOutcome::NotRoot),
        (Err(_), Some(true)) => Ok(RootProbeOutcome::Root),
        (Ok(V2ValidatedSecretInboundOutcome::Replay { .. }), None) => Ok(RootProbeOutcome::Replay),
        (Err(error), None) => Err(error),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_and_seal_root(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    metadata: &V2DirectMetadata,
    body: &V2DirectBody,
    session: &V2DirectSessionState,
    delivery: &TrustedDirectDeliveryContext,
    plaintext: &V2SecretJsonPayload,
    document: &Value,
    registry: &DeviceJoinRemoteRegistry,
    now: OffsetDateTime,
) -> crate::ImResult<RootInboundValidation> {
    let is_root = match root_system_type(plaintext) {
        Ok(value) => value,
        Err(_) => return Ok(RootInboundValidation::NotRoot),
    };
    if !is_root {
        return Ok(RootInboundValidation::NotRoot);
    }
    match decode_root_secret_payload(plaintext) {
        Err(_) => Ok(RootInboundValidation::Terminal),
        Ok(RootSecretPayload::NotRoot) => Ok(RootInboundValidation::NotRoot),
        Ok(RootSecretPayload::Root(envelope)) => match validate_and_seal_pending_root(
            core, client, metadata, body, session, delivery, &envelope, document, registry, now,
        ) {
            Ok(plan) => Ok(RootInboundValidation::Root(plan)),
            Err(crate::ImError::PermissionDenied)
            | Err(crate::ImError::InvalidInput { .. })
            | Err(crate::ImError::Serialization { .. }) => Ok(RootInboundValidation::Terminal),
            Err(error) => Err(error),
        },
    }
}

fn with_v2_runtime<T>(
    core: &crate::core::ImCore,
    scope: &V2OwnerScope,
    action: impl FnOnce(&V2EstablishedDirectRuntime<'_, '_>) -> crate::ImResult<T>,
) -> crate::ImResult<T> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let vault = core
        .inner()
        .identity_vault()
        .ok_or(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        })?
        .vault();
    let store = SqliteV2DirectStateStore::new_with_secret_vault(&connection, vault, scope.clone())?;
    action(&V2EstablishedDirectRuntime::new(&store))
}

fn import_coordinator_exists(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
) -> crate::ImResult<bool> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let count: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM identity_root_import_completion_v1
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND message_id = ?3
  AND phase <> 'terminal_failed'"#,
            rusqlite::params![
                client.current_identity().id.as_str(),
                client.exact_protocol_device_id()?,
                message_id,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(count == 1)
}

struct CompletionRecord {
    did: String,
    local_device_id: String,
    message_id: String,
    sender_device_id: String,
    recipient_device_id: String,
    sender_e2ee_key_id: String,
    recipient_e2ee_key_id: String,
    imported_at: String,
    expires_at: String,
    pending_root_ref: SecretRef,
    root_key_id: String,
    root_fingerprint: String,
    document_version: u64,
    document_hash: String,
    registry_version: u64,
    phase: RootImportCompletionPhase,
    completion_params_json: Option<String>,
    completion_request_hash: Option<String>,
    completion_result_json: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CompletionSuccess {
    did: String,
    device_id: String,
    role: String,
    management_ready: bool,
    auth_generation: u64,
    registry_version: u64,
    completed_message_id: String,
}

async fn drive_root_import_completion(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let existing = load_completion_record(&connection, client, message_id)?;
    drop(connection);
    if let Some(result_json) = existing.completion_result_json.as_deref() {
        let success: CompletionSuccess =
            serde_json::from_str(result_json).map_err(redacted_serialization)?;
        validate_completion_success(core, client, &existing, &success)?;
        return converge_completed_root_import(core, client, &success, None).await;
    }
    let (params, request_hash) = prepare_completion_params(core, client, message_id)?;
    let current_token = client.runtime().key_provider.valid_auth_token()?;
    let result = match current_token {
        Some(token) => call_root_import_completion(client, &token, params.clone())
            .await
            .map(Some),
        None => Ok(None),
    };
    match result {
        Ok(Some(result)) => {
            let success =
                persist_completion_success(core, client, &existing, &request_hash, result)?;
            converge_completed_root_import(core, client, &success, None).await
        }
        Ok(None) | Err(crate::ImError::AuthRequired | crate::ImError::SessionExpired) => {
            recover_unknown_completion(core, client, &existing, &request_hash, params).await
        }
        Err(error) if completion_error_allows_state_probe(&error) => {
            recover_unknown_completion(core, client, &existing, &request_hash, params).await
        }
        Err(error) => Err(error),
    }
}

/// Resumes every durable receiver-side coordinator for the selected identity.
/// The coordinator contains no root plaintext; secret material is reopened
/// only by the exact phase that requires proof or pending-to-active repair.
pub(crate) async fn recover_root_import_completions(
    client: &crate::core::ImClient,
) -> crate::ImResult<usize> {
    let core = client.core_handle();
    let local_device_id = client.exact_protocol_device_id()?;
    let message_ids = {
        let connection = crate::internal::local_state::open_writable(
            &core.inner().sdk_paths().local_state.sqlite_path,
        )?;
        let mut statement = connection
            .prepare(
                r#"SELECT message_id FROM identity_root_import_completion_v1
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3
  AND phase <> 'terminal_failed'
ORDER BY created_at, message_id"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                rusqlite::params![
                    client.current_identity().id.as_str(),
                    client.did().as_str(),
                    local_device_id,
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        rows
    };

    let mut recovered = 0_usize;
    for message_id in message_ids {
        drive_root_import_completion(&core, client, &message_id).await?;
        recovered = recovered.saturating_add(1);
    }
    Ok(recovered)
}

async fn call_root_import_completion(
    client: &crate::core::ImClient,
    token: &str,
    params: Value,
) -> crate::ImResult<Value> {
    let mut transport =
        crate::internal::transport::CoreHttpTransport::new_with_ephemeral_bearer(client, token)?;
    transport
        .authenticated_rpc(
            crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT,
            "device_root_import_complete",
            params,
        )
        .await
}

async fn recover_unknown_completion(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    record: &CompletionRecord,
    request_hash: &str,
    params: Value,
) -> crate::ImResult<()> {
    let entry = local_device_entry(core, client)?;
    let authorization = entry
        .device_state
        .as_ref()
        .and_then(|state| state.authorization.as_ref())
        .filter(|authorization| {
            authorization.role == DeviceAuthorizationRole::Member && !authorization.management_ready
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let next_generation = authorization
        .auth_generation
        .checked_add(1)
        .ok_or(crate::ImError::PermissionDenied)?;
    let next_registry_version = record
        .registry_version
        .checked_add(1)
        .ok_or(crate::ImError::PermissionDenied)?;
    let before = expected_device_access(
        &entry,
        authorization,
        DeviceAuthorizationRole::Member,
        false,
        authorization.auth_generation,
    )?;
    let after = expected_device_access(
        &entry,
        authorization,
        DeviceAuthorizationRole::Admin,
        true,
        next_generation,
    )?;
    let mut fresh = crate::internal::transport::CoreHttpTransport::new_pending_device_transition(
        client,
        client.runtime().key_provider.clone(),
        before.clone(),
        after.clone(),
    );
    let token = fresh.refresh_jwt_async().await?;
    if token_matches_expected(&token, &before) {
        let result = call_root_import_completion(client, &token, params).await?;
        let success = persist_completion_success(core, client, record, request_hash, result)?;
        return converge_completed_root_import(core, client, &success, None).await;
    }
    if !token_matches_expected(&token, &after) {
        return Err(crate::ImError::PermissionDenied);
    }

    // A fresh Admin principal is authoritative evidence that the completion
    // transaction committed even if its HTTPS response was lost. V1 advances
    // both counters exactly once, so reconstruct only the closed success shape
    // and confirm it with one exact Registry read before promotion.
    let success = CompletionSuccess {
        did: record.did.clone(),
        device_id: record.local_device_id.clone(),
        role: "admin".to_owned(),
        management_ready: true,
        auth_generation: next_generation,
        registry_version: next_registry_version,
        completed_message_id: record.message_id.clone(),
    };
    let result = serde_json::to_value(&success).map_err(redacted_serialization)?;
    let success = persist_completion_success(core, client, record, request_hash, result)?;
    converge_completed_root_import(core, client, &success, Some(token)).await
}

fn persist_completion_success(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    record: &CompletionRecord,
    request_hash: &str,
    result: Value,
) -> crate::ImResult<CompletionSuccess> {
    let success: CompletionSuccess =
        serde_json::from_value(result.clone()).map_err(redacted_serialization)?;
    validate_completion_success(core, client, record, &success)?;
    let result_json = String::from_utf8(
        serde_json_canonicalizer::to_vec(&result).map_err(redacted_serialization)?,
    )
    .map_err(|_| crate::ImError::Serialization {
        detail: "completion result is not UTF-8".to_owned(),
    })?;
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let changed = connection
        .execute(
            r#"UPDATE identity_root_import_completion_v1
SET phase = 'completion_accepted', completion_result_json = ?1,
    updated_at = ?2, last_error_code = NULL
WHERE owner_identity_id = ?3 AND local_device_id = ?4 AND message_id = ?5
  AND completion_request_hash = ?6
  AND phase IN ('proof_prepared', 'completion_pending', 'completion_accepted')"#,
            rusqlite::params![
                result_json,
                format_time(OffsetDateTime::now_utc())?,
                client.current_identity().id.as_str(),
                client.exact_protocol_device_id()?,
                record.message_id,
                request_hash,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(success)
}

async fn converge_completed_root_import(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    success: &CompletionSuccess,
    fresh_admin_token: Option<String>,
) -> crate::ImResult<()> {
    let entry = local_device_entry(core, client)?;
    let authorization = entry
        .device_state
        .as_ref()
        .and_then(|state| state.authorization.as_ref())
        .ok_or(crate::ImError::PermissionDenied)?;
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let record = load_completion_record(&connection, client, &success.completed_message_id)?;
    drop(connection);
    validate_completion_success(core, client, &record, success)?;

    let expected = expected_device_access(
        &entry,
        authorization,
        DeviceAuthorizationRole::Admin,
        true,
        success.auth_generation,
    )?;
    let token = match fresh_admin_token {
        Some(token) => {
            crate::internal::access_token::validate_device_access_token(
                &token,
                &borrow_expected_device_access(&expected),
            )?;
            token
        }
        None => {
            let mut fresh = crate::internal::transport::CoreHttpTransport::new_pending_device(
                client,
                client.runtime().key_provider.clone(),
                expected,
            );
            fresh.refresh_jwt_async().await?
        }
    };

    if matches!(
        record.phase,
        RootImportCompletionPhase::CompletionAccepted | RootImportCompletionPhase::TokenRefreshed
    ) {
        update_completion_phase(
            core,
            client,
            &success.completed_message_id,
            RootImportCompletionPhase::CompletionAccepted,
            RootImportCompletionPhase::TokenRefreshed,
        )?;
        let transport = crate::internal::transport::CoreHttpTransport::new_with_ephemeral_bearer(
            client, &token,
        )?;
        let mut remote = DeviceJoinAdminHttpAdapter::new(transport);
        let registry = remote.registry(client.did(), false).await?;
        validate_completed_registry(client, authorization, &record, success, &registry)?;
        update_completion_phase(
            core,
            client,
            &success.completed_message_id,
            RootImportCompletionPhase::TokenRefreshed,
            RootImportCompletionPhase::RegistryConfirmed,
        )?;
    } else if !matches!(
        record.phase,
        RootImportCompletionPhase::RegistryConfirmed | RootImportCompletionPhase::Promoted
    ) {
        return Err(crate::ImError::PermissionDenied);
    }

    // Always replay the local promotion repair, including phase=promoted.
    // This closes both crash windows: index committed before coordinator and
    // coordinator committed before pending Vault cleanup.
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let confirmed = load_completion_record(&connection, client, &success.completed_message_id)?;
    drop(connection);
    crate::internal::identity_root_promotion::repair_root_import_promotion(
        core,
        client,
        crate::internal::identity_root_promotion::RootImportPromotionRequest {
            completed_message_id: success.completed_message_id.clone(),
            auth_generation: success.auth_generation,
            checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: confirmed.document_version,
                document_hash: confirmed.document_hash,
                registry_version: success.registry_version,
            },
            pending_root_ref: confirmed.pending_root_ref,
            root_key_id: confirmed.root_key_id,
            root_public_key_fingerprint: confirmed.root_fingerprint,
        },
    )?;

    // The new bearer becomes durable only after the active Root ref, local
    // Admin projection and crash coordinator have converged.
    client.runtime().key_provider.persist_auth_token(&token)?;
    Ok(())
}

fn validate_completed_registry(
    client: &crate::core::ImClient,
    authorization: &crate::internal::identity_device_state::DeviceAuthorizationProjection,
    record: &CompletionRecord,
    success: &CompletionSuccess,
    registry: &DeviceJoinRemoteRegistry,
) -> crate::ImResult<()> {
    let local = registry_device(&registry.devices, &success.device_id)?;
    if registry.did != *client.did()
        || registry.checkpoint.document_version != record.document_version
        || registry.checkpoint.document_hash != record.document_hash
        || registry.checkpoint.registry_version != success.registry_version
        || local.status != DeviceAuthorizationStatus::Active
        || local.role != DeviceAuthorizationRole::Admin
        || !local.management_ready
        || local.auth_generation != success.auth_generation
        || local.signing_key_id != authorization.signing_key_id
        || local.e2ee_key_id != authorization.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_completion_success(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    record: &CompletionRecord,
    success: &CompletionSuccess,
) -> crate::ImResult<()> {
    let entry = local_device_entry(core, client)?;
    let state = entry
        .device_state
        .as_ref()
        .filter(|state| state.mode == IdentityDeviceMode::VNext)
        .ok_or(crate::ImError::PermissionDenied)?;
    let authorization = state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let expected_generation = expected_completion_generation(
        record.phase,
        authorization.role,
        authorization.management_ready,
        authorization.auth_generation,
    )?;
    if success.did != record.did
        || success.did != client.did().as_str()
        || success.device_id != record.local_device_id
        || success.device_id != client.exact_protocol_device_id()?
        || success.role != "admin"
        || !success.management_ready
        || success.auth_generation != expected_generation
        || success.registry_version
            != record
                .registry_version
                .checked_add(1)
                .ok_or(crate::ImError::PermissionDenied)?
        || success.completed_message_id != record.message_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn expected_completion_generation(
    phase: RootImportCompletionPhase,
    role: DeviceAuthorizationRole,
    management_ready: bool,
    auth_generation: u64,
) -> crate::ImResult<u64> {
    if role == DeviceAuthorizationRole::Admin && management_ready {
        if matches!(
            phase,
            RootImportCompletionPhase::RegistryConfirmed | RootImportCompletionPhase::Promoted
        ) {
            return Ok(auth_generation);
        }
        return Err(crate::ImError::PermissionDenied);
    }
    if role == DeviceAuthorizationRole::Member && !management_ready {
        if phase == RootImportCompletionPhase::Promoted {
            return Err(crate::ImError::PermissionDenied);
        }
        return auth_generation
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied);
    }
    Err(crate::ImError::PermissionDenied)
}

fn expected_device_access(
    entry: &crate::internal::identity_store::IndexEntry,
    authorization: &crate::internal::identity_device_state::DeviceAuthorizationProjection,
    role: DeviceAuthorizationRole,
    management_ready: bool,
    auth_generation: u64,
) -> crate::ImResult<crate::internal::transport::ExpectedDeviceAccessOwned> {
    if entry.did.trim().is_empty()
        || entry.user_id.trim().is_empty()
        || authorization.protocol_device_id.as_str().trim().is_empty()
        || authorization.signing_key_id.trim().is_empty()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(crate::internal::transport::ExpectedDeviceAccessOwned {
        did: entry.did.clone(),
        user_id: entry.user_id.clone(),
        device_id: authorization.protocol_device_id.as_str().to_owned(),
        key_id: authorization.signing_key_id.clone(),
        auth_generation,
        role,
        management_ready,
    })
}

fn borrow_expected_device_access(
    expected: &crate::internal::transport::ExpectedDeviceAccessOwned,
) -> crate::internal::access_token::ExpectedDeviceAccess<'_> {
    crate::internal::access_token::ExpectedDeviceAccess {
        did: &expected.did,
        user_id: &expected.user_id,
        device_id: &expected.device_id,
        key_id: &expected.key_id,
        auth_generation: expected.auth_generation,
        role: expected.role,
        management_ready: expected.management_ready,
    }
}

fn token_matches_expected(
    token: &str,
    expected: &crate::internal::transport::ExpectedDeviceAccessOwned,
) -> bool {
    crate::internal::access_token::validate_device_access_token(
        token,
        &borrow_expected_device_access(expected),
    )
    .is_ok()
}

fn completion_error_allows_state_probe(error: &crate::ImError) -> bool {
    match error {
        crate::ImError::AuthRequired
        | crate::ImError::SessionExpired
        | crate::ImError::PermissionDenied
        | crate::ImError::TransportUnavailable { .. }
        | crate::ImError::Io { .. } => true,
        crate::ImError::Service {
            status_code: Some(status),
            ..
        } => matches!(*status, 401 | 403 | 408 | 425 | 429 | 500..=599),
        _ => false,
    }
}

fn update_completion_phase(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
    from: RootImportCompletionPhase,
    to: RootImportCompletionPhase,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let changed = connection
        .execute(
            r#"UPDATE identity_root_import_completion_v1
SET phase = ?1, updated_at = ?2
WHERE owner_identity_id = ?3 AND local_device_id = ?4 AND message_id = ?5
  AND phase IN (?6, ?1)"#,
            rusqlite::params![
                to.as_str(),
                format_time(OffsetDateTime::now_utc())?,
                client.current_identity().id.as_str(),
                client.exact_protocol_device_id()?,
                message_id,
                from.as_str(),
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn prepare_completion_params(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
) -> crate::ImResult<(Value, String)> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    let record = load_completion_record(&connection, client, message_id)?;
    if let (Some(params), Some(hash)) = (
        record.completion_params_json.as_deref(),
        record.completion_request_hash.as_deref(),
    ) {
        let value = serde_json::from_str(params).map_err(redacted_serialization)?;
        return Ok((value, hash.to_owned()));
    }
    if record.phase != RootImportCompletionPhase::ImportSealed {
        return Err(crate::ImError::PermissionDenied);
    }

    let vault = core
        .inner()
        .identity_vault()
        .ok_or(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        })?
        .vault();
    let opened = vault.open(&record.pending_root_ref)?;
    let pending: PendingRootSecretV1 =
        serde_json::from_slice(opened.expose_secret()).map_err(redacted_serialization)?;
    let pending = Zeroizing::new(pending);
    let canonical_pending = Zeroizing::new(
        serde_json_canonicalizer::to_vec(&*pending).map_err(redacted_serialization)?,
    );
    if canonical_pending.as_slice() != opened.expose_secret()
        || pending.schema_version != 1
        || pending.did != record.did
        || pending.message_id != record.message_id
        || pending.sender_device_id != record.sender_device_id
        || pending.recipient_device_id != record.recipient_device_id
        || pending.sender_e2ee_key_id != record.sender_e2ee_key_id
        || pending.recipient_e2ee_key_id != record.recipient_e2ee_key_id
        || pending.root_key_id != record.root_key_id
        || pending.root_public_key_fingerprint != record.root_fingerprint
        || pending.envelope_expires_at != record.expires_at
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let mut nonce_bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
    let statement = serde_json::json!({
        "type": "awiki.device.root-possession.v1",
        "message_id": record.message_id.clone(),
        "did": record.did.clone(),
        "sending_device_id": record.sender_device_id.clone(),
        "importing_device_id": record.recipient_device_id.clone(),
        "sender_e2ee_key_id": record.sender_e2ee_key_id.clone(),
        "recipient_e2ee_key_id": record.recipient_e2ee_key_id.clone(),
        "root_key_id": record.root_key_id.clone(),
        "root_public_key_fingerprint": record.root_fingerprint.clone(),
        "document_version": record.document_version,
        "document_hash": record.document_hash.clone(),
        "registry_version": record.registry_version,
        "imported_at": record.imported_at.clone(),
        "expires_at": record.expires_at.clone(),
        "nonce": nonce,
    });
    let root_private = anp::PrivateKeyMaterial::from_pem(&pending.root_private_key_pkcs8_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let signed_statement = anp::proof::generate_object_proof(
        &statement,
        &root_private,
        &pending.root_key_id,
        &pending.did,
        Some(record.imported_at.clone()),
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    drop(root_private);

    let device_signing_pem = Zeroizing::new(
        client
            .runtime()
            .key_provider
            .device_request_signing_private_pem()?,
    );
    let device_signing = anp::PrivateKeyMaterial::from_pem(&device_signing_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let device_key_id = local_device_entry(core, client)?
        .device_state
        .and_then(|state| state.authorization)
        .filter(|authorization| authorization.protocol_device_id.as_str() == record.local_device_id)
        .map(|authorization| authorization.signing_key_id)
        .ok_or(crate::ImError::PermissionDenied)?;
    let unsigned_params = serde_json::json!({
        "operation_id": record.message_id.clone(),
        "type": "awiki.device.root-key-import-complete.v1",
        "statement": signed_statement,
    });
    let params = anp::proof::generate_object_proof(
        &unsigned_params,
        &device_signing,
        &device_key_id,
        &record.did,
        Some(record.imported_at.clone()),
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    drop(device_signing);
    let canonical = serde_json_canonicalizer::to_vec(&params).map_err(redacted_serialization)?;
    let request_hash = format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(&canonical))
    );
    let params_json = String::from_utf8(canonical).map_err(|_| crate::ImError::Serialization {
        detail: "completion params are not UTF-8".to_owned(),
    })?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let changed = transaction
        .execute(
            r#"UPDATE identity_root_import_completion_v1
SET phase = 'completion_pending', completion_params_json = ?1,
    completion_request_hash = ?2, updated_at = ?3
WHERE owner_identity_id = ?4 AND local_device_id = ?5 AND message_id = ?6
  AND phase = 'import_sealed'
  AND completion_params_json IS NULL AND completion_request_hash IS NULL"#,
            rusqlite::params![
                params_json,
                request_hash,
                format_time(OffsetDateTime::now_utc())?,
                client.current_identity().id.as_str(),
                record.local_device_id,
                message_id,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok((params, request_hash))
}

fn load_completion_record(
    connection: &rusqlite::Connection,
    client: &crate::core::ImClient,
    message_id: &str,
) -> crate::ImResult<CompletionRecord> {
    connection
        .query_row(
            r#"SELECT owner_did, local_device_id, message_id, sender_device_id,
recipient_device_id, sender_e2ee_key_id, recipient_e2ee_key_id,
imported_at, envelope_expires_at, pending_root_ref_json, root_key_id,
root_fingerprint, document_version, document_hash, registry_version,
phase, completion_params_json, completion_request_hash, completion_result_json
FROM identity_root_import_completion_v1
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND message_id = ?3"#,
            rusqlite::params![
                client.current_identity().id.as_str(),
                client.exact_protocol_device_id()?,
                message_id,
            ],
            |row| {
                let pending_ref_json: String = row.get(9)?;
                let pending_root_ref =
                    serde_json::from_str(&pending_ref_json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            9,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                let phase_text: String = row.get(15)?;
                let phase = parse_completion_phase(&phase_text).map_err(|_| {
                    rusqlite::Error::InvalidColumnType(
                        15,
                        "phase".to_owned(),
                        rusqlite::types::Type::Text,
                    )
                })?;
                Ok(CompletionRecord {
                    did: row.get(0)?,
                    local_device_id: row.get(1)?,
                    message_id: row.get(2)?,
                    sender_device_id: row.get(3)?,
                    recipient_device_id: row.get(4)?,
                    sender_e2ee_key_id: row.get(5)?,
                    recipient_e2ee_key_id: row.get(6)?,
                    imported_at: row.get(7)?,
                    expires_at: row.get(8)?,
                    pending_root_ref,
                    root_key_id: row.get(10)?,
                    root_fingerprint: row.get(11)?,
                    document_version: row.get(12)?,
                    document_hash: row.get(13)?,
                    registry_version: row.get(14)?,
                    phase,
                    completion_params_json: row.get(16)?,
                    completion_request_hash: row.get(17)?,
                    completion_result_json: row.get(18)?,
                })
            },
        )
        .map_err(crate::internal::local_state::local_state_unavailable)
}

fn parse_completion_phase(value: &str) -> crate::ImResult<RootImportCompletionPhase> {
    match value {
        "import_sealed" => Ok(RootImportCompletionPhase::ImportSealed),
        "proof_prepared" => Ok(RootImportCompletionPhase::ProofPrepared),
        "completion_pending" => Ok(RootImportCompletionPhase::CompletionPending),
        "completion_accepted" => Ok(RootImportCompletionPhase::CompletionAccepted),
        "token_refreshed" => Ok(RootImportCompletionPhase::TokenRefreshed),
        "registry_confirmed" => Ok(RootImportCompletionPhase::RegistryConfirmed),
        "promoted" => Ok(RootImportCompletionPhase::Promoted),
        "terminal_failed" => Ok(RootImportCompletionPhase::TerminalFailed),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct PendingRootSecretV1 {
    schema_version: u8,
    did: String,
    message_id: String,
    root_key_id: String,
    root_public_key_fingerprint: String,
    sender_device_id: String,
    recipient_device_id: String,
    sender_e2ee_key_id: String,
    recipient_e2ee_key_id: String,
    envelope_expires_at: String,
    root_private_key_pkcs8_pem: String,
}

pub(crate) fn validate_and_seal_pending_root(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    metadata: &V2DirectMetadata,
    body: &V2DirectBody,
    session: &V2DirectSessionState,
    delivery: &TrustedDirectDeliveryContext,
    envelope: &RootKeyEnvelope,
    document: &Value,
    registry: &DeviceJoinRemoteRegistry,
    now: OffsetDateTime,
) -> crate::ImResult<RootImportSealedPlan> {
    delivery.validate()?;
    validate_envelope_field_bounds(envelope)?;
    validate_outer_equality(metadata, body, session, delivery, envelope)?;
    let accepted_at = authoritative_root_accepted_at(delivery)?;
    let imported_at = validate_envelope_time(envelope, accepted_at, now)?;
    let local_entry = local_device_entry(core, client)?;
    validate_registry_and_manifest(&local_entry, client, envelope, document, registry)?;
    let root_der = decode_canonical_root_der(&envelope.root_private_key_pkcs8_b64u)?;
    validate_root_key_material(envelope, document, &root_der)?;

    let pending_ref = expected_pending_ref(&local_entry, client, &envelope.message_id)?;
    let root_pem = canonical_root_pem(&root_der)?;
    let pending_secret = Zeroizing::new(PendingRootSecretV1 {
        schema_version: 1,
        did: envelope.did.clone(),
        message_id: envelope.message_id.clone(),
        root_key_id: envelope.root_key_id.clone(),
        root_public_key_fingerprint: envelope.root_public_key_fingerprint.clone(),
        sender_device_id: envelope.sender_device_id.clone(),
        recipient_device_id: envelope.recipient_device_id.clone(),
        sender_e2ee_key_id: envelope.sender_e2ee_key_id.clone(),
        recipient_e2ee_key_id: envelope.recipient_e2ee_key_id.clone(),
        envelope_expires_at: envelope.expires_at.clone(),
        root_private_key_pkcs8_pem: root_pem.to_string(),
    });
    let pending_bytes = Zeroizing::new(
        serde_json_canonicalizer::to_vec(&*pending_secret).map_err(redacted_serialization)?,
    );
    let vault = core
        .inner()
        .identity_vault()
        .ok_or(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        })?
        .vault();
    let sealed = vault.seal_if_absent(SealSecretRequest {
        metadata: SecretMetadata {
            workspace_id: pending_ref.workspace_id.clone(),
            device_id: pending_ref.device_id.clone(),
            identity_id: pending_ref.identity_id.clone(),
            did: pending_ref.did.clone(),
            kind: SecretKind::IdentityRootImportPending,
            key_id: pending_ref.key_id.clone(),
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        },
        plaintext: SecretBytes::from_vec(pending_bytes.to_vec()),
    })?;
    let actual_ref = match sealed {
        SealIfAbsentResult::Sealed(secret_ref) | SealIfAbsentResult::AlreadyExists(secret_ref) => {
            secret_ref
        }
    };
    if actual_ref != pending_ref {
        return Err(crate::ImError::PermissionDenied);
    }
    let opened = vault.open(&pending_ref)?;
    if opened.expose_secret() != pending_bytes.as_slice() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(RootImportSealedPlan {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        local_device_id: client.exact_protocol_device_id()?,
        message_id: envelope.message_id.clone(),
        sender_device_id: envelope.sender_device_id.clone(),
        recipient_device_id: envelope.recipient_device_id.clone(),
        sender_e2ee_key_id: envelope.sender_e2ee_key_id.clone(),
        recipient_e2ee_key_id: envelope.recipient_e2ee_key_id.clone(),
        accepted_at: accepted_at.to_owned(),
        imported_at,
        envelope_expires_at: envelope.expires_at.clone(),
        pending_root_ref_json: serde_json::to_string(&pending_ref)
            .map_err(redacted_serialization)?,
        root_key_id: envelope.root_key_id.clone(),
        root_fingerprint: envelope.root_public_key_fingerprint.clone(),
        document_version: envelope.document_version,
        document_hash: envelope.document_hash.clone(),
        registry_version: envelope.registry_version,
        now: format_time(now)?,
    })
}

fn authoritative_root_accepted_at(
    delivery: &TrustedDirectDeliveryContext,
) -> crate::ImResult<&str> {
    delivery
        .accepted_at
        .as_deref()
        .ok_or_else(|| crate::ImError::unsupported("root-import-mailbox-hydration-required"))
}

pub(crate) fn persist_import_sealed_tx(
    transaction: &rusqlite::Transaction<'_>,
    plan: &RootImportSealedPlan,
) -> crate::ImResult<()> {
    transaction
        .execute(
            r#"
INSERT INTO identity_root_import_completion_v1 (
    owner_identity_id, owner_did, local_device_id, message_id,
    sender_device_id, recipient_device_id, sender_e2ee_key_id,
    recipient_e2ee_key_id, accepted_at,
    imported_at, envelope_expires_at, pending_root_ref_json, root_key_id,
    root_fingerprint, document_version, document_hash, registry_version,
    phase, created_at, updated_at
) VALUES (
    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
    ?16, ?17, 'import_sealed', ?18, ?18
)
ON CONFLICT(owner_identity_id, local_device_id, message_id) DO NOTHING"#,
            rusqlite::params![
                plan.owner_identity_id,
                plan.owner_did,
                plan.local_device_id,
                plan.message_id,
                plan.sender_device_id,
                plan.recipient_device_id,
                plan.sender_e2ee_key_id,
                plan.recipient_e2ee_key_id,
                plan.accepted_at,
                plan.imported_at,
                plan.envelope_expires_at,
                plan.pending_root_ref_json,
                plan.root_key_id,
                plan.root_fingerprint,
                plan.document_version,
                plan.document_hash,
                plan.registry_version,
                plan.now,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let exact: i64 = transaction
        .query_row(
            r#"
SELECT COUNT(*) FROM identity_root_import_completion_v1
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND message_id = ?3
  AND owner_did = ?4 AND sender_device_id = ?5 AND recipient_device_id = ?6
  AND sender_e2ee_key_id = ?7 AND recipient_e2ee_key_id = ?8
  AND accepted_at = ?9 AND imported_at = ?10 AND envelope_expires_at = ?11
  AND pending_root_ref_json = ?12 AND root_key_id = ?13
  AND root_fingerprint = ?14 AND document_version = ?15
  AND document_hash = ?16 AND registry_version = ?17
  AND phase <> 'terminal_failed'"#,
            rusqlite::params![
                plan.owner_identity_id,
                plan.local_device_id,
                plan.message_id,
                plan.owner_did,
                plan.sender_device_id,
                plan.recipient_device_id,
                plan.sender_e2ee_key_id,
                plan.recipient_e2ee_key_id,
                plan.accepted_at,
                plan.imported_at,
                plan.envelope_expires_at,
                plan.pending_root_ref_json,
                plan.root_key_id,
                plan.root_fingerprint,
                plan.document_version,
                plan.document_hash,
                plan.registry_version,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if exact != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_outer_equality(
    metadata: &V2DirectMetadata,
    body: &V2DirectBody,
    session: &V2DirectSessionState,
    delivery: &TrustedDirectDeliveryContext,
    envelope: &RootKeyEnvelope,
) -> crate::ImResult<()> {
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    session
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    match body {
        V2DirectBody::Init(value) => value
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?,
        V2DirectBody::Cipher(value) => value
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?,
    }
    if envelope.system_type != ROOT_KEY_ENVELOPE_SYSTEM_TYPE
        || envelope.message_id != metadata.message_id
        || metadata.message_id != metadata.operation_id
        || delivery.message_id != metadata.message_id
        || delivery.operation_id != metadata.operation_id
        || delivery.sender_did != metadata.sender_did
        || delivery.sender_device_id != metadata.sender_device_id
        || delivery.recipient_did != metadata.target.did
        || delivery.recipient_device_id != metadata.recipient_device_id
        || delivery.content_type != metadata.content_type
        || delivery.target_kind != metadata.target.kind
        || envelope.did != metadata.sender_did
        || envelope.did != metadata.target.did
        || envelope.sender_device_id != metadata.sender_device_id
        || envelope.recipient_device_id != metadata.recipient_device_id
        || envelope.sender_device_id == envelope.recipient_device_id
        || session.disabled
        || session.status != V2_SESSION_STATUS_ESTABLISHED
        || session.binding.peer_did != metadata.sender_did
        || session.binding.peer_device_id != metadata.sender_device_id
        || session.binding.peer_e2ee_key_id != envelope.sender_e2ee_key_id
        || session.binding.local_did != metadata.target.did
        || session.binding.local_device_id != metadata.recipient_device_id
        || session.binding.local_e2ee_key_id != envelope.recipient_e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if let V2DirectBody::Init(init) = body {
        if init.sender_static_key_agreement_id != envelope.sender_e2ee_key_id {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

fn validate_envelope_time(
    envelope: &RootKeyEnvelope,
    accepted_at: &str,
    now: OffsetDateTime,
) -> crate::ImResult<String> {
    let now = now.to_offset(time::UtcOffset::UTC);
    let issued_at = parse_whole_second_time("issued_at", &envelope.issued_at)?;
    let expires_at = parse_whole_second_time("expires_at", &envelope.expires_at)?;
    let accepted_at = parse_six_microsecond_time("accepted_at", accepted_at)?;
    let accepted_ceiling = if accepted_at.nanosecond() == 0 {
        accepted_at
    } else {
        accepted_at
            .replace_nanosecond(0)
            .map_err(|_| crate::ImError::PermissionDenied)?
            + Duration::seconds(1)
    };
    let current_whole_second = now
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let imported_at = accepted_ceiling.max(current_whole_second);
    if expires_at <= issued_at
        || expires_at - issued_at > Duration::seconds(ROOT_ENVELOPE_MAX_WINDOW_SECONDS)
        || accepted_at > expires_at
        || imported_at > expires_at
    {
        return Err(crate::ImError::PermissionDenied);
    }
    format_time(imported_at)
}

fn validate_envelope_field_bounds(envelope: &RootKeyEnvelope) -> crate::ImResult<()> {
    for value in [
        envelope.message_id.as_str(),
        envelope.sender_device_id.as_str(),
        envelope.recipient_device_id.as_str(),
    ] {
        if value.is_empty()
            || value.len() > 128
            || value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    if envelope.did.len() < 9
        || envelope.did.len() > 512
        || !envelope.did.starts_with("did:wba:")
        || envelope.did.contains('#')
        || envelope.did.contains('?')
    {
        return Err(crate::ImError::PermissionDenied);
    }
    for key_id in [
        envelope.root_key_id.as_str(),
        envelope.sender_e2ee_key_id.as_str(),
        envelope.recipient_e2ee_key_id.as_str(),
    ] {
        if key_id.len() < 11
            || key_id.len() > 640
            || !key_id.starts_with(&format!("{}#", envelope.did))
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    if envelope.document_version == 0
        || envelope.registry_version == 0
        || envelope.document_version > i64::MAX as u64
        || envelope.registry_version > i64::MAX as u64
        || !canonical_prefixed_digest(&envelope.root_public_key_fingerprint, "e1_")
        || !canonical_prefixed_digest(&envelope.document_hash, "sha256:")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn canonical_prefixed_digest(value: &str, prefix: &str) -> bool {
    let Some(encoded) = value.strip_prefix(prefix) else {
        return false;
    };
    if encoded.len() != 43 || encoded.contains('=') {
        return false;
    }
    URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .filter(|bytes| bytes.len() == 32)
        .is_some_and(|bytes| URL_SAFE_NO_PAD.encode(bytes) == encoded)
}

fn validate_registry_and_manifest(
    entry: &crate::internal::identity_store::IndexEntry,
    client: &crate::core::ImClient,
    envelope: &RootKeyEnvelope,
    document: &Value,
    registry: &DeviceJoinRemoteRegistry,
) -> crate::ImResult<()> {
    if envelope.did != client.did().as_str()
        || registry.did != *client.did()
        || envelope.document_version != registry.checkpoint.document_version
        || envelope.document_hash != registry.checkpoint.document_hash
        || envelope.registry_version != registry.checkpoint.registry_version
        || crate::internal::identity_wire::document::document_hash(document)?
            != envelope.document_hash
        || document.get("id").and_then(Value::as_str) != Some(envelope.did.as_str())
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let state = entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    state.validate_for_did(client.did())?;
    let local = state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if state.mode != IdentityDeviceMode::VNext
        || state.checkpoint.as_ref() != Some(&registry.checkpoint)
        || local.status != DeviceAuthorizationStatus::Active
        || local.role != DeviceAuthorizationRole::Member
        || local.management_ready
        || local.protocol_device_id.as_str() != envelope.recipient_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let sender = registry_device(&registry.devices, &envelope.sender_device_id)?;
    let recipient = registry_device(&registry.devices, &envelope.recipient_device_id)?;
    if sender.status != DeviceAuthorizationStatus::Active
        || sender.role != DeviceAuthorizationRole::Admin
        || !sender.management_ready
        || recipient.status != DeviceAuthorizationStatus::Active
        || recipient.role != DeviceAuthorizationRole::Member
        || recipient.management_ready
        || recipient.signing_key_id != local.signing_key_id
        || recipient.e2ee_key_id != local.e2ee_key_id
        || recipient.auth_generation != local.auth_generation
        || sender.e2ee_key_id != envelope.sender_e2ee_key_id
        || recipient.e2ee_key_id != envelope.recipient_e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    for device in [sender, recipient] {
        let manifest = anp::authentication::find_eligible_device(
            document,
            &device.device_id,
            anp::authentication::PROFILE_DIRECT_E2EE_V2,
        )
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
        if manifest.signing_key_id != device.signing_key_id
            || manifest.e2ee_key_id != device.e2ee_key_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

fn validate_root_key_material(
    envelope: &RootKeyEnvelope,
    document: &Value,
    root_der: &[u8],
) -> crate::ImResult<()> {
    if root_der.len() != 48 || root_der[..ED25519_PKCS8_PREFIX.len()] != ED25519_PKCS8_PREFIX {
        return Err(crate::ImError::PermissionDenied);
    }
    let seed: [u8; 32] = root_der[ED25519_PKCS8_PREFIX.len()..]
        .try_into()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let private = ed25519_dalek::SigningKey::from_bytes(&seed);
    let method = unique_method(document, &envelope.root_key_id)?;
    let public = anp::authentication::extract_public_key(method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let anp::PublicKeyMaterial::Ed25519(expected) = &public else {
        return Err(crate::ImError::PermissionDenied);
    };
    let fingerprint = format!(
        "e1_{}",
        anp::authentication::compute_multikey_fingerprint(&public)
            .map_err(|_| crate::ImError::PermissionDenied)?
    );
    if private.verifying_key() != *expected
        || fingerprint != envelope.root_public_key_fingerprint
        || envelope.did.rsplit(':').next() != Some(fingerprint.as_str())
        || method.get("controller").and_then(Value::as_str) != Some(envelope.did.as_str())
        || document
            .get("proof")
            .and_then(|proof| proof.get("verificationMethod"))
            .and_then(Value::as_str)
            != Some(envelope.root_key_id.as_str())
        || !document
            .get("assertionMethod")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values.iter().any(|value| {
                    value.as_str() == Some(envelope.root_key_id.as_str())
                        || value.get("id").and_then(Value::as_str)
                            == Some(envelope.root_key_id.as_str())
                })
            })
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn canonical_root_pem(der: &[u8]) -> crate::ImResult<Zeroizing<String>> {
    if der.len() != 48 || der[..ED25519_PKCS8_PREFIX.len()] != ED25519_PKCS8_PREFIX {
        return Err(crate::ImError::PermissionDenied);
    }
    let encoded = Zeroizing::new(STANDARD.encode(der));
    if encoded.len() != 64 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(Zeroizing::new(format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        encoded.as_str()
    )))
}

fn decode_canonical_root_der(value: &str) -> crate::ImResult<Zeroizing<Vec<u8>>> {
    if value.len() != 64 || value.contains('=') {
        return Err(crate::ImError::PermissionDenied);
    }
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| crate::ImError::PermissionDenied)?,
    );
    if URL_SAFE_NO_PAD.encode(decoded.as_slice()) != value {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(decoded)
}

fn expected_pending_ref(
    entry: &crate::internal::identity_store::IndexEntry,
    client: &crate::core::ImClient,
    message_id: &str,
) -> crate::ImResult<SecretRef> {
    let anchor = entry
        .vault_migration
        .as_ref()
        .and_then(|metadata| metadata.vnext_refs.as_ref())
        .map(|refs| &refs.device_request_signing_private)
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut hasher = Sha256::new();
    hasher.update(client.did().as_str().as_bytes());
    hasher.update([0]);
    hasher.update(message_id.as_bytes());
    let key_id = format!("root-import-pending:{}", hex_lower(&hasher.finalize()));
    Ok(SecretMetadata {
        workspace_id: anchor.workspace_id.clone(),
        device_id: anchor.device_id.clone(),
        identity_id: anchor.identity_id.clone(),
        did: anchor.did.clone(),
        kind: SecretKind::IdentityRootImportPending,
        key_id,
        key_version: 1,
        policy: SecretAccessPolicy::no_prompt_local_secret(),
    }
    .secret_ref())
}

fn unique_method<'a>(document: &'a Value, key_id: &str) -> crate::ImResult<&'a Value> {
    let mut matches = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(key_id));
    let method = matches.next().ok_or(crate::ImError::PermissionDenied)?;
    if matches.next().is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(method)
}

fn registry_device<'a>(
    devices: &'a [DeviceJoinRemoteDeviceSummary],
    device_id: &str,
) -> crate::ImResult<&'a DeviceJoinRemoteDeviceSummary> {
    let mut matches = devices
        .iter()
        .filter(|device| device.device_id == device_id);
    let device = matches.next().ok_or(crate::ImError::PermissionDenied)?;
    if matches.next().is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(device)
}

fn local_device_entry(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<crate::internal::identity_store::IndexEntry> {
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
        .load_index()?
        .credentials
        .get(alias)
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)
}

fn parse_whole_second_time(field: &str, value: &str) -> crate::ImResult<OffsetDateTime> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| crate::ImError::invalid_input(Some(field.to_owned()), "invalid timestamp"))?;
    if parsed.offset() != time::UtcOffset::UTC
        || parsed.nanosecond() != 0
        || format_time(parsed)? != value
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(parsed)
}

fn parse_six_microsecond_time(field: &str, value: &str) -> crate::ImResult<OffsetDateTime> {
    let bytes = value.as_bytes();
    if bytes.len() != 27
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[26] != b'Z'
        || bytes[..4]
            .iter()
            .chain(&bytes[5..7])
            .chain(&bytes[8..10])
            .chain(&bytes[11..13])
            .chain(&bytes[14..16])
            .chain(&bytes[17..19])
            .chain(&bytes[20..26])
            .any(|byte| !byte.is_ascii_digit())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| crate::ImError::invalid_input(Some(field.to_owned()), "invalid timestamp"))?;
    if parsed.offset() != time::UtcOffset::UTC || parsed.nanosecond() % 1_000 != 0 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(parsed)
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn redacted_serialization(error: impl std::fmt::Display) -> crate::ImError {
    let _ = error;
    crate::ImError::Serialization {
        detail: "root import payload is invalid".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anp::direct_e2ee::{V2Target, DIRECT_E2EE_SECURITY_PROFILE};

    fn envelope() -> RootKeyEnvelope {
        let digest = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        RootKeyEnvelope {
            system_type: ROOT_KEY_ENVELOPE_SYSTEM_TYPE.to_owned(),
            message_id: "root-message-1".to_owned(),
            did: "did:wba:example.test:users:alice:e1_test".to_owned(),
            root_key_id: "did:wba:example.test:users:alice:e1_test#root".to_owned(),
            root_public_key_fingerprint: format!("e1_{digest}"),
            root_private_key_pkcs8_b64u: URL_SAFE_NO_PAD
                .encode([ED25519_PKCS8_PREFIX.as_slice(), &[9_u8; 32]].concat()),
            sender_device_id: "device-a".to_owned(),
            sender_e2ee_key_id: "did:wba:example.test:users:alice:e1_test#device-a-e2ee".to_owned(),
            recipient_device_id: "device-b".to_owned(),
            recipient_e2ee_key_id: "did:wba:example.test:users:alice:e1_test#device-b-e2ee"
                .to_owned(),
            document_version: 4,
            document_hash: format!("sha256:{digest}"),
            registry_version: 7,
            issued_at: "2026-07-24T00:00:00Z".to_owned(),
            expires_at: "2026-07-24T00:10:00Z".to_owned(),
        }
    }

    fn metadata() -> V2DirectMetadata {
        V2DirectMetadata {
            anp_version: None,
            profile: DIRECT_E2EE_PROFILE_V2.to_owned(),
            security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
            sender_did: "did:wba:example.test:users:alice:e1_test".to_owned(),
            sender_device_id: "device-a".to_owned(),
            target: V2Target {
                kind: "agent".to_owned(),
                did: "did:wba:example.test:users:alice:e1_test".to_owned(),
            },
            recipient_device_id: "device-b".to_owned(),
            operation_id: "root-message-1".to_owned(),
            message_id: "root-message-1".to_owned(),
            content_type: "application/anp-direct-cipher+json".to_owned(),
            created_at: None,
        }
    }

    #[test]
    fn root_envelope_jcs_and_closed_schema_matrix() {
        let envelope = envelope();
        let canonical = serde_json_canonicalizer::to_vec(&envelope).unwrap();
        let secret = V2SecretJsonPayload::from_canonical_json_object(canonical).unwrap();
        assert!(matches!(
            decode_root_secret_payload(&secret).unwrap(),
            RootSecretPayload::Root(_)
        ));

        let pretty = serde_json::to_vec_pretty(&envelope).unwrap();
        let noncanonical = V2SecretJsonPayload::from_canonical_json_object(pretty).unwrap();
        assert!(decode_root_secret_payload(&noncanonical).is_err());

        let mut value = serde_json::to_value(&envelope).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), Value::Bool(true));
        let unknown = V2SecretJsonPayload::from_canonical_json_object(
            serde_json_canonicalizer::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(decode_root_secret_payload(&unknown).is_err());
    }

    #[test]
    fn root_versions_and_id_bounds_fail_before_vault() {
        let mut candidate = envelope();
        validate_envelope_field_bounds(&candidate).unwrap();
        candidate.document_version = 0;
        assert!(validate_envelope_field_bounds(&candidate).is_err());
        candidate.document_version = i64::MAX as u64 + 1;
        assert!(validate_envelope_field_bounds(&candidate).is_err());
        candidate.document_version = 1;
        candidate.sender_device_id = "bad device".to_owned();
        assert!(validate_envelope_field_bounds(&candidate).is_err());
    }

    #[test]
    fn root_imported_at_uses_accepted_ceiling_and_current_whole_second() {
        let envelope = envelope();
        let imported = validate_envelope_time(
            &envelope,
            "2026-07-24T00:00:00.123456Z",
            OffsetDateTime::parse("2026-07-24T00:00:00.900000Z", &Rfc3339).unwrap(),
        )
        .unwrap();
        assert_eq!(imported, "2026-07-24T00:00:01Z");
        assert!(validate_envelope_time(
            &envelope,
            "2026-07-24T00:00:00Z",
            OffsetDateTime::parse("2026-07-24T00:00:00Z", &Rfc3339).unwrap(),
        )
        .is_err());
    }

    #[test]
    fn ordinary_delivery_context_does_not_apply_root_timestamp_rules() {
        let context = TrustedDirectDeliveryContext::from_stored_message(
            &metadata(),
            Some("2026-07-24T00:00:00Z".to_owned()),
            TrustedDirectDeliverySource::Mailbox,
        )
        .unwrap();
        context.validate().unwrap();
        assert!(
            parse_six_microsecond_time("accepted_at", context.accepted_at.as_deref().unwrap())
                .is_err()
        );
    }

    #[test]
    fn realtime_hint_cannot_claim_authoritative_accepted_at() {
        let mut hint = TrustedDirectDeliveryContext::realtime_hint(&metadata()).unwrap();
        assert!(hint.accepted_at.is_none());
        assert!(matches!(
            authoritative_root_accepted_at(&hint),
            Err(crate::ImError::UnsupportedCapability { capability })
                if capability == "root-import-mailbox-hydration-required"
        ));
        hint.accepted_at = Some("2026-07-24T00:00:00.000000Z".to_owned());
        assert!(hint.validate().is_err());
    }

    #[test]
    fn response_loss_fresh_token_matrix_accepts_only_exact_member_or_next_admin() {
        let did = "did:wba:example.test:users:alice:e1_test";
        let key_id = format!("{did}#device-b-sign");
        let before = crate::internal::transport::ExpectedDeviceAccessOwned {
            did: did.to_owned(),
            user_id: "user-alice".to_owned(),
            device_id: "device-b".to_owned(),
            key_id: key_id.clone(),
            auth_generation: 1,
            role: DeviceAuthorizationRole::Member,
            management_ready: false,
        };
        let after = crate::internal::transport::ExpectedDeviceAccessOwned {
            auth_generation: 2,
            role: DeviceAuthorizationRole::Admin,
            management_ready: true,
            ..before.clone()
        };
        let member = test_access_token(&before);
        let admin = test_access_token(&after);
        assert!(token_matches_expected(&member, &before));
        assert!(!token_matches_expected(&member, &after));
        assert!(token_matches_expected(&admin, &after));
        assert!(!token_matches_expected(&admin, &before));

        let wrong_generation = crate::internal::transport::ExpectedDeviceAccessOwned {
            auth_generation: 3,
            ..after.clone()
        };
        assert!(!token_matches_expected(
            &test_access_token(&wrong_generation),
            &before
        ));
        assert!(!token_matches_expected(
            &test_access_token(&wrong_generation),
            &after
        ));
    }

    #[test]
    fn response_loss_probe_is_narrowly_limited_to_unknown_or_auth_failures() {
        assert!(completion_error_allows_state_probe(
            &crate::ImError::TransportUnavailable {
                detail: "response lost".to_owned(),
            }
        ));
        assert!(completion_error_allows_state_probe(
            &crate::ImError::SessionExpired
        ));
        assert!(completion_error_allows_state_probe(
            &crate::ImError::Service {
                status_code: Some(503),
                code: None,
                message: "temporarily unavailable".to_owned(),
                data: None,
            }
        ));
        assert!(!completion_error_allows_state_probe(
            &crate::ImError::Service {
                status_code: Some(409),
                code: Some("completion_conflict".to_owned()),
                message: "conflict".to_owned(),
                data: None,
            }
        ));
        assert!(!completion_error_allows_state_probe(
            &crate::ImError::InvalidInput {
                field: Some("params".to_owned()),
                message: "invalid".to_owned(),
            }
        ));
    }

    #[test]
    fn promotion_crash_generation_accepts_registry_confirmed_member_or_exact_admin() {
        assert_eq!(
            expected_completion_generation(
                RootImportCompletionPhase::RegistryConfirmed,
                DeviceAuthorizationRole::Member,
                false,
                1,
            )
            .unwrap(),
            2
        );
        assert_eq!(
            expected_completion_generation(
                RootImportCompletionPhase::RegistryConfirmed,
                DeviceAuthorizationRole::Admin,
                true,
                2,
            )
            .unwrap(),
            2
        );
        assert!(expected_completion_generation(
            RootImportCompletionPhase::CompletionAccepted,
            DeviceAuthorizationRole::Admin,
            true,
            2,
        )
        .is_err());
    }

    #[test]
    fn promoted_cleanup_recovery_requires_exact_admin_projection() {
        assert_eq!(
            expected_completion_generation(
                RootImportCompletionPhase::Promoted,
                DeviceAuthorizationRole::Admin,
                true,
                2,
            )
            .unwrap(),
            2
        );
        assert!(expected_completion_generation(
            RootImportCompletionPhase::Promoted,
            DeviceAuthorizationRole::Member,
            false,
            1,
        )
        .is_err());
    }

    fn test_access_token(
        expected: &crate::internal::transport::ExpectedDeviceAccessOwned,
    ) -> String {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let scopes = match (expected.role, expected.management_ready) {
            (DeviceAuthorizationRole::Admin, true) => {
                vec!["device:manage", "device:read", "message:connect"]
            }
            _ => vec![
                "device:read",
                "device:root-import-complete",
                "message:connect",
            ],
        };
        let claims = serde_json::json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": expected.did,
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": expected.did,
            "user_id": expected.user_id,
            "device_id": expected.device_id,
            "key_id": expected.key_id,
            "auth_generation": expected.auth_generation,
            "scopes": scopes,
            "iat": now,
            "nbf": now,
            "exp": now + 300,
            "jti": format!("test-{}", expected.auth_generation),
        });
        format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn import_sealed_plan_is_idempotent_and_conflict_closed_in_one_transaction() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        let plan = RootImportSealedPlan {
            owner_identity_id: "identity-a".to_owned(),
            owner_did: "did:wba:example.test:users:alice:e1_test".to_owned(),
            local_device_id: "device-b".to_owned(),
            message_id: "root-message-1".to_owned(),
            sender_device_id: "device-a".to_owned(),
            recipient_device_id: "device-b".to_owned(),
            sender_e2ee_key_id: "did:wba:example.test:users:alice:e1_test#device-a-e2ee".to_owned(),
            recipient_e2ee_key_id: "did:wba:example.test:users:alice:e1_test#device-b-e2ee"
                .to_owned(),
            accepted_at: "2026-07-24T00:00:00.000000Z".to_owned(),
            imported_at: "2026-07-24T00:00:00Z".to_owned(),
            envelope_expires_at: "2026-07-24T00:10:00Z".to_owned(),
            pending_root_ref_json: r#"{"ref":"opaque"}"#.to_owned(),
            root_key_id: "did:wba:example.test:users:alice:e1_test#root".to_owned(),
            root_fingerprint: format!("e1_{}", URL_SAFE_NO_PAD.encode([7_u8; 32])),
            document_version: 4,
            document_hash: format!("sha256:{}", URL_SAFE_NO_PAD.encode([8_u8; 32])),
            registry_version: 7,
            now: "2026-07-24T00:00:00Z".to_owned(),
        };
        let transaction = connection.transaction().unwrap();
        persist_import_sealed_tx(&transaction, &plan).unwrap();
        persist_import_sealed_tx(&transaction, &plan).unwrap();
        transaction.commit().unwrap();

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM identity_root_import_completion_v1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let mut conflicting = plan;
        conflicting.registry_version += 1;
        let transaction = connection.transaction().unwrap();
        assert!(persist_import_sealed_tx(&transaction, &conflicting).is_err());
        transaction.rollback().unwrap();
    }
}
