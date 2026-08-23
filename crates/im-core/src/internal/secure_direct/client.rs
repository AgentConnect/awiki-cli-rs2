#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use anp::direct_e2ee::{
    ApplicationPlaintext, DirectE2eeError, DirectE2eeSession, DirectEnvelopeMetadata,
    OneTimePrekey, PrekeyBundle, PreparedPrekeyBundle, SignedPrekeyStore as _,
};
use anp::{PrivateKeyMaterial, PublicKeyMaterial};
use rusqlite::Connection;
use serde_json::{Map, Value};

use super::sqlite_store::{
    AnpDirectOneTimePrekeyStore, AnpDirectSessionStore, AnpDirectSignedPrekeyStore,
};

const DIRECT_E2EE_PROFILE: &str = "anp.direct.e2ee.v1";
const DIRECT_E2EE_SECURITY_PROFILE: &str = "direct-e2ee";
const DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE: usize = 16;

pub(crate) type DirectSecureRpcResult = crate::ImResult<Map<String, Value>>;
pub(crate) type DirectSecureRpc<'a> =
    dyn FnMut(&str, Map<String, Value>) -> DirectSecureRpcResult + 'a;
pub(crate) type DirectSecureDidResolver<'a> = dyn FnMut(&str) -> crate::ImResult<Value> + 'a;

#[derive(Debug, Clone)]
pub(crate) struct DirectSecurePrekeyPublishRequest {
    pub(crate) method: String,
    pub(crate) params: Map<String, Value>,
}

pub(crate) struct DirectSecurePrekeySigningPreparation {
    prepared: PreparedPrekeyBundle,
    owner_did: String,
    local_service_did: String,
    one_time_prekeys: Vec<OneTimePrekey>,
}

impl DirectSecurePrekeySigningPreparation {
    pub(crate) fn signing_input(&self) -> &[u8] {
        self.prepared.signing_input()
    }

    pub(crate) fn complete(
        self,
        signature: &[u8],
    ) -> crate::ImResult<DirectSecurePrekeyPublishRequest> {
        let bundle = anp::direct_e2ee::complete_prekey_bundle(self.prepared, signature)
            .map_err(map_direct_error)?;
        let request = super::prekey_lifecycle::prekey_bundle_publish_request(
            &self.owner_did,
            &self.local_service_did,
            &bundle,
            &self.one_time_prekeys,
        )?;
        direct_secure_request_method_params(request)
    }
}

pub(crate) struct DirectSecureClientInput<'a> {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) identity_name: String,
    pub(crate) signing_key_id: String,
    pub(crate) agreement_key_id: String,
    pub(crate) identity_signer: Arc<dyn crate::internal::key_provider::IdentitySigner>,
    pub(crate) local_did_document: Value,
    pub(crate) local_state: &'a Connection,
}

pub(crate) struct PreparedDirectSecureClient<'a> {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) identity_name: String,
    pub(crate) local_service_did: String,
    pub(crate) signing_key_id: String,
    pub(crate) agreement_key_id: String,
    pub(crate) identity_signer: Arc<dyn crate::internal::key_provider::IdentitySigner>,
    pub(crate) local_did_document: Value,
    pub(crate) local_state: &'a Connection,
}

pub(crate) struct MessageServiceDirectSecureClient<'a> {
    prepared: PreparedDirectSecureClient<'a>,
    rpc: Box<DirectSecureRpc<'a>>,
    resolver: Box<DirectSecureDidResolver<'a>>,
    pending_by_peer: HashMap<String, Vec<Map<String, Value>>>,
}

