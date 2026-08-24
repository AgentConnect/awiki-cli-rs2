use anp::direct_e2ee::models::{SESSION_STATUS_ESTABLISHED, SESSION_STATUS_PENDING_CONFIRMATION};
use anp::direct_e2ee::{
    ApplicationPlaintext, DirectE2eeSession, DirectEnvelopeMetadata, OneTimePrekey, PrekeyBundle,
};
use serde_json::Value;

use crate::internal::auth::session::AsyncSessionProvider;
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport};

use super::client::{direct_secure_request_method_params, map_direct_error};
use super::send::{
    DirectSecureAttachmentLocalEffect, DirectSecureAttachmentSend,
    DirectSecureAttachmentSendResult, DirectSecureLocalEffect, DirectSecureTextSendResult,
};
use super::sqlite_store::{
    direct_session_from_blob, direct_session_metadata_json, direct_session_to_blob,
    DirectInitSendCommit, DirectInitSessionCommitResult, DirectSessionCasResult,
    DirectSessionRecord,
};

const DIRECT_E2EE_PROFILE: &str = "anp.direct.e2ee.v1";
const DIRECT_E2EE_SECURITY_PROFILE: &str = "direct-e2ee";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AsyncDirectSecureSendFallback {
    NoEstablishedSession,
}

