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
async fn async_direct_secure_attachment_sender_sends_cipher_and_non_secret_grant_ref() {
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
    let committed = committed_attachment();
    let object_key = committed.full_manifest["attachments"][0]["encryption_info"]
        ["object_key_b64u"]
        .as_str()
        .unwrap()
        .to_owned();
    let nonce = committed.full_manifest["attachments"][0]["encryption_info"]["nonce_b64u"]
        .as_str()
        .unwrap()
        .to_owned();
    let calls = Arc::new(Mutex::new(Vec::<RecordedAsyncCall>::new()));

    let result = AsyncDirectSecureTextSender::new(
        &client,
        ReadyAsyncSessionProvider,
        RecordingAsyncTransport {
            calls: Arc::clone(&calls),
            prekey_response: None,
        },
        NoopDirectoryTransport,
    )
    .send_attachment_follow_up_if_ready(super::super::send::DirectSecureAttachmentSend {
        request: secure_direct_attachment_request("did:example:bob"),
        resolved_target_did: None,
        committed,
        local_persistence: super::super::send::DirectSecureLocalPersistence::Deferred,
    })
    .await
    .unwrap()
    .expect("established session should send direct attachment follow-up");

    assert_eq!(result.sdk_result.message.id.as_str(), "msg-async-direct");
    assert_eq!(result.target_did, "did:example:bob");
    assert!(matches!(
        result.local_effect,
        super::super::send::DirectSecureAttachmentLocalEffect::PersistOutgoing
    ));
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "direct.send");
    assert_eq!(
        calls[0].params["meta"]["content_type"],
        "application/anp-direct-cipher+json"
    );
    assert!(
        !calls[0].params["body"].to_string().contains(&object_key),
        "direct E2EE outer body leaked object key"
    );
    assert!(
        !calls[0].params["body"].to_string().contains(&nonce),
        "direct E2EE outer body leaked nonce"
    );
    let grant_refs = calls[0].params["client"]["attachment_grant_refs"]
        .as_array()
        .unwrap();
    assert_eq!(grant_refs.len(), 1);
    assert_eq!(grant_refs[0]["attachment_id"], "att-async-secure");
    assert_eq!(grant_refs[0]["object_encryption_mode"], "object-e2ee");
    assert_eq!(grant_refs[0]["plaintext_size"], "12");
    let client_context_text = serde_json::to_string(&calls[0].params["client"]).unwrap();
    assert!(!client_context_text.contains("object_key_b64u"));
    assert!(!client_context_text.contains("nonce_b64u"));
    assert!(!client_context_text.contains(&object_key));
    assert!(!client_context_text.contains(&nonce));
    let result_text = serde_json::to_string(&result.sdk_result).unwrap();
    assert!(!result_text.contains("object_key_b64u"));
    assert!(!result_text.contains("nonce_b64u"));
    assert!(!result_text.contains(&object_key));
    assert!(!result_text.contains(&nonce));
}

#[tokio::test]
async fn async_direct_secure_attachment_sender_initializes_session_without_leaking_secrets() {
    let alice = TestIdentity::new("alice.async-attachment-init.example", "alice");
    let bob = TestIdentity::new("bob.async-attachment-init.example", "bob");
    let fixture = Fixture::new(&alice);
    let client = fixture.client();
    let bob_bundle = test_prekey_bundle(&bob);
    let committed = committed_attachment();
    let object_key = committed.full_manifest["attachments"][0]["encryption_info"]
        ["object_key_b64u"]
        .as_str()
        .unwrap()
        .to_owned();
    let nonce = committed.full_manifest["attachments"][0]["encryption_info"]["nonce_b64u"]
        .as_str()
        .unwrap()
        .to_owned();
    let calls = Arc::new(Mutex::new(Vec::<RecordedAsyncCall>::new()));

    let result = AsyncDirectSecureTextSender::new(
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
    .send_attachment_async_if_ready(super::super::send::DirectSecureAttachmentSend {
        request: secure_direct_attachment_request(&bob.did),
        resolved_target_did: None,
        committed,
        local_persistence: super::super::send::DirectSecureLocalPersistence::Deferred,
    })
    .await
    .unwrap()
    .expect("missing session should send direct attachment init when prekey material exists");

    assert_eq!(result.sdk_result.message.id.as_str(), "msg-async-direct");
    assert_eq!(result.target_did, bob.did);
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "direct.e2ee.get_prekey_bundle");
    assert_eq!(calls[1].method, "direct.send");
    assert_eq!(
        calls[1].params["meta"]["content_type"],
        "application/anp-direct-init+json"
    );
    assert!(
        !calls[1].params["body"].to_string().contains(&object_key),
        "direct E2EE init outer body leaked object key"
    );
    assert!(
        !calls[1].params["body"].to_string().contains(&nonce),
        "direct E2EE init outer body leaked nonce"
    );
    let client_context_text = serde_json::to_string(&calls[1].params["client"]).unwrap();
    assert!(!client_context_text.contains("object_key_b64u"));
    assert!(!client_context_text.contains("nonce_b64u"));
    assert!(!client_context_text.contains(&object_key));
    assert!(!client_context_text.contains(&nonce));
    let saved = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .get_direct_secure_session("alice-id", &bob.did)
        .await
        .unwrap()
        .unwrap();
    let saved_session = direct_session_from_blob(&saved.state_blob).unwrap();
    assert_eq!(saved_session.status, SESSION_STATUS_PENDING_CONFIRMATION);
    assert_eq!(saved_session.send_n, 1);
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
            bearer_token: None,
        })
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        Ok(crate::auth::SessionUpdate {
            subject: crate::ids::Did::parse("did:example:alice").unwrap(),
            previous_expires_at: None,
            new_expires_at: None,
            refreshed: true,
            bearer_token: None,
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
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
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
    let signing_private = anp::PrivateKeyMaterial::from_pem(&identity.signing_private_pem).unwrap();
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
        delegated_signing: None,
    }
}