pub(crate) fn prepare_direct_secure_client(
    input: DirectSecureClientInput<'_>,
) -> crate::ImResult<PreparedDirectSecureClient<'_>> {
    let owner_identity_id = required("owner_identity_id", &input.owner_identity_id)?;
    let owner_did = required("owner_did", &input.owner_did)?;
    let identity_name = required("identity_name", &input.identity_name)?;
    let signing_key_id = default_key_id(&owner_did, &input.signing_key_id, "key-1");
    let agreement_key_id = default_key_id(&owner_did, &input.agreement_key_id, "key-3");
    if !matches!(
        input.identity_signer.public_key(&signing_key_id)?,
        PublicKeyMaterial::Ed25519(_) | PublicKeyMaterial::Secp256k1(_)
    ) || !matches!(
        input.identity_signer.public_key(&agreement_key_id)?,
        PublicKeyMaterial::X25519(_)
    ) {
        return Err(crate::ImError::PermissionDenied);
    }
    let local_service_did =
        anp::direct_e2ee::message_service_did_from_document(&input.local_did_document)
            .map_err(map_direct_error)?;
    Ok(PreparedDirectSecureClient {
        owner_identity_id,
        owner_did,
        identity_name,
        local_service_did,
        signing_key_id,
        agreement_key_id,
        identity_signer: input.identity_signer,
        local_did_document: input.local_did_document,
        local_state: input.local_state,
    })
}

impl<'a> MessageServiceDirectSecureClient<'a> {
    pub(crate) fn new(
        prepared: PreparedDirectSecureClient<'a>,
        rpc: Box<DirectSecureRpc<'a>>,
        resolver: Box<DirectSecureDidResolver<'a>>,
    ) -> Self {
        Self {
            prepared,
            rpc,
            resolver,
            pending_by_peer: HashMap::new(),
        }
    }

    pub(crate) fn publish_prekey_bundle(&mut self) -> DirectSecureRpcResult {
        let bundle = self.ensure_fresh_prekey_bundle()?;
        self.publish_prekey_bundle_rpc(&bundle)
    }

    pub(crate) fn prepare_prekey_bundle_publish_request(
        &mut self,
    ) -> crate::ImResult<DirectSecurePrekeyPublishRequest> {
        let bundle = self.build_fresh_prekey_bundle()?;
        self.prekey_bundle_publish_request(&bundle)
    }

    pub(crate) fn prepare_prekey_bundle_signing(
        &mut self,
    ) -> crate::ImResult<DirectSecurePrekeySigningPreparation> {
        let prepared = self.prepare_fresh_prekey_bundle()?;
        let one_time_prekeys = self.one_time_prekey_store()?.list_one_time_prekeys()?;
        Ok(DirectSecurePrekeySigningPreparation {
            prepared,
            owner_did: self.prepared.owner_did.clone(),
            local_service_did: self.prepared.local_service_did.clone(),
            one_time_prekeys,
        })
    }

    pub(crate) fn ensure_fresh_prekey_bundle(&mut self) -> crate::ImResult<PrekeyBundle> {
        self.build_fresh_prekey_bundle()
    }

    fn build_fresh_prekey_bundle(&mut self) -> crate::ImResult<PrekeyBundle> {
        let prepared = self.prepare_fresh_prekey_bundle()?;
        let signature = self
            .prepared
            .identity_signer
            .sign(&self.prepared.signing_key_id, prepared.signing_input())?;
        anp::direct_e2ee::complete_prekey_bundle(prepared, &signature).map_err(map_direct_error)
    }

    fn prepare_fresh_prekey_bundle(&mut self) -> crate::ImResult<PreparedPrekeyBundle> {
        self.ensure_fresh_one_time_prekeys(DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE)?;
        let signed_prekey = match self.signed_prekey_store()?.load_latest_signed_prekey()? {
            Some((_private_key, metadata))
                if !super::prekey_lifecycle::signed_prekey_needs_rotation(&metadata) =>
            {
                metadata
            }
            Some(_) | None => {
                let private_key = generated_x25519_private_key()?;
                let metadata = super::prekey_lifecycle::create_signed_prekey(&private_key)?;
                self.signed_prekey_store()?
                    .save_signed_prekey(&metadata.key_id, &private_key, &metadata)
                    .map_err(map_direct_error)?;
                metadata
            }
        };
        self.prepare_prekey_bundle(signed_prekey)
    }