#[derive(Debug)]
pub(crate) enum AsyncDirectSecureSendOutcome {
    Sent(DirectSecureTextSendResult),
    Fallback(AsyncDirectSecureSendFallback),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AsyncDirectSecureFollowUpSend {
    pub(crate) target_did: String,
    pub(crate) operation_id: String,
    pub(crate) message_id: String,
    pub(crate) plaintext: ApplicationPlaintext,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AsyncDirectSecureFollowUpResult {
    pub(crate) raw: serde_json::Map<String, Value>,
    pub(crate) session_id: String,
    pub(crate) message_id: String,
    pub(crate) operation_id: String,
    pub(crate) delivery_state: String,
    pub(crate) accepted_at: String,
    pub(crate) server_seq: Option<i64>,
}

pub(crate) struct AsyncDirectSecureTextSender<'a, P, M, D> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    message_transport: M,
    directory_transport: D,
}

impl<'a, P, M, D> AsyncDirectSecureTextSender<'a, P, M, D>
where
    P: AsyncSessionProvider,
    M: AsyncAuthenticatedRpcTransport,
    D: AsyncRpcTransport,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        message_transport: M,
        directory_transport: D,
    ) -> Self {
        Self {
            client,
            session_provider,
            message_transport,
            directory_transport,
        }
    }

    pub(crate) async fn send_async_if_ready(
        self,
        input: super::send::DirectSecureTextSend,
    ) -> crate::ImResult<AsyncDirectSecureSendOutcome> {
        let (peer, target_did) =
            super::send::direct_target(&input.request.target, input.resolved_target_did)?;
        let (text, kind) = super::send::text_body(&input.request.body)?;
        super::send::validate_secure_direct_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;

        let operation_id = operation_id_for_request(&input.request)?;
        let message_id = message_id_for_request(&input.request, &operation_id);
        if operation_id != message_id {
            return Err(crate::ImError::invalid_input(
                Some("delivery.idempotency_key".to_owned()),
                "direct E2EE requires delivery.idempotency_key to match client_message_id",
            ));
        }

        let mut message_transport = self.message_transport;
        let directory_transport = self.directory_transport;
        let db = self.client.core_inner().local_state_db().await?;
        let owner_identity_id = self.client.current_identity().id.as_str().to_owned();
        let Some(record) = db
            .get_direct_secure_session(owner_identity_id, target_did.clone())
            .await?
        else {
            return send_init_if_ready(
                self.client,
                db,
                message_transport,
                directory_transport,
                peer,
                target_did,
                input.target_handle.clone(),
                input.peer_scope.clone(),
                text,
                kind,
                operation_id,
                message_id,
            )
            .await;
        };
        if is_pending_confirmation_record(&record)? {
            return queued_pending_confirmation_result(
                self.client,
                peer,
                target_did,
                input.target_handle.clone(),
                input.peer_scope.clone(),
                text,
                kind,
                operation_id,
                message_id,
            );
        }
        if !is_established_record(&record)? {
            return Ok(AsyncDirectSecureSendOutcome::Fallback(
                AsyncDirectSecureSendFallback::NoEstablishedSession,
            ));
        }

        let owner_did = self.client.did().as_str().to_owned();
        let target_did_for_crypto = target_did.clone();
        let text_for_crypto = text.to_owned();
        let operation_id_for_crypto = operation_id.clone();
        let message_id_for_crypto = message_id.clone();
        let encrypted = crate::internal::runtime::worker::run_blocking(move || {
            encrypt_follow_up_from_record(
                record,
                &owner_did,
                &target_did_for_crypto,
                &operation_id_for_crypto,
                &message_id_for_crypto,
                &text_for_crypto,
            )
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;

        let saved = db
            .save_direct_secure_session_if_revision(
                encrypted.updated_record.clone(),
                encrypted.expected_revision,
            )
            .await?;
        match saved {
            DirectSessionCasResult::Saved(_) => {}
            DirectSessionCasResult::Stale { .. } => {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "direct E2EE session changed before async send could persist mutation"
                        .to_owned(),
                });
            }
        }

        let raw = <M as AsyncAuthenticatedRpcTransport>::authenticated_rpc(
            &mut message_transport,
            super::send::MESSAGE_RPC_ENDPOINT,
            &encrypted.request.method,
            Value::Object(encrypted.request.params),
        )
        .await
        .and_then(super::send::object_result)?;
        let mut sdk_result = super::send::sdk_result_from_secure_result(
            &raw,
            self.client.did().clone(),
            peer,
            &target_did,
            text,
            kind.clone(),
            Vec::new(),
        )?;
        crate::messages::normalize_direct_send_result_for_peer_scope(
            &mut sdk_result,
            input.peer_scope.as_ref(),
            input.target_handle.as_deref(),
            Some(target_did.as_str()),
        )?;
        Ok(AsyncDirectSecureSendOutcome::Sent(
            DirectSecureTextSendResult {
                sdk_result,
                queued_outbox_id: None,
                target_did,
                target_handle: input.target_handle,
                peer_scope: input.peer_scope,
                text: text.to_owned(),
                kind,
                raw: Some(Value::Object(raw)),
                local_effect: DirectSecureLocalEffect::PersistOutgoing,
            },
        ))
    }

    pub(crate) async fn send_follow_up_if_ready(
        self,
        input: super::send::DirectSecureTextSend,
    ) -> crate::ImResult<AsyncDirectSecureSendOutcome> {
        let (peer, target_did) =
            super::send::direct_target(&input.request.target, input.resolved_target_did)?;
        let (text, kind) = super::send::text_body(&input.request.body)?;
        super::send::validate_secure_direct_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;

        let operation_id = operation_id_for_request(&input.request)?;
        let message_id = message_id_for_request(&input.request, &operation_id);
        if operation_id != message_id {
            return Err(crate::ImError::invalid_input(
                Some("delivery.idempotency_key".to_owned()),
                "direct E2EE requires delivery.idempotency_key to match client_message_id",
            ));
        }

        let db = self.client.core_inner().local_state_db().await?;
        let owner_identity_id = self.client.current_identity().id.as_str().to_owned();
        let Some(record) = db
            .get_direct_secure_session(owner_identity_id, target_did.clone())
            .await?
        else {
            return Ok(AsyncDirectSecureSendOutcome::Fallback(
                AsyncDirectSecureSendFallback::NoEstablishedSession,
            ));
        };
        if !is_established_record(&record)? {
            return Ok(AsyncDirectSecureSendOutcome::Fallback(
                AsyncDirectSecureSendFallback::NoEstablishedSession,
            ));
        }

        let owner_did = self.client.did().as_str().to_owned();
        let target_did_for_crypto = target_did.clone();
        let text_for_crypto = text.to_owned();
        let operation_id_for_crypto = operation_id.clone();
        let message_id_for_crypto = message_id.clone();
        let encrypted = crate::internal::runtime::worker::run_blocking(move || {
            encrypt_follow_up_from_record(
                record,
                &owner_did,
                &target_did_for_crypto,
                &operation_id_for_crypto,
                &message_id_for_crypto,
                &text_for_crypto,
            )
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;

        let saved = db
            .save_direct_secure_session_if_revision(
                encrypted.updated_record.clone(),
                encrypted.expected_revision,
            )
            .await?;
        match saved {
            DirectSessionCasResult::Saved(_) => {}
            DirectSessionCasResult::Stale { .. } => {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "direct E2EE session changed before async send could persist mutation"
                        .to_owned(),
                });
            }
        }

        let mut message_transport = self.message_transport;
        let raw = <M as AsyncAuthenticatedRpcTransport>::authenticated_rpc(
            &mut message_transport,
            super::send::MESSAGE_RPC_ENDPOINT,
            &encrypted.request.method,
            Value::Object(encrypted.request.params),
        )
        .await
        .and_then(super::send::object_result)?;
        let mut sdk_result = super::send::sdk_result_from_secure_result(
            &raw,
            self.client.did().clone(),
            peer,
            &target_did,
            text,
            kind.clone(),
            Vec::new(),
        )?;
        crate::messages::normalize_direct_send_result_for_peer_scope(
            &mut sdk_result,
            input.peer_scope.as_ref(),
            input.target_handle.as_deref(),
            Some(target_did.as_str()),
        )?;
        Ok(AsyncDirectSecureSendOutcome::Sent(
            DirectSecureTextSendResult {
                sdk_result,
                queued_outbox_id: None,
                target_did,
                target_handle: input.target_handle,
                peer_scope: input.peer_scope,
                text: text.to_owned(),
                kind,
                raw: Some(Value::Object(raw)),
                local_effect: DirectSecureLocalEffect::PersistOutgoing,
            },
        ))
    }

    pub(crate) async fn send_attachment_follow_up_if_ready(
        self,
        input: DirectSecureAttachmentSend,
    ) -> crate::ImResult<Option<DirectSecureAttachmentSendResult>> {
        let (peer, target_did) =
            super::send::direct_target(&input.request.target, input.resolved_target_did)?;
        super::send::validate_secure_direct_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;

        let operation_id = operation_id_for_request(&input.request)?;
        let message_id = message_id_for_request(&input.request, &operation_id);
        if operation_id != message_id {
            return Err(crate::ImError::invalid_input(
                Some("delivery.idempotency_key".to_owned()),
                "direct E2EE requires delivery.idempotency_key to match client_message_id",
            ));
        }

        let db = self.client.core_inner().local_state_db().await?;
        let owner_identity_id = self.client.current_identity().id.as_str().to_owned();
        let Some(record) = db
            .get_direct_secure_session(owner_identity_id, target_did.clone())
            .await?
        else {
            return Ok(None);
        };
        if !is_established_record(&record)? {
            return Ok(None);
        }

        let owner_did = self.client.did().as_str().to_owned();
        let target_did_for_crypto = target_did.clone();
        let operation_id_for_crypto = operation_id.clone();
        let message_id_for_crypto = message_id.clone();
        let full_manifest = input.committed.full_manifest.clone();
        let redacted_manifest = input.committed.redacted_manifest.clone();
        let grant_ref = input.committed.grant_ref.clone();
        let encrypted = crate::internal::runtime::worker::run_blocking(move || {
            encrypt_follow_up_plaintext_from_record(
                record,
                &owner_did,
                &target_did_for_crypto,
                &operation_id_for_crypto,
                &message_id_for_crypto,
                ApplicationPlaintext::new_json(
                    crate::attachments::manifest::attachment_manifest_content_type(),
                    full_manifest,
                ),
            )
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;

        let saved = db
            .save_direct_secure_session_if_revision(
                encrypted.updated_record.clone(),
                encrypted.expected_revision,
            )
            .await?;
        match saved {
            DirectSessionCasResult::Saved(_) => {}
            DirectSessionCasResult::Stale { .. } => {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "direct E2EE session changed before async attachment send could persist mutation"
                        .to_owned(),
                });
            }
        }

        let mut params = encrypted.request.params;
        params.insert(
            "client".to_owned(),
            super::send::attachment_client_context(grant_ref),
        );
        let mut message_transport = self.message_transport;
        let raw = <M as AsyncAuthenticatedRpcTransport>::authenticated_rpc(
            &mut message_transport,
            super::send::MESSAGE_RPC_ENDPOINT,
            &encrypted.request.method,
            Value::Object(params),
        )
        .await
        .and_then(super::send::object_result)?;
        let mut sdk_result = super::send::sdk_result_from_secure_attachment_result(
            &raw,
            self.client.did().clone(),
            peer,
            &target_did,
            &redacted_manifest,
            Vec::new(),
        )?;
        crate::messages::normalize_direct_send_result_for_peer_scope(
            &mut sdk_result,
            input.peer_scope.as_ref(),
            input.target_handle.as_deref(),
            Some(target_did.as_str()),
        )?;
        Ok(Some(DirectSecureAttachmentSendResult {
            sdk_result,
            target_did,
            target_handle: input.target_handle,
            peer_scope: input.peer_scope,
            redacted_manifest,
            raw: Some(Value::Object(raw)),
            local_effect: DirectSecureAttachmentLocalEffect::PersistOutgoing,
        }))
    }

    pub(crate) async fn send_attachment_async_if_ready(
        self,
        input: DirectSecureAttachmentSend,
    ) -> crate::ImResult<Option<DirectSecureAttachmentSendResult>> {
        let (peer, target_did) =
            super::send::direct_target(&input.request.target, input.resolved_target_did.clone())?;
        super::send::validate_secure_direct_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;

        let operation_id = operation_id_for_request(&input.request)?;
        let message_id = message_id_for_request(&input.request, &operation_id);
        if operation_id != message_id {
            return Err(crate::ImError::invalid_input(
                Some("delivery.idempotency_key".to_owned()),
                "direct E2EE requires delivery.idempotency_key to match client_message_id",
            ));
        }

        let db = self.client.core_inner().local_state_db().await?;
        let owner_identity_id = self.client.current_identity().id.as_str().to_owned();
        if db
            .get_direct_secure_session(owner_identity_id, target_did.clone())
            .await?
            .is_some()
        {
            return self.send_attachment_follow_up_if_ready(input).await;
        }

        let committed = input.committed;
        send_attachment_init_if_ready(
            self.client,
            db,
            self.message_transport,
            self.directory_transport,
            peer,
            target_did,
            input.target_handle,
            input.peer_scope,
            operation_id,
            message_id,
            committed,
        )
        .await
    }
}

