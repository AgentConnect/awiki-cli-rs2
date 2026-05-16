use crate::anpsdk::{
    self, ApplicationPlaintext, DirectEnvelopeMetadata, FileOneTimePrekeyStore, FileSessionStore,
    FileSignedPrekeyStore, OneTimePrekey, PrekeyBundle, PrivateKeyMaterial,
};
use crate::identity::{types::StoredIdentity, Manager};
use serde_json::{Map, Value};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const DIRECT_E2EE_PROFILE: &str = "anp.direct.e2ee.v1";
const DIRECT_E2EE_SECURITY_PROFILE: &str = "direct-e2ee";
const DEFAULT_SIGNED_PREKEY_ID: &str = "spk-initial";
const DEFAULT_SIGNED_PREKEY_EXPIRY: &str = "2030-01-01T00:00:00Z";
const DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE: usize = 16;

pub type SecureE2EERpcResult = Result<Map<String, Value>, String>;
pub type SecureE2EERpc = dyn FnMut(&str, Map<String, Value>) -> SecureE2EERpcResult;
pub type SecureE2EEDidResolver = dyn FnMut(&str) -> Result<Value, String>;

pub struct PreparedSecureE2EEClient {
    pub owner_did: String,
    pub signing_key_id: String,
    pub agreement_key_id: String,
    pub signing_private: PrivateKeyMaterial,
    pub agreement_private: PrivateKeyMaterial,
    pub session_store: FileSessionStore,
    pub signed_prekey_store: FileSignedPrekeyStore,
    pub one_time_prekey_store: FileOneTimePrekeyStore,
}

pub struct MessageServiceE2EEClient {
    owner_did: String,
    local_service_did: String,
    signing_key_id: String,
    agreement_key_id: String,
    signing_private: PrivateKeyMaterial,
    agreement_private: PrivateKeyMaterial,
    session_store: FileSessionStore,
    signed_prekey_store: FileSignedPrekeyStore,
    one_time_prekey_store: FileOneTimePrekeyStore,
    rpc: Box<SecureE2EERpc>,
    resolver: Box<SecureE2EEDidResolver>,
}

pub fn prepare_secure_e2ee_client_for_record(
    manager: Option<&Manager>,
    record: Option<&StoredIdentity>,
) -> Result<PreparedSecureE2EEClient, String> {
    let manager = manager.ok_or_else(|| "identity manager is required".to_string())?;
    let record = record.ok_or_else(|| "identity record is required".to_string())?;
    let paths = manager
        .paths_for_identity(&record.identity_name)
        .map_err(|err| err.to_string())?;
    let signing_private = anpsdk::PrivateKeyMaterial::from_pem(&record.key1_private_pem)
        .map_err(|err| format!("parse DID signing private key: {err}"))?;
    let agreement_private =
        anpsdk::PrivateKeyMaterial::from_pem(&record.e2ee_agreement_private_pem)
            .map_err(|err| format!("parse E2EE agreement private key: {err}"))?;
    let identity_dir = Path::new(&paths.identity_dir);
    let session_store = FileSessionStore::new(identity_dir.join("p5-e2ee-sessions"))
        .map_err(|err| err.to_string())?;
    let signed_prekey_store = FileSignedPrekeyStore::new(identity_dir.join("p5-signed-prekeys"))
        .map_err(|err| err.to_string())?;
    let one_time_prekey_store =
        FileOneTimePrekeyStore::new(identity_dir.join("p5-one-time-prekeys"))
            .map_err(|err| err.to_string())?;
    Ok(PreparedSecureE2EEClient {
        owner_did: record.did.clone(),
        signing_key_id: format!("{}#key-1", record.did),
        agreement_key_id: format!("{}#key-3", record.did),
        signing_private,
        agreement_private,
        session_store,
        signed_prekey_store,
        one_time_prekey_store,
    })
}

pub fn new_secure_e2ee_client_for_record(
    manager: Option<&Manager>,
    record: Option<&StoredIdentity>,
    rpc: Box<SecureE2EERpc>,
) -> Result<MessageServiceE2EEClient, String> {
    let prepared = prepare_secure_e2ee_client_for_record(manager, record)?;
    let manager_for_resolver = manager.cloned();
    let record_for_resolver = record.cloned();
    let resolver: Box<SecureE2EEDidResolver> = Box::new(move |did| {
        resolve_secure_e2ee_local_document(
            manager_for_resolver.as_ref(),
            record_for_resolver.as_ref(),
            did,
        )
        .or_else(|| anpsdk::resolve_did_document_sync(did, true).ok())
        .ok_or_else(|| format!("resolve DID document: {did}"))
    });
    MessageServiceE2EEClient::new(prepared, rpc, resolver)
}

