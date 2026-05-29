use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anp::direct_e2ee::{
    ApplicationPlaintext, DirectE2eeError, DirectE2eeSession, DirectEnvelopeMetadata,
    OneTimePrekey, PrekeyBundle,
};
use anp::{PrivateKeyMaterial, PublicKeyMaterial};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

pub(crate) type DirectSecureFileRuntimeRpc =
    dyn FnMut(&str, Map<String, Value>) -> crate::ImResult<Map<String, Value>>;
pub(crate) type DirectSecureFileRuntimeResolver = dyn FnMut(&str) -> crate::ImResult<Value>;

const DIRECT_E2EE_PROFILE: &str = "anp.direct.e2ee.v1";
const DIRECT_E2EE_SECURITY_PROFILE: &str = "direct-e2ee";
const DEFAULT_SIGNED_PREKEY_ID: &str = "spk-initial";
const DEFAULT_SIGNED_PREKEY_EXPIRY: &str = "2030-01-01T00:00:00Z";
const DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE: usize = 16;
const SECURE_SESSION_DIR_NAME: &str = "p5-e2ee-sessions";
const SECURE_SIGNED_PREKEY_DIR_NAME: &str = "p5-signed-prekeys";
const SECURE_ONE_TIME_PREKEY_DIR_NAME: &str = "p5-one-time-prekeys";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DirectSecureFileRuntimeIdentity {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) identity_name: String,
    pub(crate) identity_dir: PathBuf,
    pub(crate) signing_private_pem: String,
    pub(crate) agreement_private_pem: String,
    pub(crate) local_did_document: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectSecureFileOutboxFlushScope {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) credential_name: String,
    pub(crate) sqlite_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DirectSecureLocalAckInput {
    pub(crate) sender_did: String,
    pub(crate) recipient_did: String,
    pub(crate) sender_identity_dir: PathBuf,
    pub(crate) recipient: DirectSecureLocalAckRecipient,
    pub(crate) session_id: String,
    pub(crate) replied_message_id: String,
    pub(crate) ack_message_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DirectSecureLocalAckRecipient {
    pub(crate) identity: DirectSecureFileRuntimeIdentity,
    pub(crate) session_id: String,
}

struct PreparedDirectSecureFileRuntimeClient {
    owner_identity_id: String,
    owner_did: String,
    identity_name: String,
    local_service_did: String,
    signing_key_id: String,
    agreement_key_id: String,
    signing_private: PrivateKeyMaterial,
    agreement_private: PrivateKeyMaterial,
    session_store: FileSessionStore,
    signed_prekey_store: FileSignedPrekeyStore,
    one_time_prekey_store: FileOneTimePrekeyStore,
}

pub(crate) struct DirectSecureFileRuntimeClient {
    prepared: PreparedDirectSecureFileRuntimeClient,
    rpc: Box<DirectSecureFileRuntimeRpc>,
    resolver: Box<DirectSecureFileRuntimeResolver>,
    pending_by_peer: HashMap<String, Vec<Map<String, Value>>>,
}

impl DirectSecureFileRuntimeClient {
    pub(crate) fn new(
        identity: DirectSecureFileRuntimeIdentity,
        rpc: Box<DirectSecureFileRuntimeRpc>,
        mut resolver: Box<DirectSecureFileRuntimeResolver>,
    ) -> crate::ImResult<Self> {
        let prepared = prepare_direct_secure_file_runtime_client(identity, &mut resolver)?;
        Ok(Self {
            prepared,
            rpc,
            resolver,
            pending_by_peer: HashMap::new(),
        })
    }

    pub(crate) fn publish_prekey_bundle(&mut self) -> crate::ImResult<Map<String, Value>> {
        let bundle = self.ensure_fresh_prekey_bundle()?;
        self.publish_prekey_bundle_rpc(&bundle)
    }

    pub(crate) fn ensure_fresh_prekey_bundle(&mut self) -> crate::ImResult<PrekeyBundle> {
        self.ensure_fresh_one_time_prekeys(DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE)?;
        let signed_prekey = match self
            .prepared
            .signed_prekey_store
            .load_latest_signed_prekey()
        {
            Ok(Some((_private_key, metadata))) => metadata,
            Ok(None) => {
                let private_key = generated_x25519_private_key()?;
                let metadata = signed_prekey_from_private_key(
                    DEFAULT_SIGNED_PREKEY_ID,
                    &private_key,
                    DEFAULT_SIGNED_PREKEY_EXPIRY,
                )?;
                self.prepared
                    .signed_prekey_store
                    .save_signed_prekey(&metadata.key_id, &private_key, &metadata)
                    .map_err(map_direct_error)?;
                metadata
            }
            Err(err) => return Err(map_direct_error(err)),
        };
        self.build_prekey_bundle(signed_prekey)
    }

    pub(crate) fn send_text(
        &mut self,
        peer_did: &str,
        text: &str,
        operation_id: &str,
        message_id: &str,
    ) -> crate::ImResult<Map<String, Value>> {
        self.send_application_plaintext(
            peer_did,
            ApplicationPlaintext::new_text("text/plain", text),
            operation_id,
            message_id,
        )
    }

    pub(crate) fn send_json(
        &mut self,
        peer_did: &str,
        payload: Map<String, Value>,
        operation_id: &str,
        message_id: &str,
    ) -> crate::ImResult<Map<String, Value>> {
        self.send_application_plaintext(
            peer_did,
            ApplicationPlaintext::new_json("application/json", Value::Object(payload)),
            operation_id,
            message_id,
        )
    }

    pub(crate) fn process_incoming(
        &mut self,
        message: Map<String, Value>,
    ) -> crate::ImResult<Map<String, Value>> {
        let meta = message.get("meta").and_then(Value::as_object);
        let sender_did = string_value(meta.and_then(|value| value.get("sender_did")));
        let recipient_did = meta
            .and_then(|value| value.get("target"))
            .and_then(Value::as_object)
            .and_then(|value| value.get("did"))
            .map(string_value_from_value)
            .unwrap_or_default();
        let content_type = string_value(meta.and_then(|value| value.get("content_type")));
        let metadata = DirectEnvelopeMetadata {
            sender_did: sender_did.clone(),
            recipient_did,
            message_id: string_value(meta.and_then(|value| value.get("message_id"))),
            profile: string_value(meta.and_then(|value| value.get("profile"))),
            security_profile: string_value(meta.and_then(|value| value.get("security_profile"))),
        };
        let body = message.get("body").cloned().unwrap_or(Value::Null);
        match content_type.as_str() {
            "application/anp-direct-init+json" => {
                self.process_incoming_init(&sender_did, &metadata, &body)
            }
            "application/anp-direct-cipher+json" => {
                self.process_incoming_cipher(message, &sender_did, &metadata, &body)
            }
            _ => Err(crate::ImError::Serialization {
                detail: format!("unsupported direct E2EE content type: {content_type}"),
            }),
        }
    }

    pub(crate) fn current_session_id(&self, peer_did: &str) -> String {
        self.prepared
            .session_store
            .find_by_peer_did(peer_did)
            .ok()
            .flatten()
            .map(|session| session.session_id)
            .unwrap_or_default()
    }

    fn send_application_plaintext(
        &mut self,
        peer_did: &str,
        plaintext: ApplicationPlaintext,
        operation_id: &str,
        message_id: &str,
    ) -> crate::ImResult<Map<String, Value>> {
        anp::direct_e2ee::validate_direct_send_ids(operation_id, message_id)
            .map_err(map_direct_error)?;
        let peer_did = required("peer_did", peer_did)?;
        let metadata = DirectEnvelopeMetadata {
            sender_did: self.prepared.owner_did.clone(),
            recipient_did: peer_did.clone(),
            message_id: message_id.to_owned(),
            profile: DIRECT_E2EE_PROFILE.to_owned(),
            security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
        };
        let mut session = self
            .prepared
            .session_store
            .find_by_peer_did(&peer_did)
            .map_err(map_direct_error)?;
        if let Some(mut session) = session.take() {
            let (_pending, body) = DirectE2eeSession::encrypt_follow_up(
                &mut session,
                &metadata,
                operation_id,
                &plaintext,
            )
            .map_err(map_direct_error)?;
            self.prepared
                .session_store
                .save_session(&session)
                .map_err(map_direct_error)?;
            let request = anp::direct_e2ee::direct_cipher_send_request(
                &self.prepared.owner_did,
                &peer_did,
                operation_id,
                message_id,
                &body,
            )
            .map_err(map_direct_error)?;
            return self.call_request(request);
        }

        let verified = self.get_verified_prekey_bundle(&peer_did)?;
        let did_document = (self.resolver)(&peer_did)?;
        let recipient_static_public = anp::direct_e2ee::extract_x25519_public_key(
            &did_document,
            &verified.bundle.static_key_agreement_id,
        )
        .map_err(map_direct_error)?;
        let recipient_signed_prekey_public = decode_public_key_b64u(
            &verified.bundle.signed_prekey.public_key_b64u,
            "signed_prekey.public_key_b64u",
        )?;
        let (recipient_one_time_prekey_public, recipient_one_time_prekey_id) =
            if let Some(one_time_prekey) = &verified.one_time_prekey {
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
        let PrivateKeyMaterial::X25519(agreement_private) = &self.prepared.agreement_private else {
            return Err(expected_x25519_private_key());
        };
        let (session, _pending, body) = DirectE2eeSession::initiate_session_with_opk(
            &metadata,
            operation_id,
            &self.prepared.agreement_key_id,
            agreement_private,
            &verified.bundle,
            &recipient_static_public,
            &recipient_signed_prekey_public,
            recipient_one_time_prekey_public.as_ref(),
            recipient_one_time_prekey_id,
            &plaintext,
        )
        .map_err(map_direct_error)?;
        self.prepared
            .session_store
            .save_session(&session)
            .map_err(map_direct_error)?;
        let request = anp::direct_e2ee::direct_init_send_request(
            &self.prepared.owner_did,
            &peer_did,
            operation_id,
            message_id,
            &body,
        )
        .map_err(map_direct_error)?;
        self.call_request(request)
    }

    fn process_incoming_init(
        &mut self,
        sender_did: &str,
        metadata: &DirectEnvelopeMetadata,
        body: &Value,
    ) -> crate::ImResult<Map<String, Value>> {
        let init_body = super::wire::direct_init_body_from_value(body);
        let existing_session = self.existing_session(&init_body.session_id)?;
        let sender_document = (self.resolver)(sender_did)?;
        let sender_static_public = anp::direct_e2ee::extract_x25519_public_key(
            &sender_document,
            &init_body.sender_static_key_agreement_id,
        )
        .map_err(map_direct_error)?;
        let (signed_prekey_material, _metadata) = self
            .prepared
            .signed_prekey_store
            .load_signed_prekey(&init_body.recipient_signed_prekey_id)
            .map_err(map_direct_error)?;
        let PrivateKeyMaterial::X25519(signed_prekey_private) = &signed_prekey_material else {
            return Err(expected_x25519_private_key());
        };
        let one_time_prekey_id = init_body
            .recipient_one_time_prekey_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let one_time_prekey_material = if let Some(key_id) = one_time_prekey_id.as_deref() {
            let (material, _metadata) = self
                .prepared
                .one_time_prekey_store
                .load_one_time_prekey(key_id)
                .map_err(map_direct_error)?;
            Some(material)
        } else {
            None
        };
        let one_time_prekey_private = match one_time_prekey_material.as_ref() {
            Some(PrivateKeyMaterial::X25519(key)) => Some(key),
            Some(_) => return Err(expected_x25519_private_key()),
            None => None,
        };
        let PrivateKeyMaterial::X25519(agreement_private) = &self.prepared.agreement_private else {
            return Err(expected_x25519_private_key());
        };
        let (session, plaintext) = DirectE2eeSession::accept_incoming_init_with_opk(
            metadata,
            &self.prepared.agreement_key_id,
            agreement_private,
            signed_prekey_private,
            one_time_prekey_private,
            &sender_static_public,
            &init_body,
        )
        .map_err(map_direct_error)?;
        if let Some(existing_session) = existing_session {
            if existing_session.peer_did != sender_did {
                return Err(crate::ImError::Serialization {
                    detail: "direct E2EE init session id is already bound to another peer"
                        .to_owned(),
                });
            }
            return Ok(decrypted_plaintext_result(&plaintext));
        }
        if let Some(key_id) = one_time_prekey_id {
            self.prepared
                .one_time_prekey_store
                .delete_one_time_prekey(&key_id)
                .map_err(map_direct_error)?;
        }
        self.prepared
            .session_store
            .save_session(&session)
            .map_err(map_direct_error)?;
        let mut result = decrypted_plaintext_result(&plaintext);
        if let Some(pending) = self.pending_by_peer.get(sender_did).cloned() {
            if !pending.is_empty() {
                let mut pending_results = Vec::new();
                for pending_message in pending {
                    if let Ok(result) = self.process_incoming(pending_message) {
                        pending_results.push(Value::Object(result));
                    }
                }
                self.pending_by_peer.remove(sender_did);
                result.insert("pending_results".to_owned(), Value::Array(pending_results));
            }
        }
        Ok(result)
    }

    fn process_incoming_cipher(
        &mut self,
        message: Map<String, Value>,
        sender_did: &str,
        metadata: &DirectEnvelopeMetadata,
        body: &Value,
    ) -> crate::ImResult<Map<String, Value>> {
        let cipher_body = super::wire::direct_cipher_body_from_value(body);
        let mut session = match self
            .prepared
            .session_store
            .load_session(&cipher_body.session_id)
        {
            Ok(session) => session,
            Err(_) => {
                self.pending_by_peer
                    .entry(sender_did.to_owned())
                    .or_default()
                    .push(message);
                return Ok(Map::from_iter([(
                    "state".to_owned(),
                    Value::String("pending".to_owned()),
                )]));
            }
        };
        match DirectE2eeSession::decrypt_follow_up(&mut session, metadata, &cipher_body, "") {
            Ok(plaintext) => {
                self.prepared
                    .session_store
                    .save_session(&session)
                    .map_err(map_direct_error)?;
                Ok(Map::from_iter([
                    ("state".to_owned(), Value::String("decrypted".to_owned())),
                    (
                        "plaintext".to_owned(),
                        anp::direct_e2ee::plaintext_to_value(&plaintext),
                    ),
                ]))
            }
            Err(_) => Ok(Map::from_iter([(
                "state".to_owned(),
                Value::String("undecryptable".to_owned()),
            )])),
        }
    }

    fn get_verified_prekey_bundle(
        &mut self,
        target_did: &str,
    ) -> crate::ImResult<VerifiedPrekeyBundle> {
        let did_document = (self.resolver)(target_did)?;
        let target_service_did = anp::direct_e2ee::message_service_did_from_document(&did_document)
            .map_err(map_direct_error)?;
        let response =
            match self.fetch_prekey_bundle_response(target_did, &target_service_did, true) {
                Ok(response) => response,
                Err(err)
                    if anp::direct_e2ee::should_retry_without_opk_message(&err.to_string()) =>
                {
                    self.fetch_prekey_bundle_response(target_did, &target_service_did, false)?
                }
                Err(err) => return Err(err),
            };
        let bundle_value = response
            .get("prekey_bundle")
            .cloned()
            .ok_or_else(|| missing_field("prekey_bundle"))?;
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
        Ok(VerifiedPrekeyBundle {
            bundle,
            one_time_prekey,
        })
    }

    fn fetch_prekey_bundle_response(
        &mut self,
        target_did: &str,
        target_service_did: &str,
        require_opk: bool,
    ) -> crate::ImResult<Map<String, Value>> {
        let operation_id = format!("op-get-prekey-{}", operation_nonce_hex());
        let request = anp::direct_e2ee::prekey_bundle_get_request(
            &self.prepared.owner_did,
            target_service_did,
            target_did,
            require_opk,
            &operation_id,
        );
        self.call_request(request)
    }

    fn publish_prekey_bundle_rpc(
        &mut self,
        bundle: &PrekeyBundle,
    ) -> crate::ImResult<Map<String, Value>> {
        let one_time_prekeys = self
            .prepared
            .one_time_prekey_store
            .list_one_time_prekeys()
            .map_err(map_direct_error)?;
        let request = anp::direct_e2ee::prekey_bundle_publish_request(
            &self.prepared.owner_did,
            &self.prepared.local_service_did,
            bundle,
            &one_time_prekeys,
        );
        self.call_request(request)
    }

    fn build_prekey_bundle(
        &self,
        signed_prekey: anp::direct_e2ee::SignedPrekey,
    ) -> crate::ImResult<PrekeyBundle> {
        anp::direct_e2ee::build_prekey_bundle(
            &format!("spk-{}-{}", unix_seconds(), signed_prekey.key_id),
            &self.prepared.owner_did,
            &self.prepared.agreement_key_id,
            signed_prekey,
            &self.prepared.signing_private,
            &self.prepared.signing_key_id,
            None,
        )
        .map_err(map_direct_error)
    }

    fn ensure_fresh_one_time_prekeys(&mut self, min_count: usize) -> crate::ImResult<()> {
        if min_count == 0 {
            return Ok(());
        }
        let current = self
            .prepared
            .one_time_prekey_store
            .list_one_time_prekeys()
            .map_err(map_direct_error)?;
        if current.len() >= min_count {
            return Ok(());
        }
        let prefix = unix_nanos();
        for index in current.len()..min_count {
            let key_id = format!("opk-{prefix}-{index:03}");
            let private_key = generated_x25519_private_key()?;
            let metadata = one_time_prekey_from_private_key(&key_id, &private_key)?;
            self.prepared
                .one_time_prekey_store
                .save_one_time_prekey(&key_id, &private_key, &metadata)
                .map_err(map_direct_error)?;
        }
        Ok(())
    }

    fn call_request(&mut self, request: Value) -> crate::ImResult<Map<String, Value>> {
        let object = request
            .as_object()
            .ok_or_else(|| missing_field("request"))?;
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| missing_field("method"))?
            .to_owned();
        let params = object
            .get("params")
            .and_then(Value::as_object)
            .ok_or_else(|| missing_field("params"))?
            .clone();
        (self.rpc)(&method, params)
    }

    fn existing_session(
        &self,
        session_id: &str,
    ) -> crate::ImResult<Option<anp::direct_e2ee::DirectSessionState>> {
        if session_id.trim().is_empty() {
            return Ok(None);
        }
        match self.prepared.session_store.load_session(session_id) {
            Ok(session) => Ok(Some(session)),
            Err(DirectE2eeError::SessionNotFound(_)) => Ok(None),
            Err(err) => Err(map_direct_error(err)),
        }
    }
}

#[cfg(feature = "blocking")]
pub(crate) fn flush_direct_secure_file_outbox(
    scope: &DirectSecureFileOutboxFlushScope,
    peer_filter_did: &str,
    client: &mut DirectSecureFileRuntimeClient,
) -> Vec<String> {
    let connection = match crate::internal::local_state::open_writable(&scope.sqlite_path) {
        Ok(connection) => connection,
        Err(err) => return vec![format!("Failed to open secure outbox store: {err}")],
    };
    let outbox_scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: scope.owner_identity_id.clone(),
        owner_did: scope.owner_did.clone(),
        credential_name: scope.credential_name.clone(),
    };
    super::outbox::flush_queued_secure_outbox_with_sender(
        &connection,
        &outbox_scope,
        peer_filter_did,
        |request| {
            let send = match request.original_type.as_str() {
                "text" | "" => client.send_text(
                    &request.target_did,
                    &request.plaintext,
                    &request.outbox_id,
                    &request.outbox_id,
                ),
                "json" => client.send_json(
                    &request.target_did,
                    request.json_payload.unwrap_or_default(),
                    &request.outbox_id,
                    &request.outbox_id,
                ),
                _ => Err(crate::ImError::invalid_input(
                    Some("original_type".to_owned()),
                    format!("unsupported original_type: {}", request.original_type),
                )),
            };
            let send = match send {
                Ok(result) => super::outbox::SecureOutboxSendOutcome::Success {
                    message_id: string_value(result.get("message_id")),
                    operation_id: string_value(result.get("operation_id")),
                    delivery_state: string_value(result.get("delivery_state")),
                    accepted_at: string_value(result.get("accepted_at")),
                },
                Err(err) => super::outbox::SecureOutboxSendOutcome::Error(err.to_string()),
            };
            let session_id = match &send {
                super::outbox::SecureOutboxSendOutcome::Success { .. } => {
                    client.current_session_id(&request.target_did)
                }
                super::outbox::SecureOutboxSendOutcome::Error(_) => String::new(),
            };
            super::outbox::SecureOutboxSendResult { send, session_id }
        },
    )
}