pub(crate) async fn send_established_follow_up_payload_async<M>(
    client: &crate::core::ImClient,
    db: &crate::internal::local_state::actor::LocalStateDb,
    message_transport: &mut M,
    input: AsyncDirectSecureFollowUpSend,
) -> crate::ImResult<AsyncDirectSecureFollowUpResult>
where
    M: AsyncAuthenticatedRpcTransport,
{
    let target_did = required("target_did", &input.target_did)?;
    let operation_id = required("operation_id", &input.operation_id)?;
    let message_id = required("message_id", &input.message_id)?;
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let Some(record) = db
        .get_direct_secure_session(owner_identity_id, target_did.clone())
        .await?
    else {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "direct E2EE session is not established".to_owned(),
        });
    };
    if !is_established_record(&record)? {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "direct E2EE session is not established".to_owned(),
        });
    }

    let owner_did = client.did().as_str().to_owned();
    let encrypted = crate::internal::runtime::worker::run_blocking(move || {
        encrypt_follow_up_plaintext_from_record(
            record,
            &owner_did,
            &target_did,
            &operation_id,
            &message_id,
            input.plaintext,
        )
    })
    .await
    .map_err(|err| crate::ImError::Internal {
        message: err.to_string(),
    })??;

    let saved = db
        .save_direct_secure_session_if_revision(
            encrypted.updated_record.clone(),
            encrypted.expected_revision,
        )
        .await?;
    let session_id = match saved {
        DirectSessionCasResult::Saved(saved) => saved.session_id,
        DirectSessionCasResult::Stale { .. } => {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "direct E2EE session changed before async send could persist mutation"
                    .to_owned(),
            })
        }
    };

    let raw = <M as AsyncAuthenticatedRpcTransport>::authenticated_rpc(
        message_transport,
        super::send::MESSAGE_RPC_ENDPOINT,
        &encrypted.request.method,
        Value::Object(encrypted.request.params),
    )
    .await
    .and_then(super::send::object_result)?;
    let message_id = default_string(
        &map_string_value(&raw, "message_id"),
        &default_string(&encrypted.message_id, "secure-direct-message"),
    );
    let operation_id = default_string(
        &map_string_value(&raw, "operation_id"),
        &encrypted.operation_id,
    );
    let delivery_state = default_string(&map_string_value(&raw, "delivery_state"), "accepted");
    let accepted_at = default_string(
        &map_string_value(&raw, "accepted_at"),
        &map_string_value(&raw, "finalized_at"),
    );
    let server_seq = raw
        .get("server_seq")
        .or_else(|| raw.get("server_sequence"))
        .and_then(Value::as_i64);
    Ok(AsyncDirectSecureFollowUpResult {
        raw,
        session_id,
        message_id,
        operation_id,
        delivery_state,
        accepted_at,
        server_seq,
    })
}