impl MessageServiceE2EEClient {
    pub fn new(
        prepared: PreparedSecureE2EEClient,
        rpc: Box<SecureE2EERpc>,
        mut resolver: Box<SecureE2EEDidResolver>,
    ) -> Result<Self, String> {
        let local_document = resolver(&prepared.owner_did)?;
        let local_service_did = anpsdk::message_service_did_from_document(&local_document)
            .map_err(|err| err.to_string())?;
        Ok(Self {
            owner_did: prepared.owner_did,
            local_service_did,
            signing_key_id: prepared.signing_key_id,
            agreement_key_id: prepared.agreement_key_id,
            signing_private: prepared.signing_private,
            agreement_private: prepared.agreement_private,
            session_store: prepared.session_store,
            signed_prekey_store: prepared.signed_prekey_store,
            one_time_prekey_store: prepared.one_time_prekey_store,
            rpc,
            resolver,
        })
    }

    pub fn publish_prekey_bundle(&mut self) -> SecureE2EERpcResult {
        let bundle = self.ensure_fresh_prekey_bundle()?;
        self.publish_prekey_bundle_rpc(&bundle)
    }

    pub fn ensure_fresh_prekey_bundle(&mut self) -> Result<PrekeyBundle, String> {
        self.ensure_fresh_one_time_prekeys(DEFAULT_ONE_TIME_PREKEY_BATCH_SIZE)?;
        let signed_prekey = match self.signed_prekey_store.load_latest_signed_prekey() {
            Ok(Some((_private_key, metadata))) => metadata,
            Ok(None) => {
                let private_key = generated_x25519_private_key()?;
                let metadata = signed_prekey_from_private_key(
                    DEFAULT_SIGNED_PREKEY_ID,
                    &private_key,
                    DEFAULT_SIGNED_PREKEY_EXPIRY,
                )?;
                self.signed_prekey_store
                    .save_signed_prekey(&metadata.key_id, &private_key, &metadata)
                    .map_err(|err| err.to_string())?;
                metadata
            }
            Err(err) => return Err(err.to_string()),
        };
        let bundle = self.build_prekey_bundle(signed_prekey)?;
        let _ = self.publish_prekey_bundle_rpc(&bundle);
        Ok(bundle)
    }

    pub fn send_text(
        &mut self,
        peer_did: &str,
        text: &str,
        operation_id: &str,
        message_id: &str,
    ) -> SecureE2EERpcResult {
        self.send_application_plaintext(
            peer_did,
            ApplicationPlaintext::new_text("text/plain", text),
            operation_id,
            message_id,
        )
    }

    pub fn send_json(
        &mut self,
        peer_did: &str,
        payload: Map<String, Value>,
        operation_id: &str,
        message_id: &str,
    ) -> SecureE2EERpcResult {
        self.send_application_plaintext(
            peer_did,
            ApplicationPlaintext {
                application_content_type: "application/json".to_string(),
                conversation_id: None,
                reply_to_message_id: None,
                annotations: None,
                text: None,
                payload: Some(Value::Object(payload)),
                payload_b64u: None,
            },
            operation_id,
            message_id,
        )
    }