#[cfg(not(feature = "blocking"))]
pub(crate) fn flush_direct_secure_file_outbox(
    _scope: &DirectSecureFileOutboxFlushScope,
    _peer_filter_did: &str,
    _client: &mut DirectSecureFileRuntimeClient,
) -> Vec<String> {
    vec!["direct secure file outbox flush is disabled in the async cutover build".to_owned()]
}

pub(crate) fn encrypt_direct_secure_file_ack(
    input: &DirectSecureLocalAckInput,
) -> crate::ImResult<Value> {
    let mut sender_store =
        FileSessionStore::new(input.sender_identity_dir.join(SECURE_SESSION_DIR_NAME))
            .map_err(map_direct_error)?;
    let mut sender_session = sender_store
        .find_by_peer_did(&input.recipient_did)
        .map_err(map_direct_error)?
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: format!(
                "direct secure session not found for peer {}",
                input.recipient_did
            ),
        })?;
    let metadata = direct_envelope_metadata(
        &input.sender_did,
        &input.recipient_did,
        &input.ack_message_id,
    );
    let ack_plaintext = ApplicationPlaintext::new_json(
        "application/json",
        Value::Object(super::control::build_secure_ack_payload(
            &input.session_id,
            &input.replied_message_id,
        )),
    );
    let (_pending, ack_body) = DirectE2eeSession::encrypt_follow_up(
        &mut sender_session,
        &metadata,
        &input.ack_message_id,
        &ack_plaintext,
    )
    .map_err(map_direct_error)?;
    let ack_body_value =
        serde_json::to_value(&ack_body).map_err(|err| crate::ImError::Serialization {
            detail: format!("serialize direct secure ack body: {err}"),
        })?;
    if !can_process_direct_secure_file_ack(
        input.recipient.clone(),
        &input.sender_did,
        &input.ack_message_id,
        &ack_body_value,
    ) {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "recipient could not process direct secure ACK".to_owned(),
        });
    }
    sender_store
        .save_session(&sender_session)
        .map_err(map_direct_error)?;
    Ok(ack_body_value)
}