#[derive(Debug)]
struct EncryptedFollowUp {
    updated_record: DirectSessionRecord,
    expected_revision: i64,
    request: super::client::DirectSecurePrekeyPublishRequest,
    operation_id: String,
    message_id: String,
}

struct EncryptedInit {
    commit: DirectInitSendCommit,
    request: super::client::DirectSecurePrekeyPublishRequest,
}

struct AsyncInitLocalMaterial {
    agreement_key_id: String,
    identity_signer: std::sync::Arc<dyn crate::internal::key_provider::IdentitySigner>,
    identity_session:
        Option<std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>>,
}

struct VerifiedPrekeyBundle {
    did_document: Value,
    bundle: PrekeyBundle,
    one_time_prekey: Option<OneTimePrekey>,
}

struct AsyncInitEncryptInput {
    owner_identity_id: String,
    owner_did: String,
    agreement_key_id: String,
    static_to_signed_prekey_dh: zeroize::Zeroizing<[u8; 32]>,
    target_did: String,
    prekey: VerifiedPrekeyBundle,
    operation_id: String,
    message_id: String,
    plaintext: ApplicationPlaintext,
}

#[allow(clippy::too_many_arguments)]
async fn send_init_if_ready<M, D>(
    client: &crate::core::ImClient,
    db: crate::internal::local_state::actor::LocalStateDb,
    mut message_transport: M,
    mut directory_transport: D,
    peer: crate::ids::PeerRef,
    target_did: String,
    target_handle: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: crate::messages::MessageKind,
    operation_id: String,
    message_id: String,
) -> crate::ImResult<AsyncDirectSecureSendOutcome>
where
    M: AsyncAuthenticatedRpcTransport,
    D: AsyncRpcTransport,
{
    let Some(prekey) = fetch_verified_prekey_bundle_async(
        client,
        &mut message_transport,
        &mut directory_transport,
        &target_did,
    )
    .await?
    else {
        return Ok(AsyncDirectSecureSendOutcome::Fallback(
            AsyncDirectSecureSendFallback::NoEstablishedSession,
        ));
    };
    let local_material = async_init_local_material(client).await?;
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let owner_did = client.did().as_str().to_owned();
    let target_did_for_crypto = target_did.clone();
    let operation_id_for_crypto = operation_id.clone();
    let message_id_for_crypto = message_id.clone();
    let plaintext = ApplicationPlaintext::new_text("text/plain", text);
    let recipient_signed_prekey_public = decode_public_key_b64u(
        &prekey.bundle.signed_prekey.public_key_b64u,
        "signed_prekey.public_key_b64u",
    )?;
    let static_to_signed_prekey_dh =
        crate::internal::identity_provider::derive_shared_secret_or_fallback(
            local_material.identity_session.as_ref(),
            &local_material.identity_signer,
            &local_material.agreement_key_id,
            recipient_signed_prekey_public,
        )
        .await?;
    let encrypted = crate::internal::runtime::worker::run_blocking(move || {
        encrypt_init_from_prekey(AsyncInitEncryptInput {
            owner_identity_id,
            owner_did,
            agreement_key_id: local_material.agreement_key_id,
            static_to_signed_prekey_dh,
            target_did: target_did_for_crypto,
            prekey,
            operation_id: operation_id_for_crypto,
            message_id: message_id_for_crypto,
            plaintext,
        })
    })
    .await
    .map_err(|err| crate::ImError::Internal {
        message: err.to_string(),
    })??;

    match db
        .save_outgoing_direct_init_session(encrypted.commit)
        .await?
    {
        DirectInitSessionCommitResult::Saved(_) => {}
        DirectInitSessionCommitResult::Existing(_) => {}
        DirectInitSessionCommitResult::Stale { .. } => {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "direct E2EE session changed before async init send could persist mutation"
                    .to_owned(),
            });
        }
    }

    let raw = <M as AsyncAuthenticatedRpcTransport>::authenticated_rpc(
        &mut message_transport,
        super::send::MESSAGE_RPC_ENDPOINT,
        &encrypted.request.method,
        Value::Object(encrypted.request.params),
    )
    .await
    .and_then(super::send::object_result)?;
    let mut sdk_result = super::send::sdk_result_from_secure_result(
        &raw,
        client.did().clone(),
        peer,
        &target_did,
        text,
        kind.clone(),
        Vec::new(),
    )?;
    crate::messages::normalize_direct_send_result_for_peer_scope(
        &mut sdk_result,
        peer_scope.as_ref(),
        target_handle.as_deref(),
        Some(target_did.as_str()),
    )?;
    Ok(AsyncDirectSecureSendOutcome::Sent(
        DirectSecureTextSendResult {
            sdk_result,
            queued_outbox_id: None,
            target_did,
            target_handle,
            peer_scope,
            text: text.to_owned(),
            kind,
            raw: Some(Value::Object(raw)),
            local_effect: DirectSecureLocalEffect::PersistOutgoing,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
async fn send_attachment_init_if_ready<M, D>(
    client: &crate::core::ImClient,
    db: crate::internal::local_state::actor::LocalStateDb,
    mut message_transport: M,
    mut directory_transport: D,
    peer: crate::ids::PeerRef,
    target_did: String,
    target_handle: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    operation_id: String,
    message_id: String,
    committed: crate::internal::attachment_runtime::upload::PreparedCommittedAttachment,
) -> crate::ImResult<Option<DirectSecureAttachmentSendResult>>
where
    M: AsyncAuthenticatedRpcTransport,
    D: AsyncRpcTransport,
{
    let Some(prekey) = fetch_verified_prekey_bundle_async(
        client,
        &mut message_transport,
        &mut directory_transport,
        &target_did,
    )
    .await?
    else {
        return Ok(None);
    };
    let local_material = async_init_local_material(client).await?;
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let owner_did = client.did().as_str().to_owned();
    let target_did_for_crypto = target_did.clone();
    let operation_id_for_crypto = operation_id.clone();
    let message_id_for_crypto = message_id.clone();
    let full_manifest = committed.full_manifest.clone();
    let redacted_manifest = committed.redacted_manifest.clone();
    let grant_ref = committed.grant_ref.clone();
    let recipient_signed_prekey_public = decode_public_key_b64u(
        &prekey.bundle.signed_prekey.public_key_b64u,
        "signed_prekey.public_key_b64u",
    )?;
    let static_to_signed_prekey_dh =
        crate::internal::identity_provider::derive_shared_secret_or_fallback(
            local_material.identity_session.as_ref(),
            &local_material.identity_signer,
            &local_material.agreement_key_id,
            recipient_signed_prekey_public,
        )
        .await?;
    let encrypted = crate::internal::runtime::worker::run_blocking(move || {
        encrypt_init_from_prekey(AsyncInitEncryptInput {
            owner_identity_id,
            owner_did,
            agreement_key_id: local_material.agreement_key_id,
            static_to_signed_prekey_dh,
            target_did: target_did_for_crypto,
            prekey,
            operation_id: operation_id_for_crypto,
            message_id: message_id_for_crypto,
            plaintext: ApplicationPlaintext::new_json(
                crate::attachments::manifest::attachment_manifest_content_type(),
                full_manifest,
            ),
        })
    })
    .await
    .map_err(|err| crate::ImError::Internal {
        message: err.to_string(),
    })??;

    match db
        .save_outgoing_direct_init_session(encrypted.commit)
        .await?
    {
        DirectInitSessionCommitResult::Saved(_) => {}
        DirectInitSessionCommitResult::Existing(_) => {}
        DirectInitSessionCommitResult::Stale { .. } => {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "direct E2EE session changed before async attachment init send could persist mutation"
                    .to_owned(),
            });
        }
    }

    let mut params = encrypted.request.params;
    params.insert(
        "client".to_owned(),
        super::send::attachment_client_context(grant_ref),
    );
    let raw = <M as AsyncAuthenticatedRpcTransport>::authenticated_rpc(
        &mut message_transport,
        super::send::MESSAGE_RPC_ENDPOINT,
        &encrypted.request.method,
        Value::Object(params),
    )
    .await
    .and_then(super::send::object_result)?;
    let mut sdk_result = super::send::sdk_result_from_secure_attachment_result(
        &raw,
        client.did().clone(),
        peer,
        &target_did,
        &redacted_manifest,
        Vec::new(),
    )?;
    crate::messages::normalize_direct_send_result_for_peer_scope(
        &mut sdk_result,
        peer_scope.as_ref(),
        target_handle.as_deref(),
        Some(target_did.as_str()),
    )?;
    Ok(Some(DirectSecureAttachmentSendResult {
        sdk_result,
        target_did,
        target_handle,
        peer_scope,
        redacted_manifest,
        raw: Some(Value::Object(raw)),
        local_effect: DirectSecureAttachmentLocalEffect::PersistOutgoing,
    }))
}

fn encrypt_follow_up_from_record(
    record: DirectSessionRecord,
    owner_did: &str,
    target_did: &str,
    operation_id: &str,
    message_id: &str,
    text: &str,
) -> crate::ImResult<EncryptedFollowUp> {
    encrypt_follow_up_plaintext_from_record(
        record,
        owner_did,
        target_did,
        operation_id,
        message_id,
        ApplicationPlaintext::new_text("text/plain", text),
    )
}

fn encrypt_follow_up_plaintext_from_record(
    mut record: DirectSessionRecord,
    owner_did: &str,
    target_did: &str,
    operation_id: &str,
    message_id: &str,
    plaintext: ApplicationPlaintext,
) -> crate::ImResult<EncryptedFollowUp> {
    let expected_revision = record.revision;
    let mut session = direct_session_from_blob(&record.state_blob).map_err(map_direct_error)?;
    if session.status != SESSION_STATUS_ESTABLISHED {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "direct E2EE session is not established".to_owned(),
        });
    }
    let metadata = DirectEnvelopeMetadata {
        sender_did: owner_did.to_owned(),
        recipient_did: target_did.to_owned(),
        message_id: message_id.to_owned(),
        profile: DIRECT_E2EE_PROFILE.to_owned(),
        security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
    };
    let (_pending, body) =
        DirectE2eeSession::encrypt_follow_up(&mut session, &metadata, operation_id, &plaintext)
            .map_err(map_direct_error)?;
    record.session_id = session.session_id.clone();
    record.state_blob = direct_session_to_blob(&session).map_err(map_direct_error)?;
    record.metadata_json = direct_session_metadata_json(&session).map_err(map_direct_error)?;
    record.updated_at = now_utc_like();
    let request = anp::direct_e2ee::direct_cipher_send_request(
        owner_did,
        target_did,
        operation_id,
        message_id,
        &body,
    )
    .map_err(map_direct_error)
    .and_then(direct_secure_request_method_params)?;
    Ok(EncryptedFollowUp {
        updated_record: record,
        expected_revision,
        request,
        operation_id: operation_id.to_owned(),
        message_id: message_id.to_owned(),
    })
}