    pub(crate) fn send_text(
        &mut self,
        peer_did: &str,
        text: &str,
        operation_id: &str,
        message_id: &str,
    ) -> DirectSecureRpcResult {
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
    ) -> DirectSecureRpcResult {
        self.send_application_plaintext(
            peer_did,
            ApplicationPlaintext::new_json("application/json", Value::Object(payload)),
            operation_id,
            message_id,
        )
    }

    pub(crate) fn send_json_with_client_context(
        &mut self,
        peer_did: &str,
        content_type: &str,
        payload: Value,
        operation_id: &str,
        message_id: &str,
        client_context: Option<Value>,
    ) -> DirectSecureRpcResult {
        self.send_application_plaintext_with_client_context(
            peer_did,
            ApplicationPlaintext::new_json(content_type, payload),
            operation_id,
            message_id,
            client_context,
        )
    }

    pub(crate) fn process_incoming(
        &mut self,
        message: Map<String, Value>,
    ) -> DirectSecureRpcResult {
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
        self.session_store()
            .ok()
            .and_then(|store| store.find_by_peer_did(peer_did).ok())
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
    ) -> DirectSecureRpcResult {
        self.send_application_plaintext_with_client_context(
            peer_did,
            plaintext,
            operation_id,
            message_id,
            None,
        )
    }

