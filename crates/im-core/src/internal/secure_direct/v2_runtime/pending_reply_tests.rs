use super::tests::{established_pair, scope, vault};
use super::*;
use anp::direct_e2ee::{V2PrekeyBundle, V2SignedPrekey, V2_SESSION_STATUS_PENDING_CONFIRMATION};
use x25519_dalek::{PublicKey, StaticSecret};

fn pending_pair() -> (V2DirectSessionState, V2DirectSessionState) {
    let (alice, bob) = established_pair();
    let alice_static = StaticSecret::from([71; 32]);
    let bob_static = StaticSecret::from([73; 32]);
    let bob_spk = StaticSecret::from([75; 32]);
    let bundle = V2PrekeyBundle {
        bundle_id: "bundle-bob".into(),
        owner_did: bob.binding.local_did.clone(),
        owner_device_id: bob.binding.local_device_id.clone(),
        suite: bob.binding.suite.clone(),
        static_key_agreement_id: bob.binding.local_e2ee_key_id.clone(),
        signed_prekey: V2SignedPrekey {
            key_id: "spk-bob".into(),
            public_key_b64u: URL_SAFE_NO_PAD.encode(PublicKey::from(&bob_spk).as_bytes()),
            expires_at: "2030-01-01T00:00:00Z".into(),
        },
        proof: serde_json::json!({
            "type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022",
            "verificationMethod": "did:example:alice#sign-laptop",
            "proofPurpose": "assertionMethod", "created": "2026-09-11T00:00:00Z",
            "proofValue": "zTestProof"
        }),
    };
    let metadata = outbound_metadata_for_content_type(
        &alice.binding,
        "initial-message",
        CONTENT_TYPE_DIRECT_INIT_V2,
    );
    let (alice, _, body) = V2DirectE2eeSession::initiate_session(
        &alice.binding,
        &metadata,
        &alice_static,
        &bundle,
        &PublicKey::from(&bob_static).to_bytes(),
        None,
        &session_established_plaintext("initial-message").unwrap(),
    )
    .unwrap();
    let (bob, _, _) = V2DirectE2eeSession::accept_incoming_init(
        &bob.binding,
        &metadata,
        &bob_static,
        &bundle,
        &bob_spk,
        None,
        &PublicKey::from(&alice_static).to_bytes(),
        &body,
    )
    .unwrap();
    assert_eq!(alice.status, V2_SESSION_STATUS_PENDING_CONFIRMATION);
    (alice, bob)
}

