//! Production sender orchestration for AWiki-local root-key transfer.
//!
//! The public boundary exposes only an abstract delivery result. Root
//! plaintext and private transport metadata stay below this module and are
//! carried only inside an established exact-device P5 v2 session.

use anp::direct_e2ee::{V2SecretJsonPayload, V2SessionBinding, MTI_DIRECT_E2EE_SUITE_V2};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::internal::identity_device_join_runtime::{
    DeviceJoinAdminHttpAdapter, DeviceJoinAdminRemote, DeviceJoinRemoteDeviceSummary,
    DeviceJoinRemoteRegistry,
};
use crate::internal::secure_direct::v2_runtime::{
    PreparedV2Outbound, V2EstablishedDirectRuntime, V2ExactSessionPreflight,
};
use crate::internal::secure_direct::v2_store::{SqliteV2DirectStateStore, V2OwnerScope};

const ROOT_TRANSFER_PREFLIGHT_DEADLINE_SECONDS: u64 = 10;
const ROOT_TRANSFER_HANDLE_TTL_SECONDS: i64 = 60;
const ROOT_TRANSFER_ENVELOPE_TTL_SECONDS: i64 = 600;
const ROOT_KEY_ENVELOPE_V1: &str = "awiki.device.root-key-envelope.v1";
pub(crate) const ROOT_KEY_TRANSFER_MESSAGE_ID_PREFIX: &str = "msg-root-key-";
const ED25519_PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

type RootTransferError = crate::identity::RootKeyTransferError;
type RootTransferErrorCode = crate::identity::RootKeyTransferErrorCode;
type RootTransferResult<T> = crate::identity::RootKeyTransferResult<T>;

#[derive(Clone)]
enum PreparedRootTransport {
    EstablishedSession,
    Prekey(anp::direct_e2ee::V2GetPrekeyBundleResult),
}

#[derive(Clone)]
enum PreparedRootDelivery {
    New {
        message_id: crate::ids::MessageId,
        transport: PreparedRootTransport,
    },
    ResumePending {
        message_id: crate::ids::MessageId,
    },
}

#[derive(Clone)]
pub(crate) struct RootKeyTransferAuthorizationState {
    identity_id: crate::ids::IdentityId,
    did: crate::ids::Did,
    local_alias: String,
    sender: DeviceJoinRemoteDeviceSummary,
    recipient: DeviceJoinRemoteDeviceSummary,
    checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint,
    root_key_id: String,
    root_public_key_fingerprint: String,
    delivery: PreparedRootDelivery,
    expires_at: OffsetDateTime,
}

enum RootKeyTransferAuthorizationLease {
    Ready(RootKeyTransferAuthorizationState),
    Consumed,
}

#[derive(Default)]
pub(crate) struct RootKeyTransferAuthorizationStore {
    entries: Mutex<HashMap<String, RootKeyTransferAuthorizationLease>>,
}

enum RootKeyTransferAuthorizationClaim {
    Claimed(RootKeyTransferAuthorizationState),
    Expired,
    Consumed,
    Invalid,
}

impl RootKeyTransferAuthorizationStore {
    fn issue(
        &self,
        state: RootKeyTransferAuthorizationState,
    ) -> RootTransferResult<crate::identity::RootKeyTransferAuthorizationHandle> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        use rand::RngCore as _;

        let mut random = [0_u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut random);
        let encoded = URL_SAFE_NO_PAD.encode(random);
        let handle =
            crate::identity::RootKeyTransferAuthorizationHandle::from_generated(encoded.clone())
                .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
        entries.retain(|_, entry| match entry {
            RootKeyTransferAuthorizationLease::Ready(existing) => {
                existing.expires_at > OffsetDateTime::now_utc()
            }
            RootKeyTransferAuthorizationLease::Consumed => false,
        });
        entries.insert(encoded, RootKeyTransferAuthorizationLease::Ready(state));
        Ok(handle)
    }

    fn claim(
        &self,
        handle: &crate::identity::RootKeyTransferAuthorizationHandle,
        now: OffsetDateTime,
    ) -> RootTransferResult<RootKeyTransferAuthorizationClaim> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
        let Some(entry) = entries.get_mut(handle.expose_to_core()) else {
            return Ok(RootKeyTransferAuthorizationClaim::Invalid);
        };
        match entry {
            RootKeyTransferAuthorizationLease::Consumed => {
                Ok(RootKeyTransferAuthorizationClaim::Consumed)
            }
            RootKeyTransferAuthorizationLease::Ready(state) if state.expires_at <= now => {
                *entry = RootKeyTransferAuthorizationLease::Consumed;
                Ok(RootKeyTransferAuthorizationClaim::Expired)
            }
            RootKeyTransferAuthorizationLease::Ready(state) => {
                let claimed = state.clone();
                *entry = RootKeyTransferAuthorizationLease::Consumed;
                Ok(RootKeyTransferAuthorizationClaim::Claimed(claimed))
            }
        }
    }
}

#[derive(Serialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct RootKeyEnvelopeV1 {
    system_type: String,
    message_id: String,
    did: String,
    root_key_id: String,
    root_public_key_fingerprint: String,
    root_private_key_pkcs8_b64u: String,
    sender_device_id: String,
    sender_e2ee_key_id: String,
    recipient_device_id: String,
    recipient_e2ee_key_id: String,
    document_version: u64,
    document_hash: String,
    registry_version: u64,
    issued_at: String,
    expires_at: String,
}

