use super::*;

use std::path::Path;

fn test_config() -> crate::ImCoreConfig {
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
    }
}

fn test_paths(root: &Path) -> crate::ImCorePaths {
    crate::ImCorePaths {
        identities: crate::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        },
        local_state: crate::LocalStatePaths {
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        runtime: crate::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn open_vault_core(root: &Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
                root.join("vault"),
                "join-test-workspace",
                "join-test-vault-device",
            ),
        ),
    )
    .unwrap()
}

fn sample_proof() -> DeviceProof {
    DeviceProof {
        proof_type: DEVICE_PROOF_TYPE.to_owned(),
        key_id: "did:wba:awiki.test:alice#device-a-sign".to_owned(),
        created_at: "2026-07-19T01:02:03Z".to_owned(),
        expires_at: "2026-07-19T01:07:03Z".to_owned(),
        nonce: "proof-nonce-fixed".to_owned(),
        signature: "signature-is-not-part-of-proof-bytes".to_owned(),
    }
}

#[test]
fn device_proof_canonical_bytes_and_hash_fixture_are_frozen() {
    let params = json!({
        "z": 3,
        "operation_id": "op-fixed",
        "nested": {"b": 2, "a": 1},
    });
    let canonical = device_proof_bytes(
        &sample_proof(),
        JOIN_CHALLENGE_PURPOSE,
        JOIN_CHALLENGE_METHOD,
        &params,
    )
    .unwrap();
    let expected = concat!(
        r#"{"created_at":"2026-07-19T01:02:03Z","expires_at":"2026-07-19T01:07:03Z","key_id":"did:wba:awiki.test:alice#device-a-sign","method":"device_join_challenge","nonce":"proof-nonce-fixed","params":{"nested":{"a":1,"b":2},"operation_id":"op-fixed","z":3},"purpose":"awiki.device.join.challenge.v1","type":"awiki-device-signature-v1"}"#,
    );

    assert_eq!(canonical, expected.as_bytes());
    assert_eq!(
        hash_bytes(&canonical),
        "sha256:MbTQijG_NDem8bMN06IFyaZ7Etu-AR87dZersdKmDwg",
        "cross-repository proof fixture changed"
    );
}

#[test]
fn join_transcript_hash_and_sas_fixture_are_frozen() {
    let transcript = json!({
        "type": "awiki.device.join.transcript.v1",
        "did": "did:wba:awiki.test:alice",
        "join_session_id": "join-fixed",
        "admin_device_id": "admin-a",
        "new_device_id": "new-b",
        "join_request_hash": "sha256:join-fixed",
        "challenge_id": "challenge-fixed",
        "challenge_hash": "sha256:challenge-fixed",
        "new_pairing_public_key": "new-pairing-fixed",
        "admin_pairing_public_key": "admin-pairing-fixed",
        "new_signing_public_key": {
            "type": "Multikey",
            "id": "did:wba:awiki.test:alice#new-b-sign",
            "controller": "did:wba:awiki.test:alice",
            "publicKeyMultibase": "zSigningFixed"
        },
        "new_e2ee_public_key": {
            "type": "X25519KeyAgreementKey2019",
            "id": "did:wba:awiki.test:alice#new-b-e2ee",
            "controller": "did:wba:awiki.test:alice",
            "publicKeyMultibase": "zE2eeFixed"
        },
        "document_version": 7,
        "document_hash": "sha256:document-fixed"
    });
    let canonical = canonical_bytes(&transcript).unwrap();

    assert_eq!(
        hash_bytes(&canonical),
        "sha256:ppcgk77uz_4SWvSPr5liRB7ZDngFnIph9s6di2OHgtI",
        "cross-repository transcript fixture changed"
    );
    assert_eq!(
        derive_sas(&[0x42_u8; 32], &transcript).unwrap(),
        "270643",
        "both devices must derive the same six-digit SAS from the frozen transcript"
    );

    let mut tampered = transcript;
    tampered["new_device_id"] = Value::String("attacker-device".to_owned());
    assert_ne!(derive_sas(&[0x42_u8; 32], &tampered).unwrap(), "270643");
}

