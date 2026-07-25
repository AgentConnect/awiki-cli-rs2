//! Minimal product runtime for established P5 v2 exact-device sessions.
//!
//! This layer is reusable by text/JSON callers. It owns no AWiki control
//! schema: callers provide a validated P5 application plaintext.

use anp::direct_e2ee::{
    direct_send_request_v2, parse_direct_send_result_v2, V2ApplicationPlaintext, V2DirectBody,
    V2DirectCipherBody, V2DirectE2eeSession, V2DirectInitBody, V2DirectMetadata,
    V2DirectSendResult, V2DirectSessionState, V2GetPrekeyBundleResult, V2PendingOutboundRecord,
    V2SecretJsonPayload, V2SessionBinding, V2Target, CONTENT_TYPE_DIRECT_CIPHER_V2,
    CONTENT_TYPE_DIRECT_INIT_V2, DIRECT_E2EE_PROFILE_V2, DIRECT_E2EE_SECURITY_PROFILE,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest as _, Sha256};

use super::v2_store::{SqliteV2DirectStateStore, V2InboundCommit, V2SessionExpectation};

pub(crate) const SESSION_ESTABLISHED_SYSTEM_TYPE: &str = "awiki.device.session-established.v1";
pub(crate) const SESSION_REPLY_OPERATION_PREFIX: &str = "p5-v2-session-reply:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V2SessionControlKind {
    Established,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct V2SessionControlPayload {
    system_type: String,
    #[serde(default)]
    init_message_id: Option<String>,
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

pub(crate) fn session_reply_operation_id(init_message_id: &str) -> crate::ImResult<String> {
    session_control_operation_id(SESSION_REPLY_OPERATION_PREFIX, init_message_id)
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
    if system_type != SESSION_ESTABLISHED_SYSTEM_TYPE {
        return Ok(None);
    }
    let control: V2SessionControlPayload =
        serde_json::from_value(payload.clone()).map_err(serialization_error)?;
    match control.system_type.as_str() {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V2ExactSessionPreflight {
    Established,
    Absent,
    Conflict,
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

    /// Generic exact-device send preflight. Only an established Session with
    /// no retained Init is reusable; pending-confirmation is a conflict, not
    /// an established channel and not permission to derive a parallel Init.
    pub(crate) fn exact_session_preflight(
        &self,
        binding: &V2SessionBinding,
    ) -> crate::ImResult<V2ExactSessionPreflight> {
        if let Some(selected) = self.store.select_established_session(binding)? {
            if selected.state.disabled
                || self
                    .store
                    .select_pending_init_operation(binding, &selected.state.session_id)?
                    .is_some()
            {
                return Ok(V2ExactSessionPreflight::Conflict);
            }
            return Ok(V2ExactSessionPreflight::Established);
        }
        if self
            .store
            .select_pending_confirmation_session(binding)?
            .is_some()
        {
            return Ok(V2ExactSessionPreflight::Conflict);
        }
        Ok(V2ExactSessionPreflight::Absent)
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
        self.prepare_outbound_inner(binding, operation_id, plaintext, now)
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
            return prepared_from_pending(existing);
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
        })
    }

    /// Secret-payload equivalent of `prepare_session_init`. It persists the
    /// exact Init/session state before transport and never materializes the
    /// application payload as a public `serde_json::Value`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_session_init_secret_json(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2SecretJsonPayload,
        local_static_private: &x25519_dalek::StaticSecret,
        recipient: &V2GetPrekeyBundleResult,
        recipient_static_public: &[u8; 32],
        now: &str,
    ) -> crate::ImResult<PreparedV2Outbound> {
        self.prepare_session_init_secret_json_with_commit(
            binding,
            operation_id,
            plaintext,
            local_static_private,
            recipient,
            recipient_static_public,
            now,
            |_| Ok(()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_session_init_secret_json_with_commit(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2SecretJsonPayload,
        local_static_private: &x25519_dalek::StaticSecret,
        recipient: &V2GetPrekeyBundleResult,
        recipient_static_public: &[u8; 32],
        now: &str,
        commit: impl FnOnce(&rusqlite::Transaction<'_>) -> crate::ImResult<()>,
    ) -> crate::ImResult<PreparedV2Outbound> {
        binding.validate().map_err(v2_error)?;
        recipient.validate().map_err(v2_error)?;
        require_operation_id(operation_id)?;
        if recipient.target_did != binding.peer_did
            || recipient.target_device_id != binding.peer_device_id
            || recipient.prekey_bundle.static_key_agreement_id != binding.peer_e2ee_key_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if self.store.load_pending(binding, operation_id)?.is_some()
            || self.store.select_established_session(binding)?.is_some()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let metadata =
            outbound_metadata_for_content_type(binding, operation_id, CONTENT_TYPE_DIRECT_INIT_V2);
        let (state, pending, body) = V2DirectE2eeSession::initiate_session_secret_json(
            binding,
            &metadata,
            local_static_private,
            &recipient.prekey_bundle,
            recipient_static_public,
            recipient.one_time_prekey.as_ref(),
            plaintext,
        )
        .map_err(v2_error)?;
        self.store.commit_outbound_with(
            &state,
            &pending,
            V2SessionExpectation::Absent,
            now,
            commit,
        )?;
        Ok(PreparedV2Outbound {
            binding: binding.clone(),
            metadata,
            body: V2DirectBody::Init(body),
        })
    }

    /// Encrypts a secret JSON payload on one established exact-device Session.
    /// Repeating the same operation resumes the byte-identical pending record.
    pub(crate) fn prepare_outbound_secret_json(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2SecretJsonPayload,
        now: &str,
    ) -> crate::ImResult<PreparedV2Outbound> {
        self.prepare_outbound_secret_json_with_commit(binding, operation_id, plaintext, now, |_| {
            Ok(())
        })
    }

    pub(crate) fn prepare_outbound_secret_json_with_commit(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2SecretJsonPayload,
        now: &str,
        commit: impl FnOnce(&rusqlite::Transaction<'_>) -> crate::ImResult<()>,
    ) -> crate::ImResult<PreparedV2Outbound> {
        binding.validate().map_err(v2_error)?;
        require_operation_id(operation_id)?;
        if let Some(existing) = self.store.load_pending(binding, operation_id)? {
            let stored = self
                .store
                .load_session(binding, &existing.session_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
            self.store.commit_outbound_with(
                &stored.state,
                &existing,
                V2SessionExpectation::Revision(stored.revision),
                now,
                commit,
            )?;
            return prepared_from_pending(existing);
        }
        let stored = self
            .store
            .select_established_session(binding)?
            .ok_or_else(|| crate::ImError::unsupported("p5-v2-established-session-required"))?;
        if self
            .store
            .select_pending_init_operation(binding, &stored.state.session_id)?
            .is_some()
        {
            return Err(crate::ImError::PermissionDenied);
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
        self.store.commit_outbound_with(
            &next_state,
            &pending,
            V2SessionExpectation::Revision(stored.revision),
            now,
            commit,
        )?;
        Ok(PreparedV2Outbound {
            binding: binding.clone(),
            metadata,
            body: V2DirectBody::Cipher(body),
        })
    }

    fn prepare_outbound_inner(
        &self,
        binding: &V2SessionBinding,
        operation_id: &str,
        plaintext: &V2ApplicationPlaintext,
        now: &str,
    ) -> crate::ImResult<PreparedV2Outbound> {
        binding.validate().map_err(v2_error)?;
        plaintext.validate().map_err(v2_error)?;
        require_operation_id(operation_id)?;
        if let Some(existing) = self.store.load_pending(binding, operation_id)? {
            return prepared_from_pending(existing);
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
        self.store.commit_outbound(
            &next_state,
            &pending,
            V2SessionExpectation::Revision(stored.revision),
            now,
        )?;
        Ok(PreparedV2Outbound {
            binding: binding.clone(),
            metadata,
            body: V2DirectBody::Cipher(body),
        })
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
        prepared_from_pending(pending).map(Some)
    }

    /// Restores an exact durable retry after process loss without reconstructing
    /// or advancing any ratchet state.
    pub(crate) fn resume_outbound_for_exact_device(
        &self,
        operation_id: &str,
        expected_peer_device_id: &str,
    ) -> crate::ImResult<Option<PreparedV2Outbound>> {
        let Some(pending) = self
            .store
            .load_pending_for_exact_device(operation_id, expected_peer_device_id)?
        else {
            return Ok(None);
        };
        prepared_from_pending(pending).map(Some)
    }

    pub(crate) fn mark_outbound_accepted(
        &self,
        prepared: &PreparedV2Outbound,
    ) -> crate::ImResult<bool> {
        self.store
            .mark_pending_accepted(&prepared.binding(), &prepared.metadata.operation_id)
    }

    pub(crate) fn mark_outbound_accepted_with_commit(
        &self,
        prepared: &PreparedV2Outbound,
        hook: impl FnOnce(&rusqlite::Transaction<'_>) -> crate::ImResult<()>,
    ) -> crate::ImResult<bool> {
        self.store.mark_pending_accepted_with_commit(
            &prepared.binding(),
            &prepared.metadata.operation_id,
            hook,
        )
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
        self.decrypt_inbound_secret_json_validated_with_commit(
            binding,
            metadata,
            body,
            now,
            |plaintext, _, next_state| validator(plaintext, next_state),
            |_, _| Ok(()),
        )
    }

    /// Secret Cipher validation with one secret-free caller update committed
    /// atomically alongside the ratchet and replay marker.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decrypt_inbound_secret_json_validated_with_commit<T>(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectCipherBody,
        now: &str,
        validator: impl FnOnce(
            &V2SecretJsonPayload,
            &V2DirectSessionState,
            &V2DirectSessionState,
        ) -> crate::ImResult<T>,
        commit: impl FnOnce(&rusqlite::Transaction<'_>, &T) -> crate::ImResult<()>,
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
        let pre_state = stored.state;
        let mut next_state = pre_state.clone();
        let plaintext = V2DirectE2eeSession::decrypt_follow_up_secret_json(
            &mut next_state,
            binding,
            metadata,
            body,
        )
        .map_err(v2_error)?;
        let validated = validator(&plaintext, &pre_state, &next_state)?;
        match self.store.commit_inbound_with(
            &next_state,
            &metadata.message_id,
            &digest,
            None,
            V2SessionExpectation::Revision(stored.revision),
            now,
            |transaction| commit(transaction, &validated),
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

    /// Secret Init validation with one secret-free caller update committed
    /// atomically alongside Session creation, replay, and OPK consumption.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decrypt_inbound_init_secret_json_validated_with_commit<T>(
        &self,
        binding: &V2SessionBinding,
        metadata: &V2DirectMetadata,
        body: &V2DirectInitBody,
        local_static_private: &x25519_dalek::StaticSecret,
        sender_static_public: &[u8; 32],
        now: &str,
        validator: impl FnOnce(&V2SecretJsonPayload, &V2DirectSessionState) -> crate::ImResult<T>,
        commit: impl FnOnce(&rusqlite::Transaction<'_>, &T) -> crate::ImResult<()>,
    ) -> crate::ImResult<V2ValidatedSecretInboundOutcome<T>> {
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
            return Ok(V2ValidatedSecretInboundOutcome::Replay {
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
        let (next_state, plaintext, consumed_opk_id) =
            V2DirectE2eeSession::accept_incoming_init_secret_json(
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
        match self.store.commit_inbound_with(
            &next_state,
            &metadata.message_id,
            &digest,
            consumed_opk_id.as_deref(),
            V2SessionExpectation::Absent,
            now,
            |transaction| commit(transaction, &validated),
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

fn prepared_from_pending(pending: V2PendingOutboundRecord) -> crate::ImResult<PreparedV2Outbound> {
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
        V2DirectSessionState, DIRECT_E2EE_V2_SESSION_STATE_FORMAT, MTI_DIRECT_E2EE_SUITE_V2,
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
    fn root_sender_ledger_and_p5_pending_share_one_sqlite_transaction() {
        let root = tempfile::tempdir().unwrap();
        let connection = Connection::open(root.path().join("sender.sqlite")).unwrap();
        let secret_vault = vault(&root.path().join("sender-vault"), 71);
        let (sender_state, _) = established_pair();
        let sender_scope = scope(
            "identity-alice-phone",
            "did:example:alice",
            "alice-phone",
            "did:example:alice#phone-e2ee",
        );
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            secret_vault,
            sender_scope,
        )
        .unwrap();
        store
            .commit_inbound(
                &sender_state,
                "sender-setup",
                "sha256:sender-setup",
                None,
                V2SessionExpectation::Absent,
                "2026-07-20T00:00:00Z",
            )
            .unwrap();
        let runtime = V2EstablishedDirectRuntime::new(&store);
        runtime
            .prepare_outbound_secret_json_with_commit(
                &sender_state.binding,
                "root-atomic-1",
                &root_secret_plaintext(),
                "2026-07-20T00:00:01Z",
                |transaction| {
                    transaction
                        .execute(
                            r#"INSERT INTO identity_root_transfer_sender_v1 (
owner_identity_id, owner_did, local_device_id, message_id,
recipient_device_id, phase, created_at, updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, 'pending_delivery', ?6, ?6)"#,
                            rusqlite::params![
                                "identity-alice-phone",
                                "did:example:alice",
                                "alice-phone",
                                "root-atomic-1",
                                "alice-laptop",
                                "2026-07-20T00:00:01Z",
                            ],
                        )
                        .map(|_| ())
                        .map_err(crate::internal::local_state::local_state_unavailable)
                },
            )
            .unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_pending WHERE operation_id = 'root-atomic-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM identity_root_transfer_sender_v1 WHERE message_id = 'root-atomic-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );

        let failed = runtime.prepare_outbound_secret_json_with_commit(
            &sender_state.binding,
            "root-atomic-rollback",
            &root_secret_plaintext(),
            "2026-07-20T00:00:02Z",
            |_| Err(crate::ImError::PermissionDenied),
        );
        assert!(failed.is_err());
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_pending WHERE operation_id = 'root-atomic-rollback'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }
}
