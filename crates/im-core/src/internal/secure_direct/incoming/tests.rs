use serde_json::json;

use super::*;

#[test]
fn incoming_processor_decrypts_in_server_order_and_filters_secure_controls() {
    let mut messages = vec![
        json!({
            "id": "msg-2",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": DIRECT_CIPHER_CONTENT_TYPE,
            "server_seq": 2,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                "ciphertext_b64u": "cipher-2"
            }
        }),
        json!({
            "id": "msg-1",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": DIRECT_CIPHER_CONTENT_TYPE,
            "server_seq": 1,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "0"},
                "ciphertext_b64u": "cipher-1"
            }
        }),
        json!({
            "id": "ack-session-1",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": DIRECT_CIPHER_CONTENT_TYPE,
            "server_seq": 3,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "2"},
                "ciphertext_b64u": "cipher-ack"
            }
        }),
    ];
    let mut seen = Vec::new();

    let warnings =
        maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, |notification| {
            let notification = Value::Object(notification);
            let message_id = notification
                .pointer("/meta/message_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            seen.push(message_id.clone());
            let plaintext = if message_id.starts_with("ack-") {
                json!({
                    "application_content_type": "application/json",
                    "payload": {
                        "system_type": super::super::control::SECURE_ACK_SYSTEM_TYPE,
                        "session_id": "session-1",
                        "acked_message_id": "msg-1"
                    }
                })
            } else {
                json!({
                    "application_content_type": "text/plain",
                    "text": format!("plain-{message_id}")
                })
            };
            Ok(Map::from_iter([
                ("state".to_owned(), json!("decrypted")),
                ("plaintext".to_owned(), plaintext),
            ]))
        });

    assert!(warnings.is_empty());
    assert_eq!(seen, vec!["msg-1", "msg-2", "ack-session-1"]);
    let displayable = filter_displayable_direct_e2ee_messages(messages);
    assert_eq!(displayable.len(), 2);
    assert_eq!(displayable[0]["content"], json!("plain-msg-2"));
    assert_eq!(displayable[1]["content"], json!("plain-msg-1"));
    assert_eq!(displayable[0]["content_type"], json!("text/plain"));
}

#[test]
fn incoming_processor_redacts_failed_ciphertext_without_leaking_body() {
    let mut messages = vec![json!({
        "id": "msg-failed",
        "sender_did": "did:example:bob",
        "receiver_did": "did:example:alice",
        "content_type": DIRECT_CIPHER_CONTENT_TYPE,
        "server_seq": 1,
        "content": {
            "session_id": "session-1",
            "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
            "ciphertext_b64u": "SECRET-CIPHERTEXT"
        }
    })];

    let warnings =
        maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, |_notification| {
            Err(crate::ImError::Serialization {
                detail: "decrypt failed".to_owned(),
            })
        });

    assert_eq!(warnings.len(), 1);
    assert_eq!(messages[0]["secure"], json!(true));
    assert_eq!(messages[0]["decryption_state"], json!("failed"));
    assert_eq!(messages[0]["content"], Value::Null);
    assert!(!messages[0].to_string().contains("SECRET-CIPHERTEXT"));
}

#[test]
fn direct_notification_from_message_view_accepts_string_content() {
    let notification = direct_e2ee_notification_from_message_view(&json!({
        "id": "msg-1",
        "sender_did": "did:example:bob",
        "receiver_did": "did:example:alice",
        "content_type": DIRECT_CIPHER_CONTENT_TYPE,
        "content": r#"{"session_id":"session-1","ratchet_header":{"dh_pub_b64u":"dh","pn":"0","n":"1"},"ciphertext_b64u":"cipher"}"#,
    }))
    .unwrap();
    let notification = Value::Object(notification);

    assert_eq!(
        notification.pointer("/meta/profile"),
        Some(&json!(DIRECT_E2EE_PROFILE))
    );
    assert_eq!(
        notification.pointer("/meta/security_profile"),
        Some(&json!(DIRECT_E2EE_SECURITY_PROFILE))
    );
    assert_eq!(
        notification.pointer("/body/ciphertext_b64u"),
        Some(&json!("cipher"))
    );
}