pub(crate) fn can_process_direct_secure_file_ack(
    recipient: DirectSecureLocalAckRecipient,
    metadata_sender_did: &str,
    ack_message_id: &str,
    ack_body: &Value,
) -> bool {
    let metadata = direct_envelope_metadata(
        metadata_sender_did,
        &recipient.identity.owner_did,
        ack_message_id,
    );
    let local_document = recipient.identity.local_did_document.clone();
    let owner_did = recipient.identity.owner_did.clone();
    let mut client = match DirectSecureFileRuntimeClient::new(
        recipient.identity.clone(),
        Box::new(|_, _| {
            Err(crate::ImError::TransportUnavailable {
                detail: "local secure ack delivery does not use outbound rpc".to_owned(),
            })
        }),
        Box::new(move |did| {
            if did == owner_did {
                return Ok(local_document.clone());
            }
            Err(crate::ImError::TransportUnavailable {
                detail: format!("local DID document unavailable for {did}"),
            })
        }),
    ) {
        Ok(client) => client,
        Err(_) => return false,
    };
    let processed = client.process_incoming(Map::from_iter([
        (
            "meta".to_owned(),
            Value::Object(Map::from_iter([
                (
                    "sender_did".to_owned(),
                    Value::String(metadata.sender_did.clone()),
                ),
                (
                    "target".to_owned(),
                    Value::Object(Map::from_iter([
                        ("kind".to_owned(), Value::String("agent".to_owned())),
                        (
                            "did".to_owned(),
                            Value::String(metadata.recipient_did.clone()),
                        ),
                    ])),
                ),
                (
                    "message_id".to_owned(),
                    Value::String(metadata.message_id.clone()),
                ),
                (
                    "profile".to_owned(),
                    Value::String(metadata.profile.clone()),
                ),
                (
                    "security_profile".to_owned(),
                    Value::String(metadata.security_profile.clone()),
                ),
                (
                    "content_type".to_owned(),
                    Value::String("application/anp-direct-cipher+json".to_owned()),
                ),
            ])),
        ),
        ("body".to_owned(), ack_body.clone()),
    ]));
    if processed
        .ok()
        .is_some_and(|result| string_value(result.get("state")) == "decrypted")
    {
        return true;
    }

    let mut recipient_store = match FileSessionStore::new(
        recipient
            .identity
            .identity_dir
            .join(SECURE_SESSION_DIR_NAME),
    ) {
        Ok(store) => store,
        Err(_) => return false,
    };
    let mut recipient_session = match recipient_store.load_session(&recipient.session_id) {
        Ok(session) => session,
        Err(_) => return false,
    };
    let direct_body = super::wire::direct_cipher_body_from_value(ack_body);
    if DirectE2eeSession::decrypt_follow_up(&mut recipient_session, &metadata, &direct_body, "")
        .is_err()
    {
        return false;
    }
    recipient_store.save_session(&recipient_session).is_ok()
}