    fn send_application_plaintext(
        &mut self,
        peer_did: &str,
        plaintext: ApplicationPlaintext,
        operation_id: &str,
        message_id: &str,
    ) -> SecureE2EERpcResult {
        anpsdk::validate_direct_send_ids(operation_id, message_id)
            .map_err(|err| err.to_string())?;
        let metadata = DirectEnvelopeMetadata {
            sender_did: self.owner_did.clone(),
            recipient_did: peer_did.to_string(),
            message_id: message_id.to_string(),
            profile: DIRECT_E2EE_PROFILE.to_string(),
            security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_string(),
        };
        let mut session = self
            .session_store
            .find_by_peer_did(peer_did)
            .map_err(|err| err.to_string())?;
        if let Some(mut session) = session.take() {
            let (_pending, body) = anpsdk::DirectE2eeSession::encrypt_follow_up(
                &mut session,
                &metadata,
                operation_id,
                &plaintext,
            )
            .map_err(|err| err.to_string())?;
            self.session_store
                .save_session(&session)
                .map_err(|err| err.to_string())?;
            let request = anpsdk::direct_cipher_send_request(
                &self.owner_did,
                peer_did,
                operation_id,
                message_id,
                &body,
            )
            .map_err(|err| err.to_string())?;
            return self.call_request(request);
        }

        let verified = self.get_verified_prekey_bundle(peer_did)?;
        let did_document = (self.resolver)(peer_did)?;
        let recipient_static_public = anpsdk::extract_x25519_public_key(
            &did_document,
            &verified.bundle.static_key_agreement_id,
        )
        .map_err(|err| err.to_string())?;
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
        let agreement_private = match &self.agreement_private {
            PrivateKeyMaterial::X25519(key) => key,
            _ => return Err("invalid field: expected 32-byte private key".to_string()),
        };
        let (session, _pending, body) = anpsdk::DirectE2eeSession::initiate_session_with_opk(
            &metadata,
            operation_id,
            &self.agreement_key_id,
            agreement_private,
            &verified.bundle,
            &recipient_static_public,
            &recipient_signed_prekey_public,
            recipient_one_time_prekey_public.as_ref(),
            recipient_one_time_prekey_id,
            &plaintext,
        )
        .map_err(|err| err.to_string())?;
        self.session_store
            .save_session(&session)
            .map_err(|err| err.to_string())?;
        let request = anpsdk::direct_init_send_request(
            &self.owner_did,
            peer_did,
            operation_id,
            message_id,
            &body,
        )
        .map_err(|err| err.to_string())?;
        self.call_request(request)
    }

    fn get_verified_prekey_bundle(
        &mut self,
        target_did: &str,
    ) -> Result<VerifiedPrekeyBundle, String> {
        let did_document = (self.resolver)(target_did)?;
        let target_service_did = anpsdk::message_service_did_from_document(&did_document)
            .map_err(|err| err.to_string())?;
        let response =
            match self.fetch_prekey_bundle_response(target_did, &target_service_did, true) {
                Ok(response) => response,
                Err(err) if anpsdk::should_retry_without_opk_message(&err) => {
                    self.fetch_prekey_bundle_response(target_did, &target_service_did, false)?
                }
                Err(err) => return Err(err),
            };
        let bundle_value = response
            .get("prekey_bundle")
            .ok_or_else(|| "invalid field: prekey_bundle".to_string())?
            .clone();
        let bundle: PrekeyBundle = serde_json::from_value(bundle_value)
            .map_err(|_| "invalid field: prekey_bundle".to_string())?;
        anpsdk::verify_prekey_bundle(&bundle, &did_document).map_err(|err| err.to_string())?;
        let one_time_prekey = response
            .get("one_time_prekey")
            .cloned()
            .map(serde_json::from_value::<OneTimePrekey>)
            .transpose()
            .map_err(|err| format!("invalid field: one_time_prekey: {err}"))?;
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
    ) -> SecureE2EERpcResult {
        let operation_id = format!("op-get-prekey-{}", operation_nonce_hex());
        let request = anpsdk::prekey_bundle_get_request(
            &self.owner_did,
            target_service_did,
            target_did,
            require_opk,
            &operation_id,
        );
        self.call_request(request)
    }

    fn publish_prekey_bundle_rpc(&mut self, bundle: &PrekeyBundle) -> SecureE2EERpcResult {
        let one_time_prekeys = self
            .one_time_prekey_store
            .list_one_time_prekeys()
            .map_err(|err| err.to_string())?;
        let request = anpsdk::prekey_bundle_publish_request(
            &self.owner_did,
            &self.local_service_did,
            bundle,
            &one_time_prekeys,
        );
        self.call_request(request)
    }

    fn build_prekey_bundle(
        &self,
        signed_prekey: anpsdk::SignedPrekey,
    ) -> Result<PrekeyBundle, String> {
        anpsdk::build_prekey_bundle(
            &format!("spk-{}-{}", unix_seconds(), signed_prekey.key_id),
            &self.owner_did,
            &self.agreement_key_id,
            signed_prekey,
            &self.signing_private,
            &self.signing_key_id,
            None,
        )
        .map_err(|err| err.to_string())
    }

