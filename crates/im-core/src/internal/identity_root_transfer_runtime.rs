//! Production sender orchestration for AWiki-local root-key transfer.
//!
//! The public boundary exposes only an abstract delivery result. Root
//! plaintext and private transport metadata stay below this module and are
//! carried only inside an established exact-device P5 v2 session.

use anp::direct_e2ee::{
    V2DirectBody, V2DirectMetadata, V2SecretJsonPayload, V2SessionBinding, DIRECT_E2EE_PROFILE_V2,
    MTI_DIRECT_E2EE_SUITE_V2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::internal::identity_device_join_runtime::{
    DeviceJoinAdminHttpAdapter, DeviceJoinAdminRemote, DeviceJoinRemoteDeviceSummary,
    DeviceJoinRemoteRegistry,
};
use crate::internal::identity_root_transfer::{
    validate_current_transfer_route, RootControlDirectBinding, RootImportTransportContext,
    RootKeyAckResumeInput, RootKeyEnvelopeImportInput, RootKeyEnvelopePrepareInput,
    RootKeyImportedCompletion, RootKeyTransferCore, ROOT_KEY_CONTROL_DELIVERY_CLASS,
};
use crate::internal::secure_direct::v2_runtime::{
    classify_session_control, is_session_init_operation_id, is_session_reply_operation_id,
    parse_send_result, session_establish_plaintext, session_established_plaintext,
    session_init_operation_id, session_reply_operation_id, PreparedV2Outbound,
    V2EstablishedDirectRuntime, V2RootControlSessionReadiness, V2SessionControlKind,
    V2ValidatedInboundOutcome, V2ValidatedSecretInboundOutcome, SESSION_ESTABLISHMENT_PENDING,
};
use crate::internal::secure_direct::v2_store::{
    SqliteV2DirectStateStore, V2OwnerScope, V2PrivateOutboundSidecar, V2PrivateOutboundStatus,
};
use crate::internal::transport::AsyncAuthenticatedRestTransport;
use crate::internal::transport::AsyncRpcTransport;

pub(crate) const ROOT_CONTROL_ENDPOINT: &str = "/im/private/root-control";
const ROOT_CONTROL_TTL_SECONDS: i64 = 300;
const USER_PRESENCE_TTL_SECONDS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootKeyTransferDelivery {
    pub(crate) did: String,
    pub(crate) sender_device_id: String,
    pub(crate) recipient_device_id: String,
    pub(crate) message_id: String,
    pub(crate) accepted_at: String,
}

pub(crate) fn list_root_key_transfers(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<Vec<V2PrivateOutboundStatus>> {
    if !core.inner().root_key_transfer_enabled() {
        return Err(crate::ImError::unsupported(
            "awiki-root-key-transfer-disabled",
        ));
    }
    let entry = local_device_entry(core, client)?;
    let state = entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let scope =
        V2OwnerScope::from_identity_state(&client.current_identity().id, client.did(), state)?;
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;
    with_v2_runtime(core, &scope, |direct| {
        direct.list_private_outbound_statuses(&now)
    })
}

pub(crate) async fn retry_root_key_transfer(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
    user_presence_confirmed: bool,
) -> crate::ImResult<V2PrivateOutboundStatus> {
    if !user_presence_confirmed {
        return Err(crate::ImError::PermissionDenied);
    }
    let before = unique_root_transfer_status(core, client, message_id)?;
    if !before.retryable {
        return Err(crate::ImError::PermissionDenied);
    }
    let local_entry = local_device_entry(core, client)?;
    let mut local_state = local_entry
        .device_state
        .as_ref()
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut authorization = local_state
        .authorization
        .as_ref()
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let local_device_id = authorization.protocol_device_id.as_str().to_owned();
    if local_device_id == before.sender_device_id {
        send_root_key(core, client, &before.recipient_device_id, message_id, true).await?;
    } else if local_device_id == before.recipient_device_id {
        repair_committed_root_import_root_ref(core, client, message_id)?;
        let registry = if authorization.management_ready {
            repair_committed_root_import_auth_ref(
                core,
                client,
                message_id,
                authorization.auth_generation,
            )?;
            let mut remote = DeviceJoinAdminHttpAdapter::production(client);
            remote.registry(client.did(), false).await?
        } else {
            match probe_root_ready_retry(client).await? {
                RootReadyRetryProbe::ContinueAck(registry) => registry,
                RootReadyRetryProbe::ServerAlreadyCommitted => {
                    // The server can commit root import and rotate auth_generation
                    // even when the ready response or subsequent token response is
                    // lost. Re-enter the persisted completion with the sole legal
                    // next generation; token issuance itself proves the server is
                    // already management-ready before local convergence proceeds.
                    let checkpoint = local_state
                        .checkpoint
                        .as_ref()
                        .ok_or(crate::ImError::PermissionDenied)?;
                    let inferred_ready = RootManagementReadyResult {
                        did: client.did().as_str().to_owned(),
                        device_id: local_device_id.clone(),
                        management_ready: true,
                        auth_generation: authorization
                            .auth_generation
                            .checked_add(1)
                            .ok_or(crate::ImError::PermissionDenied)?,
                        registry_version: checkpoint
                            .registry_version
                            .checked_add(1)
                            .ok_or(crate::ImError::PermissionDenied)?,
                        completed_message_id: message_id.to_owned(),
                    };
                    let completion = local_entry
                        .root_key_import
                        .as_ref()
                        .filter(|record| record.message_id() == message_id)
                        .map(|record| record.completion.clone())
                        .ok_or(crate::ImError::PermissionDenied)?;
                    complete_local_management_ready(core, client, &completion, &inferred_ready)
                        .await?;
                    let refreshed = local_device_entry(core, client)?;
                    local_state = refreshed
                        .device_state
                        .ok_or(crate::ImError::PermissionDenied)?;
                    authorization = local_state
                        .authorization
                        .clone()
                        .filter(|current| {
                            current.protocol_device_id.as_str() == local_device_id
                                && current.management_ready
                                && current.auth_generation == inferred_ready.auth_generation
                        })
                        .ok_or(crate::ImError::PermissionDenied)?;
                    let mut retry_remote = DeviceJoinAdminHttpAdapter::production(client);
                    retry_remote.registry(client.did(), false).await?
                }
            }
        };
        let local = registry_device(&registry.devices, &local_device_id)?;
        let peer = registry_device(&registry.devices, &before.sender_device_id)?;
        let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
        let did_document = crate::internal::discovery::did_document::resolve_did_document_async(
            &mut resolver,
            client.did().as_str(),
        )
        .await?;
        validate_retry_ack_route(&authorization, local, peer, &did_document)?;
        let binding = same_did_binding(client.did().as_str(), local, peer);
        let scope = V2OwnerScope::from_identity_state(
            &client.current_identity().id,
            client.did(),
            &local_state,
        )?;
        let local_alias = client
            .current_identity()
            .local_alias
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let ack = RootKeyTransferCore::from_core_for_rollout(core, true)?.resume_imported_ack(
            RootKeyAckResumeInput {
                local_alias,
                message_id,
                current_did_document: &did_document,
                current_registry: &registry,
            },
        )?;
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(time_error)?;
        send_imported_ack(core, client, &scope, &binding, ack, &now).await?;
    } else {
        return Err(crate::ImError::PermissionDenied);
    }
    unique_root_transfer_status(core, client, message_id)
}

fn unique_root_transfer_status(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
) -> crate::ImResult<V2PrivateOutboundStatus> {
    let mut matches = list_root_key_transfers(core, client)?
        .into_iter()
        .filter(|status| status.operation_id == message_id);
    let status = matches.next().ok_or(crate::ImError::PermissionDenied)?;
    if matches.next().is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(status)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootControlReceiveOutcome {
    EnvelopeImported,
    EnvelopeReplayAcknowledged,
    ImportedAckConsumed,
}

/// Handles only the two fixed, non-secret P5 v2 session-control messages used
/// to bootstrap the later private root-control Cipher. Ordinary Direct JSON
/// and text are deliberately left to the normal message runtime.
pub(crate) async fn receive_session_control(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    metadata: V2DirectMetadata,
    body: V2DirectBody,
) -> crate::ImResult<bool> {
    if !core.inner().root_key_transfer_enabled() || metadata.profile != DIRECT_E2EE_PROFILE_V2 {
        return Ok(false);
    }
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let recognized = match &body {
        V2DirectBody::Init(_) => is_session_init_operation_id(&metadata.operation_id),
        V2DirectBody::Cipher(_) => is_session_reply_operation_id(&metadata.operation_id),
    };
    if !recognized {
        return Ok(false);
    }
    if metadata.sender_did != client.did().as_str()
        || metadata.target.did != client.did().as_str()
        || metadata.sender_device_id == metadata.recipient_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let local_entry = local_device_entry(core, client)?;
    let local_state = local_entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let authorization = local_state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if authorization.protocol_device_id.as_str() != metadata.recipient_device_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut remote = DeviceJoinAdminHttpAdapter::production(client);
    let registry = remote.registry(client.did(), false).await?;
    let local = registry_device(&registry.devices, &metadata.recipient_device_id)?;
    let peer = registry_device(&registry.devices, &metadata.sender_device_id)?;
    let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
    let did_document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await?;
    validate_session_control_registry_route(
        matches!(&body, V2DirectBody::Init(_)),
        authorization,
        local,
        peer,
        &did_document,
    )?;
    let binding = same_did_binding(client.did().as_str(), local, peer);
    let scope = V2OwnerScope::from_identity_state(
        &client.current_identity().id,
        client.did(),
        local_state,
    )?;
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;

    match body {
        V2DirectBody::Init(init) => {
            let local_static =
                crate::internal::secure_direct::v2_prekey_runtime::local_static_private(client)?;
            let peer_static =
                crate::internal::secure_direct::v2_prekey_runtime::static_public_from_document(
                    &did_document,
                    &peer.e2ee_key_id,
                )?;
            let accepted = with_v2_runtime(core, &scope, |direct| {
                direct.decrypt_inbound_init_validated(
                    &binding,
                    &metadata,
                    &init,
                    &local_static,
                    &peer_static,
                    &now,
                    |plaintext, _| match classify_session_control(plaintext)? {
                        Some(V2SessionControlKind::Establish) => Ok(()),
                        _ => Err(crate::ImError::PermissionDenied),
                    },
                )
            })?;
            let session_id = match accepted {
                V2ValidatedInboundOutcome::Decrypted { session, .. }
                | V2ValidatedInboundOutcome::Replay { session } => session.session_id,
            };
            let reply_operation_id = session_reply_operation_id(&metadata.message_id)?;
            let reply = with_v2_runtime(core, &scope, |direct| {
                let prepared = direct.prepare_outbound(
                    &binding,
                    &reply_operation_id,
                    &session_established_plaintext(&metadata.message_id)?,
                    &now,
                )?;
                if prepared.cipher_body()?.session_id != session_id {
                    return Err(crate::ImError::PermissionDenied);
                }
                Ok(prepared)
            })?;
            crate::internal::secure_direct::v2_prekey_runtime::post_standard_direct(client, &reply)
                .await?;
            if !with_v2_runtime(core, &scope, |direct| direct.mark_outbound_accepted(&reply))? {
                return Err(crate::ImError::PermissionDenied);
            }
            Ok(true)
        }
        V2DirectBody::Cipher(cipher) => {
            let established = with_v2_runtime(core, &scope, |direct| {
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
            with_v2_runtime(core, &scope, |direct| {
                direct.complete_session_init_for_session(&binding, &session_id)
            })?;
            Ok(true)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct RootPrivateTransportSidecar {
    transport_context: RootImportTransportContext,
    completion: Option<RootKeyImportedCompletion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootManagementReadyResult {
    did: String,
    device_id: String,
    management_ready: bool,
    auth_generation: u64,
    registry_version: u64,
    completed_message_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootControlRoute {
    Envelope,
    Ack,
}

enum ValidatedRootControl {
    Envelope(crate::internal::identity_root_transfer::ImportedRootKeyAck),
    Ack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalManagementReadyTransition {
    Fresh,
    AlreadyConverged,
}

enum RootReadyRetryProbe {
    ContinueAck(DeviceJoinRemoteRegistry),
    ServerAlreadyCommitted,
}

pub(crate) async fn send_root_key(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    recipient_device_id: &str,
    message_id: &str,
    user_presence_confirmed: bool,
) -> crate::ImResult<RootKeyTransferDelivery> {
    if !core.inner().root_key_transfer_enabled() {
        return Err(crate::ImError::unsupported(
            "awiki-root-key-transfer-disabled",
        ));
    }
    if !user_presence_confirmed {
        return Err(crate::ImError::PermissionDenied);
    }
    let user_presence_at = OffsetDateTime::now_utc();
    crate::ids::ProtocolDeviceId::parse(recipient_device_id)?;
    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;

    let mut registry_remote = DeviceJoinAdminHttpAdapter::production(client);
    let registry = registry_remote.registry(client.did(), false).await?;
    let local_entry = local_device_entry(core, client)?;
    let local_state = local_entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
    let did_document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await?;
    let (sender, recipient) = validate_current_transfer_route(
        &local_entry,
        client.did(),
        &did_document,
        &registry,
        recipient_device_id,
    )?;
    let binding = same_did_binding(client.did().as_str(), sender, recipient);
    binding
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;

    let scope = V2OwnerScope::from_identity_state(
        &client.current_identity().id,
        client.did(),
        local_state,
    )?;

    let status_now = user_presence_at.format(&Rfc3339).map_err(time_error)?;
    let existing_status = with_v2_runtime(core, &scope, |direct| {
        direct.private_outbound_status(message_id, &status_now)
    })?;
    if existing_status
        .as_ref()
        .is_some_and(|status| !status.retryable)
    {
        // Completed and expired operations retain only a secret-free terminal
        // record. Their operation id can never derive a second ciphertext.
        return Err(crate::ImError::PermissionDenied);
    }
    ensure_root_control_session(
        core,
        client,
        &scope,
        &binding,
        recipient,
        &did_document,
        message_id,
    )
    .await?;
    let retry = with_v2_runtime(core, &scope, |direct| {
        direct.resume_private_outbound(&binding, message_id)
    })?;
    let prepared = if let Some(retry) = retry {
        if existing_status.is_none() {
            return Err(crate::ImError::PermissionDenied);
        }
        retry
    } else {
        if existing_status.is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        let now = OffsetDateTime::now_utc();
        let envelope = RootKeyTransferCore::from_core_for_rollout(core, true)?.prepare_envelope(
            RootKeyEnvelopePrepareInput {
                local_alias,
                did_document: &did_document,
                registry: &registry,
                recipient_device_id,
                message_id,
                user_presence_at,
                now,
                expires_at: now + Duration::seconds(ROOT_CONTROL_TTL_SECONDS),
            },
        )?;
        let plaintext = V2SecretJsonPayload::from_canonical_json_object(
            envelope.plaintext().expose_secret().to_vec(),
        )
        .map_err(|_| crate::ImError::PermissionDenied)?;
        let sidecar = private_sidecar(
            message_id,
            RootPrivateTransportSidecar {
                transport_context: envelope.transport_context().clone(),
                completion: None,
            },
        )?;
        let now_text = now.format(&Rfc3339).map_err(time_error)?;
        with_v2_runtime(core, &scope, |direct| {
            direct.prepare_private_outbound_secret_json(
                &binding, message_id, &plaintext, &now_text, sidecar,
            )
        })?
    };

    if OffsetDateTime::now_utc() - user_presence_at > Duration::seconds(USER_PRESENCE_TTL_SECONDS) {
        return Err(crate::ImError::PermissionDenied);
    }
    let response = match post_private_control(client, &prepared).await {
        Ok(response) => response,
        Err(error) => {
            mark_private_failure(core, &scope, &prepared)?;
            return Err(error);
        }
    };
    let accepted = match parse_send_result(&response, &prepared) {
        Ok(accepted) => accepted,
        Err(error) => {
            mark_private_failure(core, &scope, &prepared)?;
            return Err(error);
        }
    };
    if !with_v2_runtime(core, &scope, |direct| {
        direct.mark_outbound_accepted(&prepared)
    })? {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(RootKeyTransferDelivery {
        did: client.did().as_str().to_owned(),
        sender_device_id: prepared.metadata.sender_device_id,
        recipient_device_id: accepted.recipient_device_id,
        message_id: accepted.message_id,
        accepted_at: accepted.accepted_at,
    })
}

#[allow(clippy::too_many_arguments)]
async fn ensure_root_control_session(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    scope: &V2OwnerScope,
    binding: &V2SessionBinding,
    recipient: &DeviceJoinRemoteDeviceSummary,
    recipient_did_document: &Value,
    root_message_id: &str,
) -> crate::ImResult<()> {
    let readiness = with_v2_runtime(core, scope, |direct| {
        direct.root_control_session_readiness(binding)
    })?;
    let prepared = match readiness {
        V2RootControlSessionReadiness::Ready => return Ok(()),
        V2RootControlSessionReadiness::Pending(prepared) => prepared,
        V2RootControlSessionReadiness::Absent => {
            crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published(
                core, client,
            )
            .await?;
            let operation_id = session_init_operation_id(root_message_id)?;
            let fetched = crate::internal::secure_direct::v2_prekey_runtime::fetch_verified_prekey(
                client,
                client.did().as_str(),
                &recipient.device_id,
                recipient_did_document,
                root_message_id,
            )
            .await?;
            let local_static =
                crate::internal::secure_direct::v2_prekey_runtime::local_static_private(client)?;
            let recipient_static =
                crate::internal::secure_direct::v2_prekey_runtime::static_public_from_document(
                    recipient_did_document,
                    &recipient.e2ee_key_id,
                )?;
            let now = OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(time_error)?;
            with_v2_runtime(core, scope, |direct| {
                direct.prepare_session_init(
                    binding,
                    &operation_id,
                    &session_establish_plaintext(),
                    &local_static,
                    &fetched,
                    &recipient_static,
                    &now,
                )
            })?
        }
    };
    prepared.init_body()?;
    crate::internal::secure_direct::v2_prekey_runtime::post_standard_direct(client, &prepared)
        .await?;
    if !with_v2_runtime(core, scope, |direct| {
        direct.mark_outbound_accepted(&prepared)
    })? {
        return Err(crate::ImError::PermissionDenied);
    }
    Err(crate::ImError::unsupported(SESSION_ESTABLISHMENT_PENDING))
}

/// Consumes one message returned by the AWiki-private root-control inbox
/// projection. `metadata` and `body` are unmodified standard P5 v2 wire
/// objects; `transport_context` is a separate same-domain sidecar and never
/// participates in ANP AAD reconstruction.
pub(crate) async fn receive_root_control(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    metadata: anp::direct_e2ee::V2DirectMetadata,
    body: anp::direct_e2ee::V2DirectCipherBody,
    transport_context: RootImportTransportContext,
) -> crate::ImResult<RootControlReceiveOutcome> {
    if !core.inner().root_key_transfer_enabled() {
        return Err(crate::ImError::unsupported(
            "awiki-root-key-transfer-disabled",
        ));
    }
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    body.validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    transport_context.validate()?;
    if metadata.sender_did != client.did().as_str()
        || metadata.target.did != client.did().as_str()
        || metadata.recipient_device_id == metadata.sender_device_id
        || transport_context.message_id != metadata.message_id
        || transport_context.delivery_class != ROOT_KEY_CONTROL_DELIVERY_CLASS
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let local_entry = local_device_entry(core, client)?;
    let local_state = local_entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let authorization = local_state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if authorization.protocol_device_id.as_str() != metadata.recipient_device_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut registry_remote = DeviceJoinAdminHttpAdapter::production(client);
    let registry = registry_remote.registry(client.did(), false).await?;
    let local = registry_device(&registry.devices, &metadata.recipient_device_id)?;
    let peer = registry_device(&registry.devices, &metadata.sender_device_id)?;
    let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
    let did_document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await?;
    let route = root_control_route(&metadata, &transport_context)?;
    validate_root_control_registry_route(route, authorization, local, peer, &did_document)?;
    let binding = same_did_binding(client.did().as_str(), local, peer);

    let scope = V2OwnerScope::from_identity_state(
        &client.current_identity().id,
        client.did(),
        local_state,
    )?;
    let now = OffsetDateTime::now_utc();
    let now_text = now.format(&Rfc3339).map_err(time_error)?;
    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let root_core = RootKeyTransferCore::from_core_for_rollout(core, true)?;
    let decrypted = with_v2_runtime(core, &scope, |direct| {
        direct.decrypt_inbound_secret_json_validated(
            &binding,
            &metadata,
            &body,
            &now_text,
            |plaintext, session| {
                let plaintext = crate::internal::platform_secret::SecretBytes::from_vec(
                    plaintext.expose_secret().to_vec(),
                );
                match route {
                    RootControlRoute::Envelope => {
                        let direct_binding = RootControlDirectBinding::from_decrypted_session(
                            metadata.message_id.clone(),
                            session,
                        )?;
                        root_core
                            .import_envelope(
                                &plaintext,
                                RootKeyEnvelopeImportInput {
                                    local_alias,
                                    direct_binding: &direct_binding,
                                    transport_context: &transport_context,
                                    current_did_document: &did_document,
                                    current_registry: &registry,
                                    now,
                                },
                            )
                            .map(ValidatedRootControl::Envelope)
                    }
                    RootControlRoute::Ack => root_core
                        .validate_imported_ack_plaintext(
                            &plaintext,
                            &transport_context,
                            &did_document,
                            &registry,
                            now,
                        )
                        .map(|_| ValidatedRootControl::Ack),
                }
            },
        )
    })?;
    match decrypted {
        V2ValidatedSecretInboundOutcome::Decrypted {
            validated: ValidatedRootControl::Envelope(ack),
            session,
        } => {
            repair_committed_root_import_root_ref(
                core,
                client,
                &ack.completion().ack_for_message_id,
            )?;
            with_v2_runtime(core, &scope, |direct| {
                direct.complete_session_reply_for_session(&binding, &session.session_id)
            })?;
            let replayed_import = ack.replayed();
            send_imported_ack(core, client, &scope, &binding, ack, &now_text).await?;
            Ok(if replayed_import {
                RootControlReceiveOutcome::EnvelopeReplayAcknowledged
            } else {
                RootControlReceiveOutcome::EnvelopeImported
            })
        }
        V2ValidatedSecretInboundOutcome::Decrypted {
            validated: ValidatedRootControl::Ack,
            ..
        } => {
            with_v2_runtime(core, &scope, |direct| {
                direct.mark_private_outbound_completed(&binding, &metadata.message_id)
            })?;
            Ok(RootControlReceiveOutcome::ImportedAckConsumed)
        }
        V2ValidatedSecretInboundOutcome::Replay { session } => match route {
            RootControlRoute::Ack => {
                with_v2_runtime(core, &scope, |direct| {
                    direct.mark_private_outbound_completed(&binding, &metadata.message_id)
                })?;
                Ok(RootControlReceiveOutcome::ImportedAckConsumed)
            }
            RootControlRoute::Envelope => {
                with_v2_runtime(core, &scope, |direct| {
                    direct.complete_session_reply_for_session(&binding, &session.session_id)
                })?;
                let ack = root_core.resume_imported_ack(RootKeyAckResumeInput {
                    local_alias,
                    message_id: &metadata.message_id,
                    current_did_document: &did_document,
                    current_registry: &registry,
                })?;
                repair_committed_root_import_root_ref(
                    core,
                    client,
                    &ack.completion().ack_for_message_id,
                )?;
                send_imported_ack(core, client, &scope, &binding, ack, &now_text).await?;
                Ok(RootControlReceiveOutcome::EnvelopeReplayAcknowledged)
            }
        },
    }
}

async fn send_imported_ack(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    scope: &V2OwnerScope,
    binding: &V2SessionBinding,
    ack: crate::internal::identity_root_transfer::ImportedRootKeyAck,
    now: &str,
) -> crate::ImResult<()> {
    let plaintext =
        V2SecretJsonPayload::from_canonical_json_object(ack.plaintext().expose_secret().to_vec())
            .map_err(|_| crate::ImError::PermissionDenied)?;
    let sidecar = private_sidecar(
        &ack.transport_context().message_id,
        RootPrivateTransportSidecar {
            transport_context: ack.transport_context().clone(),
            completion: Some(ack.completion().clone()),
        },
    )?;
    let prepared = with_v2_runtime(core, scope, |direct| {
        direct.prepare_private_outbound_secret_json(
            binding,
            &ack.transport_context().message_id,
            &plaintext,
            now,
            sidecar,
        )
    })?;
    let response = match post_private_control(client, &prepared).await {
        Ok(response) => response,
        Err(error) => {
            mark_private_failure(core, scope, &prepared)?;
            return Err(error);
        }
    };
    let ready: RootManagementReadyResult = match serde_json::from_value(response) {
        Ok(ready) => ready,
        Err(error) => {
            mark_private_failure(core, scope, &prepared)?;
            return Err(serialization_error(error));
        }
    };
    if let Err(error) =
        validate_management_ready_response(client.did().as_str(), ack.completion(), &ready)
    {
        mark_private_failure(core, scope, &prepared)?;
        return Err(error);
    }
    if let Err(error) =
        complete_local_management_ready(core, client, ack.completion(), &ready).await
    {
        mark_private_failure(core, scope, &prepared)?;
        return Err(error);
    }
    if !with_v2_runtime(core, scope, |direct| {
        direct.mark_outbound_accepted(&prepared)
    })? {
        return Err(crate::ImError::PermissionDenied);
    }
    with_v2_runtime(core, scope, |direct| {
        direct.mark_private_outbound_completed(binding, &prepared.metadata.operation_id)
    })?;
    Ok(())
}

fn validate_management_ready_response(
    expected_did: &str,
    completion: &RootKeyImportedCompletion,
    ready: &RootManagementReadyResult,
) -> crate::ImResult<()> {
    if ready.did != expected_did
        || ready.device_id != completion.importing_device_id
        || ready.completed_message_id != completion.ack_for_message_id
        || !ready.management_ready
        || ready.auth_generation == 0
        || ready.registry_version == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

async fn complete_local_management_ready(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    completion: &RootKeyImportedCompletion,
    ready: &RootManagementReadyResult,
) -> crate::ImResult<()> {
    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let local_entry = local_device_entry(core, client)?;
    let local_state = local_entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let authorization = local_state
        .authorization
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let persisted_import = local_entry
        .root_key_import
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if persisted_import.message_id() != completion.ack_for_message_id
        || persisted_import.completion != *completion
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let transition = validate_local_management_ready_transition(
        client.did().as_str(),
        authorization,
        local_state.checkpoint.as_ref(),
        completion,
        ready,
    )?;
    if transition == LocalManagementReadyTransition::AlreadyConverged {
        repair_committed_root_import_auth_ref(
            core,
            client,
            &ready.completed_message_id,
            ready.auth_generation,
        )?;
    }

    // Always re-resolve the public document. An exact retry may be repairing a
    // document projection that failed after auth/checkpoint commit, so the
    // AlreadyConverged path must not return before validating current state.
    let mut resolver = crate::internal::transport::CorePlainTransport::new(core);
    let did_document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await?;
    let manifest = anp::authentication::find_eligible_device(
        &did_document,
        &ready.device_id,
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.signing_key_id != authorization.signing_key_id
        || manifest.e2ee_key_id != authorization.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity_store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let (mut registry_remote, fresh_token) = match transition {
        LocalManagementReadyTransition::Fresh => {
            // Root-import completion advances auth_generation before this
            // process can persist its replacement token. Neither the old
            // bearer nor signature fallback is valid, so issue once and use
            // the uncommitted access token for the verification read.
            let signing_private = anp::PrivateKeyMaterial::from_pem(
                &client
                    .runtime()
                    .key_provider
                    .device_request_signing_private_pem()?,
            )
            .map_err(|_| crate::ImError::PermissionDenied)?;
            let service_domain =
                crate::internal::identity_join_activation_pending::service_domain_from_did(
                    client.did(),
                )?;
            let token = issue_root_import_management_token(
                core,
                &identity_store,
                local_alias,
                ready,
                &did_document,
                &authorization.signing_key_id,
                &signing_private,
                &service_domain,
            )
            .await?;
            let transport =
                crate::internal::transport::CoreHttpTransport::new_with_ephemeral_bearer(
                    client,
                    &token.access_token,
                )?;
            (DeviceJoinAdminHttpAdapter::new(transport), Some(token))
        }
        LocalManagementReadyTransition::AlreadyConverged => {
            // Exact projection repair must not issue another token or mutate
            // the committed auth ref/generation. Use the current committed
            // access token through the no-persist ephemeral transport.
            let bearer = client
                .runtime()
                .key_provider
                .auth_state()?
                .bearer_token
                .map(|token| token.trim().to_owned())
                .filter(|token| !token.is_empty())
                .ok_or(crate::ImError::AuthRequired)?;
            let transport =
                crate::internal::transport::CoreHttpTransport::new_with_ephemeral_bearer(
                    client, &bearer,
                )?;
            (DeviceJoinAdminHttpAdapter::new(transport), None)
        }
    };
    let registry = registry_remote.registry(client.did(), false).await?;
    validate_management_ready_registry_checkpoint(
        completion,
        ready,
        &registry.checkpoint,
        &crate::internal::identity_wire::device_genesis::document_hash(&did_document)?,
    )?;
    let local = registry_device(&registry.devices, &ready.device_id)?;
    if local.status != crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
        || local.role != crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
        || !local.management_ready
        || local.auth_generation != ready.auth_generation
        || local.signing_key_id != authorization.signing_key_id
        || local.e2ee_key_id != authorization.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let committed_auth_ref = if let Some(token) = fresh_token {
        let secret_storage =
            crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
        Some(identity_store.converge_root_import_management_ready(
            local_alias,
            &ready.completed_message_id,
            ready.auth_generation,
            &registry.checkpoint,
            &token.access_token,
            &token.refresh_token,
            &token.expires_at,
            &secret_storage,
        )?)
    } else {
        None
    };
    identity_store.commit_root_import_management_document(
        local_alias,
        &ready.completed_message_id,
        ready.auth_generation,
        &registry.checkpoint,
        &did_document,
    )?;
    if let Some(committed_auth_ref) = committed_auth_ref {
        client
            .runtime()
            .key_provider
            .advance_vault_auth_ref(&committed_auth_ref)?;
    }
    Ok(())
}

fn repair_committed_root_import_auth_ref(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    completed_message_id: &str,
    auth_generation: u64,
) -> crate::ImResult<()> {
    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let identity_store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let committed_auth_ref = identity_store.committed_root_import_management_auth_ref(
        local_alias,
        completed_message_id,
        auth_generation,
        &secret_storage,
    )?;
    client
        .runtime()
        .key_provider
        .advance_vault_auth_ref(&committed_auth_ref)?;
    Ok(())
}

pub(crate) fn repair_committed_root_import_root_ref(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    completed_message_id: &str,
) -> crate::ImResult<()> {
    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let identity_store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let committed_root_ref = identity_store.committed_root_import_root_ref(
        local_alias,
        completed_message_id,
        &secret_storage,
    )?;
    client
        .runtime()
        .key_provider
        .advance_vault_root_ref(&committed_root_ref)
}

/// Probes the pre-import authorization generation without allowing the
/// transport to refresh, fall back to a device signature, or persist a token
/// returned by the server. Only the closed JSON-RPC service code emitted when
/// that generation is no longer authoritative proves that the ready commit
/// already happened; HTTP 401 and every other service error remain errors.
async fn probe_root_ready_retry(
    client: &crate::core::ImClient,
) -> crate::ImResult<RootReadyRetryProbe> {
    let old_bearer = client
        .runtime()
        .key_provider
        .auth_state()?
        .bearer_token
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
        .ok_or(crate::ImError::AuthRequired)?;
    let transport = crate::internal::transport::CoreHttpTransport::new_with_ephemeral_bearer(
        client,
        &old_bearer,
    )?;
    let mut remote = DeviceJoinAdminHttpAdapter::new(transport);
    classify_root_ready_retry_probe(remote.registry(client.did(), false).await)
}

fn classify_root_ready_retry_probe(
    result: crate::ImResult<DeviceJoinRemoteRegistry>,
) -> crate::ImResult<RootReadyRetryProbe> {
    match result {
        Ok(registry) => Ok(RootReadyRetryProbe::ContinueAck(registry)),
        Err(error) if is_stale_auth_generation_service(&error) => {
            Ok(RootReadyRetryProbe::ServerAlreadyCommitted)
        }
        Err(error) => Err(error),
    }
}

fn is_stale_auth_generation_service(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if code == "device.auth_generation_stale"
    )
}

#[allow(clippy::too_many_arguments)]
async fn issue_root_import_management_token(
    core: &crate::core::ImCore,
    store: &crate::internal::identity_store::IdentityStore<'_>,
    local_alias: &str,
    ready: &RootManagementReadyResult,
    did_document: &Value,
    signing_key_id: &str,
    signing_private: &anp::PrivateKeyMaterial,
    service_domain: &str,
) -> crate::ImResult<crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult> {
    for attempt in 0..2 {
        let operation_id = store.reserve_root_import_management_token_operation(
            local_alias,
            &ready.completed_message_id,
            random_root_import_token_operation_id,
        )?;
        let prepared = crate::internal::identity_wire::device_genesis::prepare_management_ready_device_token_issue(
            operation_id.clone(),
            did_document,
            &ready.device_id,
            signing_key_id,
            signing_private,
            service_domain,
        )?;
        let call = crate::internal::identity_wire::device_genesis::build_device_token_issue_call(
            &prepared,
        )?;
        let raw = crate::internal::transport::CorePlainTransport::new(core)
            .rpc(call.endpoint, call.method, call.params)
            .await?;
        match crate::internal::identity_wire::device_genesis::parse_device_token_issue_result(
            raw,
            &prepared,
            ready.auth_generation,
            OffsetDateTime::now_utc(),
        ) {
            Ok(token) => return Ok(token),
            Err(crate::ImError::SessionExpired) if attempt == 0 => {
                store.rotate_root_import_management_token_operation(
                    local_alias,
                    &ready.completed_message_id,
                    &operation_id,
                    &random_root_import_token_operation_id()?,
                )?;
            }
            Err(error) => return Err(error),
        }
    }
    Err(crate::ImError::SessionExpired)
}

fn random_root_import_token_operation_id() -> crate::ImResult<String> {
    let mut bytes = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(format!("root-ready-{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn validate_management_ready_registry_checkpoint(
    completion: &RootKeyImportedCompletion,
    ready: &RootManagementReadyResult,
    checkpoint: &crate::internal::identity_device_state::IdentityInternalCheckpoint,
    resolved_document_hash: &str,
) -> crate::ImResult<()> {
    if checkpoint.registry_version < ready.registry_version
        || checkpoint.document_version < completion.document_version
        || (checkpoint.document_version == completion.document_version
            && checkpoint.document_hash != completion.document_hash)
        || checkpoint.document_hash != resolved_document_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_local_management_ready_transition(
    expected_did: &str,
    authorization: &crate::internal::identity_device_state::DeviceAuthorizationProjection,
    checkpoint: Option<&crate::internal::identity_device_state::IdentityInternalCheckpoint>,
    completion: &RootKeyImportedCompletion,
    ready: &RootManagementReadyResult,
) -> crate::ImResult<LocalManagementReadyTransition> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };
    let checkpoint = checkpoint.ok_or(crate::ImError::PermissionDenied)?;
    if completion.did != expected_did
        || completion.importing_device_id != authorization.protocol_device_id.as_str()
        || completion.importing_device_id != ready.device_id
        || completion.ack_for_message_id != ready.completed_message_id
        || authorization.status != DeviceAuthorizationStatus::Active
        || authorization.role != DeviceAuthorizationRole::Admin
        || checkpoint.document_version < completion.document_version
        || (checkpoint.document_version == completion.document_version
            && checkpoint.document_hash != completion.document_hash)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if authorization.management_ready {
        if authorization.auth_generation != ready.auth_generation
            || checkpoint.registry_version < ready.registry_version
        {
            return Err(crate::ImError::PermissionDenied);
        }
        return Ok(LocalManagementReadyTransition::AlreadyConverged);
    }
    let expected_auth_generation = authorization
        .auth_generation
        .checked_add(1)
        .ok_or(crate::ImError::PermissionDenied)?;
    if ready.auth_generation != expected_auth_generation
        || checkpoint.registry_version >= ready.registry_version
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(LocalManagementReadyTransition::Fresh)
}

/// Opens the SQLite-backed runtime only for a synchronous state transition.
/// `rusqlite::Connection` is intentionally never retained across an await.
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

fn mark_private_failure(
    core: &crate::core::ImCore,
    scope: &V2OwnerScope,
    prepared: &PreparedV2Outbound,
) -> crate::ImResult<()> {
    with_v2_runtime(core, scope, |direct| {
        direct.mark_private_outbound_failed(prepared)
    })
}

async fn post_private_control(
    client: &crate::core::ImClient,
    prepared: &PreparedV2Outbound,
) -> crate::ImResult<Value> {
    let sidecar = prepared
        .sidecar
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let direct_request = prepared.direct_request()?;
    let body = build_private_control_http_body(&prepared.metadata, direct_request, sidecar)?;
    crate::internal::transport::CoreHttpTransport::new(client)
        .authenticated_rest_post(ROOT_CONTROL_ENDPOINT, "POST", body)
        .await
}

fn build_private_control_http_body(
    metadata: &V2DirectMetadata,
    mut direct_request: Value,
    sidecar: &V2PrivateOutboundSidecar,
) -> crate::ImResult<Value> {
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if sidecar.delivery_class() != ROOT_KEY_CONTROL_DELIVERY_CLASS
        || metadata.operation_id != metadata.message_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let (transport_context, completion) = sidecar.root_control_context();
    if transport_context.message_id != metadata.message_id {
        return Err(crate::ImError::PermissionDenied);
    }
    if completion.is_some() {
        validate_ack_transport_route(metadata, transport_context)?;
    } else {
        validate_envelope_transport_route(metadata, transport_context)?;
    }
    let request = direct_request
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    request.insert("jsonrpc".to_owned(), Value::String("2.0".to_owned()));
    request.insert(
        "id".to_owned(),
        Value::String(metadata.operation_id.clone()),
    );
    Ok(serde_json::json!({
        "direct_request": direct_request,
        "transport_context": transport_context,
        "completion": completion,
    }))
}

fn private_sidecar(
    operation_id: &str,
    sidecar: RootPrivateTransportSidecar,
) -> crate::ImResult<V2PrivateOutboundSidecar> {
    V2PrivateOutboundSidecar::root_control(
        operation_id,
        sidecar.transport_context,
        sidecar.completion,
    )
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

fn same_did_binding(
    did: &str,
    sender: &DeviceJoinRemoteDeviceSummary,
    recipient: &DeviceJoinRemoteDeviceSummary,
) -> V2SessionBinding {
    V2SessionBinding {
        local_did: did.to_owned(),
        local_device_id: sender.device_id.clone(),
        peer_did: did.to_owned(),
        peer_device_id: recipient.device_id.clone(),
        suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
        local_e2ee_key_id: sender.e2ee_key_id.clone(),
        peer_e2ee_key_id: recipient.e2ee_key_id.clone(),
    }
}

fn validate_envelope_transport_route(
    metadata: &anp::direct_e2ee::V2DirectMetadata,
    context: &RootImportTransportContext,
) -> crate::ImResult<()> {
    if context.sender_device_id != metadata.sender_device_id
        || context.recipient_device_id != metadata.recipient_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn root_control_route(
    metadata: &anp::direct_e2ee::V2DirectMetadata,
    context: &RootImportTransportContext,
) -> crate::ImResult<RootControlRoute> {
    if validate_envelope_transport_route(metadata, context).is_ok() {
        return Ok(RootControlRoute::Envelope);
    }
    validate_ack_transport_route(metadata, context)?;
    Ok(RootControlRoute::Ack)
}

fn validate_root_control_registry_route(
    route: RootControlRoute,
    authorization: &crate::internal::identity_device_state::DeviceAuthorizationProjection,
    local: &DeviceJoinRemoteDeviceSummary,
    peer: &DeviceJoinRemoteDeviceSummary,
    did_document: &Value,
) -> crate::ImResult<()> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };
    let local_ready = match route {
        RootControlRoute::Envelope => false,
        RootControlRoute::Ack => true,
    };
    for (device, management_ready) in [(local, local_ready), (peer, true)] {
        if device.status != DeviceAuthorizationStatus::Active
            || device.role != DeviceAuthorizationRole::Admin
            || device.management_ready != management_ready
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let manifest = anp::authentication::find_eligible_device(
            did_document,
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
    if authorization.status != DeviceAuthorizationStatus::Active
        || authorization.role != DeviceAuthorizationRole::Admin
        || authorization.management_ready != local_ready
        || authorization.protocol_device_id.as_str() != local.device_id
        || authorization.signing_key_id != local.signing_key_id
        || authorization.e2ee_key_id != local.e2ee_key_id
        || authorization.auth_generation != local.auth_generation
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_session_control_registry_route(
    receiving_init: bool,
    authorization: &crate::internal::identity_device_state::DeviceAuthorizationProjection,
    local: &DeviceJoinRemoteDeviceSummary,
    peer: &DeviceJoinRemoteDeviceSummary,
    did_document: &Value,
) -> crate::ImResult<()> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };
    let (local_ready, peer_ready) = if receiving_init {
        (false, true)
    } else {
        (true, false)
    };
    for (device, management_ready) in [(local, local_ready), (peer, peer_ready)] {
        if device.status != DeviceAuthorizationStatus::Active
            || device.role != DeviceAuthorizationRole::Admin
            || device.management_ready != management_ready
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let manifest = anp::authentication::find_eligible_device(
            did_document,
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
    if authorization.status != DeviceAuthorizationStatus::Active
        || authorization.role != DeviceAuthorizationRole::Admin
        || authorization.management_ready != local_ready
        || authorization.protocol_device_id.as_str() != local.device_id
        || authorization.signing_key_id != local.signing_key_id
        || authorization.e2ee_key_id != local.e2ee_key_id
        || authorization.auth_generation != local.auth_generation
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_retry_ack_route(
    authorization: &crate::internal::identity_device_state::DeviceAuthorizationProjection,
    local: &DeviceJoinRemoteDeviceSummary,
    peer: &DeviceJoinRemoteDeviceSummary,
    did_document: &Value,
) -> crate::ImResult<()> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };
    for device in [local, peer] {
        if device.status != DeviceAuthorizationStatus::Active
            || device.role != DeviceAuthorizationRole::Admin
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let manifest = anp::authentication::find_eligible_device(
            did_document,
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
    if !peer.management_ready
        || authorization.status != DeviceAuthorizationStatus::Active
        || authorization.role != DeviceAuthorizationRole::Admin
        || authorization.protocol_device_id.as_str() != local.device_id
        || authorization.signing_key_id != local.signing_key_id
        || authorization.e2ee_key_id != local.e2ee_key_id
        || authorization.auth_generation != local.auth_generation
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_ack_transport_route(
    metadata: &anp::direct_e2ee::V2DirectMetadata,
    context: &RootImportTransportContext,
) -> crate::ImResult<()> {
    if context.recipient_device_id != metadata.sender_device_id
        || context.sender_device_id != metadata.recipient_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn serialization_error(error: serde_json::Error) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

fn time_error(error: time::error::Format) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anp::direct_e2ee::{
        direct_send_request_v2, V2DirectBody, V2DirectCipherBody, V2RatchetHeader,
        CONTENT_TYPE_DIRECT_CIPHER_V2,
    };

    fn metadata() -> V2DirectMetadata {
        serde_json::from_value(serde_json::json!({
            "profile": "anp.direct.e2ee.v2",
            "security_profile": "direct-e2ee",
            "sender_did": "did:example:alice",
            "sender_device_id": "device-admin",
            "target": {"kind": "agent", "did": "did:example:alice"},
            "recipient_device_id": "device-member",
            "operation_id": "root-control-message-1",
            "message_id": "root-control-message-1",
            "content_type": CONTENT_TYPE_DIRECT_CIPHER_V2
        }))
        .unwrap()
    }

    fn cipher_body() -> V2DirectCipherBody {
        V2DirectCipherBody {
            session_id: "AAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            suite: Some(MTI_DIRECT_E2EE_SUITE_V2.to_owned()),
            ratchet_header: V2RatchetHeader {
                dh_pub_b64u: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                pn: "0".to_owned(),
                n: "0".to_owned(),
            },
            ciphertext_b64u: "AA".to_owned(),
        }
    }

    fn transport_context() -> RootImportTransportContext {
        RootImportTransportContext {
            message_id: "root-control-message-1".to_owned(),
            delivery_class: ROOT_KEY_CONTROL_DELIVERY_CLASS.to_owned(),
            sender_device_id: "device-admin".to_owned(),
            recipient_device_id: "device-member".to_owned(),
            expires_at: "2026-07-20T01:00:00Z".to_owned(),
        }
    }

    fn completion() -> RootKeyImportedCompletion {
        RootKeyImportedCompletion {
            completion_type: "awiki.device.root-key-imported.v1".to_owned(),
            ack_for_message_id: "root-control-message-1".to_owned(),
            did: "did:example:alice".to_owned(),
            sending_device_id: "device-admin".to_owned(),
            importing_device_id: "device-member".to_owned(),
            root_key_id: "did:example:alice#root".to_owned(),
            root_public_key_fingerprint: "e1_test".to_owned(),
            document_version: 19,
            document_hash: "sha256:test".to_owned(),
            result: "imported".to_owned(),
            imported_at: "2026-07-20T00:59:00Z".to_owned(),
            device_signature: "signature".to_owned(),
        }
    }

    #[test]
    fn production_private_http_body_keeps_standard_direct_and_closed_sidecar_separate() {
        let metadata = metadata();
        let direct_request =
            direct_send_request_v2(metadata.clone(), V2DirectBody::Cipher(cipher_body())).unwrap();
        let sidecar = private_sidecar(
            "root-control-message-1",
            RootPrivateTransportSidecar {
                transport_context: transport_context(),
                completion: None,
            },
        )
        .unwrap();

        let body = build_private_control_http_body(&metadata, direct_request, &sidecar).unwrap();
        assert_eq!(body["direct_request"]["jsonrpc"], "2.0");
        assert_eq!(body["direct_request"]["id"], "root-control-message-1");
        assert_eq!(body["direct_request"]["method"], "direct.send");
        assert_eq!(
            body["direct_request"]["params"]["meta"],
            serde_json::to_value(&metadata).unwrap()
        );
        assert_eq!(
            body["transport_context"]["recipient_device_id"],
            "device-member"
        );
        assert!(body["completion"].is_null());
        let serialized = serde_json::to_string(&body).unwrap();
        for forbidden in [
            "root_private_key",
            "document_version",
            "document_hash",
            "registry_version",
        ] {
            assert!(!serialized.contains(forbidden));
        }

        let mut wrong_route = metadata;
        wrong_route.recipient_device_id = "device-other".to_owned();
        let wrong_direct =
            direct_send_request_v2(wrong_route.clone(), V2DirectBody::Cipher(cipher_body()))
                .unwrap();
        assert!(build_private_control_http_body(&wrong_route, wrong_direct, &sidecar).is_err());
    }

    #[test]
    fn management_ready_response_is_strictly_bound_to_signed_completion() {
        let completion = completion();
        let ready: RootManagementReadyResult = serde_json::from_value(serde_json::json!({
            "did": "did:example:alice",
            "device_id": "device-member",
            "management_ready": true,
            "auth_generation": 2,
            "registry_version": 8,
            "completed_message_id": "root-control-message-1"
        }))
        .unwrap();
        validate_management_ready_response("did:example:alice", &completion, &ready).unwrap();

        let wrong_message: RootManagementReadyResult = serde_json::from_value(serde_json::json!({
            "did": "did:example:alice",
            "device_id": "device-member",
            "management_ready": true,
            "auth_generation": 2,
            "registry_version": 8,
            "completed_message_id": "root-control-message-other"
        }))
        .unwrap();
        assert!(validate_management_ready_response(
            "did:example:alice",
            &completion,
            &wrong_message
        )
        .is_err());
        assert!(
            serde_json::from_value::<RootManagementReadyResult>(serde_json::json!({
                "did": "did:example:alice",
                "device_id": "device-member",
                "management_ready": true,
                "auth_generation": 2,
                "registry_version": 8,
                "completed_message_id": "root-control-message-1",
                "root_private_key": "forbidden"
            }))
            .is_err()
        );
    }

    #[test]
    fn lost_ready_probe_recovers_only_from_exact_closed_wire_code() {
        let wire_error = crate::internal::json_rpc::decode_response(
            br#"{
              "jsonrpc":"2.0",
              "id":"req-1",
              "error":{
                "code":-32001,
                "message":"authorization generation is stale",
                "data":{"awiki_code":"device.auth_generation_stale"}
              }
            }"#,
        )
        .unwrap_err();
        assert!(matches!(
            classify_root_ready_retry_probe(Err(wire_error)).unwrap(),
            RootReadyRetryProbe::ServerAlreadyCommitted
        ));

        let ambiguous_wire_error = crate::internal::json_rpc::decode_response(
            br#"{
              "jsonrpc":"2.0",
              "id":"req-2",
              "error":{
                "code":-32001,
                "message":"not authenticated",
                "data":{"awiki_code":"device.unauthenticated"}
              }
            }"#,
        )
        .unwrap_err();
        assert!(matches!(
            classify_root_ready_retry_probe(Err(ambiguous_wire_error)),
            Err(crate::ImError::Service { code: Some(code), .. })
                if code == "device.unauthenticated"
        ));

        let http_unauthorized = crate::ImError::Service {
            status_code: Some(401),
            code: None,
            message: "unauthorized".to_owned(),
            data: None,
        };
        assert!(matches!(
            classify_root_ready_retry_probe(Err(http_unauthorized)),
            Err(crate::ImError::Service {
                status_code: Some(401),
                code: None,
                ..
            })
        ));
        let unrelated = crate::ImError::Service {
            status_code: None,
            code: Some("device.inactive".to_owned()),
            message: "inactive".to_owned(),
            data: None,
        };
        assert!(matches!(
            classify_root_ready_retry_probe(Err(unrelated)),
            Err(crate::ImError::Service { code: Some(code), .. })
                if code == "device.inactive"
        ));
    }

    #[test]
    fn successful_lost_ready_probe_keeps_ack_retry_path() {
        let registry = DeviceJoinRemoteRegistry {
            did: crate::ids::Did::parse("did:example:alice").unwrap(),
            checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: 19,
                document_hash: "sha256:test".to_owned(),
                registry_version: 7,
            },
            devices: Vec::new(),
            pending_join_requests: Vec::new(),
        };
        match classify_root_ready_retry_probe(Ok(registry.clone())).unwrap() {
            RootReadyRetryProbe::ContinueAck(actual) => assert_eq!(actual, registry),
            RootReadyRetryProbe::ServerAlreadyCommitted => {
                panic!("an uncommitted server must keep the ACK retry path")
            }
        }
    }

    #[test]
    fn local_management_ready_transition_accepts_fresh_and_idempotent_crash_retry() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityInternalCheckpoint,
        };

        let authorization = DeviceAuthorizationProjection {
            protocol_device_id: crate::ids::ProtocolDeviceId::parse("device-member").unwrap(),
            signing_key_id: "did:example:alice#device-member-sign".to_owned(),
            e2ee_key_id: "did:example:alice#device-member-e2ee".to_owned(),
            status: DeviceAuthorizationStatus::Active,
            role: DeviceAuthorizationRole::Admin,
            management_ready: false,
            auth_generation: 1,
        };
        let checkpoint = IdentityInternalCheckpoint {
            document_version: 20,
            document_hash: "sha256:CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC".to_owned(),
            registry_version: 7,
        };
        let mut completion = completion();
        completion.document_version = 19;
        completion.document_hash = "sha256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned();
        let ready: RootManagementReadyResult = serde_json::from_value(serde_json::json!({
            "did": "did:example:alice",
            "device_id": "device-member",
            "management_ready": true,
            "auth_generation": 2,
            "registry_version": 8,
            "completed_message_id": "root-control-message-1"
        }))
        .unwrap();

        assert_eq!(
            validate_local_management_ready_transition(
                "did:example:alice",
                &authorization,
                Some(&checkpoint),
                &completion,
                &ready,
            )
            .unwrap(),
            LocalManagementReadyTransition::Fresh
        );

        let stale_checkpoint = IdentityInternalCheckpoint {
            document_version: completion.document_version - 1,
            document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            registry_version: 7,
        };
        assert!(validate_local_management_ready_transition(
            "did:example:alice",
            &authorization,
            Some(&stale_checkpoint),
            &completion,
            &ready,
        )
        .is_err());
        let same_version_fork = IdentityInternalCheckpoint {
            document_version: completion.document_version,
            document_hash: "sha256:CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC".to_owned(),
            registry_version: 7,
        };
        assert!(validate_local_management_ready_transition(
            "did:example:alice",
            &authorization,
            Some(&same_version_fork),
            &completion,
            &ready,
        )
        .is_err());

        let mut already_ready = authorization.clone();
        already_ready.management_ready = true;
        already_ready.auth_generation = ready.auth_generation;
        let converged_checkpoint = IdentityInternalCheckpoint {
            document_version: completion.document_version,
            document_hash: completion.document_hash.clone(),
            registry_version: ready.registry_version,
        };
        assert_eq!(
            validate_local_management_ready_transition(
                "did:example:alice",
                &already_ready,
                Some(&converged_checkpoint),
                &completion,
                &ready,
            )
            .unwrap(),
            LocalManagementReadyTransition::AlreadyConverged
        );
        let mut forked_checkpoint = converged_checkpoint.clone();
        forked_checkpoint.document_hash =
            "sha256:CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC".to_owned();
        assert!(validate_local_management_ready_transition(
            "did:example:alice",
            &already_ready,
            Some(&forked_checkpoint),
            &completion,
            &ready,
        )
        .is_err());

        let mut stale_generation = authorization.clone();
        stale_generation.auth_generation = ready.auth_generation;
        assert!(validate_local_management_ready_transition(
            "did:example:alice",
            &stale_generation,
            Some(&checkpoint),
            &completion,
            &ready,
        )
        .is_err());

        let skipped_generation: RootManagementReadyResult =
            serde_json::from_value(serde_json::json!({
                "did": "did:example:alice",
                "device_id": "device-member",
                "management_ready": true,
                "auth_generation": 3,
                "registry_version": 8,
                "completed_message_id": "root-control-message-1"
            }))
            .unwrap();
        assert!(validate_local_management_ready_transition(
            "did:example:alice",
            &authorization,
            Some(&checkpoint),
            &completion,
            &skipped_generation,
        )
        .is_err());

        let mut non_advancing_checkpoint = checkpoint;
        non_advancing_checkpoint.registry_version = ready.registry_version;
        assert!(validate_local_management_ready_transition(
            "did:example:alice",
            &authorization,
            Some(&non_advancing_checkpoint),
            &completion,
            &ready,
        )
        .is_err());
    }

    #[test]
    fn management_ready_registry_cannot_roll_back_or_fork_signed_completion() {
        use crate::internal::identity_device_state::IdentityInternalCheckpoint;

        let completion = completion();
        let ready: RootManagementReadyResult = serde_json::from_value(serde_json::json!({
            "did": "did:example:alice",
            "device_id": "device-member",
            "management_ready": true,
            "auth_generation": 2,
            "registry_version": 8,
            "completed_message_id": "root-control-message-1"
        }))
        .unwrap();
        let exact = IdentityInternalCheckpoint {
            document_version: completion.document_version,
            document_hash: completion.document_hash.clone(),
            registry_version: ready.registry_version,
        };
        validate_management_ready_registry_checkpoint(
            &completion,
            &ready,
            &exact,
            &completion.document_hash,
        )
        .unwrap();

        let mut rolled_back = exact.clone();
        rolled_back.document_version -= 1;
        assert!(validate_management_ready_registry_checkpoint(
            &completion,
            &ready,
            &rolled_back,
            &rolled_back.document_hash,
        )
        .is_err());

        let mut same_version_fork = exact.clone();
        same_version_fork.document_hash = "sha256:fork".to_owned();
        assert!(validate_management_ready_registry_checkpoint(
            &completion,
            &ready,
            &same_version_fork,
            &same_version_fork.document_hash,
        )
        .is_err());

        let mut stale_registry = exact.clone();
        stale_registry.registry_version -= 1;
        assert!(validate_management_ready_registry_checkpoint(
            &completion,
            &ready,
            &stale_registry,
            &stale_registry.document_hash,
        )
        .is_err());

        let advanced = IdentityInternalCheckpoint {
            document_version: completion.document_version + 1,
            document_hash: "sha256:new-document".to_owned(),
            registry_version: ready.registry_version + 1,
        };
        validate_management_ready_registry_checkpoint(
            &completion,
            &ready,
            &advanced,
            &advanced.document_hash,
        )
        .unwrap();
    }

    #[test]
    fn root_import_token_operation_is_fixed_length_and_within_wire_limit() {
        for message_id_len in [112, 128] {
            let _maximum_length_root_message_id = "m".repeat(message_id_len);
            let operation_id = random_root_import_token_operation_id().unwrap();

            assert!(operation_id.starts_with("root-ready-"));
            assert_eq!(operation_id.len(), "root-ready-".len() + 32);
            assert!(operation_id.len() <= 128);
            assert!(operation_id.is_ascii());
        }
    }
}
