use std::collections::HashMap;
use std::sync::Mutex;

use anp::direct_e2ee::{
    ApplicationPlaintext, DirectCipherBody, DirectE2eeSession, DirectEnvelopeMetadata,
    DirectInitBody,
};
use anp::PrivateKeyMaterial;
use serde_json::{Map, Value};

use super::client::map_direct_error;
use super::sqlite_store::{
    direct_session_from_blob, direct_session_metadata_json, direct_session_to_blob,
    DirectInitSessionCommit, DirectInitSessionCommitResult, DirectSessionCasResult,
    DirectSessionRecord,
};

const DIRECT_CIPHER_CONTENT_TYPE: &str = "application/anp-direct-cipher+json";
const DIRECT_INIT_CONTENT_TYPE: &str = "application/anp-direct-init+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AsyncDirectSecureReceiveFallback {
    UnsupportedContentType,
    NoEstablishedSession,
}

#[derive(Debug)]
pub(crate) enum AsyncDirectSecureReceiveOutcome {
    Processed(Map<String, Value>),
    ProcessedWithReplay {
        result: Map<String, Value>,
        replayed: Vec<AsyncDirectSecurePendingReplay>,
    },
    Fallback(AsyncDirectSecureReceiveFallback),
}

#[derive(Debug)]
pub(crate) struct AsyncDirectSecurePendingReplay {
    pub(crate) message_id: String,
    pub(crate) notification: Map<String, Value>,
    pub(crate) result: Map<String, Value>,
}

pub(crate) struct AsyncDirectSecureIncomingProcessor<'a> {
    client: &'a crate::core::ImClient,
    pending_by_peer: Mutex<HashMap<String, Vec<PendingDirectCipher>>>,
}