pub(crate) async fn prepare_root_key_transfer(
    client: &crate::core::ImClient,
    request: crate::identity::RootKeyTransferPrepareRequest,
) -> RootTransferResult<crate::identity::RootKeyTransferPreparation> {
    let core = client.core_handle();
    let local_entry = local_device_entry(&core, client)
        .map_err(|_| root_error(RootTransferErrorCode::SenderNotEligible))?;
    let local_state = local_entry
        .device_state
        .as_ref()
        .ok_or_else(|| root_error(RootTransferErrorCode::SenderNotEligible))?;
    let authorization = local_state
        .authorization
        .as_ref()
        .ok_or_else(|| root_error(RootTransferErrorCode::SenderNotEligible))?;
    active_root_key_id(&core, &local_entry)
        .await
        .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?;

    let recipient_device_id = request.recipient_device_id.as_str().to_owned();
    let existing_delivery = sender_delivery_for_recipient(&core, client, &recipient_device_id)
        .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
    if matches!(existing_delivery, Some(SenderDeliveryState::Sent)) {
        return Err(root_error(RootTransferErrorCode::RecipientNotEligible));
    }
    let message_id = match &existing_delivery {
        Some(SenderDeliveryState::Pending(delivery)) => {
            crate::ids::MessageId::parse(&delivery.message_id)
                .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?
        }
        Some(SenderDeliveryState::Sent) => unreachable!("sent delivery returned above"),
        None => crate::ids::MessageId::parse(generate_root_key_transfer_message_id())
            .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?,
    };

    let preflight = tokio::time::timeout(
        std::time::Duration::from_secs(ROOT_TRANSFER_PREFLIGHT_DEADLINE_SECONDS),
        async {
            let mut registry_remote = DeviceJoinAdminHttpAdapter::production(client);
            let registry = registry_remote
                .registry(client.did(), false)
                .await
                .map_err(map_prepare_remote_error)?;
            let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
            let document = crate::internal::discovery::did_document::resolve_did_document_async(
                &mut resolver,
                client.did().as_str(),
            )
            .await
            .map_err(map_prepare_remote_error)?;
            let (sender, recipient) = validate_v1_transfer_route(
                &local_entry,
                client.did(),
                &document,
                &registry,
                &recipient_device_id,
            )?;
            let root_key_id = active_root_key_id(&core, &local_entry)
                .await
                .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?;
            let fingerprint = validate_root_public(&document, client.did(), &root_key_id)
                .map_err(|_| root_error(RootTransferErrorCode::SenderNotEligible))?;
            let binding = same_did_binding(client.did().as_str(), sender, recipient);
            let scope = V2OwnerScope::from_identity_state(
                &client.current_identity().id,
                client.did(),
                local_state,
            )
            .map_err(|_| root_error(RootTransferErrorCode::SenderNotEligible))?;
            let delivery = match &existing_delivery {
                Some(SenderDeliveryState::Pending(existing)) => {
                    let resumable = with_v2_runtime(&core, &scope, |direct| {
                        direct.resume_outbound_for_exact_device(
                            &existing.message_id,
                            &existing.recipient_device_id,
                        )
                    })
                    .map_err(map_preflight_error)?
                    .is_some();
                    if !resumable {
                        return Err(root_error(RootTransferErrorCode::TemporarilyUnavailable));
                    }
                    PreparedRootDelivery::ResumePending {
                        message_id: message_id.clone(),
                    }
                }
                Some(SenderDeliveryState::Sent) => unreachable!("sent delivery returned above"),
                None => {
                    let transport = match with_v2_runtime(&core, &scope, |direct| {
                        direct.exact_session_preflight(&binding)
                    })
                    .map_err(map_preflight_error)?
                    {
                        V2ExactSessionPreflight::Established => {
                            PreparedRootTransport::EstablishedSession
                        }
                        V2ExactSessionPreflight::Conflict => {
                            return Err(root_error(RootTransferErrorCode::PrekeyInvalid));
                        }
                        V2ExactSessionPreflight::Absent => {
                            let prekey = crate::internal::secure_direct::v2_prekey_runtime::fetch_verified_prekey(
                                client,
                                client.did().as_str(),
                                &recipient.device_id,
                                &document,
                                message_id.as_str(),
                            )
                            .await
                            .map_err(map_preflight_error)?;
                            PreparedRootTransport::Prekey(prekey)
                        }
                    };
                    PreparedRootDelivery::New {
                        message_id: message_id.clone(),
                        transport,
                    }
                }
            };
            Ok::<_, RootTransferError>((
                sender.clone(),
                recipient.clone(),
                root_key_id,
                fingerprint,
                delivery,
                registry,
            ))
        },
    )
    .await
    .map_err(|_| root_error(RootTransferErrorCode::PrekeyUnavailable))??;

    let (sender, recipient, root_key_id, root_public_key_fingerprint, delivery, registry) =
        preflight;
    if authorization.protocol_device_id.as_str() != sender.device_id {
        return Err(root_error(RootTransferErrorCode::SenderNotEligible));
    }
    let expires_at = whole_second(
        OffsetDateTime::now_utc() + Duration::seconds(ROOT_TRANSFER_HANDLE_TTL_SECONDS),
    )
    .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
    let state = RootKeyTransferAuthorizationState {
        identity_id: client.current_identity().id.clone(),
        did: client.did().clone(),
        local_alias: client
            .current_identity()
            .local_alias
            .clone()
            .ok_or_else(|| root_error(RootTransferErrorCode::SenderNotEligible))?,
        sender: sender.clone(),
        recipient: recipient.clone(),
        checkpoint: registry.checkpoint.clone(),
        root_key_id,
        root_public_key_fingerprint,
        delivery,
        expires_at,
    };
    let handle = client
        .core_inner()
        .root_key_transfer_authorizations
        .issue(state)?;
    Ok(crate::identity::RootKeyTransferPreparation {
        authorization_handle: handle,
        recipient: crate::identity::RootKeyTransferRecipientSummary {
            did: client.did().clone(),
            device_id: request.recipient_device_id,
            signing_key_id: recipient.signing_key_id,
            e2ee_key_id: recipient.e2ee_key_id,
            registry_version: registry.checkpoint.registry_version,
        },
        expires_at: format_time(expires_at)
            .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?,
    })
}