fn secure_direct_attachment_request(target: &str) -> crate::messages::SendMessageRequest {
    crate::messages::SendMessageRequest {
        target: crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse(target, "").unwrap(),
        ),
        body: crate::messages::MessageBody::Attachment {
            input: crate::attachments::AttachmentInput::Bytes {
                filename: Some("async-secret.pdf".to_owned()),
                mime_type: Some("application/pdf".to_owned()),
                bytes: b"not uploaded in this unit test".to_vec(),
            },
            caption: Some("async direct caption".to_owned()),
            mime_type: Some("application/pdf".to_owned()),
            filename: None,
        },
        security: crate::messages::MessageSecurityMode::SecureDirect,
        client_message_id: Some(crate::ids::MessageId::parse("msg-async-direct").unwrap()),
        delivery: crate::messages::MessageDeliveryOptions {
            idempotency_key: Some("msg-async-direct".to_owned()),
            wait_for_final_acceptance: true,
        },
        delegated_signing: None,
    }
}

fn committed_attachment() -> crate::internal::attachment_runtime::upload::PreparedCommittedAttachment
{
    let prepared = crate::attachments::manifest::PreparedAttachment {
        filename: "async-secret.pdf".to_owned(),
        mime_type: "application/pdf".to_owned(),
        size_bytes: 28,
        size_string: "28".to_owned(),
        digest_b64u: "digest-async-ciphertext".to_owned(),
        payload: b"ciphertext with auth tag bytes".to_vec(),
        object_encryption_mode: "object-e2ee".to_owned(),
        object_cipher: Some("chacha20-poly1305".to_owned()),
        plaintext_size_bytes: Some(12),
        plaintext_size_string: Some("12".to_owned()),
    };
    let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
        &prepared,
        "att-async-secure",
        "https://objects.example/att-async-secure",
    );
    let secrets = crate::attachments::manifest::ObjectE2eeAttachmentSecrets {
        object_key_b64u: "0123456789abcdefghijklmnopqrstuv".to_owned(),
        nonce_b64u: "0123456789ab".to_owned(),
    };
    let full_manifest =
        crate::attachments::manifest::build_attachment_manifest_with_object_e2ee_secrets(
            &descriptor,
            "async direct caption",
            &secrets,
        );
    let redacted_manifest = crate::attachments::manifest::build_attachment_manifest(
        &descriptor,
        "async direct caption",
    );
    let grant_ref = crate::attachments::manifest::build_attachment_grant_ref(&descriptor)
        .expect("grant ref should build");
    crate::internal::attachment_runtime::upload::PreparedCommittedAttachment {
        target_kind: "agent",
        target_did: "did:example:bob".to_owned(),
        prepared,
        slot: crate::internal::wire::attachment::AttachmentCreateSlotResult {
            attachment_id: "att-async-secure".to_owned(),
            slot_id: "slot-async-secure".to_owned(),
            upload_uri: "https://objects.example/upload/att-async-secure".to_owned(),
            upload_headers: serde_json::Map::new(),
            object_uri: "https://objects.example/att-async-secure".to_owned(),
            commit_token: "commit-async-secure".to_owned(),
            expires_at: "2026-05-24T00:05:00Z".to_owned(),
            request_service_did: "did:example:alice-service".to_owned(),
        },
        descriptor,
        redacted_manifest,
        full_manifest,
        grant_ref,
    }
}