fn decrypted_plaintext_result(plaintext: &ApplicationPlaintext) -> Map<String, Value> {
    Map::from_iter([
        ("state".to_owned(), Value::String("decrypted".to_owned())),
        (
            "plaintext".to_owned(),
            anp::direct_e2ee::plaintext_to_value(plaintext),
        ),
    ])
}

fn direct_envelope_metadata(
    sender_did: &str,
    recipient_did: &str,
    message_id: &str,
) -> DirectEnvelopeMetadata {
    DirectEnvelopeMetadata {
        sender_did: sender_did.to_owned(),
        recipient_did: recipient_did.to_owned(),
        message_id: message_id.to_owned(),
        profile: DIRECT_E2EE_PROFILE.to_owned(),
        security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
    }
}

fn prepare_direct_secure_file_runtime_client(
    identity: DirectSecureFileRuntimeIdentity,
    resolver: &mut Box<DirectSecureFileRuntimeResolver>,
) -> crate::ImResult<PreparedDirectSecureFileRuntimeClient> {
    let owner_identity_id = required("owner_identity_id", &identity.owner_identity_id)?;
    let owner_did = required("owner_did", &identity.owner_did)?;
    let identity_name = required("identity_name", &identity.identity_name)?;
    let signing_key_id = format!("{owner_did}#key-1");
    let agreement_key_id = format!("{owner_did}#key-3");
    let signing_private =
        PrivateKeyMaterial::from_pem(&identity.signing_private_pem).map_err(|err| {
            crate::ImError::Serialization {
                detail: format!("parse direct E2EE signing private key: {err}"),
            }
        })?;
    let agreement_private =
        PrivateKeyMaterial::from_pem(&identity.agreement_private_pem).map_err(|err| {
            crate::ImError::Serialization {
                detail: format!("parse direct E2EE agreement private key: {err}"),
            }
        })?;
    let local_did_document = if identity.local_did_document.is_null() {
        resolver(&owner_did)?
    } else {
        identity.local_did_document
    };
    let local_service_did =
        anp::direct_e2ee::message_service_did_from_document(&local_did_document)
            .map_err(map_direct_error)?;
    Ok(PreparedDirectSecureFileRuntimeClient {
        owner_identity_id,
        owner_did,
        identity_name,
        local_service_did,
        signing_key_id,
        agreement_key_id,
        signing_private,
        agreement_private,
        session_store: FileSessionStore::new(identity.identity_dir.join(SECURE_SESSION_DIR_NAME))
            .map_err(map_direct_error)?,
        signed_prekey_store: FileSignedPrekeyStore::new(
            identity.identity_dir.join(SECURE_SIGNED_PREKEY_DIR_NAME),
        )
        .map_err(map_direct_error)?,
        one_time_prekey_store: FileOneTimePrekeyStore::new(
            identity.identity_dir.join(SECURE_ONE_TIME_PREKEY_DIR_NAME),
        )
        .map_err(map_direct_error)?,
    })
}