    fn send_application_plaintext_with_client_context(
        &mut self,
        peer_did: &str,
        plaintext: ApplicationPlaintext,
        operation_id: &str,
        message_id: &str,
        client_context: Option<Value>,
    ) -> DirectSecureRpcResult {
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
        let mut session_store = self.session_store()?;
        if let Some(mut session) = session_store.find_by_peer_did(&peer_did)? {
            let (_pending, body) = DirectE2eeSession::encrypt_follow_up(
                &mut session,
                &metadata,
                operation_id,
                &plaintext,
            )
            .map_err(map_direct_error)?;
            anp::direct_e2ee::SessionStore::save_session(&mut session_store, &session)
                .map_err(map_direct_error)?;
            let request = anp::direct_e2ee::direct_cipher_send_request(
                &self.prepared.owner_did,
                &peer_did,
                operation_id,
                message_id,
                &body,
            )
            .map_err(map_direct_error)?;
            return self.call_request_with_client_context(request, client_context);
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
        let static_dh = self.prepared.identity_signer.ecdh(
            &self.prepared.agreement_key_id,
            &recipient_signed_prekey_public,
        )?;
        let (session, _pending, body) = DirectE2eeSession::initiate_session_with_static_dh(
            &metadata,
            operation_id,
            &self.prepared.agreement_key_id,
            &static_dh,
            &verified.bundle,
            &recipient_static_public,
            &recipient_signed_prekey_public,
            recipient_one_time_prekey_public.as_ref(),
            recipient_one_time_prekey_id,
            &plaintext,
        )
        .map_err(map_direct_error)?;
        anp::direct_e2ee::SessionStore::save_session(&mut session_store, &session)
            .map_err(map_direct_error)?;
        let request = anp::direct_e2ee::direct_init_send_request(
            &self.prepared.owner_did,
            &peer_did,
            operation_id,
            message_id,
            &body,
        )
        .map_err(map_direct_error)?;
        self.call_request_with_client_context(request, client_context)
    }

    fn process_incoming_init(
        &mut self,
        sender_did: &str,
        metadata: &DirectEnvelopeMetadata,
        body: &Value,
    ) -> DirectSecureRpcResult {
        let init_body = super::wire::direct_init_body_from_value(body);
        let existing_session = self.existing_session(&init_body.session_id)?;
        let sender_document = (self.resolver)(sender_did)?;
        let sender_static_public = anp::direct_e2ee::extract_x25519_public_key(
            &sender_document,
            &init_body.sender_static_key_agreement_id,
        )
        .map_err(map_direct_error)?;
        let signed_prekey_private = self
            .signed_prekey_store()?
            .load_signed_prekey(&init_body.recipient_signed_prekey_id)
            .map_err(map_direct_error)?;
        let one_time_prekey_id = init_body
            .recipient_one_time_prekey_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let one_time_prekey_material = if let Some(key_id) = one_time_prekey_id.as_deref() {
            self.one_time_prekey_store()?
                .load_one_time_prekey(key_id)?
                .map(|record| record.private_key)
        } else {
            None
        };
        let PrivateKeyMaterial::X25519(signed_prekey_private) = &signed_prekey_private else {
            return Err(expected_x25519_private_key());
        };
        let one_time_prekey_private = match one_time_prekey_material.as_ref() {
            Some(PrivateKeyMaterial::X25519(key)) => Some(key),
            Some(_) => return Err(expected_x25519_private_key()),
            None => None,
        };
        let sender_ephemeral_public = decode_public_key_b64u(
            &init_body.sender_ephemeral_pub_b64u,
            "sender_ephemeral_pub_b64u",
        )?;
        let static_dh = self
            .prepared
            .identity_signer
            .ecdh(&self.prepared.agreement_key_id, &sender_ephemeral_public)?;
        let (session, plaintext) = DirectE2eeSession::accept_incoming_init_with_static_dh(
            metadata,
            &self.prepared.agreement_key_id,
            &static_dh,
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
            let _ = self
                .one_time_prekey_store()?
                .mark_consumed(&key_id, &now_utc_like())?;
        }
        anp::direct_e2ee::SessionStore::save_session(&mut self.session_store()?, &session)
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
    ) -> DirectSecureRpcResult {
        let cipher_body = super::wire::direct_cipher_body_from_value(body);
        let mut session_store = self.session_store()?;
        let mut session = match anp::direct_e2ee::SessionStore::load_session(
            &session_store,
            &cipher_body.session_id,
        ) {
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
                anp::direct_e2ee::SessionStore::save_session(&mut session_store, &session)
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
            .ok_or_else(|| missing_field("prekey_bundle"))?
            .clone();
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
    ) -> DirectSecureRpcResult {
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

    fn publish_prekey_bundle_rpc(&mut self, bundle: &PrekeyBundle) -> DirectSecureRpcResult {
        let request = self.prekey_bundle_publish_request(bundle)?;
        (self.rpc)(&request.method, request.params)
    }

    fn prekey_bundle_publish_request(
        &mut self,
        bundle: &PrekeyBundle,
    ) -> crate::ImResult<DirectSecurePrekeyPublishRequest> {
        let one_time_prekeys = self.one_time_prekey_store()?.list_one_time_prekeys()?;
        let request = super::prekey_lifecycle::prekey_bundle_publish_request(
            &self.prepared.owner_did,
            &self.prepared.local_service_did,
            bundle,
            &one_time_prekeys,
        )?;
        direct_secure_request_method_params(request)
    }

    fn prepare_prekey_bundle(
        &self,
        signed_prekey: anp::direct_e2ee::SignedPrekey,
    ) -> crate::ImResult<PreparedPrekeyBundle> {
        let bundle_id = super::prekey_lifecycle::bundle_id(&signed_prekey);
        let proof_created = super::prekey_lifecycle::bundle_proof_created(&signed_prekey)?;
        let public_key = self
            .prepared
            .identity_signer
            .public_key(&self.prepared.signing_key_id)?;
        anp::direct_e2ee::prepare_prekey_bundle(
            &bundle_id,
            &self.prepared.owner_did,
            &self.prepared.agreement_key_id,
            signed_prekey,
            &public_key,
            &self.prepared.signing_key_id,
            Some(&proof_created),
        )
        .map_err(map_direct_error)
    }

    fn ensure_fresh_one_time_prekeys(&mut self, min_count: usize) -> crate::ImResult<()> {
        if min_count == 0 {
            return Ok(());
        }
        let current = self.one_time_prekey_store()?.list_one_time_prekeys()?;
        if current.len() >= min_count {
            return Ok(());
        }
        let prefix = unix_nanos();
        let mut store = self.one_time_prekey_store()?;
        for index in current.len()..min_count {
            let key_id = format!("opk-{prefix}-{index:03}");
            let private_key = generated_x25519_private_key()?;
            let metadata = one_time_prekey_from_private_key(&key_id, &private_key)?;
            store.save_one_time_prekey(&key_id, &private_key, &metadata)?;
        }
        Ok(())
    }

    fn call_request(&mut self, request: Value) -> DirectSecureRpcResult {
        self.call_request_with_client_context(request, None)
    }

    fn call_request_with_client_context(
        &mut self,
        request: Value,
        client_context: Option<Value>,
    ) -> DirectSecureRpcResult {
        let request = direct_secure_request_method_params(request)?;
        let method = request.method;
        let mut params = request.params;
        if let Some(client_context) = client_context {
            params.insert("client".to_owned(), client_context);
        }
        (self.rpc)(&method, params)
    }

    fn session_store(&self) -> crate::ImResult<AnpDirectSessionStore<'a>> {
        AnpDirectSessionStore::new(
            self.prepared.local_state,
            &self.prepared.owner_identity_id,
            &self.prepared.owner_did,
        )
    }

    fn signed_prekey_store(&self) -> crate::ImResult<AnpDirectSignedPrekeyStore<'a>> {
        AnpDirectSignedPrekeyStore::new(
            self.prepared.local_state,
            &self.prepared.owner_identity_id,
            &self.prepared.owner_did,
        )
    }

    fn one_time_prekey_store(&self) -> crate::ImResult<AnpDirectOneTimePrekeyStore<'a>> {
        AnpDirectOneTimePrekeyStore::new(
            self.prepared.local_state,
            &self.prepared.owner_identity_id,
            &self.prepared.owner_did,
        )
    }

