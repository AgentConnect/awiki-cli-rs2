use anp::direct_e2ee::models::{SESSION_STATUS_ESTABLISHED, SESSION_STATUS_PENDING_CONFIRMATION};
use anp::direct_e2ee::{
    ApplicationPlaintext, DirectE2eeSession, DirectEnvelopeMetadata, OneTimePrekey, PrekeyBundle,
};
use anp::PrivateKeyMaterial;
use serde_json::Value;

use crate::internal::auth::session::AsyncSessionProvider;
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport};

use super::client::{direct_secure_request_method_params, map_direct_error};
use super::send::{DirectSecureLocalEffect, DirectSecureTextSendResult};
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
        let sdk_result = super::send::sdk_result_from_secure_result(
            &raw,
            self.client.did().clone(),
            peer,
            &target_did,
            text,
            kind.clone(),
            Vec::new(),
        )?;
        Ok(AsyncDirectSecureSendOutcome::Sent(
            DirectSecureTextSendResult {
                sdk_result,
                queued_outbox_id: None,
                target_did,
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
        let sdk_result = super::send::sdk_result_from_secure_result(
            &raw,
            self.client.did().clone(),
            peer,
            &target_did,
            text,
            kind.clone(),
            Vec::new(),
        )?;
        Ok(AsyncDirectSecureSendOutcome::Sent(
            DirectSecureTextSendResult {
                sdk_result,
                queued_outbox_id: None,
                target_did,
                text: text.to_owned(),
                kind,
                raw: Some(Value::Object(raw)),
                local_effect: DirectSecureLocalEffect::PersistOutgoing,
            },
        ))
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
    agreement_private_pem: String,
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
    agreement_private_pem: String,
    target_did: String,
    prekey: VerifiedPrekeyBundle,
    operation_id: String,
    message_id: String,
    text: String,
}

#[allow(clippy::too_many_arguments)]
async fn send_init_if_ready<M, D>(
    client: &crate::core::ImClient,
    db: crate::internal::local_state::actor::LocalStateDb,
    mut message_transport: M,
    mut directory_transport: D,
    peer: crate::ids::PeerRef,
    target_did: String,
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
    let text_for_crypto = text.to_owned();
    let operation_id_for_crypto = operation_id.clone();
    let message_id_for_crypto = message_id.clone();
    let encrypted = crate::internal::runtime::worker::run_blocking(move || {
        encrypt_init_from_prekey(AsyncInitEncryptInput {
            owner_identity_id,
            owner_did,
            agreement_key_id: local_material.agreement_key_id,
            agreement_private_pem: local_material.agreement_private_pem,
            target_did: target_did_for_crypto,
            prekey,
            operation_id: operation_id_for_crypto,
            message_id: message_id_for_crypto,
            text: text_for_crypto,
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
    let sdk_result = super::send::sdk_result_from_secure_result(
        &raw,
        client.did().clone(),
        peer,
        &target_did,
        text,
        kind.clone(),
        Vec::new(),
    )?;
    Ok(AsyncDirectSecureSendOutcome::Sent(
        DirectSecureTextSendResult {
            sdk_result,
            queued_outbox_id: None,
            target_did,
            text: text.to_owned(),
            kind,
            raw: Some(Value::Object(raw)),
            local_effect: DirectSecureLocalEffect::PersistOutgoing,
        },
    ))
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
    let PrivateKeyMaterial::X25519(agreement_private) =
        PrivateKeyMaterial::from_pem(&input.agreement_private_pem).map_err(|err| {
            crate::ImError::Serialization {
                detail: format!("parse direct E2EE agreement private key: {err}"),
            }
        })?
    else {
        return Err(expected_x25519_private_key());
    };
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
    let (session, _, body) = DirectE2eeSession::initiate_session_with_opk(
        &metadata,
        &input.operation_id,
        &input.agreement_key_id,
        &agreement_private,
        &input.prekey.bundle,
        &recipient_static_public,
        &recipient_signed_prekey_public,
        recipient_one_time_prekey_public.as_ref(),
        recipient_one_time_prekey_id,
        &ApplicationPlaintext::new_text("text/plain", &input.text),
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
    let runtime = client.runtime();
    let agreement_private_pem = tokio::fs::read_to_string(&runtime.e2ee_agreement_private_key_path)
        .await
        .map_err(|err| crate::ImError::CredentialFileUnreadable {
            path_kind: "e2ee_agreement_private_key".to_owned(),
            detail: err.to_string(),
        })?;
    Ok(AsyncInitLocalMaterial {
        agreement_key_id: format!("{}#key-3", client.did().as_str()),
        agreement_private_pem,
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
        let raw = tokio::fs::read(&client.runtime().did_document_path)
            .await
            .map_err(|err| crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_owned(),
                detail: err.to_string(),
            })?;
        return serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        });
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
    let sdk_result = super::send::queued_sdk_result(
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
    Ok(AsyncDirectSecureSendOutcome::Sent(
        DirectSecureTextSendResult {
            sdk_result,
            queued_outbox_id: Some(outbox_id),
            target_did,
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
mod tests {
    use std::sync::{Arc, Mutex};

    use anp::direct_e2ee::models::{MTI_DIRECT_E2EE_SUITE, SESSION_STATUS_PENDING_CONFIRMATION};
    use anp::direct_e2ee::{DirectE2eeSession, DirectEnvelopeMetadata};
    use serde_json::json;

    use super::*;

    #[test]
    fn encrypt_follow_up_from_record_mutates_session_and_builds_cipher_request() {
        let record = DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: "session-1".to_owned(),
            state_blob: direct_session_to_blob(&established_session()).unwrap(),
            metadata_json: "{}".to_owned(),
            revision: 7,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        };

        let encrypted = encrypt_follow_up_from_record(
            record,
            "did:example:alice",
            "did:example:bob",
            "msg-async",
            "msg-async",
            "async secret",
        )
        .unwrap();

        assert_eq!(encrypted.expected_revision, 7);
        assert_eq!(encrypted.updated_record.revision, 7);
        let updated = direct_session_from_blob(&encrypted.updated_record.state_blob).unwrap();
        assert_eq!(updated.send_n, 1);
        assert_eq!(encrypted.request.method, "direct.send");
        assert_eq!(
            encrypted.request.params["meta"]["content_type"],
            "application/anp-direct-cipher+json"
        );
        assert_eq!(
            encrypted.request.params["meta"]["operation_id"],
            "msg-async"
        );
        assert!(!encrypted
            .request
            .params
            .values()
            .any(|value| value.to_string().contains("async secret")));
    }

    #[test]
    fn encrypt_follow_up_from_record_rejects_pending_confirmation_session() {
        let mut session = established_session();
        session.status = SESSION_STATUS_PENDING_CONFIRMATION.to_owned();
        let record = DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: "session-1".to_owned(),
            state_blob: direct_session_to_blob(&session).unwrap(),
            metadata_json: "{}".to_owned(),
            revision: 0,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        };

        let err = encrypt_follow_up_from_record(
            record,
            "did:example:alice",
            "did:example:bob",
            "msg-async",
            "msg-async",
            "async secret",
        )
        .unwrap_err();

        assert_eq!(
            err,
            crate::ImError::LocalStateUnavailable {
                detail: "direct E2EE session is not established".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn async_direct_secure_sender_uses_actor_cas_and_async_transport_for_follow_up() {
        let alice = TestIdentity::did_example("did:example:alice");
        let fixture = Fixture::new(&alice);
        let client = fixture.client();
        let session = established_session();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: session.session_id.clone(),
                state_blob: direct_session_to_blob(&session).unwrap(),
                metadata_json: direct_session_metadata_json(&session).unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::<RecordedAsyncCall>::new()));

        let outcome = AsyncDirectSecureTextSender::new(
            &client,
            ReadyAsyncSessionProvider,
            RecordingAsyncTransport {
                calls: Arc::clone(&calls),
                prekey_response: None,
            },
            NoopDirectoryTransport,
        )
        .send_follow_up_if_ready(super::super::send::DirectSecureTextSend {
            request: secure_direct_request("did:example:bob", "actor async secret"),
            resolved_target_did: None,
            local_persistence: super::super::send::DirectSecureLocalPersistence::Deferred,
        })
        .await
        .unwrap();

        let AsyncDirectSecureSendOutcome::Sent(result) = outcome else {
            panic!("established session should use async follow-up path");
        };
        assert_eq!(result.sdk_result.message.id.as_str(), "msg-async-direct");
        assert_eq!(result.target_did, "did:example:bob");
        assert!(matches!(
            result.local_effect,
            DirectSecureLocalEffect::PersistOutgoing
        ));
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, super::super::send::MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "direct.send");
        assert_eq!(
            calls[0].params["meta"]["content_type"],
            "application/anp-direct-cipher+json"
        );
        assert!(!calls[0].params.to_string().contains("actor async secret"));
        let saved = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        let saved_session = direct_session_from_blob(&saved.state_blob).unwrap();
        assert_eq!(saved_session.send_n, session.send_n + 1);
    }

    #[tokio::test]
    async fn async_direct_secure_sender_initializes_session_via_actor_and_async_transport() {
        let alice = TestIdentity::new("alice.async-init.example", "alice");
        let bob = TestIdentity::new("bob.async-init.example", "bob");
        let fixture = Fixture::new(&alice);
        let client = fixture.client();
        let db = client.core_inner().local_state_db().await.unwrap();
        let bob_bundle = test_prekey_bundle(&bob);
        let calls = Arc::new(Mutex::new(Vec::<RecordedAsyncCall>::new()));

        let outcome = AsyncDirectSecureTextSender::new(
            &client,
            ReadyAsyncSessionProvider,
            RecordingAsyncTransport {
                calls: Arc::clone(&calls),
                prekey_response: Some(json!({
                    "prekey_bundle": bob_bundle.bundle,
                    "one_time_prekey": bob_bundle.one_time_prekey,
                })),
            },
            StaticDirectoryTransport {
                did: bob.did.clone(),
                document: bob.document.clone(),
            },
        )
        .send_async_if_ready(super::super::send::DirectSecureTextSend {
            request: secure_direct_request(&bob.did, "async init secret"),
            resolved_target_did: None,
            local_persistence: super::super::send::DirectSecureLocalPersistence::Deferred,
        })
        .await
        .unwrap();

        let AsyncDirectSecureSendOutcome::Sent(result) = outcome else {
            panic!("missing session should use async init path when prekey material is available");
        };
        assert_eq!(result.sdk_result.message.id.as_str(), "msg-async-direct");
        assert_eq!(result.target_did, bob.did);
        assert!(matches!(
            result.local_effect,
            DirectSecureLocalEffect::PersistOutgoing
        ));
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].method, "direct.e2ee.get_prekey_bundle");
        assert_eq!(calls[1].method, "direct.send");
        assert_eq!(
            calls[1].params["meta"]["content_type"],
            "application/anp-direct-init+json"
        );
        assert!(!calls[1].params.to_string().contains("async init secret"));
        let saved = db
            .get_direct_secure_session("alice-id", &bob.did)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 0);
        let saved_session = direct_session_from_blob(&saved.state_blob).unwrap();
        assert_eq!(saved_session.session_id, saved.session_id);
        assert_eq!(saved_session.status, SESSION_STATUS_PENDING_CONFIRMATION);
        assert_eq!(saved_session.send_n, 1);
    }

    #[tokio::test]
    async fn async_direct_secure_sender_queues_when_session_pending_confirmation() {
        let alice = TestIdentity::did_example("did:example:alice");
        let fixture = Fixture::new(&alice);
        let client = fixture.client();
        let mut session = established_session();
        session.status = SESSION_STATUS_PENDING_CONFIRMATION.to_owned();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: session.session_id.clone(),
                state_blob: direct_session_to_blob(&session).unwrap(),
                metadata_json: direct_session_metadata_json(&session).unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::<RecordedAsyncCall>::new()));

        let outcome = AsyncDirectSecureTextSender::new(
            &client,
            ReadyAsyncSessionProvider,
            RecordingAsyncTransport {
                calls: Arc::clone(&calls),
                prekey_response: None,
            },
            NoopDirectoryTransport,
        )
        .send_async_if_ready(super::super::send::DirectSecureTextSend {
            request: secure_direct_request("did:example:bob", "queued async secret"),
            resolved_target_did: None,
            local_persistence: super::super::send::DirectSecureLocalPersistence::Deferred,
        })
        .await
        .unwrap();

        let AsyncDirectSecureSendOutcome::Sent(result) = outcome else {
            panic!("pending-confirmation session should produce queued outbox effect");
        };
        assert_eq!(
            result.queued_outbox_id.as_deref().unwrap(),
            result
                .sdk_result
                .message
                .metadata
                .attributes
                .iter()
                .find(|attribute| attribute.key == "secure_outbox_id")
                .map(|attribute| attribute.value.as_str())
                .unwrap()
        );
        assert!(matches!(
            result.local_effect,
            DirectSecureLocalEffect::QueueOutbox(_)
        ));
        assert_eq!(
            result.sdk_result.message.metadata.delivery_state.as_deref(),
            Some("queued")
        );
        assert_eq!(calls.lock().unwrap().len(), 0);
        let saved = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 0);
    }

    fn established_session() -> anp::direct_e2ee::DirectSessionState {
        let alice_static = generated_x25519_private();
        let bob_static = generated_x25519_private();
        let bob_spk = generated_x25519_private();
        let anp::PrivateKeyMaterial::X25519(alice_static_key) = &alice_static else {
            panic!("expected X25519 private key");
        };
        let anp::PrivateKeyMaterial::X25519(bob_static_key) = &bob_static else {
            panic!("expected X25519 private key");
        };
        let anp::PrivateKeyMaterial::X25519(bob_spk_key) = &bob_spk else {
            panic!("expected X25519 private key");
        };
        let alice_did = "did:example:alice";
        let bob_did = "did:example:bob";
        let bob_bundle = anp::direct_e2ee::PrekeyBundle {
            bundle_id: "bundle-bob".to_owned(),
            owner_did: bob_did.to_owned(),
            suite: MTI_DIRECT_E2EE_SUITE.to_owned(),
            static_key_agreement_id: format!("{bob_did}#key-3"),
            signed_prekey: anp::direct_e2ee::SignedPrekey {
                key_id: "spk-bob".to_owned(),
                public_key_b64u: base64url(&x25519_public(&bob_spk)),
                expires_at: "2030-01-01T00:00:00Z".to_owned(),
            },
            proof: json!({}),
        };
        let init_metadata = DirectEnvelopeMetadata {
            sender_did: alice_did.to_owned(),
            recipient_did: bob_did.to_owned(),
            message_id: "msg-init".to_owned(),
            profile: DIRECT_E2EE_PROFILE.to_owned(),
            security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
        };
        let (mut alice_session, _, init_body) = DirectE2eeSession::initiate_session(
            &init_metadata,
            "msg-init",
            &format!("{alice_did}#key-3"),
            alice_static_key,
            &bob_bundle,
            &x25519_public(&bob_static),
            &x25519_public(&bob_spk),
            &ApplicationPlaintext::new_text("text/plain", "init"),
        )
        .unwrap();
        let (mut bob_session, _) = DirectE2eeSession::accept_incoming_init(
            &init_metadata,
            &format!("{bob_did}#key-3"),
            bob_static_key,
            bob_spk_key,
            &x25519_public(&alice_static),
            &init_body,
        )
        .unwrap();
        let reply_metadata = DirectEnvelopeMetadata {
            sender_did: bob_did.to_owned(),
            recipient_did: alice_did.to_owned(),
            message_id: "msg-reply".to_owned(),
            profile: DIRECT_E2EE_PROFILE.to_owned(),
            security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
        };
        let (_, reply_body) = DirectE2eeSession::encrypt_follow_up(
            &mut bob_session,
            &reply_metadata,
            "msg-reply",
            &ApplicationPlaintext::new_text("text/plain", "reply"),
        )
        .unwrap();
        DirectE2eeSession::decrypt_follow_up(
            &mut alice_session,
            &reply_metadata,
            &reply_body,
            "text/plain",
        )
        .unwrap();
        assert_eq!(alice_session.status, SESSION_STATUS_ESTABLISHED);
        alice_session
    }

    fn generated_x25519_private() -> anp::PrivateKeyMaterial {
        let bundle = anp::authentication::create_did_wba_document(
            "keys.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        bundle.load_private_key("key-3").unwrap()
    }

    fn x25519_public(private: &anp::PrivateKeyMaterial) -> [u8; 32] {
        match private.public_key() {
            anp::PublicKeyMaterial::X25519(bytes) => bytes,
            _ => panic!("expected X25519 public key"),
        }
    }

    fn base64url(bytes: &[u8]) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        URL_SAFE_NO_PAD.encode(bytes)
    }

    struct ReadyAsyncSessionProvider;

    impl crate::internal::auth::session::AsyncSessionProvider for ReadyAsyncSessionProvider {
        async fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice").unwrap(),
                scope,
                expires_at: None,
                refreshed: false,
            })
        }

        async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            Ok(crate::auth::SessionUpdate {
                subject: crate::ids::Did::parse("did:example:alice").unwrap(),
                previous_expires_at: None,
                new_expires_at: None,
                refreshed: true,
            })
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            Ok(crate::auth::AuthStatus {
                subject: crate::ids::Did::parse("did:example:alice").unwrap(),
                has_session: true,
                expires_at: None,
                needs_refresh: false,
                warnings: Vec::new(),
            })
        }
    }

    #[derive(Debug, Clone)]
    struct RecordedAsyncCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct RecordingAsyncTransport {
        calls: Arc<Mutex<Vec<RecordedAsyncCall>>>,
        prekey_response: Option<Value>,
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingAsyncTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.lock().unwrap().push(RecordedAsyncCall {
                endpoint: endpoint.to_owned(),
                method: method.to_owned(),
                params,
            });
            if method == "direct.e2ee.get_prekey_bundle" {
                return self.prekey_response.clone().ok_or_else(|| {
                    crate::ImError::TransportUnavailable {
                        detail: "missing test prekey response".to_owned(),
                    }
                });
            }
            Ok(json!({
                "accepted": true,
                "message_id": "msg-async-direct",
                "operation_id": "msg-async-direct",
                "target_did": "did:example:bob",
                "accepted_at": "2026-05-24T00:00:00Z",
                "delivery_state": "accepted",
                "server_seq": 42,
            }))
        }
    }

    struct NoopDirectoryTransport;

    impl crate::internal::transport::AsyncRpcTransport for NoopDirectoryTransport {
        async fn rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            Err(crate::ImError::PeerNotFound {
                peer: "noop".to_owned(),
            })
        }
    }

    struct StaticDirectoryTransport {
        did: String,
        document: Value,
    }

    impl crate::internal::transport::AsyncRpcTransport for StaticDirectoryTransport {
        async fn rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            Ok(json!({
                "did_document": self.document,
                "id": self.did,
            }))
        }
    }

    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new(identity: &TestIdentity) -> Self {
            let root = tempfile::tempdir().unwrap();
            let identity_root = root.path().join("identities");
            let identity_dir = identity_root.join("alice");
            std::fs::create_dir_all(&identity_dir).unwrap();
            std::fs::create_dir_all(root.path().join("local")).unwrap();
            std::fs::write(identity_root.join("default"), "alice\n").unwrap();
            std::fs::write(
                identity_root.join("registry.json"),
                json!({
                    "default_identity": "alice",
                    "identities": [{
                        "id": "alice-id",
                        "did": identity.did,
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(
                identity_dir.join("did.json"),
                serde_json::to_vec_pretty(&identity.document).unwrap(),
            )
            .unwrap();
            std::fs::write(
                identity_dir.join("private.key"),
                &identity.signing_private_pem,
            )
            .unwrap();
            std::fs::write(
                identity_dir.join("e2ee-agreement-private.pem"),
                &identity.agreement_private_pem,
            )
            .unwrap();
            std::fs::write(
                identity_dir.join("auth.json"),
                r#"{"jwt_token":"test-token"}"#,
            )
            .unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
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
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.path().join("identities"),
                        registry_path: self.root.path().join("identities").join("registry.json"),
                        default_identity_path: Some(
                            self.root.path().join("identities").join("default"),
                        ),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.root.path().join("local").join("im.sqlite"),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.path().join("cache"),
                        temp_dir: self.root.path().join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }
    }

    struct TestIdentity {
        did: String,
        document: Value,
        signing_private_pem: String,
        agreement_private_pem: String,
    }

    impl TestIdentity {
        fn did_example(did: &str) -> Self {
            let private = generated_x25519_private();
            Self {
                did: did.to_owned(),
                document: json!({
                    "id": did,
                    "verificationMethod": [],
                }),
                signing_private_pem: private.to_pem(),
                agreement_private_pem: private.to_pem(),
            }
        }

        fn new(domain: &str, label: &str) -> Self {
            let service = anp::authentication::build_agent_message_service_with_options(
                "#message",
                format!("https://{domain}/anp-im/rpc"),
                anp::authentication::AnpMessageServiceOptions::default()
                    .with_service_did(format!("did:wba:{domain}")),
            );
            let bundle = anp::authentication::create_did_wba_document(
                domain,
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["agents".to_owned(), label.to_owned()],
                    domain: Some(domain.to_owned()),
                    challenge: Some(format!("secure-direct-async-send-{label}")),
                    services: vec![service],
                    did_profile: anp::authentication::DidProfile::E1,
                    ..Default::default()
                },
            )
            .unwrap();
            let did = bundle.did().unwrap().to_owned();
            Self {
                did,
                document: bundle.did_document.clone(),
                signing_private_pem: bundle.private_key_pem("key-1").unwrap().to_owned(),
                agreement_private_pem: bundle.private_key_pem("key-3").unwrap().to_owned(),
            }
        }
    }

    struct TestPrekeyBundle {
        bundle: anp::direct_e2ee::PrekeyBundle,
        one_time_prekey: anp::direct_e2ee::OneTimePrekey,
    }

    fn test_prekey_bundle(identity: &TestIdentity) -> TestPrekeyBundle {
        let signing_private =
            anp::PrivateKeyMaterial::from_pem(&identity.signing_private_pem).unwrap();
        let signed_prekey_private = generated_x25519_private();
        let one_time_private = generated_x25519_private();
        let signed_prekey = anp::direct_e2ee::SignedPrekey {
            key_id: "spk-bob".to_owned(),
            public_key_b64u: base64url(&x25519_public(&signed_prekey_private)),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
        };
        let bundle = anp::direct_e2ee::build_prekey_bundle(
            "bundle-bob",
            &identity.did,
            &format!("{}#key-3", identity.did),
            signed_prekey,
            &signing_private,
            &format!("{}#key-1", identity.did),
            None,
        )
        .unwrap();
        let one_time_prekey = anp::direct_e2ee::OneTimePrekey {
            key_id: "opk-bob".to_owned(),
            public_key_b64u: base64url(&x25519_public(&one_time_private)),
        };
        TestPrekeyBundle {
            bundle,
            one_time_prekey,
        }
    }

    fn secure_direct_request(target: &str, text: &str) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse(target, "").unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: text.to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::SecureDirect,
            client_message_id: Some(crate::ids::MessageId::parse("msg-async-direct").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("msg-async-direct".to_owned()),
                wait_for_final_acceptance: true,
            },
        }
    }
}