struct VerifiedPrekeyBundle {
    bundle: PrekeyBundle,
    one_time_prekey: Option<OneTimePrekey>,
}

#[derive(Clone, Debug)]
struct FileSessionStore {
    root: PathBuf,
}

impl FileSessionStore {
    fn new(root: impl AsRef<Path>) -> Result<Self, DirectE2eeError> {
        let root = root.as_ref().to_path_buf();
        create_store_dir(&root)?;
        Ok(Self { root })
    }

    fn save_session(
        &mut self,
        session: &anp::direct_e2ee::DirectSessionState,
    ) -> Result<(), DirectE2eeError> {
        write_private_json(&self.session_path(&session.session_id), session)
    }

    fn load_session(
        &self,
        session_id: &str,
    ) -> Result<anp::direct_e2ee::DirectSessionState, DirectE2eeError> {
        let path = self.session_path(session_id);
        let raw = match fs::read(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(DirectE2eeError::SessionNotFound(session_id.to_owned()));
            }
            Err(err) => return Err(store_io_error(err)),
        };
        serde_json::from_slice(&raw).map_err(store_json_error)
    }

    fn find_by_peer_did(
        &self,
        peer_did: &str,
    ) -> Result<Option<anp::direct_e2ee::DirectSessionState>, DirectE2eeError> {
        let mut entries = json_paths(&self.root)?;
        entries.sort();
        for path in entries {
            let raw = fs::read(&path).map_err(store_io_error)?;
            let session: anp::direct_e2ee::DirectSessionState =
                serde_json::from_slice(&raw).map_err(store_json_error)?;
            if session.peer_did == peer_did {
                return Ok(Some(session));
            }
        }
        Ok(None)
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.root.join(format!("{session_id}.json"))
    }
}