#[test]
fn encrypted_challenge_round_trips_and_binds_aad() {
    let did = "did:wba:awiki.test:alice";
    let signing_private =
        anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[3_u8; 32]));
    let new_e2ee_private = anp::PrivateKeyMaterial::X25519(X25519StaticSecret::from([5_u8; 32]));
    let new_pairing_private = anp::PrivateKeyMaterial::X25519(X25519StaticSecret::from([7_u8; 32]));
    let admin_pairing_private =
        anp::PrivateKeyMaterial::X25519(X25519StaticSecret::from([11_u8; 32]));
    let admin_pairing_public_key = x25519_public_b64u(&admin_pairing_private.public_key()).unwrap();
    let join_request = DeviceJoinRequest {
        request_type: DEVICE_JOIN_REQUEST_TYPE.to_owned(),
        did: did.to_owned(),
        join_session_id: "join-fixed".to_owned(),
        device_id: "new-b".to_owned(),
        signing_public_key: verification_method(
            did,
            &format!("{did}#new-b-sign"),
            "Multikey",
            &signing_private.public_key(),
        )
        .unwrap(),
        e2ee_public_key: verification_method(
            did,
            &format!("{did}#new-b-e2ee"),
            "X25519KeyAgreementKey2019",
            &new_e2ee_private.public_key(),
        )
        .unwrap(),
        pairing_public_key: x25519_public_b64u(&new_pairing_private.public_key()).unwrap(),
        profiles: DEVICE_JOIN_VNEXT_PROFILES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        requested_role: "member".to_owned(),
        issued_at: "2026-07-19T01:02:03Z".to_owned(),
        expires_at: "2026-07-19T01:12:03Z".to_owned(),
        signature: "unused-in-this-crypto-fixture".to_owned(),
    };
    let join_request_hash = canonical_hash(&serde_json::to_value(&join_request).unwrap()).unwrap();
    let challenge_plaintext = [0xa5_u8; JOIN_CHALLENGE_LEN];
    let encrypted = encrypt_challenge(
        &admin_pairing_private,
        &join_request,
        &join_request_hash,
        "challenge-fixed",
        "admin-a",
        &admin_pairing_public_key,
        "2026-07-19T01:07:03Z",
        &challenge_plaintext,
    )
    .unwrap();
    let challenge = DeviceJoinChallenge {
        operation_id: "op-fixed".to_owned(),
        join_session_id: join_request.join_session_id.clone(),
        challenge_id: "challenge-fixed".to_owned(),
        admin_device_id: "admin-a".to_owned(),
        admin_pairing_public_key,
        ciphertext: encrypted,
        challenge_expires_at: "2026-07-19T01:07:03Z".to_owned(),
        authorizing_device_proof: sample_proof(),
    };

    assert_eq!(
        decrypt_challenge(
            &new_e2ee_private,
            &join_request,
            &join_request_hash,
            &challenge,
        )
        .unwrap()
        .expose_secret(),
        challenge_plaintext
    );

    let mut tampered = challenge;
    tampered.admin_device_id = "attacker-admin".to_owned();
    assert!(matches!(
        decrypt_challenge(
            &new_e2ee_private,
            &join_request,
            &join_request_hash,
            &tampered,
        ),
        Err(crate::ImError::PermissionDenied)
    ));
}