    fn existing_session(
        &self,
        session_id: &str,
    ) -> crate::ImResult<Option<anp::direct_e2ee::DirectSessionState>> {
        if session_id.trim().is_empty() {
            return Ok(None);
        }
        let store = self.session_store()?;
        match anp::direct_e2ee::SessionStore::load_session(&store, session_id) {
            Ok(session) => Ok(Some(session)),
            Err(anp::direct_e2ee::DirectE2eeError::SessionNotFound(_)) => Ok(None),
            Err(err) => Err(map_direct_error(err)),
        }
    }
}

pub(crate) fn direct_secure_request_method_params(
    request: Value,
) -> crate::ImResult<DirectSecurePrekeyPublishRequest> {
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
    Ok(DirectSecurePrekeyPublishRequest { method, params })
}

struct VerifiedPrekeyBundle {
    bundle: PrekeyBundle,
    one_time_prekey: Option<OneTimePrekey>,
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

fn default_key_id(owner_did: &str, provided: &str, fragment: &str) -> String {
    let provided = provided.trim();
    if provided.is_empty() {
        format!("{owner_did}#{fragment}")
    } else {
        provided.to_owned()
    }
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
        _ => Err(crate::ImError::Serialization {
            detail: "expected X25519 private key".to_owned(),
        }),
    }
}

fn expected_x25519_private_key() -> crate::ImError {
    crate::ImError::Serialization {
        detail: "expected X25519 private key".to_owned(),
    }
}