impl anp::direct_e2ee::SessionStore for FileSessionStore {
    fn save_session(
        &mut self,
        session: &anp::direct_e2ee::DirectSessionState,
    ) -> Result<(), DirectE2eeError> {
        Self::save_session(self, session)
    }

    fn load_session(
        &self,
        session_id: &str,
    ) -> Result<anp::direct_e2ee::DirectSessionState, DirectE2eeError> {
        Self::load_session(self, session_id)
    }

    fn delete_session(&mut self, session_id: &str) -> Result<(), DirectE2eeError> {
        match fs::remove_file(self.session_path(session_id)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(store_io_error(err)),
        }
    }
}

#[derive(Clone, Debug)]
struct FileSignedPrekeyStore {
    root: PathBuf,
}

impl FileSignedPrekeyStore {
    fn new(root: impl AsRef<Path>) -> Result<Self, DirectE2eeError> {
        let root = root.as_ref().to_path_buf();
        create_store_dir(&root)?;
        Ok(Self { root })
    }

    fn save_signed_prekey(
        &mut self,
        key_id: &str,
        private_key: &PrivateKeyMaterial,
        metadata: &anp::direct_e2ee::SignedPrekey,
    ) -> Result<(), DirectE2eeError> {
        write_private_file(&self.pem_path(key_id), private_key.to_pem().as_bytes())?;
        write_public_json(&self.json_path(key_id), metadata)?;
        write_public_file(&self.latest_path(), key_id.as_bytes())
    }