fn encrypt_init_from_prekey(input: AsyncInitEncryptInput) -> crate::ImResult<EncryptedInit> {
    anp::direct_e2ee::verify_prekey_bundle(&input.prekey.bundle, &input.prekey.did_document)
        .map_err(map_direct_error)?;
    let recipient_static_public = anp::direct_e2ee::extract_x25519_public_key(
        &input.prekey.did_document,
        &input.prekey.bundle.static_key_agreement_id,
    )
    .map_err(map_direct_error)?;
    let recipient_signed_prekey_public = decode_public_key_b64u(
        &input.prekey.bundle.signed_prekey.public_key_b64u,
        "signed_prekey.public_key_b64u",
    )?;
    let (recipient_one_time_prekey_public, recipient_one_time_prekey_id) =
        if let Some(one_time_prekey) = &input.prekey.one_time_prekey {
            validate_one_time_prekey(one_time_prekey)?;
            (
                Some(decode_public_key_b64u(
                    &one_time_prekey.public_key_b64u,
                    "one_time_prekey.public_key_b64u",
                )?),
                Some(one_time_prekey.key_id.clone()),
            )
        } else {
            (None, None)
        };
    let metadata = DirectEnvelopeMetadata {
        sender_did: input.owner_did.clone(),
        recipient_did: input.target_did.clone(),
        message_id: input.message_id.clone(),
        profile: DIRECT_E2EE_PROFILE.to_owned(),
        security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
    };
    let (session, _, body) = DirectE2eeSession::initiate_session_with_static_dh(
        &metadata,
        &input.operation_id,
        &input.agreement_key_id,
        &input.static_to_signed_prekey_dh,
        &input.prekey.bundle,
        &recipient_static_public,
        &recipient_signed_prekey_public,
        recipient_one_time_prekey_public.as_ref(),
        recipient_one_time_prekey_id,
        &input.plaintext,
    )
    .map_err(map_direct_error)?;
    let now = now_utc_like();
    let record = DirectSessionRecord {
        owner_identity_id: input.owner_identity_id,
        owner_did: input.owner_did.clone(),
        peer_did: input.target_did.clone(),
        session_id: session.session_id.clone(),
        state_blob: direct_session_to_blob(&session).map_err(map_direct_error)?,
        metadata_json: direct_session_metadata_json(&session).map_err(map_direct_error)?,
        revision: 0,
        created_at: now.clone(),
        updated_at: now,
    };
    let request = anp::direct_e2ee::direct_init_send_request(
        &input.owner_did,
        &input.target_did,
        &input.operation_id,
        &input.message_id,
        &body,
    )
    .map_err(map_direct_error)
    .and_then(direct_secure_request_method_params)?;
    Ok(EncryptedInit {
        commit: DirectInitSendCommit {
            record,
            expected_peer_revision: None,
        },
        request,
    })
}

