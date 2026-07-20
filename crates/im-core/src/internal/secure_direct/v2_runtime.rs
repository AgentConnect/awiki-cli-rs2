//! Minimal product runtime for established P5 v2 exact-device sessions.
//!
//! This layer is reusable by text/JSON callers. It owns no AWiki control
//! schema: callers provide a validated P5 application plaintext and may attach
//! same-domain transport metadata that is persisted beside (never inside) the
//! exact retry ciphertext.

use anp::direct_e2ee::{
    direct_send_request_v2, parse_direct_send_result_v2, V2ApplicationPlaintext, V2DirectBody,
    V2DirectCipherBody, V2DirectE2eeSession, V2DirectInitBody, V2DirectMetadata,
    V2DirectSendResult, V2DirectSessionState, V2GetPrekeyBundleResult, V2PendingOutboundRecord,
    V2SecretJsonPayload, V2SessionBinding, V2Target, CONTENT_TYPE_DIRECT_CIPHER_V2,
    CONTENT_TYPE_DIRECT_INIT_V2, DIRECT_E2EE_PROFILE_V2, DIRECT_E2EE_SECURITY_PROFILE,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest as _, Sha256};

use super::v2_store::{
    SqliteV2DirectStateStore, V2InboundCommit, V2PrivateOutboundSidecar, V2PrivateOutboundStatus,
    V2SessionExpectation,
};

pub(crate) const SESSION_ESTABLISH_SYSTEM_TYPE: &str = "awiki.device.session-establish.v1";
pub(crate) const SESSION_ESTABLISHED_SYSTEM_TYPE: &str = "awiki.device.session-established.v1";
pub(crate) const SESSION_INIT_OPERATION_PREFIX: &str = "p5-v2-session-init:";
pub(crate) const SESSION_REPLY_OPERATION_PREFIX: &str = "p5-v2-session-reply:";
pub(crate) const SESSION_ESTABLISHMENT_PENDING: &str = "p5-v2-session-establishment-pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V2SessionControlKind {
    Establish,
    Established,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct V2SessionControlPayload {
    system_type: String,
    #[serde(default)]
    init_message_id: Option<String>,
}

pub(crate) fn session_establish_plaintext() -> V2ApplicationPlaintext {
    session_control_plaintext(
        serde_json::json!({"system_type": SESSION_ESTABLISH_SYSTEM_TYPE}),
        None,
    )
}

pub(crate) fn session_established_plaintext(
    init_message_id: &str,
) -> crate::ImResult<V2ApplicationPlaintext> {
    require_operation_id(init_message_id)?;
    Ok(session_control_plaintext(
        serde_json::json!({
            "init_message_id": init_message_id,
            "system_type": SESSION_ESTABLISHED_SYSTEM_TYPE
        }),
        Some(init_message_id.to_owned()),
    ))
}

pub(crate) fn session_init_operation_id(seed: &str) -> crate::ImResult<String> {
    session_control_operation_id(SESSION_INIT_OPERATION_PREFIX, seed)
}

pub(crate) fn session_reply_operation_id(init_message_id: &str) -> crate::ImResult<String> {
    session_control_operation_id(SESSION_REPLY_OPERATION_PREFIX, init_message_id)
}

pub(crate) fn is_session_init_operation_id(value: &str) -> bool {
    is_session_control_operation_id(SESSION_INIT_OPERATION_PREFIX, value)
}

pub(crate) fn is_session_reply_operation_id(value: &str) -> bool {
    is_session_control_operation_id(SESSION_REPLY_OPERATION_PREFIX, value)
}