    fn load_signed_prekey(
        &self,
        key_id: &str,
    ) -> Result<(PrivateKeyMaterial, anp::direct_e2ee::SignedPrekey), DirectE2eeError> {
        let raw = fs::read_to_string(self.pem_path(key_id)).map_err(store_io_error)?;
        let private_key = PrivateKeyMaterial::from_pem(&raw)
            .map_err(|err| DirectE2eeError::invalid_field(err.to_string()))?;
        let metadata = read_json_file(&self.json_path(key_id))?;
        Ok((private_key, metadata))
    }

    fn load_latest_signed_prekey(
        &self,
    ) -> Result<Option<(PrivateKeyMaterial, anp::direct_e2ee::SignedPrekey)>, DirectE2eeError> {
        let raw = match fs::read_to_string(self.latest_path()) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(store_io_error(err)),
        };
        self.load_signed_prekey(raw.trim()).map(Some)
    }

    fn pem_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.pem"))
    }

    fn json_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.json"))
    }

    fn latest_path(&self) -> PathBuf {
        self.root.join("latest.txt")
    }
}

impl anp::direct_e2ee::SignedPrekeyStore for FileSignedPrekeyStore {
    fn save_signed_prekey(
        &mut self,
        key_id: &str,
        private_key: &PrivateKeyMaterial,
        metadata: &anp::direct_e2ee::SignedPrekey,
    ) -> Result<(), DirectE2eeError> {
        Self::save_signed_prekey(self, key_id, private_key, metadata)
    }

    fn load_signed_prekey(&self, key_id: &str) -> Result<PrivateKeyMaterial, DirectE2eeError> {
        Self::load_signed_prekey(self, key_id).map(|(private_key, _)| private_key)
    }
}

#[derive(Clone, Debug)]
struct FileOneTimePrekeyStore {
    root: PathBuf,
}

impl FileOneTimePrekeyStore {
    fn new(root: impl AsRef<Path>) -> Result<Self, DirectE2eeError> {
        let root = root.as_ref().to_path_buf();
        create_store_dir(&root)?;
        Ok(Self { root })
    }

    fn save_one_time_prekey(
        &mut self,
        key_id: &str,
        private_key: &PrivateKeyMaterial,
        metadata: &OneTimePrekey,
    ) -> Result<(), DirectE2eeError> {
        write_private_file(&self.pem_path(key_id), private_key.to_pem().as_bytes())?;
        write_public_json(&self.json_path(key_id), metadata)
    }

    fn load_one_time_prekey(
        &self,
        key_id: &str,
    ) -> Result<(PrivateKeyMaterial, OneTimePrekey), DirectE2eeError> {
        let raw = fs::read_to_string(self.pem_path(key_id)).map_err(store_io_error)?;
        let private_key = PrivateKeyMaterial::from_pem(&raw)
            .map_err(|err| DirectE2eeError::invalid_field(err.to_string()))?;
        let metadata = read_json_file(&self.json_path(key_id))?;
        Ok((private_key, metadata))
    }

    fn list_one_time_prekeys(&self) -> Result<Vec<OneTimePrekey>, DirectE2eeError> {
        let mut result = Vec::new();
        for path in json_paths(&self.root)? {
            result.push(read_json_file(&path)?);
        }
        result.sort_by(|left: &OneTimePrekey, right| left.key_id.cmp(&right.key_id));
        Ok(result)
    }