fn generate_root_key_transfer_message_id() -> String {
    let mut bytes = [0_u8; 16];
    use rand::RngCore as _;
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "{ROOT_KEY_TRANSFER_MESSAGE_ID_PREFIX}{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

pub(crate) fn is_root_key_transfer_message_id(message_id: &str) -> bool {
    message_id
        .strip_prefix(ROOT_KEY_TRANSFER_MESSAGE_ID_PREFIX)
        .is_some_and(|suffix| !suffix.is_empty())
}

pub(crate) async fn confirm_and_send_root_key_transfer(
    client: &crate::core::ImClient,
    request: crate::identity::RootKeyTransferSendRequest,
) -> RootTransferResult<crate::identity::RootKeyTransferSendResult> {
    let state = match client
        .core_inner()
        .root_key_transfer_authorizations
        .claim(&request.authorization_handle, OffsetDateTime::now_utc())?
    {
        RootKeyTransferAuthorizationClaim::Claimed(state) => state,
        RootKeyTransferAuthorizationClaim::Expired => {
            return Err(root_error(RootTransferErrorCode::AuthorizationExpired));
        }
        RootKeyTransferAuthorizationClaim::Consumed => {
            return Err(root_error(
                RootTransferErrorCode::AuthorizationAlreadyConsumed,
            ));
        }
        RootKeyTransferAuthorizationClaim::Invalid => {
            return Err(root_error(RootTransferErrorCode::AuthorizationInvalid));
        }
    };
    if !request.user_presence_confirmed {
        return Err(root_error(RootTransferErrorCode::UserPresenceDenied));
    }
    if state.identity_id != client.current_identity().id || state.did != *client.did() {
        return Err(root_error(RootTransferErrorCode::AuthorizationInvalid));
    }

    let core = client.core_handle();
    let local_entry = local_device_entry(&core, client)
        .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
    if local_entry.credential_name != state.local_alias {
        return Err(root_error(RootTransferErrorCode::StateChanged));
    }
    let local_state = local_entry
        .device_state
        .as_ref()
        .ok_or_else(|| root_error(RootTransferErrorCode::StateChanged))?;
    let mut registry_remote = DeviceJoinAdminHttpAdapter::production(client);
    let registry = registry_remote
        .registry(client.did(), false)
        .await
        .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
    let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
    let document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await
    .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
    let (sender, recipient) = validate_v1_transfer_route(
        &local_entry,
        client.did(),
        &document,
        &registry,
        &state.recipient.device_id,
    )
    .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
    if sender != &state.sender
        || recipient != &state.recipient
        || registry.checkpoint != state.checkpoint
        || validate_root_public(&document, client.did(), &state.root_key_id)
            .ok()
            .as_deref()
            != Some(state.root_public_key_fingerprint.as_str())
    {
        return Err(root_error(RootTransferErrorCode::StateChanged));
    }

    let scope =
        V2OwnerScope::from_identity_state(&client.current_identity().id, client.did(), local_state)
            .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
    if let PreparedRootDelivery::ResumePending { message_id } = &state.delivery {
        let prepared = with_v2_runtime(&core, &scope, |direct| {
            direct.resume_outbound_for_exact_device(message_id.as_str(), &recipient.device_id)
        })
        .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?
        .ok_or_else(|| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
        let accepted =
            post_and_mark_root_key_transfer(client, &core, &scope, &prepared, message_id.as_str())
                .await?;
        return root_key_transfer_send_result(state.did, &sender.device_id, accepted);
    }
    let (message_id, transport) = match state.delivery.clone() {
        PreparedRootDelivery::New {
            message_id,
            transport,
        } => (message_id, transport),
        PreparedRootDelivery::ResumePending { .. } => {
            unreachable!("pending delivery returned above")
        }
    };

    let custody_manager = crate::internal::identity_custody::controller_custody_provider(&core)
        .await
        .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?;
    let custody_info = custody_manager
        .store_info()
        .await
        .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?;
    if local_entry.identity_custody_backend.as_deref() != Some("anp_identity")
        || local_entry.anp_identity_store_id.as_deref() != Some(custody_info.store_id.as_str())
    {
        return Err(root_error(RootTransferErrorCode::RootVaultUnavailable));
    }
    let identity_id = local_entry
        .anp_identity_id
        .as_deref()
        .ok_or_else(|| root_error(RootTransferErrorCode::RootVaultUnavailable))?;
    let custody_identity = custody_manager
        .open_identity(&crate::internal::identity_provider::ProviderIdentityRef {
            store_id: custody_info.store_id,
            identity_id: identity_id.to_owned(),
            did: client.did().as_str().to_owned(),
        })
        .await
        .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?;
    let host_status = custody_identity
        .host_status()
        .await
        .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?;
    if host_status.root_capability
        != crate::internal::identity_provider::ProviderRootCapability::Active
    {
        return Err(root_error(RootTransferErrorCode::RootVaultUnavailable));
    }
    let checkpoint = host_status
        .checkpoint
        .ok_or_else(|| root_error(RootTransferErrorCode::StateChanged))?;
    let document_digest = crate::internal::identity_wire::document::document_hash(&document)
        .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
    if checkpoint.document_version != registry.checkpoint.document_version
        || checkpoint.registry_version != registry.checkpoint.registry_version
        || checkpoint.document_digest != document_digest
        || registry.checkpoint.document_hash != document_digest
    {
        return Err(root_error(RootTransferErrorCode::StateChanged));
    }
    let exported_root = custody_identity
        .export_root_for_legacy_envelope(
            crate::internal::identity_provider::ProviderLegacyRootExportRequest {
                key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                    state.root_key_id.clone(),
                ),
                user_presence_confirmed: true,
            },
        )
        .await
        .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?;
    validate_root_private_der_matches_document(
        exported_root.as_pkcs8_der(),
        &document,
        &state.root_key_id,
    )
    .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
    let issued_at = whole_second(OffsetDateTime::now_utc())
        .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
    let envelope_expires_at = issued_at + Duration::seconds(ROOT_TRANSFER_ENVELOPE_TTL_SECONDS);
    let root_private_key_pkcs8_b64u =
        zeroize::Zeroizing::new(URL_SAFE_NO_PAD.encode(exported_root.as_pkcs8_der()));
    let envelope = zeroize::Zeroizing::new(RootKeyEnvelopeV1 {
        system_type: ROOT_KEY_ENVELOPE_V1.to_owned(),
        message_id: message_id.as_str().to_owned(),
        did: state.did.as_str().to_owned(),
        root_key_id: state.root_key_id,
        root_public_key_fingerprint: state.root_public_key_fingerprint,
        root_private_key_pkcs8_b64u: root_private_key_pkcs8_b64u.to_string(),
        sender_device_id: sender.device_id.clone(),
        sender_e2ee_key_id: sender.e2ee_key_id.clone(),
        recipient_device_id: recipient.device_id.clone(),
        recipient_e2ee_key_id: recipient.e2ee_key_id.clone(),
        document_version: registry.checkpoint.document_version,
        document_hash: registry.checkpoint.document_hash.clone(),
        registry_version: registry.checkpoint.registry_version,
        issued_at: format_time(issued_at)
            .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?,
        expires_at: format_time(envelope_expires_at)
            .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?,
    });
    let canonical = zeroize::Zeroizing::new(
        serde_json_canonicalizer::to_vec(&*envelope)
            .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?,
    );
    let plaintext = V2SecretJsonPayload::from_canonical_json_object(canonical.to_vec())
        .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
    let binding = same_did_binding(client.did().as_str(), sender, recipient);
    let now = format_time(issued_at)
        .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
    ensure_sender_envelope_format_column(&core)
        .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?;
    let prekey_local_static_dh = match &transport {
        PreparedRootTransport::Prekey(prekey) => {
            let recipient_signed_prekey: [u8; 32] = URL_SAFE_NO_PAD
                .decode(&prekey.prekey_bundle.signed_prekey.public_key_b64u)
                .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?
                .try_into()
                .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
            Some(
                crate::internal::identity_provider::derive_shared_secret_or_fallback(
                    client.runtime().identity_session.as_ref(),
                    &client.runtime().key_provider,
                    &sender.e2ee_key_id,
                    recipient_signed_prekey,
                )
                .await
                .map_err(|_| root_error(RootTransferErrorCode::RootVaultUnavailable))?,
            )
        }
        PreparedRootTransport::EstablishedSession => None,
    };
    let prepared = with_v2_runtime(&core, &scope, |direct| match &transport {
        PreparedRootTransport::EstablishedSession => direct
            .prepare_outbound_secret_json_with_commit(
                &binding,
                message_id.as_str(),
                &plaintext,
                &now,
                |transaction| {
                    persist_sender_delivery_pending_tx(
                        transaction,
                        client.current_identity().id.as_str(),
                        client.did().as_str(),
                        &sender.device_id,
                        message_id.as_str(),
                        &recipient.device_id,
                        &now,
                    )
                },
            ),
        PreparedRootTransport::Prekey(prekey) => {
            let recipient_static =
                crate::internal::secure_direct::v2_prekey_runtime::static_public_from_document(
                    &document,
                    &recipient.e2ee_key_id,
                )?;
            let local_static_dh = prekey_local_static_dh
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            direct.prepare_session_init_secret_json_with_commit(
                &binding,
                message_id.as_str(),
                &plaintext,
                local_static_dh,
                prekey,
                &recipient_static,
                &now,
                |transaction| {
                    persist_sender_delivery_pending_tx(
                        transaction,
                        client.current_identity().id.as_str(),
                        client.did().as_str(),
                        &sender.device_id,
                        message_id.as_str(),
                        &recipient.device_id,
                        &now,
                    )
                },
            )
        }
    })
    .map_err(|_| root_error(RootTransferErrorCode::StateChanged))?;
    let accepted =
        post_and_mark_root_key_transfer(client, &core, &scope, &prepared, message_id.as_str())
            .await?;
    root_key_transfer_send_result(state.did, &sender.device_id, accepted)
}

async fn post_and_mark_root_key_transfer(
    client: &crate::core::ImClient,
    core: &crate::core::ImCore,
    scope: &V2OwnerScope,
    prepared: &PreparedV2Outbound,
    message_id: &str,
) -> RootTransferResult<anp::direct_e2ee::V2DirectSendResult> {
    let accepted = match crate::internal::secure_direct::v2_prekey_runtime::post_standard_direct(
        client, prepared,
    )
    .await
    {
        Ok(accepted) => accepted,
        Err(error) if is_retryable_transport_error(&error) => {
            // The P5 pending record and sender ledger were committed together.
            // One response-loss retry therefore reuses the exact same
            // operation/message ID and ciphertext.
            crate::internal::secure_direct::v2_prekey_runtime::post_standard_direct(
                client, prepared,
            )
            .await
            .map_err(|_| root_error(RootTransferErrorCode::TransportPending))?
        }
        Err(_) => return Err(root_error(RootTransferErrorCode::TransportRejected)),
    };
    if !with_v2_runtime(core, scope, |direct| {
        direct.mark_outbound_accepted_with_commit(prepared, |transaction| {
            persist_sender_delivery_accepted_tx(
                transaction,
                client.current_identity().id.as_str(),
                client.exact_protocol_device_id()?.as_str(),
                message_id,
                &accepted.accepted_at,
            )
        })
    })
    .map_err(|_| root_error(RootTransferErrorCode::TemporarilyUnavailable))?
    {
        return Err(root_error(RootTransferErrorCode::TemporarilyUnavailable));
    }
    Ok(accepted)
}

fn root_key_transfer_send_result(
    did: crate::ids::Did,
    sender_device_id: &str,
    accepted: anp::direct_e2ee::V2DirectSendResult,
) -> RootTransferResult<crate::identity::RootKeyTransferSendResult> {
    Ok(crate::identity::RootKeyTransferSendResult {
        did,
        sender_device_id: crate::ids::ProtocolDeviceId::parse(sender_device_id)
            .map_err(|_| root_error(RootTransferErrorCode::TransportRejected))?,
        recipient_device_id: crate::ids::ProtocolDeviceId::parse(accepted.recipient_device_id)
            .map_err(|_| root_error(RootTransferErrorCode::TransportRejected))?,
        message_id: crate::ids::MessageId::parse(accepted.message_id)
            .map_err(|_| root_error(RootTransferErrorCode::TransportRejected))?,
        accepted_at: accepted.accepted_at,
    })
}

fn persist_sender_delivery_pending_tx(
    transaction: &rusqlite::Transaction<'_>,
    owner_identity_id: &str,
    owner_did: &str,
    sender_device_id: &str,
    message_id: &str,
    recipient_device_id: &str,
    now: &str,
) -> crate::ImResult<()> {
    transaction
        .execute(
            r#"INSERT INTO identity_root_transfer_sender_v1 (
owner_identity_id, owner_did, local_device_id, message_id,
recipient_device_id, envelope_format, phase, created_at, updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, 'legacy_v1', 'pending_delivery', ?6, ?6)
ON CONFLICT(owner_identity_id, local_device_id, message_id) DO NOTHING"#,
            rusqlite::params![
                owner_identity_id,
                owner_did,
                sender_device_id,
                message_id,
                recipient_device_id,
                now,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let exact: i64 = transaction
        .query_row(
            r#"SELECT COUNT(*) FROM identity_root_transfer_sender_v1
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3
  AND message_id = ?4 AND recipient_device_id = ?5
  AND envelope_format = 'legacy_v1'
  AND phase IN ('pending_delivery', 'sent')"#,
            rusqlite::params![
                owner_identity_id,
                owner_did,
                sender_device_id,
                message_id,
                recipient_device_id,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if exact != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn persist_sender_delivery_accepted_tx(
    transaction: &rusqlite::Transaction<'_>,
    owner_identity_id: &str,
    local_device_id: &str,
    message_id: &str,
    accepted_at: &str,
) -> crate::ImResult<()> {
    let changed = transaction
        .execute(
            r#"UPDATE identity_root_transfer_sender_v1
SET phase = 'sent', accepted_at = ?1, failure_code = NULL, updated_at = ?1
WHERE owner_identity_id = ?2 AND local_device_id = ?3 AND message_id = ?4
  AND (phase = 'pending_delivery' OR (phase = 'sent' AND accepted_at = ?1))"#,
            rusqlite::params![accepted_at, owner_identity_id, local_device_id, message_id,],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if changed != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingRootKeyTransferDelivery {
    message_id: String,
    recipient_device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SenderDeliveryState {
    Pending(PendingRootKeyTransferDelivery),
    Sent,
}

fn sender_delivery_for_recipient(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    recipient_device_id: &str,
) -> crate::ImResult<Option<SenderDeliveryState>> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    ensure_sender_envelope_format_column_with_connection(&connection)?;
    sender_delivery_for_recipient_with_connection(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        client.exact_protocol_device_id()?.as_str(),
        recipient_device_id,
    )
}

fn sender_delivery_for_recipient_with_connection(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    local_device_id: &str,
    recipient_device_id: &str,
) -> crate::ImResult<Option<SenderDeliveryState>> {
    let retired = connection
        .execute(
            r#"UPDATE identity_root_transfer_sender_v1
SET phase = 'terminal_failed', failure_code = 'wrapped_retry_forbidden',
    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3
  AND phase = 'pending_delivery' AND envelope_format = 'wrapped_v1'"#,
            rusqlite::params![owner_identity_id, owner_did, local_device_id],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if retired > 0 {
        return Err(crate::ImError::unsupported(
            "wrapped-root-transfer-retry-forbidden-restart-with-legacy-transfer",
        ));
    }
    let mut statement = connection
        .prepare(
            r#"SELECT message_id, recipient_device_id, phase
FROM identity_root_transfer_sender_v1
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND local_device_id = ?3
  AND recipient_device_id = ?4
  AND phase IN ('pending_delivery', 'sent')
  AND envelope_format = 'legacy_v1'
ORDER BY created_at, message_id"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let deliveries = statement
        .query_map(
            rusqlite::params![
                owner_identity_id,
                owner_did,
                local_device_id,
                recipient_device_id,
            ],
            |row| {
                Ok((
                    PendingRootKeyTransferDelivery {
                        message_id: row.get(0)?,
                        recipient_device_id: row.get(1)?,
                    },
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut deliveries = deliveries;
    if deliveries.len() > 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let Some((delivery, phase)) = deliveries.pop() else {
        return Ok(None);
    };
    match phase.as_str() {
        "pending_delivery" => Ok(Some(SenderDeliveryState::Pending(delivery))),
        "sent" => Ok(Some(SenderDeliveryState::Sent)),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn ensure_sender_envelope_format_column(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    ensure_sender_envelope_format_column_with_connection(&connection)
}

fn ensure_sender_envelope_format_column_with_connection(
    connection: &rusqlite::Connection,
) -> crate::ImResult<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(identity_root_transfer_sender_v1)")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let has_column = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .iter()
        .any(|name| name == "envelope_format");
    drop(statement);
    if !has_column {
        connection
            .execute(
                "ALTER TABLE identity_root_transfer_sender_v1 ADD COLUMN envelope_format TEXT NOT NULL DEFAULT 'legacy_v1'",
                [],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
    }
    Ok(())
}

fn root_error(code: RootTransferErrorCode) -> RootTransferError {
    RootTransferError::new(code)
}

fn map_prepare_remote_error(error: crate::ImError) -> RootTransferError {
    match error {
        crate::ImError::TransportUnavailable { .. }
        | crate::ImError::Io { .. }
        | crate::ImError::Service {
            status_code: Some(500..=599),
            ..
        } => root_error(RootTransferErrorCode::TemporarilyUnavailable),
        _ => root_error(RootTransferErrorCode::SenderNotEligible),
    }
}

fn map_preflight_error(error: crate::ImError) -> RootTransferError {
    match error {
        crate::ImError::TransportUnavailable { .. }
        | crate::ImError::Io { .. }
        | crate::ImError::Service {
            status_code: Some(500..=599),
            ..
        } => root_error(RootTransferErrorCode::PrekeyUnavailable),
        crate::ImError::UnsupportedCapability { .. } => {
            root_error(RootTransferErrorCode::TemporarilyUnavailable)
        }
        _ => root_error(RootTransferErrorCode::PrekeyInvalid),
    }
}

fn is_retryable_transport_error(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::TransportUnavailable { .. }
            | crate::ImError::Io { .. }
            | crate::ImError::Service {
                status_code: Some(500..=599),
                ..
            }
    )
}

fn whole_second(value: OffsetDateTime) -> crate::ImResult<OffsetDateTime> {
    value
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

async fn active_root_key_id(
    core: &crate::core::ImCore,
    entry: &crate::internal::identity_store::IndexEntry,
) -> crate::ImResult<String> {
    if entry.identity_custody_backend.as_deref() != Some("anp_identity") {
        return Err(crate::ImError::PermissionDenied);
    }
    let manager = crate::internal::identity_custody::controller_custody_provider(core).await?;
    let info = manager
        .store_info()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    if entry.anp_identity_store_id.as_deref() != Some(info.store_id.as_str()) {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity_id = entry
        .anp_identity_id
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let reference = crate::internal::identity_provider::ProviderIdentityRef {
        store_id: info.store_id,
        identity_id: identity_id.to_owned(),
        did: entry.did.clone(),
    };
    let identity = manager
        .open_identity(&reference)
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    let public = identity
        .public_identity()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    if public.reference != reference
        || public.state != crate::internal::identity_provider::ProviderIdentityState::Active
        || identity
            .host_status()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?
            .root_capability
            != crate::internal::identity_provider::ProviderRootCapability::Active
    {
        return Err(crate::ImError::PermissionDenied);
    }
    public
        .active_keys
        .iter()
        .find(|key| {
            key.purposes
                .contains(&crate::internal::identity_provider::ProviderKeyPurpose::RootControl)
        })
        .map(|key| key.kid.clone())
        .ok_or(crate::ImError::PermissionDenied)
}

fn validate_v1_transfer_route<'a>(
    entry: &crate::internal::identity_store::IndexEntry,
    did: &crate::ids::Did,
    document: &Value,
    registry: &'a DeviceJoinRemoteRegistry,
    recipient_device_id: &str,
) -> RootTransferResult<(
    &'a DeviceJoinRemoteDeviceSummary,
    &'a DeviceJoinRemoteDeviceSummary,
)> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityDeviceMode,
    };
    let document_hash = crate::internal::identity_wire::document::document_hash(document)
        .map_err(|_| root_error(RootTransferErrorCode::PrekeyInvalid))?;
    if registry.did != *did
        || document.get("id").and_then(Value::as_str) != Some(did.as_str())
        || document_hash != registry.checkpoint.document_hash
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(root_error(RootTransferErrorCode::PrekeyInvalid));
    }
    let state = entry
        .device_state
        .as_ref()
        .ok_or_else(|| root_error(RootTransferErrorCode::SenderNotEligible))?;
    state
        .validate_for_did(did)
        .map_err(|_| root_error(RootTransferErrorCode::SenderNotEligible))?;
    if state.mode != IdentityDeviceMode::VNext
        || state.checkpoint.as_ref() != Some(&registry.checkpoint)
    {
        return Err(root_error(RootTransferErrorCode::SenderNotEligible));
    }
    let authorization = state
        .authorization
        .as_ref()
        .ok_or_else(|| root_error(RootTransferErrorCode::SenderNotEligible))?;
    let sender = registry_device(&registry.devices, authorization.protocol_device_id.as_str())
        .map_err(|_| root_error(RootTransferErrorCode::SenderNotEligible))?;
    if sender.status != DeviceAuthorizationStatus::Active
        || sender.role != DeviceAuthorizationRole::Admin
        || !sender.management_ready
        || authorization.status != sender.status
        || authorization.role != sender.role
        || authorization.management_ready != sender.management_ready
        || authorization.signing_key_id != sender.signing_key_id
        || authorization.e2ee_key_id != sender.e2ee_key_id
        || authorization.auth_generation != sender.auth_generation
    {
        return Err(root_error(RootTransferErrorCode::SenderNotEligible));
    }
    let sender_manifest = anp::authentication::find_eligible_device(
        document,
        &sender.device_id,
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| root_error(RootTransferErrorCode::SenderNotEligible))?
    .ok_or_else(|| root_error(RootTransferErrorCode::SenderNotEligible))?;
    if sender_manifest.signing_key_id != sender.signing_key_id
        || sender_manifest.e2ee_key_id != sender.e2ee_key_id
    {
        return Err(root_error(RootTransferErrorCode::SenderNotEligible));
    }

    let recipient = registry_device(&registry.devices, recipient_device_id)
        .map_err(|_| root_error(RootTransferErrorCode::RecipientNotEligible))?;
    require_v1_recipient_eligibility(sender, recipient)?;
    let recipient_manifest = anp::authentication::find_eligible_device(
        document,
        &recipient.device_id,
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| root_error(RootTransferErrorCode::RecipientNotEligible))?
    .ok_or_else(|| root_error(RootTransferErrorCode::RecipientNotEligible))?;
    if recipient_manifest.signing_key_id != recipient.signing_key_id
        || recipient_manifest.e2ee_key_id != recipient.e2ee_key_id
    {
        return Err(root_error(RootTransferErrorCode::RecipientNotEligible));
    }
    Ok((sender, recipient))
}

fn require_v1_recipient_eligibility(
    sender: &DeviceJoinRemoteDeviceSummary,
    recipient: &DeviceJoinRemoteDeviceSummary,
) -> RootTransferResult<()> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };
    if sender.device_id == recipient.device_id
        || recipient.status != DeviceAuthorizationStatus::Active
        || recipient.role != DeviceAuthorizationRole::Member
        || recipient.management_ready
    {
        return Err(root_error(RootTransferErrorCode::RecipientNotEligible));
    }
    Ok(())
}

fn validate_root_public(
    document: &Value,
    did: &crate::ids::Did,
    root_key_id: &str,
) -> crate::ImResult<String> {
    if !root_key_id.starts_with(&format!("{}#", did.as_str()))
        || document.get("id").and_then(Value::as_str) != Some(did.as_str())
        || document
            .get("proof")
            .and_then(|proof| proof.get("verificationMethod"))
            .and_then(Value::as_str)
            != Some(root_key_id)
        || !document
            .get("assertionMethod")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values
                    .iter()
                    .any(|value| value.as_str() == Some(root_key_id))
            })
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let matches = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(root_key_id))
        .collect::<Vec<_>>();
    if matches.len() != 1
        || matches[0].get("controller").and_then(Value::as_str) != Some(did.as_str())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let public = crate::internal::identity_wire::document::extract_identity_public_key(matches[0])?;
    if !matches!(public, anp::PublicKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    let fingerprint = format!(
        "e1_{}",
        anp::authentication::compute_multikey_fingerprint(&public)
            .map_err(|_| crate::ImError::PermissionDenied)?
    );
    if did.as_str().rsplit(':').next() != Some(fingerprint.as_str()) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(fingerprint)
}

fn validate_root_private_der_matches_document(
    private_der: &[u8],
    document: &Value,
    root_key_id: &str,
) -> crate::ImResult<()> {
    let private = anp::PrivateKeyMaterial::from_pkcs8_der(private_der)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(&private, anp::PrivateKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    let method = anp::authentication::find_verification_method(document, root_key_id)
        .ok_or(crate::ImError::PermissionDenied)?;
    let public = anp::authentication::extract_public_key(&method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let challenge = b"awiki:root-transfer:export-binding:v1";
    let signature = private
        .sign_message(challenge)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    public
        .verify_message(challenge, &signature)
        .map_err(|_| crate::ImError::PermissionDenied)
}

pub(crate) fn canonical_ed25519_pkcs8_der(private_pem: &str) -> crate::ImResult<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let mut lines = private_pem.lines();
    if lines.next() != Some("-----BEGIN PRIVATE KEY-----") {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut encoded = String::new();
    let mut found_end = false;
    for line in lines {
        if line == "-----END PRIVATE KEY-----" {
            found_end = true;
            break;
        }
        if line.is_empty() || line.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(crate::ImError::PermissionDenied);
        }
        encoded.push_str(line);
    }
    if !found_end {
        return Err(crate::ImError::PermissionDenied);
    }
    let der = STANDARD
        .decode(encoded)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if der.len() != 48 || der[..ED25519_PKCS8_PREFIX.len()] != ED25519_PKCS8_PREFIX {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(der)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };

    fn device(
        device_id: &str,
        role: DeviceAuthorizationRole,
        management_ready: bool,
    ) -> DeviceJoinRemoteDeviceSummary {
        DeviceJoinRemoteDeviceSummary {
            device_id: device_id.to_owned(),
            signing_key_id: format!("did:example:alice#{device_id}-sign"),
            e2ee_key_id: format!("did:example:alice#{device_id}-e2ee"),
            status: DeviceAuthorizationStatus::Active,
            role,
            management_ready,
            auth_generation: 1,
        }
    }

    #[test]
    fn recipient_state_failure_maps_before_prekey_to_closed_recipient_error() {
        let sender = device("phone", DeviceAuthorizationRole::Admin, true);
        let eligible = device("tablet", DeviceAuthorizationRole::Member, false);
        assert!(require_v1_recipient_eligibility(&sender, &eligible).is_ok());

        for recipient in [
            device("tablet", DeviceAuthorizationRole::Admin, true),
            device("tablet", DeviceAuthorizationRole::Member, true),
            device("phone", DeviceAuthorizationRole::Member, false),
        ] {
            let error = require_v1_recipient_eligibility(&sender, &recipient).unwrap_err();
            assert_eq!(error.code, RootTransferErrorCode::RecipientNotEligible);
            assert!(!error.retryable);
        }
    }

    #[test]
    fn root_key_transfer_message_ids_have_an_explicit_interceptor_namespace() {
        let message_id = generate_root_key_transfer_message_id();

        assert!(is_root_key_transfer_message_id(&message_id));
        assert!(!is_root_key_transfer_message_id("msg-business-1"));
        assert!(!is_root_key_transfer_message_id(
            ROOT_KEY_TRANSFER_MESSAGE_ID_PREFIX
        ));
    }

    #[test]
    fn sender_ledger_records_legacy_format_without_secret_material() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                r#"CREATE TABLE identity_root_transfer_sender_v1 (
owner_identity_id TEXT NOT NULL,
owner_did TEXT NOT NULL,
local_device_id TEXT NOT NULL,
message_id TEXT NOT NULL,
recipient_device_id TEXT NOT NULL,
envelope_format TEXT NOT NULL,
phase TEXT NOT NULL,
failure_code TEXT,
accepted_at TEXT,
created_at TEXT NOT NULL,
updated_at TEXT NOT NULL,
PRIMARY KEY (owner_identity_id, local_device_id, message_id)
);"#,
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        persist_sender_delivery_pending_tx(
            &transaction,
            "identity-1",
            "did:example:alice",
            "device-admin",
            "msg-root-key-1",
            "device-member",
            "2026-08-21T00:00:00Z",
        )
        .unwrap();
        transaction.commit().unwrap();

        let (format, phase): (String, String) = connection
            .query_row(
                "SELECT envelope_format, phase FROM identity_root_transfer_sender_v1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(format, "legacy_v1");
        assert_eq!(phase, "pending_delivery");
        assert_eq!(
            sender_delivery_for_recipient_with_connection(
                &connection,
                "identity-1",
                "did:example:alice",
                "device-admin",
                "device-member",
            )
            .unwrap(),
            Some(SenderDeliveryState::Pending(
                PendingRootKeyTransferDelivery {
                    message_id: "msg-root-key-1".to_owned(),
                    recipient_device_id: "device-member".to_owned(),
                },
            ))
        );

        let transaction = connection.transaction().unwrap();
        persist_sender_delivery_accepted_tx(
            &transaction,
            "identity-1",
            "device-admin",
            "msg-root-key-1",
            "2026-08-21T00:01:00Z",
        )
        .unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            sender_delivery_for_recipient_with_connection(
                &connection,
                "identity-1",
                "did:example:alice",
                "device-admin",
                "device-member",
            )
            .unwrap(),
            Some(SenderDeliveryState::Sent)
        );
    }
}