pub(crate) fn classify_session_control(
    plaintext: &V2ApplicationPlaintext,
) -> crate::ImResult<Option<V2SessionControlKind>> {
    if plaintext.application_content_type != "application/json" {
        return Ok(None);
    }
    let Some(payload) = plaintext.payload.as_ref() else {
        return Ok(None);
    };
    let Some(system_type) = payload
        .get("system_type")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(None);
    };
    if !matches!(
        system_type,
        SESSION_ESTABLISH_SYSTEM_TYPE | SESSION_ESTABLISHED_SYSTEM_TYPE
    ) {
        return Ok(None);
    }
    let control: V2SessionControlPayload =
        serde_json::from_value(payload.clone()).map_err(serialization_error)?;
    match control.system_type.as_str() {
        SESSION_ESTABLISH_SYSTEM_TYPE if control.init_message_id.is_none() => {
            Ok(Some(V2SessionControlKind::Establish))
        }
        SESSION_ESTABLISHED_SYSTEM_TYPE => {
            let init_message_id = control
                .init_message_id
                .as_deref()
                .ok_or(crate::ImError::PermissionDenied)?;
            require_operation_id(init_message_id)?;
            if plaintext.reply_to_message_id.as_deref() != Some(init_message_id) {
                return Err(crate::ImError::PermissionDenied);
            }
            Ok(Some(V2SessionControlKind::Established))
        }
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn session_control_plaintext(
    payload: serde_json::Value,
    reply_to_message_id: Option<String>,
) -> V2ApplicationPlaintext {
    V2ApplicationPlaintext {
        application_content_type: "application/json".to_owned(),
        logical_message_id: None,
        conversation_id: None,
        reply_to_message_id,
        annotations: None,
        text: None,
        payload: Some(payload),
        payload_b64u: None,
    }
}

pub(crate) struct V2EstablishedDirectRuntime<'a, 'connection> {
    store: &'a SqliteV2DirectStateStore<'connection>,
}

pub(crate) enum V2RootControlSessionReadiness {
    Ready,
    Pending(PreparedV2Outbound),
    Absent,
}

impl<'a, 'connection> V2EstablishedDirectRuntime<'a, 'connection> {
    pub(crate) fn new(store: &'a SqliteV2DirectStateStore<'connection>) -> Self {
        Self { store }
    }

    pub(crate) fn has_established_session(
        &self,
        binding: &V2SessionBinding,
    ) -> crate::ImResult<bool> {
        Ok(self.store.select_established_session(binding)?.is_some())
    }

    /// Root-control may use only the same selected session that a subsequent
    /// private Cipher will advance. A retained outbound Init for that session
    /// must be replayed byte-for-byte until its authenticated reply retires it.
    /// Ordinary Direct callers intentionally keep using
    /// `has_established_session` and are unaffected by this stricter gate.
    pub(crate) fn root_control_session_readiness(
        &self,
        binding: &V2SessionBinding,
    ) -> crate::ImResult<V2RootControlSessionReadiness> {
        let selected = match self.store.select_established_session(binding)? {
            Some(selected) => selected,
            None => match self.store.select_pending_confirmation_session(binding)? {
                Some(selected) => selected,
                None => return Ok(V2RootControlSessionReadiness::Absent),
            },
        };
        if selected.state.disabled {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(operation_id) = self
            .store
            .select_pending_init_operation(binding, &selected.state.session_id)?
        {
            let prepared = self
                .resume_outbound(binding, &operation_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            prepared.init_body()?;
            return Ok(V2RootControlSessionReadiness::Pending(prepared));
        }
        if selected.state.status == anp::direct_e2ee::V2_SESSION_STATUS_ESTABLISHED {
            Ok(V2RootControlSessionReadiness::Ready)
        } else {
            Err(crate::ImError::PermissionDenied)
        }
    }

    pub(crate) fn complete_session_init(
        &self,
        binding: &V2SessionBinding,
        init_message_id: &str,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        self.store
            .complete_session_init(binding, init_message_id, session_id)
    }

    pub(crate) fn complete_session_init_for_session(
        &self,
        binding: &V2SessionBinding,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        self.store
            .complete_session_init_for_session(binding, session_id)
    }

    /// Encrypts and persists a follow-up before it can be sent. Repeating the
    /// same operation returns the byte-identical pending ciphertext.
    pub(crate) fn prepare_outbound(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2ApplicationPlaintext,
        now: &str,
    ) -> crate::ImResult<PreparedV2Outbound> {
        self.prepare_outbound_inner(binding, operation_id, plaintext, now, None)
    }

    /// Creates and durably persists the first standard P5 v2 Init for an
    /// exact device pair. The caller must verify the fetched bundle against
    /// the current target DID document before calling this method.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_session_init(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2ApplicationPlaintext,
        local_static_private: &x25519_dalek::StaticSecret,
        recipient: &V2GetPrekeyBundleResult,
        recipient_static_public: &[u8; 32],
        now: &str,
    ) -> crate::ImResult<PreparedV2Outbound> {
        binding.validate().map_err(v2_error)?;
        plaintext.validate().map_err(v2_error)?;
        recipient.validate().map_err(v2_error)?;
        require_operation_id(operation_id)?;
        if recipient.target_did != binding.peer_did
            || recipient.target_device_id != binding.peer_device_id
            || recipient.prekey_bundle.static_key_agreement_id != binding.peer_e2ee_key_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(existing) = self.store.load_pending(binding, operation_id)? {
            if self
                .store
                .load_private_outbound_sidecar(binding, operation_id)?
                .is_some()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let session = self
                .store
                .load_session(binding, &existing.session_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            if !self
                .store
                .session_is_enabled(binding, &existing.session_id)?
                || session.state.disabled
                || session.state.status != anp::direct_e2ee::V2_SESSION_STATUS_PENDING_CONFIRMATION
            {
                return Err(crate::ImError::PermissionDenied);
            }
            return prepared_from_pending(existing, None);
        }
        if self.store.select_established_session(binding)?.is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        let metadata =
            outbound_metadata_for_content_type(binding, operation_id, CONTENT_TYPE_DIRECT_INIT_V2);
        let (state, pending, body) = V2DirectE2eeSession::initiate_session(
            binding,
            &metadata,
            local_static_private,
            &recipient.prekey_bundle,
            recipient_static_public,
            recipient.one_time_prekey.as_ref(),
            plaintext,
        )
        .map_err(v2_error)?;
        self.store
            .commit_outbound(&state, &pending, V2SessionExpectation::Absent, now)?;
        Ok(PreparedV2Outbound {
            binding: binding.clone(),
            metadata,
            body: V2DirectBody::Init(body),
            sidecar: None,
        })
    }

    /// Same as `prepare_outbound`, but atomically persists a same-domain
    /// sidecar needed to resume a private control request after restart.
    pub(crate) fn prepare_private_outbound(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2ApplicationPlaintext,
        now: &str,
        sidecar: V2PrivateOutboundSidecar,
    ) -> crate::ImResult<PreparedV2Outbound> {
        self.prepare_outbound_inner(binding, operation_id, plaintext, now, Some(sidecar))
    }

    pub(crate) fn prepare_private_outbound_secret_json(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2SecretJsonPayload,
        now: &str,
        sidecar: V2PrivateOutboundSidecar,
    ) -> crate::ImResult<PreparedV2Outbound> {
        binding.validate().map_err(v2_error)?;
        require_operation_id(operation_id)?;
        if self
            .store
            .private_outbound_status(operation_id, now)?
            .is_some_and(|status| !status.retryable)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(existing) = self.store.load_pending(binding, operation_id)? {
            let existing_sidecar = self
                .store
                .load_private_outbound_sidecar(binding, operation_id)?;
            if existing_sidecar.as_ref() != Some(&sidecar) {
                return Err(crate::ImError::PermissionDenied);
            }
            let session = self
                .store
                .load_session(binding, &existing.session_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            if !self
                .store
                .session_is_enabled(binding, &existing.session_id)?
                || session.state.disabled
            {
                return Err(crate::ImError::PermissionDenied);
            }
            if session.state.status != anp::direct_e2ee::V2_SESSION_STATUS_ESTABLISHED
                || self
                    .store
                    .select_pending_init_operation(binding, &existing.session_id)?
                    .is_some()
            {
                return Err(crate::ImError::unsupported(SESSION_ESTABLISHMENT_PENDING));
            }
            return prepared_from_pending(existing, existing_sidecar);
        }
        let stored = match self.store.select_established_session(binding)? {
            Some(stored) => stored,
            None => match self.root_control_session_readiness(binding)? {
                V2RootControlSessionReadiness::Pending(_) => {
                    return Err(crate::ImError::unsupported(SESSION_ESTABLISHMENT_PENDING));
                }
                V2RootControlSessionReadiness::Absent => {
                    return Err(crate::ImError::unsupported(
                        "p5-v2-established-session-required",
                    ));
                }
                V2RootControlSessionReadiness::Ready => {
                    return Err(crate::ImError::PermissionDenied);
                }
            },
        };
        if self
            .store
            .select_pending_init_operation(binding, &stored.state.session_id)?
            .is_some()
        {
            return Err(crate::ImError::unsupported(SESSION_ESTABLISHMENT_PENDING));
        }
        let metadata = outbound_metadata(binding, operation_id);
        let mut next_state = stored.state;
        let (pending, body) = V2DirectE2eeSession::encrypt_follow_up_secret_json(
            &mut next_state,
            binding,
            &metadata,
            plaintext,
        )
        .map_err(v2_error)?;
        self.store.commit_outbound_with_private_sidecar(
            &next_state,
            &pending,
            V2SessionExpectation::Revision(stored.revision),
            now,
            &sidecar,
        )?;
        Ok(PreparedV2Outbound {
            binding: binding.clone(),
            metadata,
            body: V2DirectBody::Cipher(body),
            sidecar: Some(sidecar),
        })
    }

    fn prepare_outbound_inner(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2ApplicationPlaintext,
        now: &str,
        sidecar: Option<V2PrivateOutboundSidecar>,
    ) -> crate::ImResult<PreparedV2Outbound> {
        binding.validate().map_err(v2_error)?;
        plaintext.validate().map_err(v2_error)?;
        require_operation_id(operation_id)?;
        if sidecar.is_some()
            && self
                .store
                .private_outbound_status(operation_id, now)?
                .is_some_and(|status| !status.retryable)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(existing) = self.store.load_pending(binding, operation_id)? {
            let existing_sidecar = self
                .store
                .load_private_outbound_sidecar(binding, operation_id)?;
            if existing_sidecar != sidecar {
                return Err(crate::ImError::PermissionDenied);
            }
            return prepared_from_pending(existing, existing_sidecar);
        }
        let stored = self
            .store
            .select_established_session(binding)?
            .ok_or_else(|| crate::ImError::unsupported("p5-v2-established-session-required"))?;
        let metadata = outbound_metadata(binding, operation_id);
        let mut next_state = stored.state;
        let (pending, body) =
            V2DirectE2eeSession::encrypt_follow_up(&mut next_state, binding, &metadata, plaintext)
                .map_err(v2_error)?;
        match sidecar.as_ref() {
            Some(sidecar) => self.store.commit_outbound_with_private_sidecar(
                &next_state,
                &pending,
                V2SessionExpectation::Revision(stored.revision),
                now,
                sidecar,
            )?,
            None => self.store.commit_outbound(
                &next_state,
                &pending,
                V2SessionExpectation::Revision(stored.revision),
                now,
            )?,
        }
        Ok(PreparedV2Outbound {
            binding: binding.clone(),
            metadata,
            body: V2DirectBody::Cipher(body),
            sidecar,
        })
    }

    /// Loads an exact private-control retry without opening or reconstructing
    /// its encrypted inner plaintext.
    pub(crate) fn resume_private_outbound(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<Option<PreparedV2Outbound>> {
        let Some(pending) = self.store.load_pending(binding, operation_id)? else {
            if self
                .store
                .load_private_outbound_sidecar(binding, operation_id)?
                .is_some()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            return Ok(None);
        };
        let sidecar = self
            .store
            .load_private_outbound_sidecar(binding, operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        prepared_from_pending(pending, Some(sidecar)).map(Some)
    }

    /// Loads a standard P5 v2 Init/Cipher retry without advancing a ratchet.
    pub(crate) fn resume_outbound(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<Option<PreparedV2Outbound>> {
        let Some(pending) = self.store.load_pending(binding, operation_id)? else {
            return Ok(None);
        };
        if self
            .store
            .load_private_outbound_sidecar(binding, operation_id)?
            .is_some()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        prepared_from_pending(pending, None).map(Some)
    }

    pub(crate) fn mark_outbound_accepted(
        &self,
        prepared: &PreparedV2Outbound,
    ) -> crate::ImResult<bool> {
        self.store
            .mark_pending_accepted(&prepared.binding(), &prepared.metadata.operation_id)
    }

    pub(crate) fn list_private_outbound_statuses(
        &self,
        now: &str,
    ) -> crate::ImResult<Vec<V2PrivateOutboundStatus>> {
        self.store.list_private_outbound_statuses(now)
    }

    pub(crate) fn private_outbound_status(
        &self,
        operation_id: &str,
        now: &str,
    ) -> crate::ImResult<Option<V2PrivateOutboundStatus>> {
        self.store.private_outbound_status(operation_id, now)
    }

    pub(crate) fn mark_private_outbound_failed(
        &self,
        prepared: &PreparedV2Outbound,
    ) -> crate::ImResult<()> {
        self.store
            .mark_private_outbound_failed(&prepared.binding(), &prepared.metadata.operation_id)
    }

    pub(crate) fn mark_private_outbound_completed(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
    ) -> crate::ImResult<()> {
        self.store
            .mark_private_outbound_completed(binding, operation_id)
    }

    pub(crate) fn complete_session_reply_for_session(
        &self,
        binding: &V2SessionBinding,
        session_id: &str,
    ) -> crate::ImResult<bool> {
        self.store
            .complete_session_reply_for_session(binding, session_id)
    }

    /// Decrypts one exact cipher and atomically commits the advanced ratchet
    /// and replay marker. Exact replay is reported without a second decrypt.
    pub(crate) fn decrypt_inbound(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectCipherBody,
        now: &str,
    ) -> crate::ImResult<V2InboundDecryptOutcome> {
        match self.decrypt_inbound_validated(binding, metadata, body, now, |plaintext, _| {
            Ok(plaintext.clone())
        })? {
            V2ValidatedInboundOutcome::Decrypted { validated, session } => {
                Ok(V2InboundDecryptOutcome::Decrypted {
                    plaintext: validated,
                    session,
                })
            }
            V2ValidatedInboundOutcome::Replay { session } => {
                Ok(V2InboundDecryptOutcome::Replay { session })
            }
        }
    }

    /// Decrypts tentatively, validates the application-level control shape,
    /// and advances the ratchet only after that validation succeeds.
    pub(crate) fn decrypt_inbound_validated<T>(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectCipherBody,
        now: &str,
        validator: impl FnOnce(&V2ApplicationPlaintext, &V2DirectSessionState) -> crate::ImResult<T>,
    ) -> crate::ImResult<V2ValidatedInboundOutcome<T>> {
        binding.validate().map_err(v2_error)?;
        metadata.validate().map_err(v2_error)?;
        body.validate().map_err(v2_error)?;
        let session_id = body.session_id.as_str();
        let digest = cipher_digest(body)?;
        if self
            .store
            .is_exact_inbound_replay(binding, &metadata.message_id, &digest, session_id)?
        {
            let stored = self
                .store
                .load_session(binding, session_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            return Ok(V2ValidatedInboundOutcome::Replay {
                session: stored.state,
            });
        }
        let stored = self
            .store
            .load_session(binding, &body.session_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        let mut next_state = stored.state;
        let plaintext =
            V2DirectE2eeSession::decrypt_follow_up(&mut next_state, binding, metadata, body)
                .map_err(v2_error)?;
        let validated = validator(&plaintext, &next_state)?;
        match self.store.commit_inbound(
            &next_state,
            &metadata.message_id,
            &digest,
            None,
            V2SessionExpectation::Revision(stored.revision),
            now,
        )? {
            V2InboundCommit::Applied => Ok(V2ValidatedInboundOutcome::Decrypted {
                validated,
                session: next_state,
            }),
            V2InboundCommit::Replay => {
                let stored = self
                    .store
                    .load_session(binding, &body.session_id)?
                    .ok_or(crate::ImError::PermissionDenied)?;
                Ok(V2ValidatedInboundOutcome::Replay {
                    session: stored.state,
                })
            }
        }
    }

    pub(crate) fn decrypt_inbound_secret_json(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectCipherBody,
        now: &str,
    ) -> crate::ImResult<V2SecretInboundDecryptOutcome> {
        binding.validate().map_err(v2_error)?;
        metadata.validate().map_err(v2_error)?;
        body.validate().map_err(v2_error)?;
        let digest = cipher_digest(body)?;
        if self.store.is_exact_inbound_replay(
            binding,
            &metadata.message_id,
            &digest,
            &body.session_id,
        )? {
            let stored = self
                .store
                .load_session(binding, &body.session_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            return Ok(V2SecretInboundDecryptOutcome::Replay {
                session: stored.state,
            });
        }
        let stored = self
            .store
            .load_session(binding, &body.session_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        let mut next_state = stored.state;
        let plaintext = V2DirectE2eeSession::decrypt_follow_up_secret_json(
            &mut next_state,
            binding,
            metadata,
            body,
        )
        .map_err(v2_error)?;
        match self.store.commit_inbound(
            &next_state,
            &metadata.message_id,
            &digest,
            None,
            V2SessionExpectation::Revision(stored.revision),
            now,
        )? {
            V2InboundCommit::Applied => Ok(V2SecretInboundDecryptOutcome::Decrypted {
                plaintext,
                session: next_state,
            }),
            V2InboundCommit::Replay => {
                let stored = self
                    .store
                    .load_session(binding, &body.session_id)?
                    .ok_or(crate::ImError::PermissionDenied)?;
                Ok(V2SecretInboundDecryptOutcome::Replay {
                    session: stored.state,
                })
            }
        }
    }

    /// Decrypts tentatively, runs caller-owned semantic validation, and only
    /// then commits the ratchet/replay marker. The validator must be
    /// idempotent because a concurrent exact replay may win the final CAS.
    pub(crate) fn decrypt_inbound_secret_json_validated<T>(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectCipherBody,
        now: &str,
        validator: impl FnOnce(&V2SecretJsonPayload, &V2DirectSessionState) -> crate::ImResult<T>,
    ) -> crate::ImResult<V2ValidatedSecretInboundOutcome<T>> {
        binding.validate().map_err(v2_error)?;
        metadata.validate().map_err(v2_error)?;
        body.validate().map_err(v2_error)?;
        let digest = cipher_digest(body)?;
        if self.store.is_exact_inbound_replay(
            binding,
            &metadata.message_id,
            &digest,
            &body.session_id,
        )? {
            let stored = self
                .store
                .load_session(binding, &body.session_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            return Ok(V2ValidatedSecretInboundOutcome::Replay {
                session: stored.state,
            });
        }
        let stored = self
            .store
            .load_session(binding, &body.session_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
        let mut next_state = stored.state;
        let plaintext = V2DirectE2eeSession::decrypt_follow_up_secret_json(
            &mut next_state,
            binding,
            metadata,
            body,
        )
        .map_err(v2_error)?;
        let validated = validator(&plaintext, &next_state)?;
        match self.store.commit_inbound(
            &next_state,
            &metadata.message_id,
            &digest,
            None,
            V2SessionExpectation::Revision(stored.revision),
            now,
        )? {
            V2InboundCommit::Applied => Ok(V2ValidatedSecretInboundOutcome::Decrypted {
                validated,
                session: next_state,
            }),
            V2InboundCommit::Replay => {
                let stored = self
                    .store
                    .load_session(binding, &body.session_id)?
                    .ok_or(crate::ImError::PermissionDenied)?;
                Ok(V2ValidatedSecretInboundOutcome::Replay {
                    session: stored.state,
                })
            }
        }
    }

    /// Accepts an Init once the caller has resolved the peer's current DID
    /// document and extracted both exact-device static X25519 keys.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decrypt_inbound_init(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectInitBody,
        local_static_private: &x25519_dalek::StaticSecret,
        sender_static_public: &[u8; 32],
        now: &str,
    ) -> crate::ImResult<V2InboundDecryptOutcome> {
        match self.decrypt_inbound_init_validated(
            binding,
            metadata,
            body,
            local_static_private,
            sender_static_public,
            now,
            |plaintext, _| Ok(plaintext.clone()),
        )? {
            V2ValidatedInboundOutcome::Decrypted { validated, session } => {
                Ok(V2InboundDecryptOutcome::Decrypted {
                    plaintext: validated,
                    session,
                })
            }
            V2ValidatedInboundOutcome::Replay { session } => {
                Ok(V2InboundDecryptOutcome::Replay { session })
            }
        }
    }

    /// Accepts an Init only after caller-owned application validation. This
    /// keeps a malformed or unexpected control payload from consuming the OPK
    /// or creating a replay marker/session.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decrypt_inbound_init_validated<T>(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectInitBody,
        local_static_private: &x25519_dalek::StaticSecret,
        sender_static_public: &[u8; 32],
        now: &str,
        validator: impl FnOnce(&V2ApplicationPlaintext, &V2DirectSessionState) -> crate::ImResult<T>,
    ) -> crate::ImResult<V2ValidatedInboundOutcome<T>> {
        binding.validate().map_err(v2_error)?;
        metadata.validate().map_err(v2_error)?;
        body.validate().map_err(v2_error)?;
        let session_id = body.session_id.as_str();
        let digest = init_digest(body)?;
        if self
            .store
            .is_exact_inbound_replay(binding, &metadata.message_id, &digest, session_id)?
        {
            let stored = self
                .store
                .load_session(binding, session_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            return Ok(V2ValidatedInboundOutcome::Replay {
                session: stored.state,
            });
        }
        let local = self
            .store
            .load_accepted_bundle(&body.recipient_bundle_id, now)?
            .ok_or(crate::ImError::PermissionDenied)?;
        let opk = body
            .recipient_one_time_prekey_id
            .as_deref()
            .map(|key_id| {
                self.store
                    .load_available_opk(&body.recipient_bundle_id, key_id)
            })
            .transpose()?
            .flatten();
        if body.recipient_one_time_prekey_id.is_some() != opk.is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        let (next_state, plaintext, consumed_opk_id) = V2DirectE2eeSession::accept_incoming_init(
            binding,
            metadata,
            local_static_private,
            &local.bundle,
            &local.signed_prekey_private,
            opk.as_ref().map(|opk| (&opk.public, &opk.private)),
            sender_static_public,
            body,
        )
        .map_err(v2_error)?;
        let validated = validator(&plaintext, &next_state)?;
        match self.store.commit_inbound(
            &next_state,
            &metadata.message_id,
            &digest,
            consumed_opk_id.as_deref(),
            V2SessionExpectation::Absent,
            now,
        )? {
            V2InboundCommit::Applied => Ok(V2ValidatedInboundOutcome::Decrypted {
                validated,
                session: next_state,
            }),
            V2InboundCommit::Replay => {
                let stored = self
                    .store
                    .load_session(binding, &body.session_id)?
                    .ok_or(crate::ImError::PermissionDenied)?;
                Ok(V2ValidatedInboundOutcome::Replay {
                    session: stored.state,
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedV2Outbound {
    binding: V2SessionBinding,
    pub(crate) metadata: V2DirectMetadata,
    pub(crate) body: V2DirectBody,
    pub(crate) sidecar: Option<V2PrivateOutboundSidecar>,
}

impl PreparedV2Outbound {
    pub(crate) fn binding(&self) -> V2SessionBinding {
        self.binding.clone()
    }

    pub(crate) fn direct_request(&self) -> crate::ImResult<serde_json::Value> {
        direct_send_request_v2(self.metadata.clone(), self.body.clone()).map_err(v2_error)
    }

    pub(crate) fn init_body(&self) -> crate::ImResult<&V2DirectInitBody> {
        match &self.body {
            V2DirectBody::Init(body) => Ok(body),
            V2DirectBody::Cipher(_) => Err(crate::ImError::PermissionDenied),
        }
    }

    pub(crate) fn cipher_body(&self) -> crate::ImResult<&V2DirectCipherBody> {
        match &self.body {
            V2DirectBody::Cipher(body) => Ok(body),
            V2DirectBody::Init(_) => Err(crate::ImError::PermissionDenied),
        }
    }
}

pub(crate) enum V2InboundDecryptOutcome {
    Decrypted {
        plaintext: V2ApplicationPlaintext,
        session: V2DirectSessionState,
    },
    Replay {
        session: V2DirectSessionState,
    },
}

pub(crate) enum V2ValidatedInboundOutcome<T> {
    Decrypted {
        validated: T,
        session: V2DirectSessionState,
    },
    Replay {
        session: V2DirectSessionState,
    },
}

pub(crate) enum V2SecretInboundDecryptOutcome {
    Decrypted {
        plaintext: V2SecretJsonPayload,
        session: V2DirectSessionState,
    },
    Replay {
        session: V2DirectSessionState,
    },
}

pub(crate) enum V2ValidatedSecretInboundOutcome<T> {
    Decrypted {
        validated: T,
        session: V2DirectSessionState,
    },
    Replay {
        session: V2DirectSessionState,
    },
}

pub(crate) fn parse_send_result(
    value: &serde_json::Value,
    prepared: &PreparedV2Outbound,
) -> crate::ImResult<V2DirectSendResult> {
    let result = parse_direct_send_result_v2(value).map_err(v2_error)?;
    if result.message_id != prepared.metadata.message_id
        || result.operation_id != prepared.metadata.operation_id
        || result.target_did != prepared.metadata.target.did
        || result.recipient_device_id != prepared.metadata.recipient_device_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(result)
}

fn prepared_from_pending(
    pending: V2PendingOutboundRecord,
    sidecar: Option<V2PrivateOutboundSidecar>,
) -> crate::ImResult<PreparedV2Outbound> {
    pending.validate().map_err(v2_error)?;
    let body = match pending.wire_content_type.as_str() {
        CONTENT_TYPE_DIRECT_INIT_V2 => {
            V2DirectBody::Init(serde_json::from_value(pending.body).map_err(serialization_error)?)
        }
        CONTENT_TYPE_DIRECT_CIPHER_V2 => {
            V2DirectBody::Cipher(serde_json::from_value(pending.body).map_err(serialization_error)?)
        }
        _ => return Err(crate::ImError::PermissionDenied),
    };
    Ok(PreparedV2Outbound {
        binding: pending.binding.clone(),
        metadata: outbound_metadata_for_content_type(
            &pending.binding,
            &pending.operation_id,
            &pending.wire_content_type,
        ),
        body,
        sidecar,
    })
}

fn outbound_metadata(binding: &V2SessionBinding, operation_id: &str) -> V2DirectMetadata {
    outbound_metadata_for_content_type(binding, operation_id, CONTENT_TYPE_DIRECT_CIPHER_V2)
}

fn outbound_metadata_for_content_type(
    binding: &V2SessionBinding,
    operation_id: &str,
    content_type: &str,
) -> V2DirectMetadata {
    V2DirectMetadata {
        anp_version: None,
        profile: DIRECT_E2EE_PROFILE_V2.to_owned(),
        security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
        sender_did: binding.local_did.clone(),
        sender_device_id: binding.local_device_id.clone(),
        target: V2Target {
            kind: "agent".to_owned(),
            did: binding.peer_did.clone(),
        },
        recipient_device_id: binding.peer_device_id.clone(),
        operation_id: operation_id.to_owned(),
        message_id: operation_id.to_owned(),
        content_type: content_type.to_owned(),
        created_at: None,
    }
}

fn session_control_operation_id(prefix: &str, seed: &str) -> crate::ImResult<String> {
    require_operation_id(seed)?;
    let digest = Sha256::digest(seed.as_bytes());
    Ok(format!("{prefix}{}", URL_SAFE_NO_PAD.encode(&digest[..16])))
}

fn is_session_control_operation_id(prefix: &str, value: &str) -> bool {
    let Some(digest) = value.strip_prefix(prefix) else {
        return false;
    };
    digest.len() == 22
        && URL_SAFE_NO_PAD
            .decode(digest)
            .is_ok_and(|decoded| decoded.len() == 16)
}

fn init_digest(body: &V2DirectInitBody) -> crate::ImResult<String> {
    body_digest(body)
}

fn cipher_digest(body: &V2DirectCipherBody) -> crate::ImResult<String> {
    body_digest(body)
}

fn body_digest<T: serde::Serialize>(body: &T) -> crate::ImResult<String> {
    let encoded = serde_json::to_vec(body).map_err(serialization_error)?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(encoded))
    ))
}

fn require_operation_id(value: &str) -> crate::ImResult<()> {
    if value.trim().is_empty() || value != value.trim() {
        Err(crate::ImError::invalid_input(
            Some("operation_id".to_owned()),
            "operation_id must be a non-empty exact value",
        ))
    } else {
        Ok(())
    }
}

fn serialization_error(error: serde_json::Error) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
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
    use crate::internal::secure_direct::secret_store::DirectSecretVault;
    use crate::internal::secure_direct::v2_store::{SqliteV2DirectStateStore, V2OwnerScope};
    use crate::vault::{DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SecretKind};
    use anp::direct_e2ee::{
        V2DirectSessionState, V2GetPrekeyBundleResult, V2OneTimePrekey, V2PrekeyBundle,
        V2SignedPrekey, DIRECT_E2EE_V2_SESSION_STATE_FORMAT, MTI_DIRECT_E2EE_SUITE_V2,
        V2_SESSION_STATUS_ESTABLISHED,
    };
    use rusqlite::Connection;
    use std::sync::Arc;

    fn scope(identity_id: &str, did: &str, device_id: &str, key_id: &str) -> V2OwnerScope {
        let did = crate::ids::Did::parse(did).unwrap();
        V2OwnerScope::from_identity_state(
            &crate::ids::IdentityId::parse(identity_id).unwrap(),
            &did,
            &IdentityDeviceState {
                schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                mode: IdentityDeviceMode::VNext,
                authorization: Some(DeviceAuthorizationProjection {
                    protocol_device_id: crate::ids::ProtocolDeviceId::parse(device_id).unwrap(),
                    signing_key_id: format!("{}#sign", did.as_str()),
                    e2ee_key_id: key_id.to_owned(),
                    status: DeviceAuthorizationStatus::Active,
                    role: DeviceAuthorizationRole::Admin,
                    management_ready: true,
                    auth_generation: 1,
                }),
                checkpoint: Some(IdentityInternalCheckpoint {
                    document_version: 1,
                    document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                    registry_version: 1,
                }),
            },
        )
        .unwrap()
    }

    fn vault(path: &std::path::Path, byte: u8) -> DirectSecretVault {
        Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([byte; 32]),
            FileSecretVaultStore::new(path),
        ))
    }

    fn root_private_sidecar(
        operation_id: &str,
        sender_device_id: &str,
        recipient_device_id: &str,
    ) -> V2PrivateOutboundSidecar {
        V2PrivateOutboundSidecar::root_control(
            operation_id,
            crate::internal::identity_root_transfer::RootImportTransportContext {
                message_id: operation_id.to_owned(),
                delivery_class: "awiki-root-key-control".to_owned(),
                sender_device_id: sender_device_id.to_owned(),
                recipient_device_id: recipient_device_id.to_owned(),
                expires_at: "2030-07-20T00:05:00Z".to_owned(),
            },
            None,
        )
        .unwrap()
    }

    fn root_secret_plaintext() -> V2SecretJsonPayload {
        V2SecretJsonPayload::from_canonical_json_object(
            br#"{"opaque_test_value":"secret","system_type":"awiki.device.root-key.v1"}"#.to_vec(),
        )
        .unwrap()
    }

    fn established_pair() -> (V2DirectSessionState, V2DirectSessionState) {
        let alice_ratchet = x25519_dalek::StaticSecret::from([7; 32]);
        let bob_ratchet = x25519_dalek::StaticSecret::from([8; 32]);
        let alice_public = x25519_dalek::PublicKey::from(&alice_ratchet).to_bytes();
        let bob_public = x25519_dalek::PublicKey::from(&bob_ratchet).to_bytes();
        let session_id = URL_SAFE_NO_PAD.encode([1; 16]);
        let common_root = URL_SAFE_NO_PAD.encode([2; 32]);
        let alice_binding = V2SessionBinding {
            local_did: "did:example:alice".to_owned(),
            local_device_id: "alice-phone".to_owned(),
            peer_did: "did:example:alice".to_owned(),
            peer_device_id: "alice-laptop".to_owned(),
            suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
            local_e2ee_key_id: "did:example:alice#phone-e2ee".to_owned(),
            peer_e2ee_key_id: "did:example:alice#laptop-e2ee".to_owned(),
        };
        let bob_binding = V2SessionBinding {
            local_did: alice_binding.peer_did.clone(),
            local_device_id: alice_binding.peer_device_id.clone(),
            peer_did: alice_binding.local_did.clone(),
            peer_device_id: alice_binding.local_device_id.clone(),
            suite: alice_binding.suite.clone(),
            local_e2ee_key_id: alice_binding.peer_e2ee_key_id.clone(),
            peer_e2ee_key_id: alice_binding.local_e2ee_key_id.clone(),
        };
        let alice = V2DirectSessionState {
            state_format: DIRECT_E2EE_V2_SESSION_STATE_FORMAT.to_owned(),
            binding: alice_binding,
            session_id: session_id.clone(),
            root_key_b64u: common_root.clone(),
            send_chain_key_b64u: Some(URL_SAFE_NO_PAD.encode([3; 32])),
            recv_chain_key_b64u: Some(URL_SAFE_NO_PAD.encode([4; 32])),
            ratchet_private_key_b64u: URL_SAFE_NO_PAD.encode(alice_ratchet.to_bytes()),
            ratchet_public_key_b64u: URL_SAFE_NO_PAD.encode(alice_public),
            peer_ratchet_public_key_b64u: Some(URL_SAFE_NO_PAD.encode(bob_public)),
            send_n: 0,
            recv_n: 0,
            previous_send_chain_length: 0,
            skipped_message_keys: vec![],
            is_initiator: true,
            status: V2_SESSION_STATUS_ESTABLISHED.to_owned(),
            disabled: false,
        };
        let bob = V2DirectSessionState {
            state_format: DIRECT_E2EE_V2_SESSION_STATE_FORMAT.to_owned(),
            binding: bob_binding,
            session_id,
            root_key_b64u: common_root,
            send_chain_key_b64u: Some(URL_SAFE_NO_PAD.encode([4; 32])),
            recv_chain_key_b64u: Some(URL_SAFE_NO_PAD.encode([3; 32])),
            ratchet_private_key_b64u: URL_SAFE_NO_PAD.encode(bob_ratchet.to_bytes()),
            ratchet_public_key_b64u: URL_SAFE_NO_PAD.encode(bob_public),
            peer_ratchet_public_key_b64u: Some(URL_SAFE_NO_PAD.encode(alice_public)),
            send_n: 0,
            recv_n: 0,
            previous_send_chain_length: 0,
            skipped_message_keys: vec![],
            is_initiator: false,
            status: V2_SESSION_STATUS_ESTABLISHED.to_owned(),
            disabled: false,
        };
        (alice, bob)
    }

    #[test]
    fn exact_private_retry_survives_restart_and_decrypts_once() {
        let root = tempfile::tempdir().unwrap();
        let alice_db = Connection::open(root.path().join("alice.sqlite")).unwrap();
        let bob_db = Connection::open(root.path().join("bob.sqlite")).unwrap();
        let alice_vault = vault(&root.path().join("alice-vault"), 31);
        let bob_vault = vault(&root.path().join("bob-vault"), 32);
        let (alice_state, bob_state) = established_pair();
        let alice_scope = scope(
            "identity-alice-phone",
            "did:example:alice",
            "alice-phone",
            "did:example:alice#phone-e2ee",
        );
        let bob_scope = scope(
            "identity-alice-laptop",
            "did:example:alice",
            "alice-laptop",
            "did:example:alice#laptop-e2ee",
        );
        let alice_store = SqliteV2DirectStateStore::new_with_secret_vault(
            &alice_db,
            alice_vault.clone(),
            alice_scope.clone(),
        )
        .unwrap();
        let bob_store =
            SqliteV2DirectStateStore::new_with_secret_vault(&bob_db, bob_vault, bob_scope).unwrap();
        alice_store
            .commit_inbound(
                &alice_state,
                "setup-alice",
                "sha256:setup-alice",
                None,
                V2SessionExpectation::Absent,
                "2026-07-20T00:00:00Z",
            )
            .unwrap();
        bob_store
            .commit_inbound(
                &bob_state,
                "setup-bob",
                "sha256:setup-bob",
                None,
                V2SessionExpectation::Absent,
                "2026-07-20T00:00:00Z",
            )
            .unwrap();

        let sidecar = V2PrivateOutboundSidecar::root_control(
            "root-message-1",
            crate::internal::identity_root_transfer::RootImportTransportContext {
                message_id: "root-message-1".to_owned(),
                delivery_class: "awiki-root-key-control".to_owned(),
                sender_device_id: "alice-phone".to_owned(),
                recipient_device_id: "alice-laptop".to_owned(),
                expires_at: "2030-07-20T00:05:00Z".to_owned(),
            },
            None,
        )
        .unwrap();
        let plaintext = V2ApplicationPlaintext {
            application_content_type: "application/json".to_owned(),
            logical_message_id: None,
            conversation_id: None,
            reply_to_message_id: None,
            annotations: None,
            text: None,
            payload: Some(serde_json::json!({
                "system_type": "awiki.device.root-key.v1",
                "opaque_test_value": "secret"
            })),
            payload_b64u: None,
        };
        let runtime = V2EstablishedDirectRuntime::new(&alice_store);
        assert!(matches!(
            runtime
                .root_control_session_readiness(&alice_state.binding)
                .unwrap(),
            V2RootControlSessionReadiness::Ready
        ));
        let first = runtime
            .prepare_private_outbound(
                &alice_state.binding,
                "root-message-1",
                &plaintext,
                "2026-07-20T00:00:01Z",
                sidecar.clone(),
            )
            .unwrap();
        drop(runtime);
        drop(alice_store);

        let restarted_store = SqliteV2DirectStateStore::new_with_secret_vault(
            &alice_db,
            alice_vault.clone(),
            alice_scope,
        )
        .unwrap();
        let restarted = V2EstablishedDirectRuntime::new(&restarted_store);
        let retry = restarted
            .resume_private_outbound(&alice_state.binding, "root-message-1")
            .unwrap()
            .unwrap();
        assert_eq!(retry.body, first.body);
        assert_eq!(retry.sidecar, Some(sidecar));
        assert_eq!(
            retry.direct_request().unwrap(),
            first.direct_request().unwrap()
        );
        assert_eq!(
            restarted
                .private_outbound_status("root-message-1", "2026-07-20T00:00:01Z")
                .unwrap()
                .unwrap()
                .phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::PendingDelivery
        );
        restarted.mark_private_outbound_failed(&retry).unwrap();
        let failed = restarted
            .private_outbound_status("root-message-1", "2026-07-20T00:00:01Z")
            .unwrap()
            .unwrap();
        assert_eq!(
            failed.phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::Failed
        );
        assert!(failed.retryable);

        let bob_runtime = V2EstablishedDirectRuntime::new(&bob_store);
        let inbound = bob_runtime
            .decrypt_inbound(
                &bob_state.binding,
                &first.metadata,
                first.cipher_body().unwrap(),
                "2026-07-20T00:00:02Z",
            )
            .unwrap();
        let V2InboundDecryptOutcome::Decrypted { plaintext, .. } = inbound else {
            panic!("first delivery must decrypt");
        };
        assert_eq!(plaintext.payload.unwrap()["opaque_test_value"], "secret");
        assert!(matches!(
            bob_runtime
                .decrypt_inbound(
                    &bob_state.binding,
                    &first.metadata,
                    first.cipher_body().unwrap(),
                    "2026-07-20T00:00:03Z"
                )
                .unwrap(),
            V2InboundDecryptOutcome::Replay { .. }
        ));

        assert!(restarted.mark_outbound_accepted(&retry).unwrap());
        assert_eq!(
            restarted
                .private_outbound_status("root-message-1", "2026-07-20T00:00:04Z")
                .unwrap()
                .unwrap()
                .phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::AwaitingImport
        );
        let accepted_retry = restarted
            .resume_private_outbound(&alice_state.binding, "root-message-1")
            .unwrap()
            .expect("accepted private ciphertext remains retryable until completion");
        assert_eq!(accepted_retry, retry);
        assert_eq!(
            alice_vault
                .list()
                .unwrap()
                .into_iter()
                .filter(|secret_ref| { secret_ref.kind == SecretKind::DirectE2eeV2PendingOutbound })
                .count(),
            1
        );
        let mut wrong_peer_binding = alice_state.binding.clone();
        wrong_peer_binding.peer_device_id = "alice-tablet".to_owned();
        assert!(restarted
            .mark_private_outbound_completed(&wrong_peer_binding, "root-message-1")
            .is_err());
        restarted
            .mark_private_outbound_completed(&alice_state.binding, "root-message-1")
            .unwrap();
        let completed = restarted
            .private_outbound_status("root-message-1", "2026-07-20T00:00:04Z")
            .unwrap()
            .unwrap();
        assert_eq!(
            completed.phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::Completed
        );
        assert!(!completed.retryable);
        assert!(restarted
            .resume_private_outbound(&alice_state.binding, "root-message-1")
            .unwrap()
            .is_none());
        assert_eq!(
            alice_db
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_pending WHERE operation_id = 'root-message-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            alice_db
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_private_outbound WHERE operation_id = 'root-message-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            alice_db
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_private_outbound_tombstones WHERE operation_id = 'root-message-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            alice_vault
                .list()
                .unwrap()
                .into_iter()
                .filter(|secret_ref| { secret_ref.kind == SecretKind::DirectE2eeV2PendingOutbound })
                .count(),
            0
        );
        restarted
            .mark_private_outbound_completed(&alice_state.binding, "root-message-1")
            .unwrap();

        let ack_sidecar = V2PrivateOutboundSidecar::root_control(
            "root-ack-1",
            crate::internal::identity_root_transfer::RootImportTransportContext {
                message_id: "root-ack-1".to_owned(),
                delivery_class: "awiki-root-key-control".to_owned(),
                sender_device_id: "alice-phone".to_owned(),
                recipient_device_id: "alice-laptop".to_owned(),
                expires_at: "2030-07-20T00:05:00Z".to_owned(),
            },
            Some(
                crate::internal::identity_root_transfer::RootKeyImportedCompletion {
                    completion_type: "awiki.device.root-key-imported.v1".to_owned(),
                    ack_for_message_id: "root-ack-1".to_owned(),
                    did: "did:example:alice".to_owned(),
                    sending_device_id: "alice-phone".to_owned(),
                    importing_device_id: "alice-laptop".to_owned(),
                    root_key_id: "did:example:alice#root".to_owned(),
                    root_public_key_fingerprint: "sha256:test".to_owned(),
                    document_version: 1,
                    document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                    result: "imported".to_owned(),
                    imported_at: "2026-07-20T00:00:04Z".to_owned(),
                    device_signature: "test-signature".to_owned(),
                },
            ),
        )
        .unwrap();
        let ack_plaintext = V2ApplicationPlaintext {
            application_content_type: "application/json".to_owned(),
            logical_message_id: None,
            conversation_id: None,
            reply_to_message_id: None,
            annotations: None,
            text: None,
            payload: Some(serde_json::json!({"system_type": "test-ack"})),
            payload_b64u: None,
        };
        let ack = bob_runtime
            .prepare_private_outbound(
                &bob_state.binding,
                "root-ack-1",
                &ack_plaintext,
                "2026-07-20T00:00:04Z",
                ack_sidecar,
            )
            .unwrap();
        assert!(bob_runtime.mark_outbound_accepted(&ack).unwrap());
        let accepted_ack = bob_runtime
            .private_outbound_status("root-ack-1", "2026-07-20T00:00:04Z")
            .unwrap()
            .unwrap();
        assert_eq!(
            accepted_ack.phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::Importing
        );
        assert!(accepted_ack.retryable);
        bob_runtime
            .mark_private_outbound_completed(&bob_state.binding, "root-ack-1")
            .unwrap();
        assert_eq!(
            bob_runtime
                .private_outbound_status("root-ack-1", "2026-07-20T00:00:04Z")
                .unwrap()
                .unwrap()
                .phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::Completed
        );
        assert_eq!(
            bob_db
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_pending WHERE operation_id = 'root-ack-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn first_authenticated_cipher_retires_session_reply_after_restart() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("reply.sqlite");
        let connection = Connection::open(&db_path).unwrap();
        let secret_vault = vault(&root.path().join("reply-vault"), 41);
        let (_, responder_state) = established_pair();
        let responder_scope = scope(
            "identity-alice-laptop",
            "did:example:alice",
            "alice-laptop",
            "did:example:alice#laptop-e2ee",
        );
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            secret_vault.clone(),
            responder_scope.clone(),
        )
        .unwrap();
        store
            .commit_inbound(
                &responder_state,
                "setup-reply",
                "sha256:setup-reply",
                None,
                V2SessionExpectation::Absent,
                "2026-07-20T00:00:00Z",
            )
            .unwrap();
        let runtime = V2EstablishedDirectRuntime::new(&store);
        let operation_id = session_reply_operation_id("session-init-1").unwrap();
        let reply = runtime
            .prepare_outbound(
                &responder_state.binding,
                &operation_id,
                &session_established_plaintext("session-init-1").unwrap(),
                "2026-07-20T00:00:01Z",
            )
            .unwrap();
        assert!(runtime.mark_outbound_accepted(&reply).unwrap());
        drop(runtime);
        drop(store);
        drop(connection);

        let connection = Connection::open(&db_path).unwrap();
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            secret_vault.clone(),
            responder_scope,
        )
        .unwrap();
        let runtime = V2EstablishedDirectRuntime::new(&store);
        assert!(runtime
            .complete_session_reply_for_session(
                &responder_state.binding,
                &responder_state.session_id
            )
            .unwrap());
        assert!(!runtime
            .complete_session_reply_for_session(
                &responder_state.binding,
                &responder_state.session_id
            )
            .unwrap());
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_pending WHERE operation_id = ?1",
                    [&operation_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            secret_vault
                .list()
                .unwrap()
                .into_iter()
                .filter(|secret_ref| secret_ref.kind == SecretKind::DirectE2eeV2PendingOutbound)
                .count(),
            0
        );
    }

    #[test]
    fn expired_private_retry_becomes_secret_free_failed_tombstone() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("expired.sqlite");
        let connection = Connection::open(&db_path).unwrap();
        let secret_vault = vault(&root.path().join("expired-vault"), 42);
        let (sender_state, _) = established_pair();
        let sender_scope = scope(
            "identity-alice-phone",
            "did:example:alice",
            "alice-phone",
            "did:example:alice#phone-e2ee",
        );
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            secret_vault.clone(),
            sender_scope.clone(),
        )
        .unwrap();
        store
            .commit_inbound(
                &sender_state,
                "setup-expired",
                "sha256:setup-expired",
                None,
                V2SessionExpectation::Absent,
                "2026-07-20T00:00:00Z",
            )
            .unwrap();
        let runtime = V2EstablishedDirectRuntime::new(&store);
        let sidecar = V2PrivateOutboundSidecar::root_control(
            "root-expired-1",
            crate::internal::identity_root_transfer::RootImportTransportContext {
                message_id: "root-expired-1".to_owned(),
                delivery_class: "awiki-root-key-control".to_owned(),
                sender_device_id: "alice-phone".to_owned(),
                recipient_device_id: "alice-laptop".to_owned(),
                expires_at: "2026-07-20T00:00:02Z".to_owned(),
            },
            None,
        )
        .unwrap();
        let plaintext = V2ApplicationPlaintext {
            application_content_type: "application/json".to_owned(),
            logical_message_id: None,
            conversation_id: None,
            reply_to_message_id: None,
            annotations: None,
            text: None,
            payload: Some(serde_json::json!({"system_type": "test-expiring-root"})),
            payload_b64u: None,
        };
        runtime
            .prepare_private_outbound(
                &sender_state.binding,
                "root-expired-1",
                &plaintext,
                "2026-07-20T00:00:01Z",
                sidecar.clone(),
            )
            .unwrap();
        let failed = runtime
            .private_outbound_status("root-expired-1", "2026-07-20T00:00:03Z")
            .unwrap()
            .unwrap();
        assert_eq!(
            failed.phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::Failed
        );
        assert!(!failed.retryable);
        assert!(failed.completed_at.is_none());
        assert!(runtime
            .resume_private_outbound(&sender_state.binding, "root-expired-1")
            .unwrap()
            .is_none());
        assert!(runtime
            .prepare_private_outbound(
                &sender_state.binding,
                "root-expired-1",
                &plaintext,
                "2026-07-20T00:00:03Z",
                sidecar,
            )
            .is_err());
        drop(runtime);
        drop(store);
        drop(connection);

        let connection = Connection::open(&db_path).unwrap();
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            secret_vault.clone(),
            sender_scope,
        )
        .unwrap();
        let runtime = V2EstablishedDirectRuntime::new(&store);
        let failed_after_restart = runtime
            .private_outbound_status("root-expired-1", "2026-07-20T00:00:04Z")
            .unwrap()
            .unwrap();
        assert_eq!(
            failed_after_restart.phase,
            crate::internal::secure_direct::v2_store::V2PrivateOutboundPhase::Failed
        );
        assert!(!failed_after_restart.retryable);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_pending WHERE operation_id = 'root-expired-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_private_outbound WHERE operation_id = 'root-expired-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            secret_vault
                .list()
                .unwrap()
                .into_iter()
                .filter(|secret_ref| secret_ref.kind == SecretKind::DirectE2eeV2PendingOutbound)
                .count(),
            0
        );
    }

    #[test]
    fn empty_stores_establish_with_init_accept_first_reply_and_exact_retry() {
        let root = tempfile::tempdir().unwrap();
        let alice_db = Connection::open(root.path().join("alice-empty.sqlite")).unwrap();
        let bob_db = Connection::open(root.path().join("bob-empty.sqlite")).unwrap();
        let alice_vault = vault(&root.path().join("alice-empty-vault"), 41);
        let bob_vault = vault(&root.path().join("bob-empty-vault"), 42);
        let alice_scope = scope(
            "identity-alice-phone-empty",
            "did:example:alice",
            "alice-phone",
            "did:example:alice#phone-e2ee",
        );
        let bob_scope = scope(
            "identity-alice-laptop-empty",
            "did:example:alice",
            "alice-laptop",
            "did:example:alice#laptop-e2ee",
        );
        let alice_binding = V2SessionBinding {
            local_did: "did:example:alice".to_owned(),
            local_device_id: "alice-phone".to_owned(),
            peer_did: "did:example:alice".to_owned(),
            peer_device_id: "alice-laptop".to_owned(),
            suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
            local_e2ee_key_id: "did:example:alice#phone-e2ee".to_owned(),
            peer_e2ee_key_id: "did:example:alice#laptop-e2ee".to_owned(),
        };
        let bob_binding = V2SessionBinding {
            local_did: alice_binding.peer_did.clone(),
            local_device_id: alice_binding.peer_device_id.clone(),
            peer_did: alice_binding.local_did.clone(),
            peer_device_id: alice_binding.local_device_id.clone(),
            suite: alice_binding.suite.clone(),
            local_e2ee_key_id: alice_binding.peer_e2ee_key_id.clone(),
            peer_e2ee_key_id: alice_binding.local_e2ee_key_id.clone(),
        };
        let alice_static = x25519_dalek::StaticSecret::from([43; 32]);
        let bob_static = x25519_dalek::StaticSecret::from([44; 32]);
        let bob_spk = x25519_dalek::StaticSecret::from([45; 32]);
        let bob_opk = x25519_dalek::StaticSecret::from([46; 32]);
        let opk = V2OneTimePrekey {
            key_id: "bob-opk-1".to_owned(),
            public_key_b64u: URL_SAFE_NO_PAD
                .encode(x25519_dalek::PublicKey::from(&bob_opk).to_bytes()),
        };
        let bundle = V2PrekeyBundle {
            bundle_id: "bob-bundle-1".to_owned(),
            owner_did: bob_binding.local_did.clone(),
            owner_device_id: bob_binding.local_device_id.clone(),
            suite: MTI_DIRECT_E2EE_SUITE_V2.to_owned(),
            static_key_agreement_id: bob_binding.local_e2ee_key_id.clone(),
            signed_prekey: V2SignedPrekey {
                key_id: "bob-spk-1".to_owned(),
                public_key_b64u: URL_SAFE_NO_PAD
                    .encode(x25519_dalek::PublicKey::from(&bob_spk).to_bytes()),
                expires_at: "2030-01-01T00:00:00Z".to_owned(),
            },
            proof: serde_json::json!({
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "verificationMethod": "did:example:alice#laptop-sign",
                "proofPurpose": "assertionMethod",
                "created": "2026-07-20T00:00:00Z",
                "proofValue": "zTestProof"
            }),
        };

        let alice_store = SqliteV2DirectStateStore::new_with_secret_vault(
            &alice_db,
            alice_vault.clone(),
            alice_scope.clone(),
        )
        .unwrap();
        let bob_store = SqliteV2DirectStateStore::new_with_secret_vault(
            &bob_db,
            bob_vault.clone(),
            bob_scope.clone(),
        )
        .unwrap();
        assert!(alice_store
            .select_established_session(&alice_binding)
            .unwrap()
            .is_none());
        assert!(bob_store
            .select_established_session(&bob_binding)
            .unwrap()
            .is_none());
        bob_store
            .publish_local_bundle(
                &bundle,
                &bob_spk,
                &[(opk.clone(), bob_opk)],
                "2026-07-20T00:00:00Z",
            )
            .unwrap();
        let fetched = V2GetPrekeyBundleResult {
            target_did: bob_binding.local_did.clone(),
            target_device_id: bob_binding.local_device_id.clone(),
            prekey_bundle: bundle.clone(),
            one_time_prekey: Some(opk.clone()),
        };
        let init_plaintext = session_establish_plaintext();
        let alice_runtime = V2EstablishedDirectRuntime::new(&alice_store);
        assert!(matches!(
            alice_runtime
                .root_control_session_readiness(&alice_binding)
                .unwrap(),
            V2RootControlSessionReadiness::Absent
        ));
        let init = alice_runtime
            .prepare_session_init(
                &alice_binding,
                "session-init-1",
                &init_plaintext,
                &alice_static,
                &fetched,
                &x25519_dalek::PublicKey::from(&bob_static).to_bytes(),
                "2026-07-20T00:00:01Z",
            )
            .unwrap();
        assert!(matches!(init.body, V2DirectBody::Init(_)));
        assert!(matches!(
            alice_runtime
                .root_control_session_readiness(&alice_binding)
                .unwrap(),
            V2RootControlSessionReadiness::Pending(ref pending) if pending == &init
        ));
        let pending_error = alice_runtime
            .prepare_private_outbound_secret_json(
                &alice_binding,
                "root-before-confirmation",
                &root_secret_plaintext(),
                "2026-07-20T00:00:01Z",
                root_private_sidecar("root-before-confirmation", "alice-phone", "alice-laptop"),
            )
            .unwrap_err();
        assert!(matches!(
            pending_error,
            crate::ImError::UnsupportedCapability { capability }
                if capability == SESSION_ESTABLISHMENT_PENDING
        ));
        assert_eq!(
            alice_db
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_private_outbound",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );

        let init_session_id = init.init_body().unwrap().session_id.clone();
        assert_eq!(
            alice_db
                .execute(
                    "UPDATE direct_e2ee_v2_sessions SET disabled = 1 WHERE session_id = ?1",
                    [&init_session_id],
                )
                .unwrap(),
            1
        );
        assert!(matches!(
            alice_runtime
                .root_control_session_readiness(&alice_binding)
                .unwrap(),
            V2RootControlSessionReadiness::Absent
        ));
        assert!(matches!(
            alice_runtime.prepare_session_init(
                &alice_binding,
                "session-init-1",
                &init_plaintext,
                &alice_static,
                &fetched,
                &x25519_dalek::PublicKey::from(&bob_static).to_bytes(),
                "2026-07-20T00:00:01Z",
            ),
            Err(crate::ImError::PermissionDenied)
        ));
        assert_eq!(
            alice_db
                .execute(
                    "UPDATE direct_e2ee_v2_sessions SET disabled = 0 WHERE session_id = ?1",
                    [&init_session_id],
                )
                .unwrap(),
            1
        );
        assert!(matches!(
            alice_runtime
                .root_control_session_readiness(&alice_binding)
                .unwrap(),
            V2RootControlSessionReadiness::Pending(ref pending) if pending == &init
        ));
        drop(alice_runtime);
        drop(alice_store);

        let restarted_alice_store =
            SqliteV2DirectStateStore::new_with_secret_vault(&alice_db, alice_vault, alice_scope)
                .unwrap();
        let restarted_alice = V2EstablishedDirectRuntime::new(&restarted_alice_store);
        let init_retry = restarted_alice
            .prepare_session_init(
                &alice_binding,
                "session-init-1",
                &init_plaintext,
                &alice_static,
                &fetched,
                &x25519_dalek::PublicKey::from(&bob_static).to_bytes(),
                "2026-07-20T00:00:02Z",
            )
            .unwrap();
        assert_eq!(init_retry, init);

        let bob_spk_rotated = x25519_dalek::StaticSecret::from([47; 32]);
        let mut rotated_bundle = bundle.clone();
        rotated_bundle.bundle_id = "bob-bundle-2".to_owned();
        rotated_bundle.signed_prekey.key_id = "bob-spk-2".to_owned();
        rotated_bundle.signed_prekey.public_key_b64u =
            URL_SAFE_NO_PAD.encode(x25519_dalek::PublicKey::from(&bob_spk_rotated).to_bytes());
        bob_store
            .publish_local_bundle(
                &rotated_bundle,
                &bob_spk_rotated,
                &[],
                "2026-07-20T00:00:02Z",
            )
            .unwrap();
        drop(bob_store);
        let bob_store =
            SqliteV2DirectStateStore::new_with_secret_vault(&bob_db, bob_vault, bob_scope).unwrap();
        assert_eq!(
            bob_store
                .load_active_bundle()
                .unwrap()
                .unwrap()
                .bundle
                .bundle_id,
            "bob-bundle-2"
        );
        assert!(bob_store
            .load_accepted_bundle("bob-bundle-1", "2026-07-20T00:00:03Z")
            .unwrap()
            .is_some());

        let bob_runtime = V2EstablishedDirectRuntime::new(&bob_store);
        let accepted = bob_runtime
            .decrypt_inbound_init(
                &bob_binding,
                &init.metadata,
                init.init_body().unwrap(),
                &bob_static,
                &x25519_dalek::PublicKey::from(&alice_static).to_bytes(),
                "2026-07-20T00:00:03Z",
            )
            .unwrap();
        let V2InboundDecryptOutcome::Decrypted { plaintext, .. } = accepted else {
            panic!("fresh init must decrypt");
        };
        assert_eq!(
            classify_session_control(&plaintext).unwrap(),
            Some(V2SessionControlKind::Establish)
        );
        assert_eq!(
            plaintext.payload.unwrap()["system_type"],
            "awiki.device.session-establish.v1"
        );
        assert!(matches!(
            bob_runtime
                .root_control_session_readiness(&bob_binding)
                .unwrap(),
            V2RootControlSessionReadiness::Ready
        ));
        assert!(bob_store
            .load_available_opk(&bundle.bundle_id, &opk.key_id)
            .unwrap()
            .is_none());
        assert!(matches!(
            bob_runtime
                .decrypt_inbound_init(
                    &bob_binding,
                    &init.metadata,
                    init.init_body().unwrap(),
                    &bob_static,
                    &x25519_dalek::PublicKey::from(&alice_static).to_bytes(),
                    "2026-07-20T00:00:04Z",
                )
                .unwrap(),
            V2InboundDecryptOutcome::Replay { .. }
        ));
        assert!(restarted_alice.mark_outbound_accepted(&init_retry).unwrap());

        let reply_plaintext = session_established_plaintext("session-init-1").unwrap();
        let reply = bob_runtime
            .prepare_outbound(
                &bob_binding,
                "session-reply-1",
                &reply_plaintext,
                "2026-07-20T00:00:05Z",
            )
            .unwrap();
        let received_reply = restarted_alice
            .decrypt_inbound(
                &alice_binding,
                &reply.metadata,
                reply.cipher_body().unwrap(),
                "2026-07-20T00:00:06Z",
            )
            .unwrap();
        let V2InboundDecryptOutcome::Decrypted { plaintext, session } = received_reply else {
            panic!("first reply must decrypt");
        };
        assert_eq!(
            classify_session_control(&plaintext).unwrap(),
            Some(V2SessionControlKind::Established)
        );
        assert_eq!(session.status, V2_SESSION_STATUS_ESTABLISHED);
        assert!(matches!(
            restarted_alice
                .root_control_session_readiness(&alice_binding)
                .unwrap(),
            V2RootControlSessionReadiness::Pending(ref pending) if pending == &init
        ));
        let still_pending_error = restarted_alice
            .prepare_private_outbound_secret_json(
                &alice_binding,
                "root-before-init-cleanup",
                &root_secret_plaintext(),
                "2026-07-20T00:00:06Z",
                root_private_sidecar("root-before-init-cleanup", "alice-phone", "alice-laptop"),
            )
            .unwrap_err();
        assert!(matches!(
            still_pending_error,
            crate::ImError::UnsupportedCapability { capability }
                if capability == SESSION_ESTABLISHMENT_PENDING
        ));
        assert_eq!(
            alice_db
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_private_outbound",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert!(restarted_alice
            .complete_session_init_for_session(&alice_binding, &session.session_id)
            .unwrap());
        assert!(matches!(
            restarted_alice
                .root_control_session_readiness(&alice_binding)
                .unwrap(),
            V2RootControlSessionReadiness::Ready
        ));
        let root = restarted_alice
            .prepare_private_outbound_secret_json(
                &alice_binding,
                "root-after-confirmation",
                &root_secret_plaintext(),
                "2026-07-20T00:00:07Z",
                root_private_sidecar("root-after-confirmation", "alice-phone", "alice-laptop"),
            )
            .unwrap();
        assert!(matches!(root.body, V2DirectBody::Cipher(_)));

        let next = restarted_alice
            .prepare_outbound(
                &alice_binding,
                "post-establish-control",
                &init_plaintext,
                "2026-07-20T00:00:08Z",
            )
            .unwrap();
        assert!(matches!(next.body, V2DirectBody::Cipher(_)));
    }
}