    fn delete_one_time_prekey(&mut self, key_id: &str) -> Result<(), DirectE2eeError> {
        remove_file_if_exists(self.pem_path(key_id))?;
        remove_file_if_exists(self.json_path(key_id))
    }

    fn pem_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.pem"))
    }

    fn json_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.json"))
    }
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

fn generated_x25519_private_key() -> crate::ImResult<PrivateKeyMaterial> {
    let bundle = anp::authentication::create_did_wba_document(
        "awiki.ai",
        anp::authentication::DidDocumentOptions::default(),
    )
    .map_err(|err| crate::ImError::Serialization {
        detail: format!("generate direct E2EE X25519 private key: {err}"),
    })?;
    bundle
        .load_private_key("key-3")
        .map_err(|err| crate::ImError::Serialization {
            detail: format!("load generated direct E2EE X25519 private key: {err}"),
        })
}

fn signed_prekey_from_private_key(
    key_id: &str,
    private_key: &PrivateKeyMaterial,
    expires_at: &str,
) -> crate::ImResult<anp::direct_e2ee::SignedPrekey> {
    Ok(anp::direct_e2ee::SignedPrekey {
        key_id: key_id.to_owned(),
        public_key_b64u: x25519_public_key_b64u(private_key)?,
        expires_at: expires_at.to_owned(),
    })
}

fn one_time_prekey_from_private_key(
    key_id: &str,
    private_key: &PrivateKeyMaterial,
) -> crate::ImResult<OneTimePrekey> {
    Ok(OneTimePrekey {
        key_id: key_id.to_owned(),
        public_key_b64u: x25519_public_key_b64u(private_key)?,
    })
}

fn x25519_public_key_b64u(private_key: &PrivateKeyMaterial) -> crate::ImResult<String> {
    match private_key.public_key() {
        PublicKeyMaterial::X25519(bytes) => {
            use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
            Ok(URL_SAFE_NO_PAD.encode(bytes))
        }
        _ => Err(expected_x25519_private_key()),
    }
}

fn expected_x25519_private_key() -> crate::ImError {
    crate::ImError::Serialization {
        detail: "expected X25519 private key".to_owned(),
    }
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

fn validate_one_time_prekey(prekey: &OneTimePrekey) -> crate::ImResult<()> {
    if prekey.key_id.trim().is_empty() {
        return Err(missing_field("one_time_prekey.key_id"));
    }
    if prekey.public_key_b64u.trim().is_empty() {
        return Err(missing_field("one_time_prekey.public_key_b64u"));
    }
    Ok(())
}

fn map_direct_error(error: DirectE2eeError) -> crate::ImError {
    crate::ImError::Serialization {
        detail: format!("direct E2EE: {error}"),
    }
}

fn missing_field(field: &'static str) -> crate::ImError {
    crate::ImError::Serialization {
        detail: format!("missing field: {field}"),
    }
}

fn string_value(value: Option<&Value>) -> String {
    value.map(string_value_from_value).unwrap_or_default()
}

fn string_value_from_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
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

fn unix_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_nanos() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn operation_nonce_hex() -> String {
    use rand::RngCore;

    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn json_paths(root: &Path) -> Result<Vec<PathBuf>, DirectE2eeError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(store_io_error(err)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(store_io_error)?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<(), DirectE2eeError> {
    let raw = serde_json::to_vec_pretty(value).map_err(store_json_error)?;
    write_private_file(path, &raw)
}

fn write_public_json<T: Serialize>(path: &Path, value: &T) -> Result<(), DirectE2eeError> {
    let raw = serde_json::to_vec_pretty(value).map_err(store_json_error)?;
    write_public_file(path, &raw)
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, DirectE2eeError> {
    let raw = fs::read(path).map_err(store_io_error)?;
    serde_json::from_slice(&raw).map_err(store_json_error)
}

fn remove_file_if_exists(path: PathBuf) -> Result<(), DirectE2eeError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(store_io_error(err)),
    }
}

fn store_io_error(err: std::io::Error) -> DirectE2eeError {
    DirectE2eeError::invalid_field(err.to_string())
}

fn store_json_error(err: serde_json::Error) -> DirectE2eeError {
    DirectE2eeError::invalid_field(err.to_string())
}

#[cfg(unix)]
fn create_store_dir(path: &Path) -> Result<(), DirectE2eeError> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o755);
    builder.create(path).map_err(store_io_error)
}

#[cfg(not(unix))]
fn create_store_dir(path: &Path) -> Result<(), DirectE2eeError> {
    fs::create_dir_all(path).map_err(store_io_error)
}

#[cfg(unix)]
fn write_private_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(store_io_error)?;
    file.write_all(raw).map_err(store_io_error)
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    fs::write(path, raw).map_err(store_io_error)
}

#[cfg(unix)]
fn write_public_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(path)
        .map_err(store_io_error)?;
    file.write_all(raw).map_err(store_io_error)
}

#[cfg(not(unix))]
fn write_public_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    fs::write(path, raw).map_err(store_io_error)
}