pub(crate) fn decode_public_key_b64u(value: &str, field: &str) -> crate::ImResult<[u8; 32]> {
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

fn decrypted_plaintext_result(plaintext: &ApplicationPlaintext) -> Map<String, Value> {
    Map::from_iter([
        ("state".to_owned(), Value::String("decrypted".to_owned())),
        (
            "plaintext".to_owned(),
            anp::direct_e2ee::plaintext_to_value(plaintext),
        ),
    ])
}

pub(crate) fn map_direct_error(error: DirectE2eeError) -> crate::ImError {
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use anp::direct_e2ee::SessionStore as _;
    use serde_json::json;

    use super::*;
    use crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore;

    #[test]
    fn secure_direct_client_publishes_prekey_bundle_from_sqlite_state() {
        let db = Connection::open_in_memory().unwrap();
        let identity = test_identity("alice.example", "alice");
        let identity_signer = identity.signer();
        let calls = Rc::new(RefCell::new(Vec::<(String, Map<String, Value>)>::new()));
        let rpc_calls = calls.clone();
        let mut client = MessageServiceDirectSecureClient::new(
            prepare_direct_secure_client(DirectSecureClientInput {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: identity.did.clone(),
                identity_name: "alice".to_owned(),
                signing_key_id: format!("{}#key-1", identity.did),
                agreement_key_id: format!("{}#key-3", identity.did),
                identity_signer,
                local_did_document: identity.document,
                local_state: &db,
            })
            .unwrap(),
            Box::new(move |method, params| {
                rpc_calls
                    .borrow_mut()
                    .push((method.to_owned(), params.clone()));
                Ok(Map::new())
            }),
            Box::new(|did| {
                Err(crate::ImError::PeerNotFound {
                    peer: did.to_owned(),
                })
            }),
        );

        let response = client.publish_prekey_bundle().unwrap();
        let repeated_response = client.publish_prekey_bundle().unwrap();

        assert!(response.is_empty());
        assert!(repeated_response.is_empty());
        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "direct.e2ee.publish_prekey_bundle");
        assert_eq!(calls[0], calls[1]);
        let body = calls[0].1.get("body").and_then(Value::as_object).unwrap();
        assert!(body.get("prekey_bundle").is_some());
        assert_eq!(
            body.get("one_time_prekeys")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE)
        );
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();
        assert!(store.active_signed_prekey("alice-id").unwrap().is_some());
        assert_eq!(
            store
                .list_available_one_time_prekeys("alice-id")
                .unwrap()
                .len(),
            DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE
        );
        let signed_prekey_count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM direct_e2ee_signed_prekeys WHERE owner_identity_id = 'alice-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(signed_prekey_count, 1);
    }

    #[test]
    fn secure_direct_client_replayed_init_and_failed_cipher_do_not_overwrite_session() {
        let alice_db = Connection::open_in_memory().unwrap();
        let bob_db = Connection::open_in_memory().unwrap();
        let alice = test_identity("alice.example", "alice");
        let bob = test_identity("bob.example", "bob");
        let (bob_bundle, bob_one_time_prekey) = {
            let mut bob_prekey_client = MessageServiceDirectSecureClient::new(
                prepared_client(&bob, "bob-id", &bob_db),
                Box::new(|_method, _params| Ok(Map::new())),
                Box::new(|did| {
                    Err(crate::ImError::PeerNotFound {
                        peer: did.to_owned(),
                    })
                }),
            );
            let bundle = bob_prekey_client.ensure_fresh_prekey_bundle().unwrap();
            let one_time_prekey = bob_prekey_client
                .one_time_prekey_store()
                .unwrap()
                .list_one_time_prekeys()
                .unwrap()
                .into_iter()
                .next()
                .unwrap();
            (bundle, one_time_prekey)
        };

        let alice_sends = Rc::new(RefCell::new(Vec::<Map<String, Value>>::new()));
        let alice_sends_for_rpc = Rc::clone(&alice_sends);
        let bob_bundle_for_rpc = bob_bundle.clone();
        let bob_opk_for_rpc = bob_one_time_prekey.clone();
        let mut alice_client = MessageServiceDirectSecureClient::new(
            prepared_client(&alice, "alice-id", &alice_db),
            Box::new(move |method, params| match method {
                "direct.e2ee.get_prekey_bundle" => Ok(object(json!({
                    "prekey_bundle": bob_bundle_for_rpc,
                    "one_time_prekey": bob_opk_for_rpc,
                }))),
                "direct.send" => {
                    alice_sends_for_rpc.borrow_mut().push(params.clone());
                    Ok(object(json!({
                        "message_id": map_pointer(&params, "/meta/message_id").unwrap_or(Value::Null),
                        "operation_id": map_pointer(&params, "/meta/operation_id").unwrap_or(Value::Null),
                        "delivery_state": "accepted",
                    })))
                }
                other => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected alice RPC method: {other}"),
                }),
            }),
            resolver_for(&bob),
        );

        alice_client
            .send_text(&bob.did, "hello bob", "msg-init", "msg-init")
            .unwrap();
        let init_params = captured_send(&alice_sends, 0);

        let bob_sends = Rc::new(RefCell::new(Vec::<Map<String, Value>>::new()));
        let bob_sends_for_rpc = Rc::clone(&bob_sends);
        let mut bob_client = MessageServiceDirectSecureClient::new(
            prepared_client(&bob, "bob-id", &bob_db),
            Box::new(move |method, params| match method {
                "direct.send" => {
                    bob_sends_for_rpc.borrow_mut().push(params.clone());
                    Ok(object(json!({
                        "message_id": map_pointer(&params, "/meta/message_id").unwrap_or(Value::Null),
                        "operation_id": map_pointer(&params, "/meta/operation_id").unwrap_or(Value::Null),
                        "delivery_state": "accepted",
                    })))
                }
                other => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected bob RPC method: {other}"),
                }),
            }),
            resolver_for(&alice),
        );

        let init_result = bob_client.process_incoming(init_params.clone()).unwrap();
        assert_eq!(init_result["state"], json!("decrypted"));
        assert_eq!(init_result["plaintext"]["text"], json!("hello bob"));

        bob_client
            .send_text(&alice.did, "reply from bob", "msg-reply", "msg-reply")
            .unwrap();
        let reply_params = captured_send(&bob_sends, 0);

        let reply_result = alice_client.process_incoming(reply_params.clone()).unwrap();
        assert_eq!(reply_result["state"], json!("decrypted"));
        assert_eq!(reply_result["plaintext"]["text"], json!("reply from bob"));

        alice_client
            .send_text(
                &bob.did,
                "follow up from alice",
                "msg-follow-up",
                "msg-follow-up",
            )
            .unwrap();
        let follow_up_params = captured_send(&alice_sends, 1);
        let session_id = string_value(map_pointer(&follow_up_params, "/body/session_id").as_ref());
        let bob_session_after_reply = stored_session(&bob_db, "bob-id", &bob.did, &session_id);

        let replayed_init = bob_client.process_incoming(init_params).unwrap();
        assert_eq!(replayed_init["state"], json!("decrypted"));
        assert_eq!(
            stored_session(&bob_db, "bob-id", &bob.did, &session_id),
            bob_session_after_reply,
            "replayed direct-init must not replace an established responder session"
        );

        let self_cipher = bob_client.process_incoming(reply_params).unwrap();
        assert_eq!(self_cipher["state"], json!("undecryptable"));
        assert_eq!(
            stored_session(&bob_db, "bob-id", &bob.did, &session_id),
            bob_session_after_reply,
            "failed self-cipher decrypt must not persist ratchet mutations"
        );

        let follow_up = bob_client.process_incoming(follow_up_params).unwrap();
        assert_eq!(follow_up["state"], json!("decrypted"));
        assert_eq!(
            follow_up["plaintext"]["text"],
            json!("follow up from alice")
        );
    }

    #[test]
    fn prepare_direct_secure_client_rejects_empty_owner_identity() {
        let db = Connection::open_in_memory().unwrap();
        let identity = test_identity("alice.example", "alice");
        let identity_signer = identity.signer();

        let err = match prepare_direct_secure_client(DirectSecureClientInput {
            owner_identity_id: " ".to_owned(),
            owner_did: identity.did,
            identity_name: "alice".to_owned(),
            signing_key_id: String::new(),
            agreement_key_id: String::new(),
            identity_signer,
            local_did_document: identity.document,
            local_state: &db,
        }) {
            Ok(_) => panic!("empty owner identity must fail"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(field),
                ..
            } if field == "owner_identity_id"
        ));
    }

    fn prepared_client<'a>(
        identity: &TestIdentity,
        owner_identity_id: &str,
        db: &'a Connection,
    ) -> PreparedDirectSecureClient<'a> {
        prepare_direct_secure_client(DirectSecureClientInput {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: identity.did.clone(),
            identity_name: owner_identity_id.to_owned(),
            signing_key_id: format!("{}#key-1", identity.did),
            agreement_key_id: format!("{}#key-3", identity.did),
            identity_signer: identity.signer(),
            local_did_document: identity.document.clone(),
            local_state: db,
        })
        .unwrap()
    }

    fn resolver_for(identity: &TestIdentity) -> Box<DirectSecureDidResolver<'static>> {
        let did = identity.did.clone();
        let document = identity.document.clone();
        Box::new(move |candidate| {
            if candidate == did {
                Ok(document.clone())
            } else {
                Err(crate::ImError::PeerNotFound {
                    peer: candidate.to_owned(),
                })
            }
        })
    }

    fn captured_send(
        sends: &Rc<RefCell<Vec<Map<String, Value>>>>,
        index: usize,
    ) -> Map<String, Value> {
        sends.borrow().get(index).cloned().unwrap()
    }

    fn stored_session(
        db: &Connection,
        owner_identity_id: &str,
        owner_did: &str,
        session_id: &str,
    ) -> anp::direct_e2ee::DirectSessionState {
        AnpDirectSessionStore::new(db, owner_identity_id, owner_did)
            .unwrap()
            .load_session(session_id)
            .unwrap()
    }

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    fn map_pointer(object: &Map<String, Value>, pointer: &str) -> Option<Value> {
        Value::Object(object.clone()).pointer(pointer).cloned()
    }

    fn string_value(value: Option<&Value>) -> String {
        value.and_then(Value::as_str).unwrap_or_default().to_owned()
    }

    struct TestIdentity {
        did: String,
        document: Value,
        signing_private_pem: String,
        agreement_private_pem: String,
    }

    impl TestIdentity {
        fn signer(&self) -> Arc<dyn crate::internal::key_provider::IdentitySigner> {
            Arc::new(
                crate::internal::key_provider::HostedIdentitySigner::new_for_request_signing_key(
                    &crate::identity::HostedIdentityMaterial {
                        identity_id: self.did.clone(),
                        did: self.did.clone(),
                        handle: None,
                        display_name: None,
                        did_document: self.document.clone(),
                        default_signing_private_key_pem: self.signing_private_pem.clone(),
                        e2ee_agreement_private_key_pem: Some(self.agreement_private_pem.clone()),
                        auth_token: None,
                    },
                    &format!("{}#key-1", self.did),
                )
                .unwrap(),
            )
        }
    }

    fn test_identity(domain: &str, label: &str) -> TestIdentity {
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
                services: vec![service],
                ..Default::default()
            },
        )
        .unwrap();
        let did = bundle.did().unwrap().to_owned();
        let signing_private_pem = bundle.private_key_pem("key-1").unwrap().to_owned();
        let agreement_private_pem = bundle.private_key_pem("key-3").unwrap().to_owned();
        let document = bundle.did_document;
        assert_eq!(document.get("id"), Some(&json!(did)));
        TestIdentity {
            did,
            document,
            signing_private_pem,
            agreement_private_pem,
        }
    }
}