#[test]
fn realtime_notification_normalizer_returns_plaintext_without_wire_body() {
    let projection = maybe_normalize_direct_e2ee_notification_with_processor(
        secure_direct_notification("msg-secure", "SECRET-CIPHER"),
        DirectDecryptMode::ReadOnly,
        |_params| {
            Ok(Map::from_iter([
                ("state".to_owned(), json!("decrypted")),
                (
                    "plaintext".to_owned(),
                    json!({
                        "application_content_type": "text/plain",
                        "text": "decrypted realtime text"
                    }),
                ),
            ]))
        },
    );

    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::Normalized
    );
    assert!(projection.warnings.is_empty());
    let notification = projection.notification.unwrap();
    assert_eq!(
        notification.pointer("/params/meta/content_type"),
        Some(&json!("text/plain"))
    );
    assert_eq!(
        notification.pointer("/params/body/text"),
        Some(&json!("decrypted realtime text"))
    );
    assert_eq!(
        notification.pointer("/params/secure_state"),
        Some(&json!("decrypted"))
    );
    assert_eq!(
        notification.pointer("/params/secure_wire_content_type"),
        Some(&json!(DIRECT_CIPHER_CONTENT_TYPE))
    );
    let encoded = serde_json::to_string(&notification).unwrap();
    assert!(!encoded.contains("SECRET-CIPHER"));
    assert!(!encoded.contains("ratchet_header"));
    assert!(!encoded.contains("session_id"));
}

#[test]
fn realtime_notification_normalizer_redacts_attachment_manifest_secrets() {
    let projection = maybe_normalize_direct_e2ee_notification_with_processor(
        secure_direct_notification("secure-attachment-rt", "SECRET-CIPHER"),
        DirectDecryptMode::ReadOnly,
        |_params| {
            Ok(Map::from_iter([
                ("state".to_owned(), json!("decrypted")),
                (
                    "plaintext".to_owned(),
                    json!({
                        "application_content_type": ATTACHMENT_MANIFEST_CONTENT_TYPE,
                        "payload": {
                            "attachments": [{
                                "attachment_id": "att-secure-rt",
                                "filename": "secret.txt",
                                "mime_type": "text/plain",
                                "size": "27",
                                "digest": {
                                    "alg": "sha-256",
                                    "value_b64u": "ciphertext-digest"
                                },
                                "access_info": {
                                    "object_uri": "https://objects.example/secure"
                                },
                                "encryption_info": {
                                    "mode": "object-e2ee",
                                    "object_cipher": "chacha20-poly1305",
                                    "object_key_b64u": "OBJECT-KEY-SECRET",
                                    "nonce_b64u": "NONCE-SECRET",
                                    "plaintext_size": "11"
                                }
                            }],
                            "caption": "secure attachment",
                            "primary_attachment_id": "att-secure-rt"
                        }
                    }),
                ),
            ]))
        },
    );

    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::Normalized
    );
    let notification = projection.notification.unwrap();
    let encoded = serde_json::to_string(&notification).unwrap();
    assert!(!encoded.contains("object_key_b64u"));
    assert!(!encoded.contains("nonce_b64u"));
    assert!(!encoded.contains("OBJECT-KEY-SECRET"));
    assert!(!encoded.contains("NONCE-SECRET"));
    assert_eq!(
        notification.pointer("/params/body/payload/attachments/0/encryption_info/mode"),
        Some(&json!("object-e2ee"))
    );
    assert_eq!(
        notification.pointer("/params/body/payload/attachments/0/encryption_info/plaintext_size"),
        Some(&json!("11"))
    );
}