async fn async_init_local_material(
    client: &crate::core::ImClient,
) -> crate::ImResult<AsyncInitLocalMaterial> {
    let material = super::identity_material::agreement_material(client)?;
    Ok(AsyncInitLocalMaterial {
        agreement_key_id: material.agreement_key_id,
        identity_signer: material.identity_signer,
        identity_session: material.identity_session,
    })
}

async fn fetch_verified_prekey_bundle_async<M, D>(
    client: &crate::core::ImClient,
    message_transport: &mut M,
    directory_transport: &mut D,
    target_did: &str,
) -> crate::ImResult<Option<VerifiedPrekeyBundle>>
where
    M: AsyncAuthenticatedRpcTransport,
    D: AsyncRpcTransport,
{
    let did_document =
        resolve_target_did_document_async(client, directory_transport, target_did).await?;
    let target_service_did = anp::direct_e2ee::message_service_did_from_document(&did_document)
        .map_err(map_direct_error)?;
    let response = match fetch_prekey_bundle_response_async(
        client,
        message_transport,
        target_did,
        &target_service_did,
        true,
    )
    .await
    {
        Ok(response) => response,
        Err(err) if anp::direct_e2ee::should_retry_without_opk_message(&err.to_string()) => {
            fetch_prekey_bundle_response_async(
                client,
                message_transport,
                target_did,
                &target_service_did,
                false,
            )
            .await?
        }
        Err(err) => return Err(err),
    };
    let Some(bundle_value) = response.get("prekey_bundle").cloned() else {
        return Ok(None);
    };
    let bundle: PrekeyBundle =
        serde_json::from_value(bundle_value).map_err(|err| crate::ImError::Serialization {
            detail: format!("parse prekey_bundle: {err}"),
        })?;
    anp::direct_e2ee::verify_prekey_bundle(&bundle, &did_document).map_err(map_direct_error)?;
    let one_time_prekey = response
        .get("one_time_prekey")
        .cloned()
        .map(serde_json::from_value::<OneTimePrekey>)
        .transpose()
        .map_err(|err| crate::ImError::Serialization {
            detail: format!("parse one_time_prekey: {err}"),
        })?;
    if let Some(one_time_prekey) = &one_time_prekey {
        validate_one_time_prekey(one_time_prekey)?;
    }
    Ok(Some(VerifiedPrekeyBundle {
        did_document,
        bundle,
        one_time_prekey,
    }))
}