#[test]
fn later_cipher_waits_for_authenticated_first_reply_without_mutating_state() {
    for path in ["application", "secret", "validated_secret"] {
        let root = tempfile::tempdir().unwrap();
        let db =
            crate::internal::local_state::open_writable(&root.path().join("state.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE projected(message_id TEXT PRIMARY KEY);")
            .unwrap();
        let (alice, mut bob) = pending_pair();
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &db,
            vault(&root.path().join("vault"), 94),
            scope(
                "alice",
                &alice.binding.local_did,
                &alice.binding.local_device_id,
                &alice.binding.local_e2ee_key_id,
            ),
        )
        .unwrap();
        store
            .commit_inbound(
                &alice,
                "setup",
                "sha256:setup",
                None,
                V2SessionExpectation::Absent,
                "2026-09-11T00:00:00Z",
            )
            .unwrap();
        let runtime = V2EstablishedDirectRuntime::new(&store);
        let confirmation_metadata = outbound_metadata(&bob.binding, "confirmation");
        let bob_binding = bob.binding.clone();
        let (_, confirmation) = V2DirectE2eeSession::encrypt_follow_up(
            &mut bob,
            &bob_binding,
            &confirmation_metadata,
            &session_established_plaintext("initial-message").unwrap(),
        )
        .unwrap();
        let metadata = outbound_metadata(&bob.binding, "business-reply");
        let plaintext = V2ApplicationPlaintext {
            application_content_type: "application/json".into(),
            logical_message_id: None,
            conversation_id: None,
            reply_to_message_id: None,
            annotations: None,
            text: None,
            payload: Some(serde_json::json!({"text": "business reply"})),
            payload_b64u: None,
        };
        let (_, business) =
            V2DirectE2eeSession::encrypt_follow_up(&mut bob, &bob_binding, &metadata, &plaintext)
                .unwrap();
        assert_eq!(confirmation.ratchet_header.n, "0");
        assert_eq!(business.ratchet_header.n, "1");
        let commit = |tx: &rusqlite::Transaction<'_>, _: &()| {
            tx.execute(
                "INSERT INTO projected(message_id) VALUES('business-reply')",
                [],
            )
            .map(|_| ())
            .map_err(crate::internal::local_state::local_state_unavailable)
        };
        let receive = |metadata: &V2DirectMetadata, body: &V2DirectCipherBody| {
            let now = "2026-09-11T00:00:01Z";
            match path {
                "application" => runtime
                    .decrypt_inbound_validated_with_commit(
                        &alice.binding,
                        metadata,
                        body,
                        now,
                        |decoded, _, _| {
                            assert_eq!(decoded, &plaintext);
                            Ok(())
                        },
                        commit,
                    )
                    .map(|result| matches!(result, V2ValidatedInboundOutcome::Replay { .. })),
                "secret" => runtime
                    .decrypt_inbound_secret_json(&alice.binding, metadata, body, now)
                    .map(|result| match result {
                        V2SecretInboundDecryptOutcome::Decrypted { plaintext, .. } => {
                            assert_eq!(plaintext.expose_secret(), br#"{"text":"business reply"}"#);
                            false
                        }
                        V2SecretInboundDecryptOutcome::Replay { .. } => true,
                    }),
                _ => runtime
                    .decrypt_inbound_secret_json_validated_with_commit(
                        &alice.binding,
                        metadata,
                        body,
                        now,
                        |decoded, _, _| {
                            assert_eq!(decoded.expose_secret(), br#"{"text":"business reply"}"#);
                            Ok(())
                        },
                        commit,
                    )
                    .map(|result| matches!(result, V2ValidatedSecretInboundOutcome::Replay { .. })),
            }
        };
        let waiting = receive(&metadata, &business).unwrap_err();
        assert!(
            crate::internal::message_runtime::sync_processing::retry_at(&waiting, 1).is_some(),
            "{path}: a later valid cipher must wait for confirmation: {waiting:?}"
        );

        // Wrong-device input and a corrupted actual first reply remain terminal.
        let mut wrong_sender = metadata.clone();
        wrong_sender.sender_device_id = "unbound-device".into();
        let rejected = receive(&wrong_sender, &business).unwrap_err();
        assert!(matches!(rejected, crate::ImError::PermissionDenied));
        assert!(
            crate::internal::message_runtime::sync_processing::retry_at(&rejected, 1).is_none()
        );
        let mut tampered = confirmation.clone();
        let mut ciphertext = URL_SAFE_NO_PAD.decode(&tampered.ciphertext_b64u).unwrap();
        ciphertext[0] ^= 1;
        tampered.ciphertext_b64u = URL_SAFE_NO_PAD.encode(ciphertext);
        let rejected = receive(&confirmation_metadata, &tampered).unwrap_err();
        assert!(matches!(rejected, crate::ImError::PermissionDenied));
        assert!(
            crate::internal::message_runtime::sync_processing::retry_at(&rejected, 1).is_none()
        );
        let stored = store
            .load_session(&alice.binding, &alice.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.revision, 0);
        assert_eq!(stored.state, alice);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projected", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM direct_e2ee_v2_replay", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            1
        );

        runtime
            .decrypt_inbound(
                &alice.binding,
                &confirmation_metadata,
                &confirmation,
                "2026-09-11T00:00:02Z",
            )
            .unwrap();
        assert!(!receive(&metadata, &business).unwrap(), "{path}");
        assert!(
            receive(&metadata, &business).unwrap(),
            "{path}: replay is idempotent"
        );
        assert_eq!(
            store
                .load_session(&alice.binding, &alice.session_id)
                .unwrap()
                .unwrap()
                .revision,
            2
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projected", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            if path == "secret" { 0 } else { 1 }
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM direct_e2ee_v2_replay", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            3
        );
    }
}