#[test]
fn realtime_notification_normalizer_drops_secure_control_plaintexts() {
    let projection = maybe_normalize_direct_e2ee_notification_with_processor(
        secure_direct_notification("ack-session-1", "ACK-CIPHER"),
        DirectDecryptMode::ReadOnly,
        |_params| {
            Ok(Map::from_iter([
                ("state".to_owned(), json!("decrypted")),
                (
                    "plaintext".to_owned(),
                    json!({
                        "application_content_type": "application/json",
                        "payload": {
                            "system_type": super::super::control::SECURE_ACK_SYSTEM_TYPE,
                            "session_id": "session-1",
                            "acked_message_id": "msg-secure"
                        }
                    }),
                ),
            ]))
        },
    );

    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::DroppedControl
    );
    assert!(projection.notification.is_none());
    assert!(projection.warnings.is_empty());
}

#[test]
fn secure_init_ack_request_uses_realtime_wire_session_without_public_projection() {
    let plaintext = Map::from_iter([
        (
            "application_content_type".to_owned(),
            json!("application/json"),
        ),
        (
            "payload".to_owned(),
            json!({
                "system_type": super::super::control::SECURE_INIT_SYSTEM_TYPE,
                "reason": "manual_init"
            }),
        ),
    ]);
    let request = ack_request_from_secure_init(
        &secure_direct_init_notification("secure-init-message", "session-wire-secret"),
        " did:example:bob ",
        &plaintext,
    )
    .unwrap();

    assert_eq!(request.peer_did, "did:example:bob");
    assert_eq!(request.message_id, "secure-init-message");
    assert_eq!(request.ack_id, "ack-session-wire-secret");
    assert_eq!(
        Value::Object(request.payload),
        json!({
            "system_type": super::super::control::SECURE_ACK_SYSTEM_TYPE,
            "session_id": "session-wire-secret",
            "acked_message_id": "secure-init-message"
        })
    );

    let projection = normalize_direct_realtime_plaintext_notification(
        secure_direct_init_notification("secure-init-message", "session-wire-secret"),
        DirectDecryptMode::WithSideEffects,
        &mut plaintext.clone(),
        Vec::new(),
    );
    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::DroppedControl
    );
    assert!(projection.notification.is_none());
    assert!(projection.warnings.is_empty());
}

#[test]
fn secure_init_ack_request_fails_closed_when_session_is_missing() {
    let plaintext = Map::from_iter([
        (
            "application_content_type".to_owned(),
            json!("application/json"),
        ),
        (
            "payload".to_owned(),
            json!({
                "system_type": super::super::control::SECURE_INIT_SYSTEM_TYPE,
                "reason": "manual_init"
            }),
        ),
    ]);
    let warning = ack_request_from_secure_init(
        &secure_direct_init_notification("secure-init-message", ""),
        "did:example:bob",
        &plaintext,
    )
    .unwrap_err();

    assert!(warning.contains("session state reference"));
}

#[tokio::test]
async fn realtime_notification_client_async_normalizer_uses_actor_cas_for_established_cipher() {
    let fixture = super::super::async_receive::test_support::Fixture::new();
    let client = fixture.client();
    let exchange = super::super::async_receive::test_support::established_exchange();
    let db = client.core_inner().local_state_db().await.unwrap();
    db.save_direct_secure_session_if_revision(
        super::super::sqlite_store::DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: exchange.alice_session.session_id.clone(),
            state_blob: super::super::sqlite_store::direct_session_to_blob(&exchange.alice_session)
                .unwrap(),
            metadata_json: super::super::sqlite_store::direct_session_metadata_json(
                &exchange.alice_session,
            )
            .unwrap(),
            revision: 0,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        },
        0,
    )
    .await
    .unwrap();
    let notification =
        super::super::async_receive::test_support::direct_cipher_realtime_notification(
            &exchange.message_metadata,
            &exchange.cipher_body,
        );

    let outcome = try_normalize_direct_e2ee_notification_for_client_async(
        &client,
        notification,
        DirectDecryptMode::WithSideEffects,
        |_side_effect| async { vec!["unexpected side effect".to_owned()] },
    )
    .await;

    let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
        panic!("established cipher should be normalized by async realtime path");
    };
    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::Normalized
    );
    assert!(projection.warnings.is_empty());
    let notification = projection.notification.unwrap();
    assert_eq!(
        notification.pointer("/params/body/text"),
        Some(&json!("async receive secret"))
    );
    assert_eq!(
        notification.pointer("/params/secure_state"),
        Some(&json!("decrypted"))
    );
    let encoded = serde_json::to_string(&notification).unwrap();
    assert!(!encoded.contains("ciphertext_b64u"));
    assert!(!encoded.contains("ratchet_header"));

    let saved = db
        .get_direct_secure_session("alice-id", "did:example:bob")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 1);
    let saved_session =
        super::super::sqlite_store::direct_session_from_blob(&saved.state_blob).unwrap();
    assert_eq!(saved_session.recv_n, exchange.alice_session.recv_n + 1);
}