#[test]
fn pending_join_is_restart_safe_idempotent_and_stores_secrets_only_in_vault() {
    let root = tempfile::tempdir().unwrap();
    let did = crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap();
    let core = open_vault_core(root.path());
    let request = DeviceJoinStartRequest {
        operation_id: "start-fixed-operation".to_owned(),
        did: did.clone(),
        ttl_seconds: 300,
    };
    let started = core.device_join().start(request.clone()).unwrap();

    assert_eq!(started.session.side, DeviceJoinSide::NewDevice);
    assert_eq!(started.session.phase, DeviceJoinLocalPhase::Pending);
    assert_eq!(started.join_request.requested_role, "member");
    validate_join_request(&started.join_request, OffsetDateTime::now_utc()).unwrap();

    let state_store = JoinStateStore::new(&core);
    let state_path = state_store.path(&started.session.join_session_id, DeviceJoinSide::NewDevice);
    let state_raw = fs::read(&state_path).unwrap();
    let state_text = std::str::from_utf8(&state_raw).unwrap();
    assert!(!state_text.contains("PRIVATE KEY"));
    assert!(!state_text.contains("BEGIN"));
    let stored = state_store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    assert!(stored.signing_private_ref.is_some());
    assert!(stored.e2ee_private_ref.is_some());
    assert_eq!(
        stored.pairing_private_ref.kind,
        SecretKind::IdentityJoinPairingPrivate
    );

    let vault = crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.path().join("vault")),
    );
    let secret_refs = crate::vault::SecretVault::list(&vault).unwrap();
    assert_eq!(secret_refs.len(), 3);
    assert!(secret_refs
        .iter()
        .any(|value| value.kind == SecretKind::IdentityDeviceSigningPrivate));
    assert!(secret_refs
        .iter()
        .any(|value| value.kind == SecretKind::IdentityE2eeAgreementPrivate));
    assert!(secret_refs
        .iter()
        .any(|value| value.kind == SecretKind::IdentityJoinPairingPrivate));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&state_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    drop(core);
    let restarted = open_vault_core(root.path());
    assert_eq!(
        restarted
            .device_join()
            .session(&started.session.join_session_id, DeviceJoinSide::NewDevice,)
            .unwrap(),
        started.session
    );
    let retried = restarted.device_join().start(request.clone()).unwrap();
    assert_eq!(retried, started);

    let error = restarted
        .device_join()
        .start(DeviceJoinStartRequest {
            ttl_seconds: 301,
            ..request
        })
        .unwrap_err();
    assert!(matches!(
        error,
        crate::ImError::InvalidInput {
            field: Some(field),
            ..
        } if field == "operation_id"
    ));
}

#[test]
fn expired_challenge_marks_session_expired_and_deletes_all_pending_secrets() {
    let root = tempfile::tempdir().unwrap();
    let core = open_vault_core(root.path());
    let started = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-expiring-challenge".to_owned(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
            ttl_seconds: 300,
        })
        .unwrap();
    let store = JoinStateStore::new(&core);
    let mut stored = store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    stored.phase = DeviceJoinLocalPhase::ResponsePrepared;
    stored.challenge = Some(DeviceJoinChallenge {
        operation_id: "admin-challenge-operation".to_owned(),
        join_session_id: started.session.join_session_id.clone(),
        challenge_id: "expired-challenge".to_owned(),
        admin_device_id: "admin-a".to_owned(),
        admin_pairing_public_key: URL_SAFE_NO_PAD.encode([9_u8; 32]),
        ciphertext: EncryptedJoinChallenge {
            algorithm: DEVICE_JOIN_CHALLENGE_ALGORITHM.to_owned(),
            nonce_b64u: URL_SAFE_NO_PAD.encode([8_u8; JOIN_NONCE_LEN]),
            ciphertext_b64u: URL_SAFE_NO_PAD.encode([7_u8; 48]),
        },
        challenge_expires_at: format_time(OffsetDateTime::now_utc() - Duration::seconds(1))
            .unwrap(),
        authorizing_device_proof: sample_proof(),
    });
    store.save(&stored).unwrap();

    let summary = core
        .device_join()
        .session(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap();
    assert_eq!(summary.phase, DeviceJoinLocalPhase::Expired);

    let vault = crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.path().join("vault")),
    );
    assert!(crate::vault::SecretVault::list(&vault).unwrap().is_empty());

    let stored = store
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    assert_eq!(stored.phase, DeviceJoinLocalPhase::Expired);
    assert!(stored.signing_private_ref.is_some());
    assert!(stored.e2ee_private_ref.is_some());
    assert_eq!(
        stored.pairing_private_ref.kind,
        SecretKind::IdentityJoinPairingPrivate
    );
}

#[test]
fn pending_join_refuses_to_generate_secrets_without_secret_vault() {
    let root = tempfile::tempdir().unwrap();
    let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
    let error = core
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-without-vault".to_owned(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap(),
            ttl_seconds: 300,
        })
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        }
    ));
}