    fn ensure_fresh_one_time_prekeys(&mut self, min_count: usize) -> Result<(), String> {
        if min_count == 0 {
            return Ok(());
        }
        let current = self
            .one_time_prekey_store
            .list_one_time_prekeys()
            .map_err(|err| err.to_string())?;
        if current.len() >= min_count {
            return Ok(());
        }
        let prefix = unix_nanos();
        for index in current.len()..min_count {
            let key_id = format!("opk-{prefix}-{index:03}");
            let private_key = generated_x25519_private_key()?;
            let metadata = one_time_prekey_from_private_key(&key_id, &private_key)?;
            self.one_time_prekey_store
                .save_one_time_prekey(&key_id, &private_key, &metadata)
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    fn call_request(&mut self, request: Value) -> SecureE2EERpcResult {
        let object = request
            .as_object()
            .ok_or_else(|| "invalid field: request".to_string())?;
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing field: method".to_string())?
            .to_string();
        let params = object
            .get("params")
            .and_then(Value::as_object)
            .ok_or_else(|| "missing field: params".to_string())?
            .clone();
        (self.rpc)(&method, params)
    }
}

pub fn local_did_document(manager: Option<&Manager>, did: &str) -> Option<Value> {
    let manager = manager?;
    if did.is_empty() {
        return None;
    }
    let summaries = manager.list().ok()?;
    for summary in summaries {
        if summary.did != did {
            continue;
        }
        let record = manager.load(&summary.identity_name).ok()?;
        return record.did_document;
    }
    None
}

pub fn resolve_secure_e2ee_local_document(
    manager: Option<&Manager>,
    record: Option<&StoredIdentity>,
    did: &str,
) -> Option<Value> {
    if did.is_empty() {
        return None;
    }
    if let Some(record) = record {
        if did == record.did {
            if let Some(document) = &record.did_document {
                return Some(document.clone());
            }
        }
    }
    local_did_document(manager, did)
}

struct VerifiedPrekeyBundle {
    bundle: PrekeyBundle,
    one_time_prekey: Option<OneTimePrekey>,
}

fn generated_x25519_private_key() -> Result<PrivateKeyMaterial, String> {
    let bundle = anpsdk::create_did_wba_document("awiki.ai", anpsdk::DidDocumentOptions::default())
        .map_err(|err| err.to_string())?;
    bundle
        .load_private_key("key-3")
        .map_err(|err| err.to_string())
}

fn signed_prekey_from_private_key(
    key_id: &str,
    private_key: &PrivateKeyMaterial,
    expires_at: &str,
) -> Result<anpsdk::SignedPrekey, String> {
    Ok(anpsdk::SignedPrekey {
        key_id: key_id.to_string(),
        public_key_b64u: x25519_public_key_b64u(private_key)?,
        expires_at: expires_at.to_string(),
    })
}

fn one_time_prekey_from_private_key(
    key_id: &str,
    private_key: &PrivateKeyMaterial,
) -> Result<OneTimePrekey, String> {
    Ok(OneTimePrekey {
        key_id: key_id.to_string(),
        public_key_b64u: x25519_public_key_b64u(private_key)?,
    })
}

fn x25519_public_key_b64u(private_key: &PrivateKeyMaterial) -> Result<String, String> {
    match private_key.public_key() {
        anpsdk::PublicKeyMaterial::X25519(bytes) => {
            use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
            Ok(URL_SAFE_NO_PAD.encode(bytes))
        }
        _ => Err("invalid field: expected 32-byte private key".to_string()),
    }
}

fn decode_public_key_b64u(value: &str, field: &str) -> Result<[u8; 32], String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| format!("invalid field: {field}"))?;
    bytes
        .try_into()
        .map_err(|_| format!("invalid field: {field}"))
}

fn validate_one_time_prekey(prekey: &OneTimePrekey) -> Result<(), String> {
    if prekey.key_id.is_empty() {
        return Err("missing field: one_time_prekey.key_id".to_string());
    }
    if prekey.public_key_b64u.is_empty() {
        return Err("missing field: one_time_prekey.public_key_b64u".to_string());
    }
    Ok(())
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_nanos() -> u128 {
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