#[tokio::test]
async fn realtime_notification_client_async_normalizer_falls_back_without_established_session() {
    let fixture = super::super::async_receive::test_support::Fixture::new();
    let client = fixture.client();
    let exchange = super::super::async_receive::test_support::established_exchange();
    let notification =
        super::super::async_receive::test_support::direct_cipher_realtime_notification(
            &exchange.message_metadata,
            &exchange.cipher_body,
        );

    let outcome = try_normalize_direct_e2ee_notification_for_client_async(
        &client,
        notification.clone(),
        DirectDecryptMode::ReadOnly,
        |_side_effect| async { Vec::new() },
    )
    .await;

    assert_eq!(
        outcome,
        DirectRealtimeAsyncProjectionOutcome::Fallback(notification)
    );
}

#[tokio::test]
async fn historical_realtime_reader_drops_init_control_without_sending_v1_ack() {
    let fixture = super::super::async_receive::test_support::Fixture::new();
    let client = fixture.client();
    let exchange = super::super::async_receive::test_support::established_exchange();
    let db = client.core_inner().local_state_db().await.unwrap();
    db.save_direct_secure_session_if_revision(
        super::super::sqlite_store::DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: exchange.alice_session.session_id.clone(),
            state_blob: super::super::sqlite_store::direct_session_to_blob(&exchange.alice_session)
                .unwrap(),
            metadata_json: super::super::sqlite_store::direct_session_metadata_json(
                &exchange.alice_session,
            )
            .unwrap(),
            revision: 0,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        },
        0,
    )
    .await
    .unwrap();

    let mut bob_session = exchange.bob_session.clone();
    let metadata = anp::direct_e2ee::DirectEnvelopeMetadata {
        sender_did: "did:example:bob".to_owned(),
        recipient_did: "did:example:alice".to_owned(),
        message_id: "secure-init-control-msg".to_owned(),
        profile: DIRECT_E2EE_PROFILE.to_owned(),
        security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
    };
    let mut init_payload = super::super::control::build_secure_init_payload();
    init_payload.insert(
        "session_id".to_owned(),
        Value::String(exchange.alice_session.session_id.clone()),
    );
    let (_pending, init_control_body) = anp::direct_e2ee::DirectE2eeSession::encrypt_follow_up(
        &mut bob_session,
        &metadata,
        "secure-init-control-msg",
        &anp::direct_e2ee::ApplicationPlaintext::new_json(
            "application/json",
            Value::Object(init_payload),
        ),
    )
    .unwrap();
    let notification =
        super::super::async_receive::test_support::direct_cipher_realtime_notification(
            &metadata,
            &init_control_body,
        );
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let mut message_transport = RecordingAsyncTransport {
        calls: std::sync::Arc::clone(&calls),
    };
    let mut directory_transport = NoopAsyncTransport;

    let outcome = try_normalize_direct_e2ee_notification_for_client_with_transports_async(
        &client,
        notification,
        DirectDecryptMode::WithSideEffects,
        &mut message_transport,
        &mut directory_transport,
    )
    .await;

    let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
        panic!("secure init control should be projected by async realtime path");
    };
    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::DroppedControl
    );
    assert!(projection.notification.is_none());
    assert!(projection.warnings.is_empty());
    let calls = calls.lock().unwrap().clone();
    assert!(calls.is_empty());
    let saved = db
        .get_direct_secure_session("alice-id", "did:example:bob")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 1);
}

