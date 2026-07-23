//! Product orchestration for ordinary P5 v2 exact-device Direct messages.
//!
//! The cross-domain device set comes only from the resolved DID Document's
//! embedded `deviceManifest`. AWiki Registry roles are intentionally outside
//! this path. Each target device receives one standard `direct.send` request,
//! while a secret-free local ledger aggregates those independent deliveries
//! and reports current-attempt versus previously accepted counts for safe
//! product retry diagnostics.
//! Attachment objects are prepared once; their full Manifest is retained only
//! in the local SecretVault so a partial device fan-out can resume without
//! uploading a second object.
//! Each attachment delivery also carries the same non-secret grant ref through
//! the sender-home private adapter while its standard P5 request stays intact.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use anp::authentication::{DeviceManifestEntry, PROFILE_DIRECT_E2EE_V2};
use anp::direct_e2ee::{
    V2ApplicationPlaintext, V2DirectBody, V2DirectMetadata, V2DirectSendResult,
    V2GetPrekeyBundleResult, V2SessionBinding, DIRECT_E2EE_PROFILE_V2, MTI_DIRECT_E2EE_SUITE_V2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use super::secret_store::{
    direct_secret_key_id, direct_secret_key_id_prefix, direct_secret_ref_from_blob,
    open_direct_secret_blob_strict, seal_direct_secret_blob, DirectSecretOpenExpectation,
    DirectSecretSealInput,
};
use super::v2_runtime::{
    classify_session_control, is_session_reply_operation_id, session_established_plaintext,
    session_reply_operation_id, PreparedV2Outbound, V2EstablishedDirectRuntime,
    V2SessionControlKind, V2ValidatedInboundOutcome,
};
use super::v2_store::{SqliteV2DirectStateStore, V2OwnerScope};

const DELIVERY_OPERATION_PREFIX: &str = "p5-v2-delivery:";
pub(crate) const DEVICE_SYNC_SYSTEM_TYPE: &str = "awiki.device.sync.v1";

type AttachmentIntentLock = tokio::sync::Mutex<()>;

static ATTACHMENT_INTENT_LOCKS: OnceLock<Mutex<BTreeMap<String, Weak<AttachmentIntentLock>>>> =
    OnceLock::new();

const DELIVERY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS direct_e2ee_v2_delivery_ledger (
    owner_identity_id TEXT NOT NULL,
    owner_did TEXT NOT NULL,
    local_device_id TEXT NOT NULL,
    logical_message_id TEXT NOT NULL,
    target_did TEXT NOT NULL,
    delivery_class TEXT NOT NULL CHECK (delivery_class IN ('recipient', 'own-sync')),
    recipient_did TEXT NOT NULL,
    recipient_device_id TEXT NOT NULL,
    recipient_e2ee_key_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('pending', 'accepted', 'failed', 'ineligible')),
    wire_prepared INTEGER NOT NULL DEFAULT 0 CHECK (wire_prepared IN (0, 1)),
    failure_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    accepted_at TEXT,
    PRIMARY KEY (
        owner_identity_id,
        local_device_id,
        logical_message_id,
        target_did,
        recipient_did,
        recipient_device_id
    ),
    UNIQUE (owner_identity_id, local_device_id, operation_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_session_reply_ledger (
    owner_identity_id TEXT NOT NULL,
    owner_did TEXT NOT NULL,
    local_device_id TEXT NOT NULL,
    local_e2ee_key_id TEXT NOT NULL,
    peer_did TEXT NOT NULL,
    peer_device_id TEXT NOT NULL,
    peer_e2ee_key_id TEXT NOT NULL,
    init_message_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('pending', 'accepted')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    accepted_at TEXT,
    PRIMARY KEY (owner_identity_id, local_device_id, operation_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_v2_attachment_intents (
    owner_identity_id TEXT NOT NULL,
    owner_did TEXT NOT NULL,
    local_device_id TEXT NOT NULL,
    logical_message_id TEXT NOT NULL,
    target_did TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    full_manifest_blob BLOB NOT NULL,
    redacted_manifest_json TEXT NOT NULL,
    grant_ref_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (
        owner_identity_id,
        local_device_id,
        logical_message_id,
        target_did
    )
);

CREATE INDEX IF NOT EXISTS idx_direct_e2ee_v2_delivery_retry
ON direct_e2ee_v2_delivery_ledger (
    owner_identity_id,
    local_device_id,
    logical_message_id,
    target_did,
    phase
);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryPhase {
    Pending,
    Accepted,
    Failed,
    Ineligible,
}

impl DeliveryPhase {
    fn parse(value: &str) -> crate::ImResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "failed" => Ok(Self::Failed),
            "ineligible" => Ok(Self::Ineligible),
            _ => Err(crate::ImError::PermissionDenied),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveryRecord {
    logical_message_id: String,
    target_did: String,
    delivery_class: DeliveryClass,
    recipient_did: String,
    recipient_device_id: String,
    recipient_e2ee_key_id: String,
    operation_id: String,
    source_digest: String,
    phase: DeliveryPhase,
    wire_prepared: bool,
    accepted_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryClass {
    Recipient,
    OwnSync,
}

impl DeliveryClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Recipient => "recipient",
            Self::OwnSync => "own-sync",
        }
    }

    fn parse(value: &str) -> crate::ImResult<Self> {
        match value {
            "recipient" => Ok(Self::Recipient),
            "own-sync" => Ok(Self::OwnSync),
            _ => Err(crate::ImError::PermissionDenied),
        }
    }
}

#[derive(Debug, Clone)]
struct DeliveryTarget {
    class: DeliveryClass,
    recipient_did: String,
    device: DeviceManifestEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionReplyRecord {
    binding: V2SessionBinding,
    init_message_id: String,
    operation_id: String,
    session_id: String,
    accepted: bool,
}

#[derive(Clone, PartialEq)]
pub(crate) enum V2OrdinaryBody {
    Text { text: String, markdown: bool },
    Json { payload: Value },
    AttachmentManifest { full_manifest: Value },
}

impl fmt::Debug for V2OrdinaryBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { text, markdown } => formatter
                .debug_struct("Text")
                .field("text", text)
                .field("markdown", markdown)
                .finish(),
            Self::Json { payload } => formatter
                .debug_struct("Json")
                .field("payload", payload)
                .finish(),
            Self::AttachmentManifest { full_manifest } => formatter
                .debug_struct("AttachmentManifest")
                .field(
                    "redacted_manifest",
                    &crate::attachments::manifest::redact_attachment_manifest(full_manifest),
                )
                .finish(),
        }
    }
}

impl V2OrdinaryBody {
    fn plaintext(
        &self,
        logical_message_id: &str,
        conversation_id: Option<&str>,
    ) -> crate::ImResult<V2ApplicationPlaintext> {
        required("logical_message_id", logical_message_id)?;
        let plaintext = match self {
            Self::Text { text, markdown } => {
                if text.trim().is_empty() {
                    return Err(crate::ImError::invalid_input(
                        Some("text".to_owned()),
                        "text message must not be empty",
                    ));
                }
                V2ApplicationPlaintext {
                    application_content_type: if *markdown {
                        "text/markdown".to_owned()
                    } else {
                        "text/plain".to_owned()
                    },
                    logical_message_id: Some(logical_message_id.to_owned()),
                    conversation_id: conversation_id.map(str::to_owned),
                    reply_to_message_id: None,
                    annotations: None,
                    text: Some(text.clone()),
                    payload: None,
                    payload_b64u: None,
                }
            }
            Self::Json { payload } => {
                if !payload.is_object() || is_reserved_control_payload(payload) {
                    return Err(crate::ImError::PermissionDenied);
                }
                V2ApplicationPlaintext {
                    application_content_type: "application/json".to_owned(),
                    logical_message_id: Some(logical_message_id.to_owned()),
                    conversation_id: conversation_id.map(str::to_owned),
                    reply_to_message_id: None,
                    annotations: None,
                    text: None,
                    payload: Some(payload.clone()),
                    payload_b64u: None,
                }
            }
            Self::AttachmentManifest { full_manifest } => {
                validate_full_attachment_manifest(full_manifest)?;
                V2ApplicationPlaintext {
                    application_content_type:
                        crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
                    logical_message_id: Some(logical_message_id.to_owned()),
                    conversation_id: conversation_id.map(str::to_owned),
                    reply_to_message_id: None,
                    annotations: None,
                    text: None,
                    payload: Some(full_manifest.clone()),
                    payload_b64u: None,
                }
            }
        };
        if plaintext
            .conversation_id
            .as_deref()
            .is_some_and(|value| required("conversation_id", value).is_err())
        {
            return Err(crate::ImError::invalid_input(
                Some("conversation_id".to_owned()),
                "conversation_id must be a non-empty exact value",
            ));
        }
        plaintext
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        Ok(plaintext)
    }

    pub(crate) fn from_message_body(body: &crate::messages::MessageBody) -> crate::ImResult<Self> {
        match body {
            crate::messages::MessageBody::Text { text, kind } => Ok(Self::Text {
                text: text.clone(),
                markdown: matches!(kind, crate::messages::MessageKind::Markdown),
            }),
            crate::messages::MessageBody::Payload { payload } => Ok(Self::Json {
                payload: payload.clone(),
            }),
            crate::messages::MessageBody::Attachment { .. } => Err(crate::ImError::invalid_input(
                Some("body".to_owned()),
                "attachment body must use the single-object attachment product entrypoint",
            )),
        }
    }
}

pub(crate) struct V2DirectProductSendInput {
    pub(crate) logical_message_id: String,
    pub(crate) target_did: String,
    pub(crate) conversation_id: Option<String>,
    pub(crate) body: V2OrdinaryBody,
}

pub(crate) struct V2AttachmentProductSendInput {
    pub(crate) logical_message_id: String,
    pub(crate) target_did: String,
    pub(crate) conversation_id: Option<String>,
    pub(crate) object_target: crate::messages::MessageTarget,
    pub(crate) request: crate::attachments::AttachmentSendRequest,
}

pub(crate) struct V2AttachmentFanoutInput {
    pub(crate) logical_message_id: String,
    pub(crate) target_did: String,
    pub(crate) conversation_id: Option<String>,
}

/// Sensitive only while it is being wrapped by exact-device P5 sessions.
/// Deliberately has no `Debug` implementation.
#[derive(Clone, PartialEq)]
pub(crate) struct V2PreparedAttachmentProduct {
    full_manifest: Value,
    redacted_manifest: Value,
    grant_ref: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2AttachmentProductSendSummary {
    pub(crate) direct: V2DirectProductSendSummary,
    pub(crate) redacted_manifest: Value,
    pub(crate) grant_ref: Value,
}

pub(crate) trait V2AttachmentObjectHost {
    /// Stable digest of the caller's attachment source and options. It binds a
    /// logical-message retry to the original object without persisting bytes or
    /// local paths in the product ledger.
    fn source_digest(&self) -> &str;

    async fn prepare_and_commit_object(&mut self) -> crate::ImResult<V2PreparedAttachmentProduct>;
}

struct CoreV2AttachmentObjectHost<'a> {
    client: &'a crate::core::ImClient,
    target_did: String,
    object_target: crate::messages::MessageTarget,
    request: Option<crate::attachments::AttachmentSendRequest>,
    source_digest: String,
}

impl V2AttachmentObjectHost for CoreV2AttachmentObjectHost<'_> {
    fn source_digest(&self) -> &str {
        &self.source_digest
    }

    async fn prepare_and_commit_object(&mut self) -> crate::ImResult<V2PreparedAttachmentProduct> {
        let request = self
            .request
            .take()
            .ok_or(crate::ImError::PermissionDenied)?;
        let committed = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .prepare_and_commit_object_async(
            crate::internal::attachment_runtime::upload::AttachmentPrepareObjectInput {
                target: self.object_target.clone(),
                request,
                resolved_target_did: Some(self.target_did.clone()),
                message_security_profile: "direct-e2ee",
            },
        )
        .await?;
        prepared_attachment_product(&self.target_did, committed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct V2DirectProductSendSummary {
    pub(crate) logical_message_id: String,
    pub(crate) target_did: String,
    pub(crate) target_device_count: usize,
    pub(crate) own_sync_device_count: usize,
    /// Eligible endpoints whose delivery pipeline ran in this invocation.
    pub(crate) attempted_device_count: usize,
    /// Endpoints skipped because the durable ledger already records acceptance.
    pub(crate) previously_accepted_device_count: usize,
    /// Endpoints newly accepted by this invocation.
    pub(crate) newly_accepted_device_count: usize,
    pub(crate) accepted_device_count: usize,
    pub(crate) failed_device_count: usize,
    pub(crate) accepted_at: Option<String>,
}

impl V2DirectProductSendSummary {
    pub(crate) fn fully_accepted(&self) -> bool {
        let delivery_count = self.target_device_count + self.own_sync_device_count;
        self.target_device_count > 0
            && self.accepted_device_count == delivery_count
            && self.failed_device_count == 0
    }
}

pub(crate) struct V2DirectProductContext {
    owner_identity_id: String,
    local_did: String,
    local_device_id: String,
    local_e2ee_key_id: String,
    local_static_private: x25519_dalek::StaticSecret,
    sqlite_path: PathBuf,
    vault: Arc<dyn crate::vault::SecretVault + Send + Sync>,
    scope: V2OwnerScope,
}

struct ActiveV2LocalEndpoint {
    scope: V2OwnerScope,
    device_id: String,
    signing_key_id: String,
    e2ee_key_id: String,
}

fn active_local_endpoint_for_client(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<ActiveV2LocalEndpoint> {
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let state = index
        .credentials
        .get(alias)
        .and_then(|entry| entry.device_state.as_ref())
        .ok_or(crate::ImError::PermissionDenied)?;
    let authorization = state
        .authorization
        .as_ref()
        .filter(|authorization| {
            state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext
                && authorization.status
                    == crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let scope =
        V2OwnerScope::from_identity_state(&client.current_identity().id, client.did(), state)?;
    Ok(ActiveV2LocalEndpoint {
        scope,
        device_id: authorization.protocol_device_id.as_str().to_owned(),
        signing_key_id: authorization.signing_key_id.clone(),
        e2ee_key_id: authorization.e2ee_key_id.clone(),
    })
}

impl V2DirectProductContext {
    fn from_client(
        core: &crate::core::ImCore,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<Self> {
        let endpoint = active_local_endpoint_for_client(core, client)?;
        let vault = core
            .inner()
            .identity_vault()
            .ok_or(crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::Unavailable,
            })?
            .vault();
        Ok(Self {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            local_did: client.did().as_str().to_owned(),
            local_device_id: endpoint.device_id,
            local_e2ee_key_id: endpoint.e2ee_key_id,
            local_static_private: super::v2_prekey_runtime::local_static_private(client)?,
            sqlite_path: core.inner().sdk_paths().local_state.sqlite_path.clone(),
            vault,
            scope: endpoint.scope,
        })
    }

    fn open_connection(&self) -> crate::ImResult<Connection> {
        crate::internal::local_state::open_writable(&self.sqlite_path)
    }

    fn with_direct<T>(
        &self,
        action: impl FnOnce(&V2EstablishedDirectRuntime<'_, '_>) -> crate::ImResult<T>,
    ) -> crate::ImResult<T> {
        let connection = self.open_connection()?;
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            self.vault.clone(),
            self.scope.clone(),
        )?;
        action(&V2EstablishedDirectRuntime::new(&store))
    }

    fn with_ledger<T>(
        &self,
        action: impl FnOnce(&DeliveryLedger<'_>) -> crate::ImResult<T>,
    ) -> crate::ImResult<T> {
        let connection = self.open_connection()?;
        let ledger = DeliveryLedger::new(&connection, self)?;
        action(&ledger)
    }
}

/// Revalidates a cached P5 delivery against the current local vNext endpoint.
///
/// This intentionally performs no network request, PreKey publication, vault
/// unseal, or ratchet mutation. It only reads the current local authorization
/// projection and DID Document before a prior plaintext projection may be
/// reused.
pub(crate) fn validate_cached_inbound_endpoint_for_client(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    metadata: &V2DirectMetadata,
) -> crate::ImResult<()> {
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let endpoint = active_local_endpoint_for_client(core, client)?;
    if metadata.profile != DIRECT_E2EE_PROFILE_V2
        || metadata.target.did != client.did().as_str()
        || metadata.recipient_device_id != endpoint.device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let local_document = client.runtime().key_provider.did_document()?;
    if local_document.get("id").and_then(Value::as_str) != Some(client.did().as_str())
        || (client.did().as_str().starts_with("did:wba:")
            && !anp::authentication::validate_did_document_binding(&local_document, true))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let eligible = anp::authentication::find_eligible_device(
        &local_document,
        &endpoint.device_id,
        PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    if eligible.signing_key_id != endpoint.signing_key_id
        || eligible.e2ee_key_id != endpoint.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) trait V2DirectProductHost {
    async fn resolve_did_document(&mut self, did: &str) -> crate::ImResult<Value>;

    async fn ensure_local_prekey_published(&mut self) -> crate::ImResult<()>;

    async fn fetch_prekey(
        &mut self,
        target_did: &str,
        target_device_id: &str,
        target_did_document: &Value,
        operation_seed: &str,
    ) -> crate::ImResult<V2GetPrekeyBundleResult>;

    async fn post_direct(
        &mut self,
        prepared: &PreparedV2Outbound,
    ) -> crate::ImResult<V2DirectSendResult>;

    async fn post_direct_attachment(
        &mut self,
        prepared: &PreparedV2Outbound,
        attachment_grant_ref: &Value,
    ) -> crate::ImResult<V2DirectSendResult>;
}

struct CoreV2DirectProductHost<'a> {
    core: &'a crate::core::ImCore,
    client: &'a crate::core::ImClient,
}

impl V2DirectProductHost for CoreV2DirectProductHost<'_> {
    async fn resolve_did_document(&mut self, did: &str) -> crate::ImResult<Value> {
        crate::internal::discovery::did_document::resolve_did_document_async(
            &mut crate::internal::transport::CoreHttpTransport::new(self.client),
            did,
        )
        .await
    }

    async fn ensure_local_prekey_published(&mut self) -> crate::ImResult<()> {
        super::v2_prekey_runtime::ensure_local_prekey_published(self.core, self.client)
            .await
            .map(|_| ())
    }

    async fn fetch_prekey(
        &mut self,
        target_did: &str,
        target_device_id: &str,
        target_did_document: &Value,
        operation_seed: &str,
    ) -> crate::ImResult<V2GetPrekeyBundleResult> {
        super::v2_prekey_runtime::fetch_verified_prekey(
            self.client,
            target_did,
            target_device_id,
            target_did_document,
            operation_seed,
        )
        .await
    }

    async fn post_direct(
        &mut self,
        prepared: &PreparedV2Outbound,
    ) -> crate::ImResult<V2DirectSendResult> {
        super::v2_prekey_runtime::post_standard_direct(self.client, prepared).await
    }

    async fn post_direct_attachment(
        &mut self,
        prepared: &PreparedV2Outbound,
        attachment_grant_ref: &Value,
    ) -> crate::ImResult<V2DirectSendResult> {
        super::v2_prekey_runtime::post_standard_direct_attachment(
            self.client,
            prepared,
            attachment_grant_ref,
        )
        .await
    }
}

pub(crate) fn local_identity_uses_vnext(client: &crate::core::ImClient) -> crate::ImResult<bool> {
    let Some(alias) = client.current_identity().local_alias.as_deref() else {
        return Ok(false);
    };
    let index = crate::internal::identity_store::IdentityStore::new(
        &client.core_inner().sdk_paths().identities,
    )
    .load_index()?;
    Ok(index
        .credentials
        .get(alias)
        .and_then(|entry| entry.device_state.as_ref())
        .is_some_and(|state| {
            state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext
        }))
}

pub(crate) async fn send_for_client(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    enabled: bool,
    input: V2DirectProductSendInput,
) -> crate::ImResult<V2DirectProductSendSummary> {
    if !enabled {
        return Err(crate::ImError::unsupported(
            "awiki-multi-device-direct-disabled",
        ));
    }
    let context = V2DirectProductContext::from_client(core, client)?;
    let mut host = CoreV2DirectProductHost { core, client };
    send_with_host(&context, &mut host, input).await
}

pub(crate) async fn send_attachment_for_client(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    enabled: bool,
    input: V2AttachmentProductSendInput,
) -> crate::ImResult<V2AttachmentProductSendSummary> {
    if !enabled {
        return Err(crate::ImError::unsupported(
            "awiki-multi-device-direct-disabled",
        ));
    }
    let context = V2DirectProductContext::from_client(core, client)?;
    let mut direct_host = CoreV2DirectProductHost { core, client };
    let source_digest = attachment_source_digest(&input.object_target, &input.request)?;
    let fanout_input = V2AttachmentFanoutInput {
        logical_message_id: input.logical_message_id,
        target_did: input.target_did.clone(),
        conversation_id: input.conversation_id,
    };
    let mut object_host = CoreV2AttachmentObjectHost {
        client,
        target_did: input.target_did,
        object_target: input.object_target,
        request: Some(input.request),
        source_digest,
    };
    send_attachment_with_hosts(&context, &mut direct_host, &mut object_host, fanout_input).await
}

pub(crate) async fn send_attachment_with_hosts<D, O>(
    context: &V2DirectProductContext,
    direct_host: &mut D,
    object_host: &mut O,
    input: V2AttachmentFanoutInput,
) -> crate::ImResult<V2AttachmentProductSendSummary>
where
    D: V2DirectProductHost,
    O: V2AttachmentObjectHost,
{
    required("logical_message_id", &input.logical_message_id)?;
    required("target_did", &input.target_did)?;
    let source_digest = required("source_digest", object_host.source_digest())?.to_owned();
    let intent_lock = attachment_intent_lock(context, &input);
    let _intent_guard = intent_lock.lock().await;
    preflight_recipients(context, direct_host, &input.target_did).await?;

    let prepared = match context.with_ledger(|ledger| {
        ledger.load_attachment_intent(&input.logical_message_id, &input.target_did, &source_digest)
    })? {
        Some(prepared) => prepared,
        None => {
            // This is the sole object prepare/upload/commit call for the
            // logical-message intent in this live process. The full Manifest
            // is sealed before any device delivery; later fan-out retries
            // reopen it instead of uploading. The existing attachment runtime
            // returns only after commit, so a process crash between commit and
            // this seal remains a bounded orphan/re-upload window rather than
            // a claim of strict exactly-once object creation across crashes.
            let prepared = object_host.prepare_and_commit_object().await?;
            validate_prepared_attachment_product(&prepared)?;
            context.with_ledger(|ledger| {
                ledger.save_attachment_intent(
                    &input.logical_message_id,
                    &input.target_did,
                    &source_digest,
                    prepared,
                    &now_text(),
                )
            })?
        }
    };
    let direct_input = V2DirectProductSendInput {
        logical_message_id: input.logical_message_id,
        target_did: input.target_did,
        conversation_id: input.conversation_id,
        body: V2OrdinaryBody::AttachmentManifest {
            full_manifest: prepared.full_manifest.clone(),
        },
    };
    let direct = send_with_host_with_attachment_grant(
        context,
        direct_host,
        direct_input,
        Some(&prepared.grant_ref),
    )
    .await?;
    Ok(V2AttachmentProductSendSummary {
        direct,
        redacted_manifest: prepared.redacted_manifest,
        grant_ref: prepared.grant_ref,
    })
}

async fn preflight_recipients<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    target_did: &str,
) -> crate::ImResult<()>
where
    H: V2DirectProductHost,
{
    host.ensure_local_prekey_published().await?;
    let local_document = host.resolve_did_document(&context.local_did).await?;
    validate_local_endpoint(context, &local_document)?;
    let target_document = if target_did == context.local_did {
        local_document.clone()
    } else {
        host.resolve_did_document(target_did).await?
    };
    let targets = delivery_targets(context, target_did, &target_document, &local_document)?;
    if !targets
        .iter()
        .any(|target| target.class == DeliveryClass::Recipient)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn prepared_attachment_product(
    expected_target_did: &str,
    committed: crate::internal::attachment_runtime::upload::PreparedCommittedAttachment,
) -> crate::ImResult<V2PreparedAttachmentProduct> {
    if committed.target_did != expected_target_did {
        return Err(crate::ImError::PermissionDenied);
    }
    let prepared = V2PreparedAttachmentProduct {
        full_manifest: committed.full_manifest,
        redacted_manifest: committed.redacted_manifest,
        grant_ref: committed.grant_ref,
    };
    validate_prepared_attachment_product(&prepared)?;
    Ok(prepared)
}

fn attachment_source_digest(
    target: &crate::messages::MessageTarget,
    request: &crate::attachments::AttachmentSendRequest,
) -> crate::ImResult<String> {
    // Attachment bytes and local paths may be part of the serialized request.
    // Keep the temporary canonical buffer zeroizing and persist only its hash.
    let mut canonical = Zeroizing::new(Vec::new());
    serde_json_canonicalizer::to_writer(&(target, request), &mut *canonical).map_err(|error| {
        crate::ImError::Serialization {
            detail: format!("canonicalize P5 attachment source: {error}"),
        }
    })?;
    let digest = Sha256::digest(canonical.as_slice());
    Ok(format!("sha256:{}", URL_SAFE_NO_PAD.encode(digest)))
}

fn business_source_digest(plaintext: &V2ApplicationPlaintext) -> crate::ImResult<String> {
    // The canonical buffer can contain the message plaintext. Persist only its
    // digest and zeroize the temporary bytes on every success/error path.
    let mut canonical = Zeroizing::new(Vec::new());
    serde_json_canonicalizer::to_writer(plaintext, &mut *canonical).map_err(|error| {
        crate::ImError::Serialization {
            detail: format!("canonicalize P5 business intent: {error}"),
        }
    })?;
    let mut digest = Sha256::new();
    digest.update(b"AWIKI-P5-V2-BUSINESS-INTENT-V1\0");
    digest.update(canonical.as_slice());
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(digest.finalize())
    ))
}

fn attachment_intent_secret_id(logical_message_id: &str, target_did: &str) -> String {
    let mut digest = Sha256::new();
    for value in [
        "AWIKI-P5-V2-ATTACHMENT-INTENT-V1",
        logical_message_id,
        target_did,
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

fn attachment_intent_lock(
    context: &V2DirectProductContext,
    input: &V2AttachmentFanoutInput,
) -> Arc<AttachmentIntentLock> {
    let mut digest = Sha256::new();
    for value in [
        "AWIKI-P5-V2-ATTACHMENT-PROCESS-LOCK-V1",
        context.owner_identity_id.as_str(),
        context.local_device_id.as_str(),
        input.logical_message_id.as_str(),
        input.target_did.as_str(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    let key = URL_SAFE_NO_PAD.encode(digest.finalize());
    let locks = ATTACHMENT_INTENT_LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Weak entries keep the registry bounded by currently active attachment
    // sends rather than every logical message seen by this process.
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(AttachmentIntentLock::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

pub(crate) async fn send_with_host<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    input: V2DirectProductSendInput,
) -> crate::ImResult<V2DirectProductSendSummary>
where
    H: V2DirectProductHost,
{
    send_with_host_with_attachment_grant(context, host, input, None).await
}

async fn send_with_host_with_attachment_grant<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    input: V2DirectProductSendInput,
    attachment_grant_ref: Option<&Value>,
) -> crate::ImResult<V2DirectProductSendSummary>
where
    H: V2DirectProductHost,
{
    required("logical_message_id", &input.logical_message_id)?;
    required("target_did", &input.target_did)?;
    let plaintext = input
        .body
        .plaintext(&input.logical_message_id, input.conversation_id.as_deref())?;
    let source_digest = business_source_digest(&plaintext)?;

    // Publishing also proves that the selected local vNext device remains a
    // valid P5 endpoint. It runs before any recipient ledger or wire effect.
    host.ensure_local_prekey_published().await?;
    let local_document = host.resolve_did_document(&context.local_did).await?;
    validate_local_endpoint(context, &local_document)?;
    let _ = retry_session_replies_with_host(context, host).await?;
    let target_document = if input.target_did == context.local_did {
        local_document.clone()
    } else {
        host.resolve_did_document(&input.target_did).await?
    };
    let targets = delivery_targets(
        context,
        &input.target_did,
        &target_document,
        &local_document,
    )?;
    let current_endpoints = targets
        .iter()
        .map(delivery_endpoint_key)
        .collect::<BTreeSet<_>>();
    let now = now_text();
    context.with_ledger(|ledger| {
        ledger.mark_removed_ineligible(
            &input.logical_message_id,
            &input.target_did,
            &current_endpoints,
            &now,
        )
    })?;

    if !targets
        .iter()
        .any(|target| target.class == DeliveryClass::Recipient)
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let own_sync_plaintext = own_sync_plaintext(context, &input.target_did, &plaintext)?;

    let mut accepted_at = None;
    let mut accepted = 0_usize;
    let mut attempted = 0_usize;
    let mut previously_accepted = 0_usize;
    let mut newly_accepted = 0_usize;
    let mut failed = 0_usize;
    for target in &targets {
        let operation_id = delivery_operation_id(
            &context.local_did,
            &context.local_device_id,
            &input.target_did,
            target.class,
            &target.recipient_did,
            &target.device.device_id,
            &input.logical_message_id,
        );
        let mut record = context.with_ledger(|ledger| {
            ledger.ensure_delivery(
                &input.logical_message_id,
                &input.target_did,
                target,
                &operation_id,
                &source_digest,
                &now,
            )
        })?;
        if record.phase == DeliveryPhase::Accepted {
            cleanup_accepted_pending(context, &record)?;
            accepted += 1;
            previously_accepted += 1;
            accepted_at = max_timestamp(accepted_at, record.accepted_at.clone());
            continue;
        }
        if record.phase == DeliveryPhase::Ineligible {
            return Err(crate::ImError::PermissionDenied);
        }
        attempted += 1;

        let binding = binding_for(context, &target.device, &target.recipient_did)?;
        let recipient_document = if target.recipient_did == context.local_did {
            &local_document
        } else {
            &target_document
        };
        let delivery_plaintext = match target.class {
            DeliveryClass::Recipient => &plaintext,
            DeliveryClass::OwnSync => &own_sync_plaintext,
        };
        let prepared = match prepare_delivery(
            context,
            host,
            &binding,
            &target.device,
            recipient_document,
            &record,
            delivery_plaintext,
            &now,
        )
        .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                context.with_ledger(|ledger| {
                    ledger.mark_failed(&record, failure_code(&error), &now)
                })?;
                failed += 1;
                continue;
            }
        };
        context.with_ledger(|ledger| ledger.mark_prepared(&record, &now))?;
        record.wire_prepared = true;

        let posted = match attachment_grant_ref {
            Some(grant_ref) => host.post_direct_attachment(&prepared, grant_ref).await,
            None => host.post_direct(&prepared).await,
        };
        let result = match posted {
            Ok(result) => result,
            Err(error) => {
                context.with_ledger(|ledger| {
                    ledger.mark_failed(&record, failure_code(&error), &now_text())
                })?;
                failed += 1;
                continue;
            }
        };
        if validate_send_result(&result, &prepared).is_err() {
            context.with_ledger(|ledger| {
                ledger.mark_failed(&record, "invalid_response", &now_text())
            })?;
            failed += 1;
            continue;
        }

        // The validated server acceptance is recorded before deleting the
        // exact retry ciphertext. A crash can therefore leak no duplicate:
        // restart skips accepted delivery and only finishes pending cleanup.
        context.with_ledger(|ledger| {
            ledger.mark_accepted(&record, &result.accepted_at, &now_text())
        })?;
        let _ = context.with_direct(|direct| direct.mark_outbound_accepted(&prepared))?;
        accepted += 1;
        newly_accepted += 1;
        accepted_at = max_timestamp(accepted_at, Some(result.accepted_at));
    }

    Ok(V2DirectProductSendSummary {
        logical_message_id: input.logical_message_id,
        target_did: input.target_did,
        target_device_count: targets
            .iter()
            .filter(|target| target.class == DeliveryClass::Recipient)
            .count(),
        own_sync_device_count: targets
            .iter()
            .filter(|target| target.class == DeliveryClass::OwnSync)
            .count(),
        attempted_device_count: attempted,
        previously_accepted_device_count: previously_accepted,
        newly_accepted_device_count: newly_accepted,
        accepted_device_count: accepted,
        failed_device_count: failed,
        accepted_at,
    })
}

async fn prepare_delivery<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    binding: &V2SessionBinding,
    target: &DeviceManifestEntry,
    target_document: &Value,
    record: &DeliveryRecord,
    plaintext: &V2ApplicationPlaintext,
    now: &str,
) -> crate::ImResult<PreparedV2Outbound>
where
    H: V2DirectProductHost,
{
    if let Some(prepared) =
        context.with_direct(|direct| direct.resume_outbound(binding, &record.operation_id))?
    {
        return Ok(prepared);
    }
    if record.wire_prepared {
        // Never derive different bytes for an operation that may already have
        // been accepted remotely.
        return Err(crate::ImError::PermissionDenied);
    }
    if context.with_direct(|direct| direct.has_established_session(binding))? {
        return context.with_direct(|direct| {
            direct.prepare_outbound(binding, &record.operation_id, plaintext, now)
        });
    }

    let fetched = host
        .fetch_prekey(
            &record.recipient_did,
            &record.recipient_device_id,
            target_document,
            &record.operation_id,
        )
        .await?;
    verify_fetched_prekey(&fetched, target, &record.recipient_did, target_document)?;
    let recipient_static = super::v2_prekey_runtime::static_public_from_document(
        target_document,
        &record.recipient_e2ee_key_id,
    )?;
    context.with_direct(|direct| {
        // The first logical business payload is the Init plaintext. Session
        // confirmation happens automatically on receive; the user never has
        // to resend the first message after an empty handshake.
        direct.prepare_session_init(
            binding,
            &record.operation_id,
            plaintext,
            &context.local_static_private,
            &fetched,
            &recipient_static,
            now,
        )
    })
}

fn cleanup_accepted_pending(
    context: &V2DirectProductContext,
    record: &DeliveryRecord,
) -> crate::ImResult<()> {
    if !record.wire_prepared {
        return Err(crate::ImError::PermissionDenied);
    }
    let binding = binding_for_parts(
        context,
        &record.recipient_did,
        &record.recipient_device_id,
        &record.recipient_e2ee_key_id,
    )?;
    if let Some(prepared) =
        context.with_direct(|direct| direct.resume_outbound(&binding, &record.operation_id))?
    {
        let _ = context.with_direct(|direct| direct.mark_outbound_accepted(&prepared))?;
    }
    Ok(())
}

fn verify_fetched_prekey(
    fetched: &V2GetPrekeyBundleResult,
    target: &DeviceManifestEntry,
    target_did: &str,
    target_document: &Value,
) -> crate::ImResult<()> {
    fetched
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if fetched.target_did != target_did
        || fetched.target_device_id != target.device_id
        || fetched.prekey_bundle.static_key_agreement_id != target.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    anp::direct_e2ee::verify_prekey_bundle_v2(&fetched.prekey_bundle, target_document, Utc::now())
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn validate_local_endpoint(
    context: &V2DirectProductContext,
    local_document: &Value,
) -> crate::ImResult<()> {
    let local = anp::authentication::find_eligible_device(
        local_document,
        &context.local_device_id,
        PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    if local.e2ee_key_id != context.local_e2ee_key_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let document_public = super::v2_prekey_runtime::static_public_from_document(
        local_document,
        &context.local_e2ee_key_id,
    )?;
    if x25519_dalek::PublicKey::from(&context.local_static_private).to_bytes() != document_public {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn direct_devices(
    target_did: &str,
    target_document: &Value,
) -> crate::ImResult<Vec<DeviceManifestEntry>> {
    if target_document.get("id").and_then(Value::as_str) != Some(target_did) {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(target_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut devices = manifest
        .devices
        .into_iter()
        .filter(|device| {
            device
                .profiles
                .iter()
                .any(|profile| profile == PROFILE_DIRECT_E2EE_V2)
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| left.device_id.cmp(&right.device_id));
    Ok(devices)
}

fn delivery_targets(
    context: &V2DirectProductContext,
    target_did: &str,
    target_document: &Value,
    local_document: &Value,
) -> crate::ImResult<Vec<DeliveryTarget>> {
    let mut targets = direct_devices(target_did, target_document)?
        .into_iter()
        .filter(|device| {
            target_did != context.local_did || device.device_id != context.local_device_id
        })
        .map(|device| DeliveryTarget {
            class: DeliveryClass::Recipient,
            recipient_did: target_did.to_owned(),
            device,
        })
        .collect::<Vec<_>>();
    // One user action also reaches every currently eligible sibling device.
    // It is a separate encrypted JSON control per sibling, never a service
    // `deliveries[]` extension and never a recipient chat bubble.
    if target_did != context.local_did {
        targets.extend(
            direct_devices(&context.local_did, local_document)?
                .into_iter()
                .filter(|device| device.device_id != context.local_device_id)
                .map(|device| DeliveryTarget {
                    class: DeliveryClass::OwnSync,
                    recipient_did: context.local_did.clone(),
                    device,
                }),
        );
    }
    targets.sort_by(|left, right| {
        left.class
            .as_str()
            .cmp(right.class.as_str())
            .then_with(|| left.recipient_did.cmp(&right.recipient_did))
            .then_with(|| left.device.device_id.cmp(&right.device.device_id))
    });
    Ok(targets)
}

fn own_sync_plaintext(
    context: &V2DirectProductContext,
    target_did: &str,
    business: &V2ApplicationPlaintext,
) -> crate::ImResult<V2ApplicationPlaintext> {
    let payload = serde_json::json!({
        "system_type": DEVICE_SYNC_SYSTEM_TYPE,
        "sync_type": "outbound-message",
        "original_sender_did": context.local_did,
        "original_sender_device_id": context.local_device_id,
        "target_did": target_did,
        "message": business,
    });
    let plaintext = V2ApplicationPlaintext {
        application_content_type: "application/json".to_owned(),
        logical_message_id: business.logical_message_id.clone(),
        conversation_id: business.conversation_id.clone(),
        reply_to_message_id: None,
        annotations: None,
        text: None,
        payload: Some(payload),
        payload_b64u: None,
    };
    plaintext
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(plaintext)
}

fn validate_prepared_attachment_product(
    prepared: &V2PreparedAttachmentProduct,
) -> crate::ImResult<()> {
    validate_full_attachment_manifest(&prepared.full_manifest)?;
    if prepared.redacted_manifest
        != crate::attachments::manifest::redact_attachment_manifest(&prepared.full_manifest)
        || contains_attachment_secret_field(&prepared.redacted_manifest)
        || contains_attachment_secret_field(&prepared.grant_ref)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let redacted = crate::attachments::manifest::parse_attachment_manifest_internal(
        &prepared.redacted_manifest,
    )?;
    if redacted.attachments.is_empty()
        || redacted.attachments.iter().any(|attachment| {
            attachment.object_key_b64u.is_some() || attachment.nonce_b64u.is_some()
        })
        || !prepared.grant_ref.is_object()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_full_attachment_manifest(manifest: &Value) -> crate::ImResult<()> {
    let parsed = crate::attachments::manifest::parse_attachment_manifest_internal(manifest)?;
    if parsed.attachments.is_empty()
        || parsed.primary_attachment_id.is_empty()
        || !parsed
            .attachments
            .iter()
            .any(|attachment| attachment.descriptor.attachment_id == parsed.primary_attachment_id)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    for attachment in parsed.attachments {
        let descriptor = attachment.descriptor;
        if descriptor.attachment_id.is_empty()
            || descriptor.object_uri.is_empty()
            || descriptor.digest_b64u.is_empty()
            || descriptor.object_encryption_mode()
                != crate::attachments::manifest::OBJECT_ENCRYPTION_MODE_E2EE
            || descriptor.object_cipher.as_deref()
                != Some(crate::internal::attachment_runtime::object_crypto::OBJECT_E2EE_CIPHER)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let object_key = attachment
            .object_key_b64u
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let nonce = attachment
            .nonce_b64u
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        if URL_SAFE_NO_PAD
            .decode(object_key)
            .ok()
            .filter(|value| {
                value.len()
                    == crate::internal::attachment_runtime::object_crypto::OBJECT_E2EE_KEY_LEN
            })
            .is_none()
            || URL_SAFE_NO_PAD
                .decode(nonce)
                .ok()
                .filter(|value| {
                    value.len()
                        == crate::internal::attachment_runtime::object_crypto::OBJECT_E2EE_NONCE_LEN
                })
                .is_none()
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

fn contains_attachment_secret_field(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(key.as_str(), "object_key_b64u" | "nonce_b64u")
                || contains_attachment_secret_field(value)
        }),
        Value::Array(values) => values.iter().any(contains_attachment_secret_field),
        _ => false,
    }
}

fn delivery_endpoint_key(target: &DeliveryTarget) -> String {
    format!(
        "{}\0{}\0{}",
        target.class.as_str(),
        target.recipient_did,
        target.device.device_id
    )
}

fn binding_for(
    context: &V2DirectProductContext,
    target: &DeviceManifestEntry,
    target_did: &str,
) -> crate::ImResult<V2SessionBinding> {
    binding_for_parts(context, target_did, &target.device_id, &target.e2ee_key_id)
}

fn binding_for_parts(
    context: &V2DirectProductContext,
    peer_did: &str,
    peer_device_id: &str,
    peer_e2ee_key_id: &str,
) -> crate::ImResult<V2SessionBinding> {
    let binding = V2SessionBinding {
        local_did: context.local_did.clone(),
        local_device_id: context.local_device_id.clone(),
        peer_did: peer_did.to_owned(),
        peer_device_id: peer_device_id.to_owned(),
        suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
        local_e2ee_key_id: context.local_e2ee_key_id.clone(),
        peer_e2ee_key_id: peer_e2ee_key_id.to_owned(),
    };
    binding
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(binding)
}

fn delivery_operation_id(
    local_did: &str,
    local_device_id: &str,
    target_did: &str,
    delivery_class: DeliveryClass,
    recipient_did: &str,
    target_device_id: &str,
    logical_message_id: &str,
) -> String {
    let mut digest = Sha256::new();
    for value in [
        "AWIKI-P5-V2-DELIVERY-V1",
        local_did,
        local_device_id,
        target_did,
        delivery_class.as_str(),
        recipient_did,
        target_device_id,
        logical_message_id,
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!(
        "{DELIVERY_OPERATION_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(&digest.finalize()[..16])
    )
}

fn validate_send_result(
    result: &V2DirectSendResult,
    prepared: &PreparedV2Outbound,
) -> crate::ImResult<()> {
    result
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if result.message_id != prepared.metadata.message_id
        || result.operation_id != prepared.metadata.operation_id
        || result.target_did != prepared.metadata.target.did
        || result.recipient_device_id != prepared.metadata.recipient_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

struct DeliveryLedger<'a> {
    connection: &'a Connection,
    owner_identity_id: &'a str,
    owner_did: &'a str,
    local_device_id: &'a str,
    vault: &'a Arc<dyn crate::vault::SecretVault + Send + Sync>,
}

impl<'a> DeliveryLedger<'a> {
    fn new(
        connection: &'a Connection,
        context: &'a V2DirectProductContext,
    ) -> crate::ImResult<Self> {
        connection
            .execute_batch(DELIVERY_SCHEMA)
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        ensure_delivery_source_digest_column(connection)?;
        Ok(Self {
            connection,
            owner_identity_id: &context.owner_identity_id,
            owner_did: &context.local_did,
            local_device_id: &context.local_device_id,
            vault: &context.vault,
        })
    }

    fn load_attachment_intent(
        &self,
        logical_message_id: &str,
        target_did: &str,
        source_digest: &str,
    ) -> crate::ImResult<Option<V2PreparedAttachmentProduct>> {
        let logical_message_id = required("logical_message_id", logical_message_id)?;
        let target_did = required("target_did", target_did)?;
        let source_digest = required("source_digest", source_digest)?;
        let row = self
            .connection
            .query_row(
                r#"SELECT source_digest, full_manifest_blob,
                          redacted_manifest_json, grant_ref_json
FROM direct_e2ee_v2_attachment_intents
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3
  AND logical_message_id = ?4 AND target_did = ?5"#,
                params![
                    self.owner_identity_id,
                    self.owner_did,
                    self.local_device_id,
                    logical_message_id,
                    target_did,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let Some((stored_source_digest, full_manifest_blob, redacted_json, grant_json)) = row
        else {
            return Ok(None);
        };
        if stored_source_digest != source_digest {
            // Reusing an idempotency/logical ID for different local bytes or
            // options must not silently distribute the first object.
            return Err(crate::ImError::PermissionDenied);
        }

        let intent_id = attachment_intent_secret_id(logical_message_id, target_did);
        let plaintext = Zeroizing::new(open_direct_secret_blob_strict(
            self.vault,
            full_manifest_blob,
            &DirectSecretOpenExpectation {
                owner_identity_id: self.owner_identity_id,
                owner_did: self.owner_did,
                device_id: self.local_device_id,
                kind: crate::vault::SecretKind::DirectE2eeV2AttachmentManifest,
                key_id_prefix: direct_secret_key_id_prefix(
                    self.owner_identity_id,
                    "v2-attachment-manifest",
                    &intent_id,
                    self.local_device_id,
                ),
            },
        )?);
        let prepared = V2PreparedAttachmentProduct {
            full_manifest: serde_json::from_slice(plaintext.as_slice()).map_err(|error| {
                crate::ImError::Serialization {
                    detail: format!("parse sealed P5 attachment Manifest: {error}"),
                }
            })?,
            redacted_manifest: serde_json::from_str(&redacted_json).map_err(|error| {
                crate::ImError::Serialization {
                    detail: format!("parse redacted P5 attachment Manifest: {error}"),
                }
            })?,
            grant_ref: serde_json::from_str(&grant_json).map_err(|error| {
                crate::ImError::Serialization {
                    detail: format!("parse P5 attachment grant ref: {error}"),
                }
            })?,
        };
        validate_prepared_attachment_product(&prepared)?;
        Ok(Some(prepared))
    }

    fn save_attachment_intent(
        &self,
        logical_message_id: &str,
        target_did: &str,
        source_digest: &str,
        prepared: V2PreparedAttachmentProduct,
        now: &str,
    ) -> crate::ImResult<V2PreparedAttachmentProduct> {
        let logical_message_id = required("logical_message_id", logical_message_id)?;
        let target_did = required("target_did", target_did)?;
        let source_digest = required("source_digest", source_digest)?;
        let now = required("now", now)?;
        validate_prepared_attachment_product(&prepared)?;

        let mut plaintext = Zeroizing::new(Vec::new());
        serde_json_canonicalizer::to_writer(&prepared.full_manifest, &mut *plaintext).map_err(
            |error| crate::ImError::Serialization {
                detail: format!("serialize sealed P5 attachment Manifest: {error}"),
            },
        )?;
        let intent_id = attachment_intent_secret_id(logical_message_id, target_did);
        let full_manifest_blob = seal_direct_secret_blob(
            Some(self.vault),
            DirectSecretSealInput {
                owner_identity_id: self.owner_identity_id,
                owner_did: self.owner_did,
                device_id: Some(self.local_device_id),
                kind: crate::vault::SecretKind::DirectE2eeV2AttachmentManifest,
                key_id: direct_secret_key_id(
                    self.owner_identity_id,
                    "v2-attachment-manifest",
                    &intent_id,
                    self.local_device_id,
                ),
                plaintext: plaintext.as_slice(),
                field: "P5 v2 attachment full Manifest",
            },
        )?;
        let secret_ref = direct_secret_ref_from_blob(&full_manifest_blob)?;
        let redacted_json =
            serde_json::to_string(&prepared.redacted_manifest).map_err(|error| {
                crate::ImError::Serialization {
                    detail: format!("serialize redacted P5 attachment Manifest: {error}"),
                }
            })?;
        let grant_json = serde_json::to_string(&prepared.grant_ref).map_err(|error| {
            crate::ImError::Serialization {
                detail: format!("serialize P5 attachment grant ref: {error}"),
            }
        })?;
        debug_assert!(!contains_attachment_secret_field(
            &serde_json::from_str(&redacted_json).unwrap_or(Value::Null)
        ));
        debug_assert!(!contains_attachment_secret_field(
            &serde_json::from_str(&grant_json).unwrap_or(Value::Null)
        ));

        let inserted = self.connection.execute(
            r#"INSERT INTO direct_e2ee_v2_attachment_intents
 (owner_identity_id, owner_did, local_device_id, logical_message_id, target_did,
  source_digest, full_manifest_blob, redacted_manifest_json, grant_ref_json,
  created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
ON CONFLICT(owner_identity_id, local_device_id, logical_message_id, target_did)
DO NOTHING"#,
            params![
                self.owner_identity_id,
                self.owner_did,
                self.local_device_id,
                logical_message_id,
                target_did,
                source_digest,
                full_manifest_blob,
                redacted_json,
                grant_json,
                now,
            ],
        );
        let inserted = match inserted {
            Ok(inserted) => inserted,
            Err(error) => {
                let _ = self.vault.delete(&secret_ref);
                return Err(crate::internal::local_state::local_state_unavailable(error));
            }
        };
        if inserted == 1 {
            return Ok(prepared);
        }
        if inserted != 0 {
            let _ = self.vault.delete(&secret_ref);
            return Err(crate::ImError::PermissionDenied);
        }

        // Another same-process send may have won the unique intent. Never keep
        // the losing Vault record and never upload again on the next retry.
        self.vault.delete(&secret_ref)?;
        self.load_attachment_intent(logical_message_id, target_did, source_digest)?
            .ok_or(crate::ImError::PermissionDenied)
    }

    fn ensure_delivery(
        &self,
        logical_message_id: &str,
        target_did: &str,
        target: &DeliveryTarget,
        operation_id: &str,
        source_digest: &str,
        now: &str,
    ) -> crate::ImResult<DeliveryRecord> {
        let logical_message_id = required("logical_message_id", logical_message_id)?;
        let target_did = required("target_did", target_did)?;
        let operation_id = required("operation_id", operation_id)?;
        let source_digest = required("source_digest", source_digest)?;
        self.connection
            .execute(
                r#"INSERT INTO direct_e2ee_v2_delivery_ledger
 (owner_identity_id, owner_did, local_device_id, logical_message_id, target_did,
  delivery_class, recipient_did, recipient_device_id, recipient_e2ee_key_id,
  operation_id, source_digest, phase, wire_prepared, failure_code, created_at, updated_at, accepted_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
        'pending', 0, NULL, ?12, ?12, NULL)
ON CONFLICT(owner_identity_id, local_device_id, logical_message_id, target_did,
            recipient_did, recipient_device_id) DO NOTHING"#,
                params![
                    self.owner_identity_id,
                    self.owner_did,
                    self.local_device_id,
                    logical_message_id,
                    target_did,
                    target.class.as_str(),
                    target.recipient_did,
                    target.device.device_id,
                    target.device.e2ee_key_id,
                    operation_id,
                    source_digest,
                    required("now", now)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let record = self
            .load(
                logical_message_id,
                target_did,
                &target.recipient_did,
                &target.device.device_id,
            )?
            .ok_or(crate::ImError::PermissionDenied)?;
        if record.logical_message_id != logical_message_id
            || record.target_did != target_did
            || record.delivery_class != target.class
            || record.recipient_did != target.recipient_did
            || record.recipient_e2ee_key_id != target.device.e2ee_key_id
            || record.operation_id != operation_id
            || record.source_digest != source_digest
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(record)
    }

    fn load(
        &self,
        logical_message_id: &str,
        target_did: &str,
        recipient_did: &str,
        recipient_device_id: &str,
    ) -> crate::ImResult<Option<DeliveryRecord>> {
        self.connection
            .query_row(
                r#"SELECT logical_message_id, target_did, delivery_class, recipient_did,
                          recipient_device_id, recipient_e2ee_key_id, operation_id,
                          source_digest, phase, wire_prepared, accepted_at
FROM direct_e2ee_v2_delivery_ledger
WHERE owner_identity_id = ?1 AND local_device_id = ?2
  AND logical_message_id = ?3 AND target_did = ?4
  AND recipient_did = ?5 AND recipient_device_id = ?6"#,
                params![
                    self.owner_identity_id,
                    self.local_device_id,
                    logical_message_id,
                    target_did,
                    recipient_did,
                    recipient_device_id,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, Option<String>>(10)?,
                    ))
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .map(
                |(
                    logical_message_id,
                    target_did,
                    delivery_class,
                    recipient_did,
                    recipient_device_id,
                    recipient_e2ee_key_id,
                    operation_id,
                    source_digest,
                    phase,
                    wire_prepared,
                    accepted_at,
                )| {
                    if !matches!(wire_prepared, 0 | 1) {
                        return Err(crate::ImError::PermissionDenied);
                    }
                    let source_digest = source_digest.ok_or(crate::ImError::PermissionDenied)?;
                    required("source_digest", &source_digest)?;
                    Ok(DeliveryRecord {
                        logical_message_id,
                        target_did,
                        delivery_class: DeliveryClass::parse(&delivery_class)?,
                        recipient_did,
                        recipient_device_id,
                        recipient_e2ee_key_id,
                        operation_id,
                        source_digest,
                        phase: DeliveryPhase::parse(&phase)?,
                        wire_prepared: wire_prepared == 1,
                        accepted_at,
                    })
                },
            )
            .transpose()
    }

    fn mark_prepared(&self, record: &DeliveryRecord, now: &str) -> crate::ImResult<()> {
        let changed = self
            .connection
            .execute(
                r#"UPDATE direct_e2ee_v2_delivery_ledger
SET wire_prepared = 1, phase = 'pending', failure_code = NULL, updated_at = ?4
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3
  AND phase IN ('pending', 'failed')"#,
                params![
                    self.owner_identity_id,
                    self.local_device_id,
                    record.operation_id,
                    required("now", now)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if changed != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn mark_failed(
        &self,
        record: &DeliveryRecord,
        failure_code: &str,
        now: &str,
    ) -> crate::ImResult<()> {
        required("failure_code", failure_code)?;
        let changed = self
            .connection
            .execute(
                r#"UPDATE direct_e2ee_v2_delivery_ledger
SET phase = 'failed', failure_code = ?4, updated_at = ?5
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3
  AND phase IN ('pending', 'failed')"#,
                params![
                    self.owner_identity_id,
                    self.local_device_id,
                    record.operation_id,
                    failure_code,
                    required("now", now)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if changed != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn mark_accepted(
        &self,
        record: &DeliveryRecord,
        accepted_at: &str,
        now: &str,
    ) -> crate::ImResult<()> {
        DateTime::parse_from_rfc3339(accepted_at).map_err(|_| crate::ImError::PermissionDenied)?;
        let changed = self
            .connection
            .execute(
                r#"UPDATE direct_e2ee_v2_delivery_ledger
SET phase = 'accepted', failure_code = NULL, accepted_at = ?4, updated_at = ?5
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3
  AND wire_prepared = 1 AND phase IN ('pending', 'failed', 'accepted')"#,
                params![
                    self.owner_identity_id,
                    self.local_device_id,
                    record.operation_id,
                    accepted_at,
                    required("now", now)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if changed != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn mark_removed_ineligible(
        &self,
        logical_message_id: &str,
        target_did: &str,
        current_endpoints: &BTreeSet<String>,
        now: &str,
    ) -> crate::ImResult<()> {
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT delivery_class, recipient_did, recipient_device_id, operation_id
FROM direct_e2ee_v2_delivery_ledger
WHERE owner_identity_id = ?1 AND local_device_id = ?2
  AND logical_message_id = ?3 AND target_did = ?4 AND phase IN ('pending', 'failed')"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let stored = statement
            .query_map(
                params![
                    self.owner_identity_id,
                    self.local_device_id,
                    logical_message_id,
                    target_did,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        drop(statement);
        for (delivery_class, recipient_did, recipient_device_id, operation_id) in stored {
            let endpoint = format!("{delivery_class}\0{recipient_did}\0{recipient_device_id}");
            if current_endpoints.contains(&endpoint) {
                continue;
            }
            self.connection
                .execute(
                    r#"UPDATE direct_e2ee_v2_delivery_ledger
SET phase = 'ineligible', failure_code = 'device_ineligible', updated_at = ?4
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3
  AND phase IN ('pending', 'failed')"#,
                    params![
                        self.owner_identity_id,
                        self.local_device_id,
                        operation_id,
                        required("now", now)?,
                    ],
                )
                .map_err(crate::internal::local_state::local_state_unavailable)?;
        }
        Ok(())
    }

    fn register_session_reply(
        &self,
        binding: &V2SessionBinding,
        init_message_id: &str,
        session_id: &str,
        now: &str,
    ) -> crate::ImResult<SessionReplyRecord> {
        binding
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        if binding.local_did != self.owner_did || binding.local_device_id != self.local_device_id {
            return Err(crate::ImError::PermissionDenied);
        }
        let operation_id = session_reply_operation_id(init_message_id)?;
        self.connection
            .execute(
                r#"INSERT INTO direct_e2ee_v2_session_reply_ledger
 (owner_identity_id, owner_did, local_device_id, local_e2ee_key_id,
  peer_did, peer_device_id, peer_e2ee_key_id, init_message_id, operation_id,
  session_id, phase, created_at, updated_at, accepted_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
        'pending', ?11, ?11, NULL)
ON CONFLICT(owner_identity_id, local_device_id, operation_id) DO NOTHING"#,
                params![
                    self.owner_identity_id,
                    self.owner_did,
                    self.local_device_id,
                    binding.local_e2ee_key_id,
                    binding.peer_did,
                    binding.peer_device_id,
                    binding.peer_e2ee_key_id,
                    init_message_id,
                    operation_id,
                    session_id,
                    required("now", now)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let record = self
            .load_session_reply(&operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if record.binding != *binding
            || record.init_message_id != init_message_id
            || record.session_id != session_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(record)
    }

    fn load_session_reply(
        &self,
        operation_id: &str,
    ) -> crate::ImResult<Option<SessionReplyRecord>> {
        self.connection
            .query_row(
                r#"SELECT owner_did, local_device_id, local_e2ee_key_id, peer_did,
       peer_device_id, peer_e2ee_key_id, init_message_id, operation_id, session_id, phase
FROM direct_e2ee_v2_session_reply_ledger
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3"#,
                params![self.owner_identity_id, self.local_device_id, operation_id],
                |row| {
                    let phase = row.get::<_, String>(9)?;
                    if !matches!(phase.as_str(), "pending" | "accepted") {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    Ok(SessionReplyRecord {
                        binding: V2SessionBinding {
                            local_did: row.get(0)?,
                            local_device_id: row.get(1)?,
                            suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
                            local_e2ee_key_id: row.get(2)?,
                            peer_did: row.get(3)?,
                            peer_device_id: row.get(4)?,
                            peer_e2ee_key_id: row.get(5)?,
                        },
                        init_message_id: row.get(6)?,
                        operation_id: row.get(7)?,
                        session_id: row.get(8)?,
                        accepted: phase == "accepted",
                    })
                },
            )
            .optional()
            .map_err(crate::internal::local_state::local_state_unavailable)
    }

    fn pending_session_replies(&self) -> crate::ImResult<Vec<SessionReplyRecord>> {
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT operation_id FROM direct_e2ee_v2_session_reply_ledger
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND phase = 'pending'
ORDER BY created_at, operation_id"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let operation_ids = statement
            .query_map(
                params![self.owner_identity_id, self.local_device_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        drop(statement);
        operation_ids
            .into_iter()
            .map(|operation_id| {
                self.load_session_reply(&operation_id)?
                    .ok_or(crate::ImError::PermissionDenied)
            })
            .collect()
    }

    fn mark_session_reply_accepted(
        &self,
        record: &SessionReplyRecord,
        accepted_at: &str,
        now: &str,
    ) -> crate::ImResult<()> {
        DateTime::parse_from_rfc3339(accepted_at).map_err(|_| crate::ImError::PermissionDenied)?;
        let changed = self
            .connection
            .execute(
                r#"UPDATE direct_e2ee_v2_session_reply_ledger
SET phase = 'accepted', accepted_at = ?4, updated_at = ?5
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND operation_id = ?3
  AND phase IN ('pending', 'accepted')"#,
                params![
                    self.owner_identity_id,
                    self.local_device_id,
                    record.operation_id,
                    accepted_at,
                    required("now", now)?,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        if changed != 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    fn remove_session_reply_for_session(
        &self,
        binding: &V2SessionBinding,
        session_id: &str,
    ) -> crate::ImResult<()> {
        self.connection
            .execute(
                r#"DELETE FROM direct_e2ee_v2_session_reply_ledger
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND peer_did = ?3
  AND peer_device_id = ?4 AND session_id = ?5"#,
                params![
                    self.owner_identity_id,
                    self.local_device_id,
                    binding.peer_did,
                    binding.peer_device_id,
                    session_id,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(())
    }
}

#[derive(Clone, PartialEq)]
pub(crate) enum V2InboundBusinessBody {
    Text { text: String, markdown: bool },
    Json { payload: Value },
    Attachment { full_manifest: Value },
}

impl fmt::Debug for V2InboundBusinessBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { text, markdown } => formatter
                .debug_struct("Text")
                .field("text", text)
                .field("markdown", markdown)
                .finish(),
            Self::Json { payload } => formatter
                .debug_struct("Json")
                .field("payload", payload)
                .finish(),
            Self::Attachment { full_manifest } => formatter
                .debug_struct("Attachment")
                .field(
                    "redacted_manifest",
                    &crate::attachments::manifest::redact_attachment_manifest(full_manifest),
                )
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2InboundBusinessProjection {
    pub(crate) logical_message_id: String,
    pub(crate) conversation_id: Option<String>,
    pub(crate) sender_did: String,
    pub(crate) sender_device_id: String,
    pub(crate) recipient_did: String,
    pub(crate) wire_message_id: String,
    pub(crate) body: V2InboundBusinessBody,
    pub(crate) session_reply_pending: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2InboundOwnSyncProjection {
    pub(crate) logical_message_id: String,
    pub(crate) conversation_id: Option<String>,
    pub(crate) original_sender_did: String,
    pub(crate) original_sender_device_id: String,
    pub(crate) target_did: String,
    pub(crate) wire_message_id: String,
    pub(crate) body: V2InboundBusinessBody,
    pub(crate) session_reply_pending: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum V2InboundProductOutcome {
    Business(V2InboundBusinessProjection),
    OwnSync(V2InboundOwnSyncProjection),
    Replay,
    ConsumedControl,
    SuppressedControl,
}

enum ValidatedInboundPlaintext {
    Business(V2InboundBusinessProjection),
    OwnSync(V2InboundOwnSyncProjection),
    SuppressedControl,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct V2OwnSyncPayload {
    system_type: String,
    sync_type: String,
    original_sender_did: String,
    original_sender_device_id: String,
    target_did: String,
    message: V2ApplicationPlaintext,
}

pub(crate) async fn receive_for_client(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    enabled: bool,
    metadata: V2DirectMetadata,
    body: V2DirectBody,
) -> crate::ImResult<V2InboundProductOutcome> {
    receive_for_client_scoped(core, client, enabled, metadata, body, None, None).await
}

pub(crate) async fn receive_for_client_scoped(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    enabled: bool,
    metadata: V2DirectMetadata,
    body: V2DirectBody,
    expected_peer_did: Option<&str>,
    delivery: Option<
        &crate::internal::identity_root_import_completion::TrustedDirectDeliveryContext,
    >,
) -> crate::ImResult<V2InboundProductOutcome> {
    if !enabled {
        return Err(crate::ImError::unsupported(
            "awiki-multi-device-direct-disabled",
        ));
    }
    if let Some(delivery) = delivery {
        match crate::internal::identity_root_import_completion::receive_root_envelope_candidate(
            core, client, &metadata, &body, delivery,
        )
        .await?
        {
            crate::internal::identity_root_import_completion::RootInboundInterceptOutcome::NotRoot => {}
            crate::internal::identity_root_import_completion::RootInboundInterceptOutcome::Consumed => {
                return Ok(V2InboundProductOutcome::ConsumedControl);
            }
            crate::internal::identity_root_import_completion::RootInboundInterceptOutcome::Replay => {
                return Ok(V2InboundProductOutcome::Replay);
            }
            crate::internal::identity_root_import_completion::RootInboundInterceptOutcome::SuppressedForHydration => {
                return Ok(V2InboundProductOutcome::SuppressedControl);
            }
        }
    }
    let context = V2DirectProductContext::from_client(core, client)?;
    #[cfg(test)]
    if let Some(outcome) = v2_product_tests::receive_registered_runtime_wire(
        &context,
        metadata.clone(),
        body.clone(),
        expected_peer_did,
    )
    .await
    {
        return outcome;
    }
    let mut host = CoreV2DirectProductHost { core, client };
    receive_with_host_scoped(&context, &mut host, metadata, body, expected_peer_did).await
}

pub(crate) async fn receive_with_host<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    metadata: V2DirectMetadata,
    body: V2DirectBody,
) -> crate::ImResult<V2InboundProductOutcome>
where
    H: V2DirectProductHost,
{
    receive_with_host_scoped(context, host, metadata, body, None).await
}

pub(crate) async fn receive_with_host_scoped<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    metadata: V2DirectMetadata,
    body: V2DirectBody,
    expected_peer_did: Option<&str>,
) -> crate::ImResult<V2InboundProductOutcome>
where
    H: V2DirectProductHost,
{
    let expected_peer_did = expected_peer_did
        .map(|value| required("expected_peer_did", value))
        .transpose()?;
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if metadata.profile != DIRECT_E2EE_PROFILE_V2
        || metadata.target.did != context.local_did
        || metadata.recipient_device_id != context.local_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    host.ensure_local_prekey_published().await?;
    let local_document = host.resolve_did_document(&context.local_did).await?;
    validate_local_endpoint(context, &local_document)?;
    let _ = retry_session_replies_with_host(context, host).await?;
    let sender_document = if metadata.sender_did == context.local_did {
        local_document.clone()
    } else {
        host.resolve_did_document(&metadata.sender_did).await?
    };
    let sender = anp::authentication::find_eligible_device(
        &sender_document,
        &metadata.sender_device_id,
        PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    let binding = inbound_binding(context, &metadata, &sender)?;
    let sender_static = super::v2_prekey_runtime::static_public_from_document(
        &sender_document,
        &sender.e2ee_key_id,
    )?;
    let now = now_text();

    match body {
        V2DirectBody::Init(init) => {
            if is_session_reply_operation_id(&metadata.operation_id) {
                return Ok(V2InboundProductOutcome::ConsumedControl);
            }
            let decrypted = context.with_direct(|direct| {
                direct.decrypt_inbound_init_validated(
                    &binding,
                    &metadata,
                    &init,
                    &context.local_static_private,
                    &sender_static,
                    &now,
                    |plaintext, _| {
                        validate_inbound_plaintext(plaintext, &metadata, expected_peer_did)
                    },
                )
            })?;
            let session_id = match &decrypted {
                V2ValidatedInboundOutcome::Decrypted { session, .. }
                | V2ValidatedInboundOutcome::Replay { session } => session.session_id.clone(),
            };
            let reply_pending = send_session_reply(
                context,
                host,
                &binding,
                &metadata.message_id,
                &session_id,
                &now,
            )
            .await
            .is_err();
            match decrypted {
                V2ValidatedInboundOutcome::Decrypted {
                    validated: ValidatedInboundPlaintext::Business(mut projection),
                    ..
                } => {
                    projection.session_reply_pending = reply_pending;
                    Ok(V2InboundProductOutcome::Business(projection))
                }
                V2ValidatedInboundOutcome::Decrypted {
                    validated: ValidatedInboundPlaintext::OwnSync(mut projection),
                    ..
                } => {
                    projection.session_reply_pending = reply_pending;
                    Ok(V2InboundProductOutcome::OwnSync(projection))
                }
                V2ValidatedInboundOutcome::Decrypted {
                    validated: ValidatedInboundPlaintext::SuppressedControl,
                    ..
                } => Ok(V2InboundProductOutcome::SuppressedControl),
                V2ValidatedInboundOutcome::Replay { .. } => Ok(V2InboundProductOutcome::Replay),
            }
        }
        V2DirectBody::Cipher(cipher) => {
            if is_session_reply_operation_id(&metadata.operation_id) {
                let established = context.with_direct(|direct| {
                    direct.decrypt_inbound_validated(
                        &binding,
                        &metadata,
                        &cipher,
                        &now,
                        |plaintext, _| match classify_session_control(plaintext)? {
                            Some(V2SessionControlKind::Established) => Ok(()),
                            _ => Err(crate::ImError::PermissionDenied),
                        },
                    )
                })?;
                let session_id = match established {
                    V2ValidatedInboundOutcome::Decrypted { session, .. }
                    | V2ValidatedInboundOutcome::Replay { session } => session.session_id,
                };
                context.with_direct(|direct| {
                    direct.complete_session_init_for_session(&binding, &session_id)
                })?;
                return Ok(V2InboundProductOutcome::ConsumedControl);
            }
            let decrypted = context.with_direct(|direct| {
                direct.decrypt_inbound_validated(
                    &binding,
                    &metadata,
                    &cipher,
                    &now,
                    |plaintext, _| {
                        validate_inbound_plaintext(plaintext, &metadata, expected_peer_did)
                    },
                )
            })?;
            let session_id = match &decrypted {
                V2ValidatedInboundOutcome::Decrypted { session, .. }
                | V2ValidatedInboundOutcome::Replay { session } => session.session_id.clone(),
            };
            context.with_direct(|direct| {
                direct.complete_session_reply_for_session(&binding, &session_id)
            })?;
            context.with_ledger(|ledger| {
                ledger.remove_session_reply_for_session(&binding, &session_id)
            })?;
            match decrypted {
                V2ValidatedInboundOutcome::Decrypted {
                    validated: ValidatedInboundPlaintext::Business(projection),
                    ..
                } => Ok(V2InboundProductOutcome::Business(projection)),
                V2ValidatedInboundOutcome::Decrypted {
                    validated: ValidatedInboundPlaintext::OwnSync(projection),
                    ..
                } => Ok(V2InboundProductOutcome::OwnSync(projection)),
                V2ValidatedInboundOutcome::Decrypted {
                    validated: ValidatedInboundPlaintext::SuppressedControl,
                    ..
                } => Ok(V2InboundProductOutcome::SuppressedControl),
                V2ValidatedInboundOutcome::Replay { .. } => Ok(V2InboundProductOutcome::Replay),
            }
        }
    }
}

async fn send_session_reply<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    binding: &V2SessionBinding,
    init_message_id: &str,
    expected_session_id: &str,
    now: &str,
) -> crate::ImResult<()>
where
    H: V2DirectProductHost,
{
    // Register the intent before preparing or posting the reply. A crash or
    // transport failure therefore leaves a discoverable, exact retry path.
    let record = context.with_ledger(|ledger| {
        ledger.register_session_reply(binding, init_message_id, expected_session_id, now)
    })?;
    if record.accepted {
        return Ok(());
    }
    deliver_session_reply(context, host, &record, now).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct V2SessionReplyRetrySummary {
    pub(crate) attempted: usize,
    pub(crate) accepted: usize,
    pub(crate) failed: usize,
    pub(crate) ineligible: usize,
}

pub(crate) async fn retry_session_replies_with_host<H>(
    context: &V2DirectProductContext,
    host: &mut H,
) -> crate::ImResult<V2SessionReplyRetrySummary>
where
    H: V2DirectProductHost,
{
    let records = context.with_ledger(|ledger| ledger.pending_session_replies())?;
    let mut summary = V2SessionReplyRetrySummary {
        attempted: records.len(),
        accepted: 0,
        failed: 0,
        ineligible: 0,
    };
    let mut documents: BTreeMap<String, Value> = BTreeMap::new();
    for record in records {
        if !documents.contains_key(&record.binding.peer_did) {
            let document = match host.resolve_did_document(&record.binding.peer_did).await {
                Ok(document) => document,
                Err(_) => {
                    summary.failed += 1;
                    continue;
                }
            };
            documents.insert(record.binding.peer_did.clone(), document);
        }
        let document = documents
            .get(&record.binding.peer_did)
            .ok_or(crate::ImError::PermissionDenied)?;
        let eligible = anp::authentication::find_eligible_device(
            document,
            &record.binding.peer_device_id,
            PROFILE_DIRECT_E2EE_V2,
        )
        .ok()
        .flatten()
        .is_some_and(|device| device.e2ee_key_id == record.binding.peer_e2ee_key_id);
        if !eligible {
            // A revoked/rotated device must never receive a delayed reply.
            context.with_direct(|direct| {
                direct.complete_session_reply_for_session(&record.binding, &record.session_id)
            })?;
            context.with_ledger(|ledger| {
                ledger.remove_session_reply_for_session(&record.binding, &record.session_id)
            })?;
            summary.ineligible += 1;
            continue;
        }
        match deliver_session_reply(context, host, &record, &now_text()).await {
            Ok(()) => summary.accepted += 1,
            Err(_) => summary.failed += 1,
        }
    }
    Ok(summary)
}

async fn deliver_session_reply<H>(
    context: &V2DirectProductContext,
    host: &mut H,
    record: &SessionReplyRecord,
    now: &str,
) -> crate::ImResult<()>
where
    H: V2DirectProductHost,
{
    let prepared = match context
        .with_direct(|direct| direct.resume_outbound(&record.binding, &record.operation_id))?
    {
        Some(prepared) => prepared,
        None => context.with_direct(|direct| {
            direct.prepare_outbound(
                &record.binding,
                &record.operation_id,
                &session_established_plaintext(&record.init_message_id)?,
                now,
            )
        })?,
    };
    if prepared.cipher_body()?.session_id != record.session_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let result = host.post_direct(&prepared).await?;
    validate_send_result(&result, &prepared)?;
    // The P5 store deliberately retains a session-reply Cipher after service
    // acceptance until the first authenticated business Cipher arrives.
    let _ = context.with_direct(|direct| direct.mark_outbound_accepted(&prepared))?;
    context.with_ledger(|ledger| {
        ledger.mark_session_reply_accepted(record, &result.accepted_at, &now_text())
    })?;
    Ok(())
}

fn validate_inbound_plaintext(
    plaintext: &V2ApplicationPlaintext,
    metadata: &V2DirectMetadata,
    expected_peer_did: Option<&str>,
) -> crate::ImResult<ValidatedInboundPlaintext> {
    plaintext
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if let Some(payload) = plaintext.payload.as_ref() {
        if payload.get("system_type").is_some() {
            return match payload.get("system_type").and_then(Value::as_str) {
                Some(DEVICE_SYNC_SYSTEM_TYPE) => {
                    validate_own_sync_plaintext(plaintext, payload, metadata, expected_peer_did)
                }
                // Every other top-level system_type, including malformed and
                // future AWiki controls, is hidden from the ordinary UI path.
                _ => {
                    validate_business_peer_scope(metadata, expected_peer_did)?;
                    Ok(ValidatedInboundPlaintext::SuppressedControl)
                }
            };
        }
    }
    validate_business_peer_scope(metadata, expected_peer_did)?;
    let logical_message_id = plaintext
        .logical_message_id
        .clone()
        .filter(|value| required("logical_message_id", value).is_ok())
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(ValidatedInboundPlaintext::Business(
        V2InboundBusinessProjection {
            logical_message_id,
            conversation_id: plaintext.conversation_id.clone(),
            sender_did: metadata.sender_did.clone(),
            sender_device_id: metadata.sender_device_id.clone(),
            recipient_did: metadata.target.did.clone(),
            wire_message_id: metadata.message_id.clone(),
            body: ordinary_business_body(plaintext)?,
            session_reply_pending: false,
        },
    ))
}

fn validate_own_sync_plaintext(
    outer: &V2ApplicationPlaintext,
    payload: &Value,
    metadata: &V2DirectMetadata,
    expected_peer_did: Option<&str>,
) -> crate::ImResult<ValidatedInboundPlaintext> {
    if outer.application_content_type != "application/json"
        || metadata.sender_did != metadata.target.did
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let sync: V2OwnSyncPayload =
        serde_json::from_value(payload.clone()).map_err(|_| crate::ImError::PermissionDenied)?;
    if sync.system_type != DEVICE_SYNC_SYSTEM_TYPE
        || sync.sync_type != "outbound-message"
        || sync.original_sender_did != metadata.sender_did
        || sync.original_sender_device_id != metadata.sender_device_id
        || required("target_did", &sync.target_did).is_err()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if expected_peer_did.is_some_and(|expected| expected != sync.target_did) {
        return Err(scoped_peer_mismatch());
    }
    sync.message
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if sync
        .message
        .payload
        .as_ref()
        .is_some_and(|inner| inner.get("system_type").is_some())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let logical_message_id = sync
        .message
        .logical_message_id
        .clone()
        .filter(|value| required("logical_message_id", value).is_ok())
        .ok_or(crate::ImError::PermissionDenied)?;
    if outer.logical_message_id.as_deref() != Some(logical_message_id.as_str())
        || outer.conversation_id != sync.message.conversation_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(ValidatedInboundPlaintext::OwnSync(
        V2InboundOwnSyncProjection {
            logical_message_id,
            conversation_id: sync.message.conversation_id.clone(),
            original_sender_did: sync.original_sender_did,
            original_sender_device_id: sync.original_sender_device_id,
            target_did: sync.target_did,
            wire_message_id: metadata.message_id.clone(),
            body: ordinary_business_body(&sync.message)?,
            session_reply_pending: false,
        },
    ))
}

fn validate_business_peer_scope(
    metadata: &V2DirectMetadata,
    expected_peer_did: Option<&str>,
) -> crate::ImResult<()> {
    if expected_peer_did.is_some_and(|expected| expected != metadata.sender_did) {
        return Err(scoped_peer_mismatch());
    }
    Ok(())
}

const SCOPED_PEER_MISMATCH_DETAIL: &str =
    "authenticated P5 peer does not match the requested Direct scope";

fn scoped_peer_mismatch() -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: SCOPED_PEER_MISMATCH_DETAIL.to_owned(),
    }
}

pub(crate) fn is_scoped_peer_mismatch(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::IdentityBindingConflict { detail }
            if detail == SCOPED_PEER_MISMATCH_DETAIL
    )
}

fn ordinary_business_body(
    plaintext: &V2ApplicationPlaintext,
) -> crate::ImResult<V2InboundBusinessBody> {
    match plaintext.application_content_type.as_str() {
        "text/plain" | "text/markdown" => Ok(V2InboundBusinessBody::Text {
            text: plaintext
                .text
                .clone()
                .ok_or(crate::ImError::PermissionDenied)?,
            markdown: plaintext.application_content_type == "text/markdown",
        }),
        "application/json" => Ok(V2InboundBusinessBody::Json {
            payload: plaintext
                .payload
                .clone()
                .filter(|payload| payload.get("system_type").is_none())
                .ok_or(crate::ImError::PermissionDenied)?,
        }),
        content_type
            if content_type == crate::attachments::manifest::attachment_manifest_content_type() =>
        {
            let full_manifest = plaintext
                .payload
                .clone()
                .ok_or(crate::ImError::PermissionDenied)?;
            validate_full_attachment_manifest(&full_manifest)?;
            Ok(V2InboundBusinessBody::Attachment { full_manifest })
        }
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn inbound_binding(
    context: &V2DirectProductContext,
    metadata: &V2DirectMetadata,
    sender: &DeviceManifestEntry,
) -> crate::ImResult<V2SessionBinding> {
    if sender.device_id != metadata.sender_device_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let binding = V2SessionBinding {
        local_did: context.local_did.clone(),
        local_device_id: context.local_device_id.clone(),
        peer_did: metadata.sender_did.clone(),
        peer_device_id: sender.device_id.clone(),
        suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
        local_e2ee_key_id: context.local_e2ee_key_id.clone(),
        peer_e2ee_key_id: sender.e2ee_key_id.clone(),
    };
    binding
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(binding)
}

pub(crate) fn parse_v2_wire_message(
    message: &Value,
) -> crate::ImResult<Option<(V2DirectMetadata, V2DirectBody)>> {
    let Some(meta) = message.get("meta") else {
        return Ok(None);
    };
    if meta.get("profile").and_then(Value::as_str) != Some(DIRECT_E2EE_PROFILE_V2) {
        return Ok(None);
    }
    let metadata: V2DirectMetadata =
        serde_json::from_value(meta.clone()).map_err(|_| crate::ImError::PermissionDenied)?;
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let body = message
        .get("body")
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let body = match metadata.content_type.as_str() {
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_INIT_V2 => {
            let body: anp::direct_e2ee::V2DirectInitBody =
                serde_json::from_value(body).map_err(|_| crate::ImError::PermissionDenied)?;
            body.validate()
                .map_err(|_| crate::ImError::PermissionDenied)?;
            V2DirectBody::Init(body)
        }
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2 => {
            let body: anp::direct_e2ee::V2DirectCipherBody =
                serde_json::from_value(body).map_err(|_| crate::ImError::PermissionDenied)?;
            body.validate()
                .map_err(|_| crate::ImError::PermissionDenied)?;
            V2DirectBody::Cipher(body)
        }
        _ => return Err(crate::ImError::PermissionDenied),
    };
    Ok(Some((metadata, body)))
}

fn is_reserved_control_payload(payload: &Value) -> bool {
    // `system_type` is a reserved top-level discriminator. Unknown and even
    // malformed values are control-plane data and must never reach chat UI.
    payload.get("system_type").is_some()
}

fn failure_code(error: &crate::ImError) -> &'static str {
    match error {
        crate::ImError::PermissionDenied => "permission_denied",
        crate::ImError::Service { .. } | crate::ImError::TransportUnavailable { .. } => "transport",
        crate::ImError::LocalStateUnavailable { .. } | crate::ImError::IdentityVault { .. } => {
            "local_state"
        }
        crate::ImError::UnsupportedCapability { .. } => "unsupported",
        _ => "delivery_failed",
    }
}

fn now_text() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn ensure_delivery_source_digest_column(connection: &Connection) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(direct_e2ee_v2_delivery_ledger)")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    drop(statement);
    if columns.iter().any(|column| column == "source_digest") {
        return Ok(());
    }
    // This table was introduced behind a disabled rollout gate. Old local
    // rows are intentionally left NULL and therefore fail closed on load;
    // only newly inserted intents receive a trusted digest.
    connection
        .execute(
            "ALTER TABLE direct_e2ee_v2_delivery_ledger ADD COLUMN source_digest TEXT",
            [],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

fn max_timestamp(current: Option<String>, candidate: Option<String>) -> Option<String> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
        (Some(current), None) => Some(current),
        (None, candidate) => candidate,
    }
}

fn required<'a>(field: &str, value: &'a str) -> crate::ImResult<&'a str> {
    if value.is_empty() || value.trim() != value {
        Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be a non-empty exact value"),
        ))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
#[path = "v2_product_tests.rs"]
pub(crate) mod v2_product_tests;