async fn fetch_prekey_bundle_response_async<T>(
    client: &crate::core::ImClient,
    transport: &mut T,
    target_did: &str,
    target_service_did: &str,
    require_opk: bool,
) -> crate::ImResult<serde_json::Map<String, Value>>
where
    T: AsyncAuthenticatedRpcTransport,
{
    let operation_id = format!("op-get-prekey-{}", operation_nonce_hex());
    let request = anp::direct_e2ee::prekey_bundle_get_request(
        client.did().as_str(),
        target_service_did,
        target_did,
        require_opk,
        &operation_id,
    );
    let request = direct_secure_request_method_params(request)?;
    <T as AsyncAuthenticatedRpcTransport>::authenticated_rpc(
        transport,
        super::send::MESSAGE_RPC_ENDPOINT,
        &request.method,
        Value::Object(request.params),
    )
    .await
    .and_then(super::send::object_result)
}

async fn resolve_target_did_document_async<T>(
    client: &crate::core::ImClient,
    directory_transport: &mut T,
    did: &str,
) -> crate::ImResult<Value>
where
    T: AsyncRpcTransport,
{
    if did == client.did().as_str() {
        return super::identity_material::local_did_document(client);
    }
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    match directory_transport
        .rpc(call.endpoint, call.method, call.params)
        .await
        .and_then(|raw| {
            super::send::did_document_from_resolve(raw).ok_or_else(|| {
                crate::ImError::PeerNotFound {
                    peer: did.to_owned(),
                }
            })
        }) {
        Ok(document) => Ok(document),
        Err(err) => match crate::internal::identity_document_cache::load_local_did_document_async(
            &client.core_inner().sdk_paths().identities,
            did,
        )
        .await
        {
            Ok(Some(document)) => Ok(document),
            Ok(None) | Err(_) => Err(err),
        },
    }
}