#[tokio::test]
async fn realtime_notification_client_async_normalizer_resolves_init_sender_document_over_async_directory(
) {
    let exchange = super::super::async_receive::test_support::incoming_init_exchange();
    let fixture = super::super::async_receive::test_support::Fixture::with_identity(
        &exchange.recipient_did,
        exchange.recipient_document.clone(),
    );
    let client = fixture.client();
    std::fs::write(
        fixture.identity_dir().join("e2ee-agreement-private.pem"),
        exchange.recipient_agreement_private.to_pem(),
    )
    .unwrap();
    seed_direct_init_prekeys(&fixture, &exchange);
    let notification = Value::Object(Map::from_iter([
        (
            "method".to_owned(),
            Value::String("direct.incoming".to_owned()),
        ),
        (
            "params".to_owned(),
            Value::Object(
                super::super::async_receive::test_support::direct_init_notification(
                    &exchange.metadata,
                    &exchange.init_body,
                ),
            ),
        ),
    ]));
    let mut message_transport = RecordingAsyncTransport {
        calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let directory_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let mut directory_transport = ResolvingAsyncDirectoryTransport {
        did: exchange.sender_did.clone(),
        document: exchange.sender_document.clone(),
        calls: std::sync::Arc::clone(&directory_calls),
    };

    let outcome = try_normalize_direct_e2ee_notification_for_client_with_transports_async(
        &client,
        notification,
        DirectDecryptMode::ReadOnly,
        &mut message_transport,
        &mut directory_transport,
    )
    .await;

    let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
        panic!("direct init should be normalized by async realtime path after DID resolve");
    };
    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::Normalized
    );
    assert!(projection.warnings.is_empty());
    let notification = projection.notification.unwrap();
    assert_eq!(
        notification.pointer("/params/body/text"),
        Some(&json!("hello from init"))
    );
    assert_eq!(
        notification.pointer("/params/secure_state"),
        Some(&json!("decrypted"))
    );
    assert_eq!(directory_calls.lock().unwrap().len(), 1);
    let saved = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .get_direct_secure_session("alice-id", exchange.sender_did)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 0);
}

#[tokio::test]
async fn realtime_notification_client_async_normalizer_redacts_bad_cipher_without_saving() {
    let fixture = super::super::async_receive::test_support::Fixture::new();
    let client = fixture.client();
    let mut exchange = super::super::async_receive::test_support::established_exchange();
    let db = client.core_inner().local_state_db().await.unwrap();
    db.save_direct_secure_session_if_revision(
        super::super::sqlite_store::DirectSessionRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            session_id: exchange.alice_session.session_id.clone(),
            state_blob: super::super::sqlite_store::direct_session_to_blob(&exchange.alice_session)
                .unwrap(),
            metadata_json: super::super::sqlite_store::direct_session_metadata_json(
                &exchange.alice_session,
            )
            .unwrap(),
            revision: 0,
            created_at: "2026-05-24T00:00:00Z".to_owned(),
            updated_at: "2026-05-24T00:00:00Z".to_owned(),
        },
        0,
    )
    .await
    .unwrap();
    exchange.cipher_body.ciphertext_b64u =
        super::super::async_receive::test_support::corrupt_b64u_tail(
            &exchange.cipher_body.ciphertext_b64u,
        );
    let notification =
        super::super::async_receive::test_support::direct_cipher_realtime_notification(
            &exchange.message_metadata,
            &exchange.cipher_body,
        );

    let outcome = try_normalize_direct_e2ee_notification_for_client_async(
        &client,
        notification,
        DirectDecryptMode::ReadOnly,
        |_side_effect| async { Vec::new() },
    )
    .await;

    let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
        panic!("bad established cipher should produce an async projection");
    };
    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::Redacted
    );
    assert!(projection.warnings.is_empty());
    let notification = projection.notification.unwrap();
    assert_eq!(
        notification.pointer("/params/secure_state"),
        Some(&json!("undecryptable"))
    );
    let encoded = serde_json::to_string(&notification).unwrap();
    assert!(!encoded.contains("ciphertext_b64u"));
    assert!(!encoded.contains("ratchet_header"));
    let saved = db
        .get_direct_secure_session("alice-id", "did:example:bob")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 0);
}