impl<'a> AsyncDirectSecureIncomingProcessor<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self {
            client,
            pending_by_peer: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn process_cipher_if_ready(
        &self,
        notification: Map<String, Value>,
    ) -> crate::ImResult<AsyncDirectSecureReceiveOutcome> {
        let input = match DirectCipherInput::from_notification(notification)? {
            Some(input) => input,
            None => {
                return Ok(AsyncDirectSecureReceiveOutcome::Fallback(
                    AsyncDirectSecureReceiveFallback::UnsupportedContentType,
                ))
            }
        };
        let db = self.client.core_inner().local_state_db().await?;
        let Some(record) = db
            .get_direct_secure_session(
                self.client.current_identity().id.as_str().to_owned(),
                input.sender_did.clone(),
            )
            .await?
        else {
            self.queue_pending_cipher(&input);
            return Ok(AsyncDirectSecureReceiveOutcome::Fallback(
                AsyncDirectSecureReceiveFallback::NoEstablishedSession,
            ));
        };
        if !is_record_for_cipher_session(&record, &input.cipher_body) {
            self.queue_pending_cipher(&input);
            return Ok(AsyncDirectSecureReceiveOutcome::Fallback(
                AsyncDirectSecureReceiveFallback::NoEstablishedSession,
            ));
        }

        let decrypted = crate::internal::runtime::worker::run_blocking(move || {
            decrypt_follow_up_from_record(record, input.metadata, input.cipher_body)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;

        let Some(decrypted) = decrypted else {
            return Ok(AsyncDirectSecureReceiveOutcome::Processed(
                undecryptable_result(),
            ));
        };
        let saved = db
            .save_direct_secure_session_if_revision(
                decrypted.updated_record,
                decrypted.expected_revision,
            )
            .await?;
        match saved {
            DirectSessionCasResult::Saved(_) => {
                Ok(AsyncDirectSecureReceiveOutcome::Processed(decrypted.result))
            }
            DirectSessionCasResult::Stale { .. } => Err(crate::ImError::LocalStateUnavailable {
                detail: "direct E2EE session changed before async receive could persist mutation"
                    .to_owned(),
            }),
        }
    }

    pub(crate) async fn process_init_if_ready(
        &self,
        notification: Map<String, Value>,
        sender_document: Value,
    ) -> crate::ImResult<AsyncDirectSecureReceiveOutcome> {
        let input = match DirectInitInput::from_notification(notification)? {
            Some(input) => input,
            None => {
                return Ok(AsyncDirectSecureReceiveOutcome::Fallback(
                    AsyncDirectSecureReceiveFallback::UnsupportedContentType,
                ))
            }
        };
        let db = self.client.core_inner().local_state_db().await?;
        let material = db
            .direct_init_session_material(
                self.client.current_identity().id.as_str().to_owned(),
                input.sender_did.clone(),
                input.init_body.session_id.clone(),
                input.init_body.recipient_signed_prekey_id.clone(),
                input.init_body.recipient_one_time_prekey_id.clone(),
            )
            .await?;
        let Some(signed_prekey_private) = material.signed_prekey_private else {
            return Ok(AsyncDirectSecureReceiveOutcome::Fallback(
                AsyncDirectSecureReceiveFallback::NoEstablishedSession,
            ));
        };
        let (agreement_key_id, agreement_private) =
            super::identity_material::agreement_key_material(self.client)?;
        let owner_identity_id = self.client.current_identity().id.as_str().to_owned();
        let owner_did = self.client.did().as_str().to_owned();
        let consume_one_time_prekey_id = input.init_body.recipient_one_time_prekey_id.clone();
        let accepted = crate::internal::runtime::worker::run_blocking(move || {
            accept_incoming_init_from_material(IncomingInitCryptoInput {
                owner_identity_id,
                owner_did,
                agreement_key_id,
                agreement_private,
                signed_prekey_private,
                one_time_prekey_private: material.one_time_prekey_private,
                peer_session_revision: material.peer_session_revision,
                existing_session: material.existing_session,
                sender_document,
                metadata: input.metadata,
                init_body: input.init_body,
            })
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;
        let Some(record) = accepted.record else {
            return Ok(AsyncDirectSecureReceiveOutcome::Processed(accepted.result));
        };
        let commit = DirectInitSessionCommit {
            record,
            expected_peer_revision: accepted.expected_peer_revision,
            consume_one_time_prekey_id,
            consumed_at: now_utc_like(),
        };
        match db.save_incoming_direct_init_session(commit).await? {
            DirectInitSessionCommitResult::Saved(_)
            | DirectInitSessionCommitResult::Existing(_) => {
                let replayed = self
                    .replay_pending_for_peer(
                        db.clone(),
                        self.client.current_identity().id.as_str().to_owned(),
                        input.sender_did,
                    )
                    .await?;
                if replayed.is_empty() {
                    Ok(AsyncDirectSecureReceiveOutcome::Processed(accepted.result))
                } else {
                    Ok(AsyncDirectSecureReceiveOutcome::ProcessedWithReplay {
                        result: accepted.result,
                        replayed,
                    })
                }
            }
            DirectInitSessionCommitResult::Stale { .. } => {
                Err(crate::ImError::LocalStateUnavailable {
                    detail:
                        "direct E2EE session changed before async init receive could persist mutation"
                            .to_owned(),
                })
            }
        }
    }

    fn queue_pending_cipher(&self, input: &DirectCipherInput) {
        self.pending_by_peer
            .lock()
            .expect("direct secure pending queue mutex poisoned")
            .entry(input.sender_did.clone())
            .or_default()
            .push(PendingDirectCipher {
                message_id: input.metadata.message_id.clone(),
                notification: input.notification.clone(),
                metadata: input.metadata.clone(),
                cipher_body: input.cipher_body.clone(),
            });
    }

    async fn replay_pending_for_peer(
        &self,
        db: crate::internal::local_state::actor::LocalStateDb,
        owner_identity_id: String,
        peer_did: String,
    ) -> crate::ImResult<Vec<AsyncDirectSecurePendingReplay>> {
        let pending = self
            .pending_by_peer
            .lock()
            .expect("direct secure pending queue mutex poisoned")
            .remove(&peer_did)
            .unwrap_or_default();
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        let mut replayed = Vec::new();
        let mut pending = pending.into_iter();
        while let Some(pending_cipher) = pending.next() {
            let Some(record) = db
                .get_direct_secure_session(owner_identity_id.clone(), peer_did.clone())
                .await?
            else {
                self.requeue_pending(peer_did.clone(), pending_cipher, pending);
                break;
            };
            if !is_record_for_cipher_session(&record, &pending_cipher.cipher_body) {
                self.requeue_pending(peer_did.clone(), pending_cipher, pending);
                break;
            }
            let message_id = pending_cipher.message_id.clone();
            let decrypted = crate::internal::runtime::worker::run_blocking(move || {
                decrypt_follow_up_from_record(
                    record,
                    pending_cipher.metadata,
                    pending_cipher.cipher_body,
                )
            })
            .await
            .map_err(|err| crate::ImError::Internal {
                message: err.to_string(),
            })??;

            let Some(decrypted) = decrypted else {
                replayed.push(AsyncDirectSecurePendingReplay {
                    message_id,
                    notification: pending_cipher.notification,
                    result: undecryptable_result(),
                });
                continue;
            };
            let saved = db
                .save_direct_secure_session_if_revision(
                    decrypted.updated_record,
                    decrypted.expected_revision,
                )
                .await?;
            match saved {
                DirectSessionCasResult::Saved(_) => {
                    replayed.push(AsyncDirectSecurePendingReplay {
                        message_id,
                        notification: pending_cipher.notification,
                        result: decrypted.result,
                    });
                }
                DirectSessionCasResult::Stale { .. } => {
                    return Err(crate::ImError::LocalStateUnavailable {
                        detail:
                            "direct E2EE session changed before async pending receive could persist mutation"
                                .to_owned(),
                    });
                }
            }
        }
        Ok(replayed)
    }

    fn requeue_pending(
        &self,
        peer_did: String,
        pending: PendingDirectCipher,
        remaining: impl Iterator<Item = PendingDirectCipher>,
    ) {
        let mut pending_by_peer = self
            .pending_by_peer
            .lock()
            .expect("direct secure pending queue mutex poisoned");
        let queue = pending_by_peer.entry(peer_did).or_default();
        queue.push(pending);
        queue.extend(remaining);
    }
}

#[derive(Debug)]
struct DirectCipherInput {
    sender_did: String,
    notification: Map<String, Value>,
    metadata: DirectEnvelopeMetadata,
    cipher_body: DirectCipherBody,
}

struct PendingDirectCipher {
    message_id: String,
    notification: Map<String, Value>,
    metadata: DirectEnvelopeMetadata,
    cipher_body: DirectCipherBody,
}

impl DirectCipherInput {
    fn from_notification(notification: Map<String, Value>) -> crate::ImResult<Option<Self>> {
        let meta = notification.get("meta").and_then(Value::as_object);
        let content_type = string_value(meta.and_then(|value| value.get("content_type")));
        if content_type != DIRECT_CIPHER_CONTENT_TYPE {
            return Ok(None);
        }
        let sender_did = string_value(meta.and_then(|value| value.get("sender_did")));
        if sender_did.trim().is_empty() {
            return Err(crate::ImError::Serialization {
                detail: "direct E2EE cipher notification is missing sender_did".to_owned(),
            });
        }
        let recipient_did = meta
            .and_then(|value| value.get("target"))
            .and_then(Value::as_object)
            .and_then(|value| value.get("did"))
            .map(string_value_from_value)
            .unwrap_or_default();
        let metadata = DirectEnvelopeMetadata {
            sender_did: sender_did.clone(),
            recipient_did,
            message_id: string_value(meta.and_then(|value| value.get("message_id"))),
            profile: string_value(meta.and_then(|value| value.get("profile"))),
            security_profile: string_value(meta.and_then(|value| value.get("security_profile"))),
        };
        let body = notification.get("body").cloned().unwrap_or(Value::Null);
        let cipher_body =
            anp::direct_e2ee::direct_cipher_body_from_value(&body).map_err(map_direct_error)?;
        Ok(Some(Self {
            sender_did,
            notification,
            metadata,
            cipher_body,
        }))
    }
}

#[derive(Debug)]
struct DirectInitInput {
    sender_did: String,
    metadata: DirectEnvelopeMetadata,
    init_body: DirectInitBody,
}

impl DirectInitInput {
    fn from_notification(notification: Map<String, Value>) -> crate::ImResult<Option<Self>> {
        let meta = notification.get("meta").and_then(Value::as_object);
        let content_type = string_value(meta.and_then(|value| value.get("content_type")));
        if content_type != DIRECT_INIT_CONTENT_TYPE {
            return Ok(None);
        }
        let sender_did = string_value(meta.and_then(|value| value.get("sender_did")));
        if sender_did.trim().is_empty() {
            return Err(crate::ImError::Serialization {
                detail: "direct E2EE init notification is missing sender_did".to_owned(),
            });
        }
        let recipient_did = meta
            .and_then(|value| value.get("target"))
            .and_then(Value::as_object)
            .and_then(|value| value.get("did"))
            .map(string_value_from_value)
            .unwrap_or_default();
        let metadata = DirectEnvelopeMetadata {
            sender_did: sender_did.clone(),
            recipient_did,
            message_id: string_value(meta.and_then(|value| value.get("message_id"))),
            profile: string_value(meta.and_then(|value| value.get("profile"))),
            security_profile: string_value(meta.and_then(|value| value.get("security_profile"))),
        };
        let body = notification.get("body").cloned().unwrap_or(Value::Null);
        let init_body =
            anp::direct_e2ee::direct_init_body_from_value(&body).map_err(map_direct_error)?;
        Ok(Some(Self {
            sender_did,
            metadata,
            init_body,
        }))
    }
}

#[derive(Debug)]
struct DecryptedFollowUp {
    updated_record: DirectSessionRecord,
    expected_revision: i64,
    result: Map<String, Value>,
}

fn decrypt_follow_up_from_record(
    mut record: DirectSessionRecord,
    metadata: DirectEnvelopeMetadata,
    cipher_body: DirectCipherBody,
) -> crate::ImResult<Option<DecryptedFollowUp>> {
    let expected_revision = record.revision;
    let mut session = direct_session_from_blob(&record.state_blob).map_err(map_direct_error)?;
    if session.session_id != cipher_body.session_id {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "direct E2EE cipher session does not match stored session".to_owned(),
        });
    }
    let plaintext =
        match DirectE2eeSession::decrypt_follow_up(&mut session, &metadata, &cipher_body, "") {
            Ok(plaintext) => plaintext,
            Err(_) => return Ok(None),
        };
    record.session_id = session.session_id.clone();
    record.state_blob = direct_session_to_blob(&session).map_err(map_direct_error)?;
    record.metadata_json = direct_session_metadata_json(&session).map_err(map_direct_error)?;
    record.updated_at = now_utc_like();
    Ok(Some(DecryptedFollowUp {
        updated_record: record,
        expected_revision,
        result: Map::from_iter([
            ("state".to_owned(), Value::String("decrypted".to_owned())),
            (
                "plaintext".to_owned(),
                anp::direct_e2ee::plaintext_to_value(&plaintext),
            ),
        ]),
    }))
}

fn is_record_for_cipher_session(
    record: &DirectSessionRecord,
    cipher_body: &DirectCipherBody,
) -> bool {
    record.session_id == cipher_body.session_id
}

fn undecryptable_result() -> Map<String, Value> {
    Map::from_iter([(
        "state".to_owned(),
        Value::String("undecryptable".to_owned()),
    )])
}

struct IncomingInitCryptoInput {
    owner_identity_id: String,
    owner_did: String,
    agreement_key_id: String,
    agreement_private: PrivateKeyMaterial,
    signed_prekey_private: PrivateKeyMaterial,
    one_time_prekey_private: Option<PrivateKeyMaterial>,
    peer_session_revision: Option<i64>,
    existing_session: Option<DirectSessionRecord>,
    sender_document: Value,
    metadata: DirectEnvelopeMetadata,
    init_body: DirectInitBody,
}

struct AcceptedIncomingInit {
    record: Option<DirectSessionRecord>,
    expected_peer_revision: Option<i64>,
    result: Map<String, Value>,
}

fn accept_incoming_init_from_material(
    input: IncomingInitCryptoInput,
) -> crate::ImResult<AcceptedIncomingInit> {
    let sender_static_public = anp::direct_e2ee::extract_x25519_public_key(
        &input.sender_document,
        &input.init_body.sender_static_key_agreement_id,
    )
    .map_err(map_direct_error)?;
    let PrivateKeyMaterial::X25519(agreement_private) = &input.agreement_private else {
        return Err(expected_x25519_private_key());
    };
    let PrivateKeyMaterial::X25519(signed_prekey_private) = &input.signed_prekey_private else {
        return Err(expected_x25519_private_key());
    };
    let one_time_prekey_private = match input.one_time_prekey_private.as_ref() {
        Some(PrivateKeyMaterial::X25519(key)) => Some(key),
        Some(_) => return Err(expected_x25519_private_key()),
        None => None,
    };
    let (session, plaintext) = DirectE2eeSession::accept_incoming_init_with_opk(
        &input.metadata,
        &input.agreement_key_id,
        agreement_private,
        signed_prekey_private,
        one_time_prekey_private,
        &sender_static_public,
        &input.init_body,
    )
    .map_err(map_direct_error)?;
    if let Some(existing) = input.existing_session {
        if existing.peer_did != input.metadata.sender_did {
            return Err(crate::ImError::Serialization {
                detail: "direct E2EE init session id is already bound to another peer".to_owned(),
            });
        }
        return Ok(AcceptedIncomingInit {
            record: None,
            expected_peer_revision: input.peer_session_revision,
            result: decrypted_plaintext_result(&plaintext),
        });
    }
    let now = now_utc_like();
    Ok(AcceptedIncomingInit {
        record: Some(DirectSessionRecord {
            owner_identity_id: input.owner_identity_id,
            owner_did: input.owner_did,
            peer_did: session.peer_did.clone(),
            session_id: session.session_id.clone(),
            state_blob: direct_session_to_blob(&session).map_err(map_direct_error)?,
            metadata_json: direct_session_metadata_json(&session).map_err(map_direct_error)?,
            revision: input.peer_session_revision.unwrap_or_default(),
            created_at: now.clone(),
            updated_at: now,
        }),
        expected_peer_revision: input.peer_session_revision,
        result: decrypted_plaintext_result(&plaintext),
    })
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

fn expected_x25519_private_key() -> crate::ImError {
    crate::ImError::Serialization {
        detail: "expected X25519 private key".to_owned(),
    }
}

fn string_value(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn string_value_from_value(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
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
pub(crate) mod test_support {
    use anp::direct_e2ee::models::{MTI_DIRECT_E2EE_SUITE, SESSION_STATUS_ESTABLISHED};
    use anp::direct_e2ee::{
        ApplicationPlaintext, DirectCipherBody, DirectE2eeSession, DirectEnvelopeMetadata,
    };
    use serde_json::{json, Map, Value};

    use super::*;

    pub(crate) struct EstablishedExchange {
        pub(crate) alice_session: anp::direct_e2ee::DirectSessionState,
        pub(crate) bob_session: anp::direct_e2ee::DirectSessionState,
        pub(crate) message_metadata: DirectEnvelopeMetadata,
        pub(crate) cipher_body: DirectCipherBody,
    }

    pub(crate) struct FirstReplyExchange {
        pub(crate) alice_session: anp::direct_e2ee::DirectSessionState,
        pub(crate) bob_session: anp::direct_e2ee::DirectSessionState,
        pub(crate) reply_metadata: DirectEnvelopeMetadata,
        pub(crate) reply_body: DirectCipherBody,
    }

    pub(crate) struct IncomingInitExchange {
        pub(crate) sender_did: String,
        pub(crate) sender_document: Value,
        pub(crate) recipient_did: String,
        pub(crate) recipient_document: Value,
        pub(crate) recipient_agreement_private: anp::PrivateKeyMaterial,
        pub(crate) recipient_signed_prekey_private: anp::PrivateKeyMaterial,
        pub(crate) recipient_one_time_prekey_private: anp::PrivateKeyMaterial,
        pub(crate) recipient_signed_prekey: anp::direct_e2ee::SignedPrekey,
        pub(crate) recipient_one_time_prekey: anp::direct_e2ee::OneTimePrekey,
        pub(crate) metadata: DirectEnvelopeMetadata,
        pub(crate) init_body: anp::direct_e2ee::DirectInitBody,
        pub(crate) sender_session: anp::direct_e2ee::DirectSessionState,
    }

    pub(crate) fn direct_cipher_notification(
        metadata: &DirectEnvelopeMetadata,
        body: &DirectCipherBody,
    ) -> Map<String, Value> {
        json!({
            "meta": {
                "sender_did": metadata.sender_did,
                "target": {
                    "kind": "agent",
                    "did": metadata.recipient_did,
                },
                "message_id": metadata.message_id,
                "profile": metadata.profile,
                "security_profile": metadata.security_profile,
                "content_type": DIRECT_CIPHER_CONTENT_TYPE,
            },
            "body": anp::direct_e2ee::direct_cipher_body_to_value(body),
        })
        .as_object()
        .cloned()
        .unwrap()
    }

    pub(crate) fn direct_cipher_realtime_notification(
        metadata: &DirectEnvelopeMetadata,
        body: &DirectCipherBody,
    ) -> Value {
        json!({
            "method": "direct.incoming",
            "params": direct_cipher_notification(metadata, body),
        })
    }

    pub(crate) fn direct_init_notification(
        metadata: &DirectEnvelopeMetadata,
        body: &anp::direct_e2ee::DirectInitBody,
    ) -> Map<String, Value> {
        json!({
            "meta": {
                "sender_did": metadata.sender_did,
                "target": {
                    "kind": "agent",
                    "did": metadata.recipient_did,
                },
                "message_id": metadata.message_id,
                "profile": metadata.profile,
                "security_profile": metadata.security_profile,
                "content_type": DIRECT_INIT_CONTENT_TYPE,
            },
            "body": anp::direct_e2ee::direct_init_body_to_value(body),
        })
        .as_object()
        .cloned()
        .unwrap()
    }

    pub(crate) fn established_exchange() -> EstablishedExchange {
        let exchange = first_reply_exchange();
        let mut alice_session = exchange.alice_session.clone();
        DirectE2eeSession::decrypt_follow_up(
            &mut alice_session,
            &exchange.reply_metadata,
            &exchange.reply_body,
            "text/plain",
        )
        .unwrap();
        assert_eq!(alice_session.status, SESSION_STATUS_ESTABLISHED);

        let mut bob_session = exchange.bob_session;
        assert_eq!(bob_session.status, SESSION_STATUS_ESTABLISHED);
        let message_metadata = DirectEnvelopeMetadata {
            sender_did: exchange.reply_metadata.sender_did,
            recipient_did: exchange.reply_metadata.recipient_did,
            message_id: "msg-async-receive".to_owned(),
            profile: "anp.direct.e2ee.v1".to_owned(),
            security_profile: "direct-e2ee".to_owned(),
        };
        let (_, cipher_body) = DirectE2eeSession::encrypt_follow_up(
            &mut bob_session,
            &message_metadata,
            "msg-async-receive",
            &ApplicationPlaintext::new_text("text/plain", "async receive secret"),
        )
        .unwrap();
        EstablishedExchange {
            alice_session,
            bob_session,
            message_metadata,
            cipher_body,
        }
    }

    pub(crate) fn first_reply_exchange() -> FirstReplyExchange {
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
            profile: "anp.direct.e2ee.v1".to_owned(),
            security_profile: "direct-e2ee".to_owned(),
        };
        let (alice_session, _, init_body) = DirectE2eeSession::initiate_session(
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
            profile: "anp.direct.e2ee.v1".to_owned(),
            security_profile: "direct-e2ee".to_owned(),
        };
        let (_, reply_body) = DirectE2eeSession::encrypt_follow_up(
            &mut bob_session,
            &reply_metadata,
            "msg-reply",
            &ApplicationPlaintext::new_text("text/plain", "reply"),
        )
        .unwrap();
        FirstReplyExchange {
            alice_session,
            bob_session,
            reply_metadata,
            reply_body,
        }
    }

    pub(crate) fn incoming_init_exchange() -> IncomingInitExchange {
        let recipient_bundle = anp::authentication::create_did_wba_document(
            "recipient.async-receive.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        let recipient_static = recipient_bundle.load_private_key("key-3").unwrap();
        let recipient_did = recipient_bundle.did().unwrap().to_owned();
        incoming_init_exchange_for_recipient(
            recipient_did.clone(),
            recipient_bundle.did_document,
            format!("{recipient_did}#key-3"),
            recipient_static,
        )
    }

    pub(crate) fn incoming_init_exchange_for_recipient(
        recipient_did: String,
        recipient_document: Value,
        recipient_agreement_key_id: String,
        recipient_static: anp::PrivateKeyMaterial,
    ) -> IncomingInitExchange {
        let sender_bundle = anp::authentication::create_did_wba_document(
            "sender.async-receive.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        let sender_static = sender_bundle.load_private_key("key-3").unwrap();
        let recipient_spk = generated_x25519_private();
        let recipient_opk = generated_x25519_private();
        let anp::PrivateKeyMaterial::X25519(sender_static_key) = &sender_static else {
            panic!("expected X25519 private key");
        };
        let anp::PrivateKeyMaterial::X25519(_) = &recipient_static else {
            panic!("expected X25519 private key");
        };
        let sender_did = sender_bundle.did().unwrap().to_owned();
        let sender_document = sender_bundle.did_document.clone();
        let recipient_signed_prekey = anp::direct_e2ee::SignedPrekey {
            key_id: "spk-alice".to_owned(),
            public_key_b64u: base64url(&x25519_public(&recipient_spk)),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
        };
        let recipient_one_time_prekey = anp::direct_e2ee::OneTimePrekey {
            key_id: "opk-alice".to_owned(),
            public_key_b64u: base64url(&x25519_public(&recipient_opk)),
        };
        let recipient_prekey_bundle = anp::direct_e2ee::PrekeyBundle {
            bundle_id: "bundle-alice".to_owned(),
            owner_did: recipient_did.clone(),
            suite: MTI_DIRECT_E2EE_SUITE.to_owned(),
            static_key_agreement_id: recipient_agreement_key_id,
            signed_prekey: recipient_signed_prekey.clone(),
            proof: json!({}),
        };
        let metadata = DirectEnvelopeMetadata {
            sender_did: sender_did.clone(),
            recipient_did: recipient_did.clone(),
            message_id: "msg-init-async".to_owned(),
            profile: "anp.direct.e2ee.v1".to_owned(),
            security_profile: "direct-e2ee".to_owned(),
        };
        let (sender_session, _, init_body) = DirectE2eeSession::initiate_session_with_opk(
            &metadata,
            "msg-init-async",
            &format!("{sender_did}#key-3"),
            sender_static_key,
            &recipient_prekey_bundle,
            &x25519_public(&recipient_static),
            &x25519_public(&recipient_spk),
            Some(&x25519_public(&recipient_opk)),
            Some(recipient_one_time_prekey.key_id.clone()),
            &ApplicationPlaintext::new_text("text/plain", "hello from init"),
        )
        .unwrap();
        IncomingInitExchange {
            sender_did,
            sender_document,
            recipient_did,
            recipient_document,
            recipient_agreement_private: recipient_static,
            recipient_signed_prekey_private: recipient_spk,
            recipient_one_time_prekey_private: recipient_opk,
            recipient_signed_prekey,
            recipient_one_time_prekey,
            metadata,
            init_body,
            sender_session,
        }
    }

    pub(crate) fn generated_x25519_private() -> anp::PrivateKeyMaterial {
        let bundle = anp::authentication::create_did_wba_document(
            "keys.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        bundle.load_private_key("key-3").unwrap()
    }

    pub(crate) fn x25519_public(private: &anp::PrivateKeyMaterial) -> [u8; 32] {
        match private.public_key() {
            anp::PublicKeyMaterial::X25519(bytes) => bytes,
            _ => panic!("expected X25519 public key"),
        }
    }

    pub(crate) fn base64url(bytes: &[u8]) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        URL_SAFE_NO_PAD.encode(bytes)
    }

    pub(crate) fn corrupt_b64u_tail(value: &str) -> String {
        let Some(last) = value.as_bytes().last().copied() else {
            return "A".to_owned();
        };
        let replacement = if last == b'A' { 'B' } else { 'A' };
        format!("{}{}", &value[..value.len() - 1], replacement)
    }

    pub(crate) struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        pub(crate) fn new() -> Self {
            Self::with_identity("did:example:alice", json!({}))
        }

        pub(crate) fn with_identity(did: &str, did_document: Value) -> Self {
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
                        "did": did,
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(identity_dir.join("did.json"), did_document.to_string()).unwrap();
            std::fs::write(identity_dir.join("private.key"), "test-key").unwrap();
            std::fs::write(
                identity_dir.join("auth.json"),
                r#"{"jwt_token":"test-token"}"#,
            )
            .unwrap();
            Self { root }
        }

        pub(crate) fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    client_version_info: None,
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

        pub(crate) fn identity_dir(&self) -> std::path::PathBuf {
            self.root.path().join("identities").join("alice")
        }

        pub(crate) fn sqlite_path(&self) -> std::path::PathBuf {
            self.root.path().join("local").join("im.sqlite")
        }
    }
}

#[cfg(test)]
mod tests {
    use anp::direct_e2ee::models::SESSION_STATUS_ESTABLISHED;
    use serde_json::json;

    use super::*;
    use test_support::*;

    #[test]
    fn decrypt_follow_up_from_record_mutates_session_and_returns_plaintext() {
        let exchange = established_exchange();
        let record = DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: exchange.alice_session.session_id.clone(),
            state_blob: direct_session_to_blob(&exchange.alice_session).unwrap(),
            metadata_json: "{}".to_owned(),
            revision: 4,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        };

        let decrypted =
            decrypt_follow_up_from_record(record, exchange.message_metadata, exchange.cipher_body)
                .unwrap()
                .unwrap();

        assert_eq!(decrypted.expected_revision, 4);
        assert_eq!(decrypted.updated_record.revision, 4);
        assert_eq!(
            decrypted.result["plaintext"]["text"],
            json!("async receive secret")
        );
        let updated = direct_session_from_blob(&decrypted.updated_record.state_blob).unwrap();
        assert_eq!(updated.recv_n, exchange.alice_session.recv_n + 1);
        assert!(!Value::Object(decrypted.result)
            .to_string()
            .contains("ciphertext_b64u"));
    }

    #[test]
    fn decrypt_follow_up_from_record_returns_undecryptable_without_mutation_for_bad_cipher() {
        let mut exchange = established_exchange();
        exchange.cipher_body.ciphertext_b64u =
            corrupt_b64u_tail(&exchange.cipher_body.ciphertext_b64u);
        let record = DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: exchange.alice_session.session_id.clone(),
            state_blob: direct_session_to_blob(&exchange.alice_session).unwrap(),
            metadata_json: "{}".to_owned(),
            revision: 0,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        };

        let decrypted =
            decrypt_follow_up_from_record(record, exchange.message_metadata, exchange.cipher_body)
                .unwrap();

        assert!(decrypted.is_none());
    }

    #[test]
    fn decrypt_follow_up_from_record_confirms_pending_session_from_first_reply() {
        let exchange = first_reply_exchange();
        let record = DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: exchange.alice_session.session_id.clone(),
            state_blob: direct_session_to_blob(&exchange.alice_session).unwrap(),
            metadata_json: "{}".to_owned(),
            revision: 0,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        };

        let decrypted =
            decrypt_follow_up_from_record(record, exchange.reply_metadata, exchange.reply_body)
                .unwrap()
                .unwrap();

        assert_eq!(decrypted.result["plaintext"]["text"], json!("reply"));
        let updated = direct_session_from_blob(&decrypted.updated_record.state_blob).unwrap();
        assert_eq!(updated.status, SESSION_STATUS_ESTABLISHED);
        assert_eq!(updated.recv_n, 1);
    }

    #[tokio::test]
    async fn async_direct_receive_processor_uses_actor_cas_for_established_cipher() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let exchange = established_exchange();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: exchange.alice_session.session_id.clone(),
                state_blob: direct_session_to_blob(&exchange.alice_session).unwrap(),
                metadata_json: direct_session_metadata_json(&exchange.alice_session).unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();

        let outcome = AsyncDirectSecureIncomingProcessor::new(&client)
            .process_cipher_if_ready(direct_cipher_notification(
                &exchange.message_metadata,
                &exchange.cipher_body,
            ))
            .await
            .unwrap();

        let AsyncDirectSecureReceiveOutcome::Processed(result) = outcome else {
            panic!("established direct cipher should be processed by async receive path");
        };
        assert_eq!(result["state"], json!("decrypted"));
        assert_eq!(result["plaintext"]["text"], json!("async receive secret"));
        assert!(!Value::Object(result)
            .to_string()
            .contains("ciphertext_b64u"));

        let saved = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        let saved_session = direct_session_from_blob(&saved.state_blob).unwrap();
        assert_eq!(saved_session.recv_n, exchange.alice_session.recv_n + 1);
    }

    #[tokio::test]
    async fn async_direct_receive_processor_confirms_pending_session_from_first_reply() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let exchange = first_reply_exchange();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: exchange.alice_session.session_id.clone(),
                state_blob: direct_session_to_blob(&exchange.alice_session).unwrap(),
                metadata_json: direct_session_metadata_json(&exchange.alice_session).unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();

        let outcome = AsyncDirectSecureIncomingProcessor::new(&client)
            .process_cipher_if_ready(direct_cipher_notification(
                &exchange.reply_metadata,
                &exchange.reply_body,
            ))
            .await
            .unwrap();

        let AsyncDirectSecureReceiveOutcome::Processed(result) = outcome else {
            panic!("pending confirmation first reply should be processed");
        };
        assert_eq!(result["state"], json!("decrypted"));
        assert_eq!(result["plaintext"]["text"], json!("reply"));
        let saved = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        let saved_session = direct_session_from_blob(&saved.state_blob).unwrap();
        assert_eq!(saved_session.status, SESSION_STATUS_ESTABLISHED);
        assert_eq!(saved_session.recv_n, 1);
    }

    #[tokio::test]
    async fn async_direct_receive_processor_falls_back_without_established_session() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let exchange = established_exchange();

        let outcome = AsyncDirectSecureIncomingProcessor::new(&client)
            .process_cipher_if_ready(direct_cipher_notification(
                &exchange.message_metadata,
                &exchange.cipher_body,
            ))
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            AsyncDirectSecureReceiveOutcome::Fallback(
                AsyncDirectSecureReceiveFallback::NoEstablishedSession
            )
        ));
    }

    #[tokio::test]
    async fn async_direct_receive_processor_accepts_init_via_actor_and_consumes_opk() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let exchange = incoming_init_exchange();
        std::fs::write(
            fixture.identity_dir().join("e2ee-agreement-private.pem"),
            exchange.recipient_agreement_private.to_pem(),
        )
        .unwrap();
        let db = client.core_inner().local_state_db().await.unwrap();
        {
            let connection =
                crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                super::super::sqlite_store::SqliteDirectSecureStateStore::new(&connection).unwrap();
            store
                .upsert_signed_prekey(&super::super::sqlite_store::DirectSignedPrekeyRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_signed_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_signed_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_signed_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status: super::super::sqlite_store::DirectPrekeyStatus::Active,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_signed_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    updated_at: "2026-05-24T00:00:00Z".to_owned(),
                })
                .unwrap();
            store
                .upsert_one_time_prekey(&super::super::sqlite_store::DirectOneTimePrekeyRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_one_time_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_one_time_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_one_time_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status: super::super::sqlite_store::DirectPrekeyStatus::Available,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_one_time_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    consumed_at: String::new(),
                })
                .unwrap();
        }

        let outcome = AsyncDirectSecureIncomingProcessor::new(&client)
            .process_init_if_ready(
                direct_init_notification(&exchange.metadata, &exchange.init_body),
                exchange.sender_document,
            )
            .await
            .unwrap();

        let AsyncDirectSecureReceiveOutcome::Processed(result) = outcome else {
            panic!("direct init should be processed by async receive path");
        };
        assert_eq!(result["state"], json!("decrypted"));
        assert_eq!(result["plaintext"]["text"], json!("hello from init"));
        let saved = db
            .get_direct_secure_session("alice-id", exchange.sender_did)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 0);
        let saved_session = direct_session_from_blob(&saved.state_blob).unwrap();
        assert_eq!(saved_session.status, SESSION_STATUS_ESTABLISHED);
        assert_eq!(saved_session.session_id, exchange.init_body.session_id);
        let connection =
            crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            super::super::sqlite_store::SqliteDirectSecureStateStore::new(&connection).unwrap();
        let consumed = store
            .get_one_time_prekey("alice-id", "opk-alice")
            .unwrap()
            .unwrap();
        assert_eq!(
            consumed.status,
            super::super::sqlite_store::DirectPrekeyStatus::Consumed
        );
    }

    #[tokio::test]
    async fn async_direct_receive_uses_exact_device_agreement_id_when_v2_gate_is_disabled() {
        let fixture = super::super::v2_product::v2_product_tests::RuntimeP5TestClientFixture::new(
            "async-receive-exact-device",
        );
        let client = fixture.client_with_direct_e2ee_enabled(false);
        assert!(!client.core_inner().direct_e2ee_v2_enabled());

        let agreement = super::super::identity_material::agreement_material(&client).unwrap();
        assert!(!agreement.agreement_key_id.ends_with("#key-3"));
        let recipient_private =
            PrivateKeyMaterial::from_pem(&agreement.agreement_private_pem).unwrap();
        let exchange = incoming_init_exchange_for_recipient(
            client.did().as_str().to_owned(),
            super::super::identity_material::local_did_document(&client).unwrap(),
            agreement.agreement_key_id.clone(),
            recipient_private,
        );
        let owner_identity_id = client.current_identity().id.as_str().to_owned();
        {
            let connection =
                crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                super::super::sqlite_store::SqliteDirectSecureStateStore::new(&connection).unwrap();
            store
                .upsert_signed_prekey(&super::super::sqlite_store::DirectSignedPrekeyRecord {
                    owner_identity_id: owner_identity_id.clone(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_signed_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_signed_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_signed_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status: super::super::sqlite_store::DirectPrekeyStatus::Active,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_signed_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-08-02T00:00:00Z".to_owned(),
                    updated_at: "2026-08-02T00:00:00Z".to_owned(),
                })
                .unwrap();
            store
                .upsert_one_time_prekey(&super::super::sqlite_store::DirectOneTimePrekeyRecord {
                    owner_identity_id: owner_identity_id.clone(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_one_time_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_one_time_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_one_time_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status: super::super::sqlite_store::DirectPrekeyStatus::Available,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_one_time_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-08-02T00:00:00Z".to_owned(),
                    consumed_at: String::new(),
                })
                .unwrap();
        }

        let outcome = AsyncDirectSecureIncomingProcessor::new(&client)
            .process_init_if_ready(
                direct_init_notification(&exchange.metadata, &exchange.init_body),
                exchange.sender_document,
            )
            .await
            .unwrap();

        let AsyncDirectSecureReceiveOutcome::Processed(result) = outcome else {
            panic!("exact-device direct init should use the bound agreement key id");
        };
        assert_eq!(result["state"], json!("decrypted"));
        assert_eq!(result["plaintext"]["text"], json!("hello from init"));
        let saved = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .get_direct_secure_session(owner_identity_id, exchange.sender_did)
            .await
            .unwrap();
        assert!(saved.is_some());
    }

    #[tokio::test]
    async fn async_direct_receive_processor_replays_pending_cipher_after_init() {
        let exchange = incoming_init_exchange();
        let fixture =
            Fixture::with_identity(&exchange.recipient_did, exchange.recipient_document.clone());
        let client = fixture.client();
        std::fs::write(
            fixture.identity_dir().join("e2ee-agreement-private.pem"),
            exchange.recipient_agreement_private.to_pem(),
        )
        .unwrap();
        {
            let connection =
                crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                super::super::sqlite_store::SqliteDirectSecureStateStore::new(&connection).unwrap();
            store
                .upsert_signed_prekey(&super::super::sqlite_store::DirectSignedPrekeyRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_signed_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_signed_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_signed_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status: super::super::sqlite_store::DirectPrekeyStatus::Active,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_signed_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    updated_at: "2026-05-24T00:00:00Z".to_owned(),
                })
                .unwrap();
            store
                .upsert_one_time_prekey(&super::super::sqlite_store::DirectOneTimePrekeyRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_one_time_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_one_time_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_one_time_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status: super::super::sqlite_store::DirectPrekeyStatus::Available,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_one_time_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    consumed_at: String::new(),
                })
                .unwrap();
        }
        let mut sender_session = exchange.sender_session.clone();
        sender_session.status = SESSION_STATUS_ESTABLISHED.to_owned();
        sender_session.recv_chain_key_b64u = sender_session.send_chain_key_b64u.clone();
        sender_session.peer_ratchet_public_key_b64u =
            Some(sender_session.ratchet_public_key_b64u.clone());
        sender_session.send_n = 1;
        let follow_up_metadata = DirectEnvelopeMetadata {
            sender_did: exchange.sender_did.clone(),
            recipient_did: exchange.recipient_did.clone(),
            message_id: "msg-pending-processor".to_owned(),
            profile: "anp.direct.e2ee.v1".to_owned(),
            security_profile: "direct-e2ee".to_owned(),
        };
        let (_, follow_up_body) = DirectE2eeSession::encrypt_follow_up(
            &mut sender_session,
            &follow_up_metadata,
            "msg-pending-processor",
            &ApplicationPlaintext::new_text("text/plain", "pending processor replay"),
        )
        .unwrap();
        let processor = AsyncDirectSecureIncomingProcessor::new(&client);

        let pending = processor
            .process_cipher_if_ready(direct_cipher_notification(
                &follow_up_metadata,
                &follow_up_body,
            ))
            .await
            .unwrap();
        assert!(matches!(
            pending,
            AsyncDirectSecureReceiveOutcome::Fallback(
                AsyncDirectSecureReceiveFallback::NoEstablishedSession
            )
        ));
        let outcome = processor
            .process_init_if_ready(
                direct_init_notification(&exchange.metadata, &exchange.init_body),
                exchange.sender_document,
            )
            .await
            .unwrap();

        let AsyncDirectSecureReceiveOutcome::ProcessedWithReplay { result, replayed } = outcome
        else {
            panic!("direct init should replay pending direct ciphers");
        };
        assert_eq!(result["plaintext"]["text"], json!("hello from init"));
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].message_id, "msg-pending-processor");
        assert_eq!(
            replayed[0].result["plaintext"]["text"],
            json!("pending processor replay")
        );
        let saved = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .get_direct_secure_session("alice-id", exchange.sender_did)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        let saved_session = direct_session_from_blob(&saved.state_blob).unwrap();
        assert_eq!(saved_session.recv_n, 2);
    }
}