fn is_established_record(record: &DirectSessionRecord) -> crate::ImResult<bool> {
    let session = direct_session_from_blob(&record.state_blob).map_err(map_direct_error)?;
    Ok(session.status == SESSION_STATUS_ESTABLISHED)
}

fn is_pending_confirmation_record(record: &DirectSessionRecord) -> crate::ImResult<bool> {
    let session = direct_session_from_blob(&record.state_blob).map_err(map_direct_error)?;
    Ok(session.status == SESSION_STATUS_PENDING_CONFIRMATION)
}

fn queued_pending_confirmation_result(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
    target_did: String,
    target_handle: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    text: &str,
    kind: crate::messages::MessageKind,
    operation_id: String,
    message_id: String,
) -> crate::ImResult<AsyncDirectSecureSendOutcome> {
    let record = super::send::pending_confirmation_outbox_record(
        client,
        &target_did,
        &kind,
        text,
        "pending-confirmation",
    );
    let outbox_id = record.outbox_id.clone();
    let mut sdk_result = super::send::queued_sdk_result(
        &outbox_id,
        client.did().clone(),
        peer,
        &target_did,
        text,
        kind.clone(),
        operation_id,
        message_id,
        Vec::new(),
    )?;
    crate::messages::normalize_direct_send_result_for_peer_scope(
        &mut sdk_result,
        peer_scope.as_ref(),
        target_handle.as_deref(),
        Some(target_did.as_str()),
    )?;
    Ok(AsyncDirectSecureSendOutcome::Sent(
        DirectSecureTextSendResult {
            sdk_result,
            queued_outbox_id: Some(outbox_id),
            target_did,
            target_handle,
            peer_scope,
            text: text.to_owned(),
            kind,
            raw: None,
            local_effect: DirectSecureLocalEffect::QueueOutbox(record),
        },
    ))
}

fn decode_public_key_b64u(value: &str, field: &str) -> crate::ImResult<[u8; 32]> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| crate::ImError::invalid_input(Some(field.to_owned()), "invalid base64url"))?;
    bytes.try_into().map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_owned()), "expected 32-byte public key")
    })
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

fn validate_one_time_prekey(prekey: &OneTimePrekey) -> crate::ImResult<()> {
    if prekey.key_id.trim().is_empty() {
        return Err(crate::ImError::Serialization {
            detail: "missing field: one_time_prekey.key_id".to_owned(),
        });
    }
    if prekey.public_key_b64u.trim().is_empty() {
        return Err(crate::ImError::Serialization {
            detail: "missing field: one_time_prekey.public_key_b64u".to_owned(),
        });
    }
    Ok(())
}

fn expected_x25519_private_key() -> crate::ImError {
    crate::ImError::Serialization {
        detail: "expected X25519 private key".to_owned(),
    }
}

fn operation_nonce_hex() -> String {
    use rand::RngCore;

    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn operation_id_for_request(
    request: &crate::messages::SendMessageRequest,
) -> crate::ImResult<String> {
    let operation_id = request
        .delivery
        .idempotency_key
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            request
                .client_message_id
                .as_ref()
                .map(|message_id| message_id.as_str().to_owned())
        })
        .unwrap_or_else(super::send::generate_message_id);
    Ok(operation_id)
}

fn message_id_for_request(
    request: &crate::messages::SendMessageRequest,
    operation_id: &str,
) -> String {
    request
        .client_message_id
        .as_ref()
        .map(|message_id| message_id.as_str().to_owned())
        .unwrap_or_else(|| operation_id.to_owned())
}

fn map_string_value(raw: &serde_json::Map<String, Value>, key: &str) -> String {
    match raw.get(key) {
        Some(Value::String(value)) => value.trim().to_owned(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn now_utc_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}

#[cfg(test)]
mod tests;