#[test]
fn realtime_notification_normalizer_redacts_failed_ciphertext() {
    let projection = maybe_normalize_direct_e2ee_notification_with_processor(
        secure_direct_notification("msg-failed", "FAILED-CIPHER"),
        DirectDecryptMode::ReadOnly,
        |_params| {
            Err(crate::ImError::Serialization {
                detail: "decrypt failed".to_owned(),
            })
        },
    );

    assert_eq!(
        projection.decision,
        DirectRealtimeNotificationDecision::Redacted
    );
    assert_eq!(projection.warnings.len(), 1);
    let notification = projection.notification.unwrap();
    assert_eq!(
        notification.pointer("/params/meta/content_type"),
        Some(&json!(REDACTED_SECURE_DIRECT_CONTENT_TYPE))
    );
    assert_eq!(notification.pointer("/params/body"), Some(&json!({})));
    assert_eq!(
        notification.pointer("/params/secure_state"),
        Some(&json!("failed"))
    );
    let encoded = serde_json::to_string(&notification).unwrap();
    assert!(!encoded.contains("FAILED-CIPHER"));
    assert!(!encoded.contains("ratchet_header"));
}

struct RecordingAsyncTransport {
    calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingAsyncTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        assert_eq!(endpoint, "/im/rpc");
        assert_eq!(method, "direct.send");
        self.calls.lock().unwrap().push(params);
        Ok(json!({
            "accepted": true,
            "message_id": "ack-secure-init",
            "operation_id": "ack-secure-init",
            "delivery_state": "accepted",
            "accepted_at": "2026-05-24T00:00:01Z"
        }))
    }
}

struct NoopAsyncTransport;

impl crate::internal::transport::AsyncRpcTransport for NoopAsyncTransport {
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

struct ResolvingAsyncDirectoryTransport {
    did: String,
    document: Value,
    calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

impl crate::internal::transport::AsyncRpcTransport for ResolvingAsyncDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        assert_eq!(
            endpoint,
            crate::internal::identity_wire::DID_PROFILE_RPC_ENDPOINT
        );
        assert_eq!(method, "resolve");
        assert_eq!(params.get("did"), Some(&json!(self.did)));
        self.calls.lock().unwrap().push(params);
        Ok(json!({
            "profile": {
                "did_document": self.document.clone()
            }
        }))
    }
}

fn seed_direct_init_prekeys(
    fixture: &super::super::async_receive::test_support::Fixture,
    exchange: &super::super::async_receive::test_support::IncomingInitExchange,
) {
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let store = super::super::sqlite_store::SqliteDirectSecureStateStore::new(&connection).unwrap();
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

fn secure_direct_notification(message_id: &str, ciphertext: &str) -> Value {
    json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": message_id,
                "sender_did": "did:example:bob",
                "target": {"kind": "agent", "did": "did:example:alice"},
                "content_type": DIRECT_CIPHER_CONTENT_TYPE,
                "profile": DIRECT_E2EE_PROFILE,
                "security_profile": DIRECT_E2EE_SECURITY_PROFILE,
            },
            "body": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                "ciphertext_b64u": ciphertext
            }
        }
    })
}

fn secure_direct_init_notification(message_id: &str, session_id: &str) -> Value {
    json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": message_id,
                "sender_did": "did:example:bob",
                "target": {"kind": "agent", "did": "did:example:alice"},
                "content_type": DIRECT_INIT_CONTENT_TYPE,
                "profile": DIRECT_E2EE_PROFILE,
                "security_profile": DIRECT_E2EE_SECURITY_PROFILE,
            },
            "body": {
                "session_id": session_id,
                "sender_ephemeral_key_b64u": "ephemeral",
                "sender_static_key_agreement_id": "did:example:bob#key-3",
                "recipient_signed_prekey_id": "spk-initial",
                "ciphertext_b64u": "SECRET-INIT-CIPHER"
            }
        }
    })
}
