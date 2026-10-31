use super::*;
use ed25519_dalek::{pkcs8::EncodePrivateKey, Signer};

fn signing_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[19u8; 32])
}
fn jwk() -> wire::Ed25519PublicJwk {
    wire::Ed25519PublicJwk {
        kty: "OKP".into(),
        crv: "Ed25519".into(),
        x: crypto::b64(&signing_key().verifying_key().to_bytes()),
    }
}
fn config() -> wire::ConfigurationPayload {
    wire::ConfigurationPayload {
        protocol: wire::PROTOCOL.into(),
        deployment_id: "testing".into(),
        tenant_id: "tenant".into(),
        realm_tenant_name: "Awiki".into(),
        configuration_version: 1,
        iat: crypto::now() - 1,
        exp: crypto::now() + 3600,
        control_base_url: "https://control.test/user-service/v1/key-recovery".into(),
        control_audience: "urn:awiki:testing".into(),
        pst_issuer: "https://control.test/user-service".into(),
        realms: (1..=3)
            .map(|n| wire::RealmConfig {
                id: format!("{n:032x}"),
                address: format!("https://realm-{n}.test/"),
                receipt_keys: vec![wire::ReceiptKey {
                    kid: "receipt".into(),
                    public_jwk: jwk(),
                }],
            })
            .collect(),
        register_threshold: 3,
        recover_threshold: 2,
        pin_hashing_mode: "Standard2019".into(),
        pin_format: "ascii-4-digits".into(),
        num_guesses: 10,
        capability_enabled: true,
        supported_actions: vec!["enable".into(), "recover".into()],
    }
}
pub(super) fn target() -> wire::Target {
    let did =
        "did:wba:control.test:user:alice:e1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
    wire::Target {
        target_id: "target-a".into(),
        full_handle: "alice.control.test".into(),
        root_kid: format!("{did}#root"),
        did,
        binding_generation: "184467440737095516160".into(),
        recovery_id: None,
        active_epoch: None,
        root_public_jwk: jwk(),
        can_enable: true,
        can_recover: false,
        unavailable_reason: None,
    }
}
pub(super) fn session() -> Session {
    let config = config();
    let config_hash = crypto::config_hash(&config).unwrap();
    Session {
        id: uuid(),
        tenant_id: config.tenant_id.clone(),
        config,
        config_hash,
        dpop_private: http::generate_key(),
        remote_challenge: None,
        resume_operation_id: None,
        discovery: None,
        pst: None,
        targets: vec![target()],
        selected: Some(target()),
        nonces: BTreeMap::new(),
        exact_requests: BTreeMap::new(),
    }
}
fn header(s: &Session) -> wire::BackupHeader {
    let target = target();
    wire::BackupHeader {
        format: "awiki.root-backup.v1".into(),
        deployment_id: s.config.deployment_id.clone(),
        tenant_id: s.tenant_id.clone(),
        recovery_id: uuid(),
        backup_epoch: 1,
        slot_id: format!("rs_{}", crypto::b64(&[3u8; 32])),
        did: target.did,
        root_kid: target.root_kid,
        root_public_jwk: target.root_public_jwk,
        document_digest_at_backup: crypto::hash(b"document"),
        config_hash: s.config_hash.clone(),
        created_at: "2026-10-30T00:00:00Z".into(),
    }
}
pub(super) fn core(root: &std::path::Path, device: &str) -> crate::ImCore {
    let paths = crate::ImCorePaths {
        identities: crate::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities/index.json"),
            default_identity_path: Some(root.join("identities/default")),
        },
        local_state: crate::LocalStatePaths {
            sqlite_path: root.join("local/im.sqlite"),
        },
        runtime: crate::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    };
    crate::ImCore::new_with_options(
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://control.test").unwrap(),
            did_domain: "control.test".into(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        },
        paths,
        crate::ImCoreOpenOptions::default()
            .with_pin_recovery_tenants(vec![PinRecoveryTenantConfig {
                tenant_id: "tenant".into(),
                deployment_id: "testing".into(),
                control_base_url: config().control_base_url,
                config_signing_keys: BTreeMap::new(),
            }])
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([7u8; 32]),
                    root.join("vault"),
                    "workspace",
                    device,
                ),
            ),
    )
    .unwrap()
}
pub(super) fn sign_jws<T: serde::Serialize>(value: &T, typ: &str) -> String {
    let header = json!({"typ":typ,"alg":"EdDSA","kid":"key"});
    let input = format!(
        "{}.{}",
        crypto::b64(&crypto::canonical(&header).unwrap()),
        crypto::b64(&crypto::canonical(value).unwrap())
    );
    format!(
        "{input}.{}",
        crypto::b64(&signing_key().sign(input.as_bytes()).to_bytes())
    )
}
#[test]
fn pin_accepts_ascii_only_and_preserves_leading_zeroes() {
    let p = SecretPin::new(b"0007".to_vec()).unwrap();
    assert_eq!(p.bytes(), b"0007");
    for value in ["１２３４", "1234 ", " 1234", "123", "12345", "123a"] {
        assert!(SecretPin::new(value.as_bytes().to_vec()).is_err());
    }
    assert!(SecretOtp::new(b"001234".to_vec()).is_ok());
    assert!(SecretOtp::new(b"1234".to_vec()).is_err());
}
#[test]
fn config_signature_and_crypto_projection_are_both_required() {
    let mut c = config();
    let hash = crypto::config_hash(&c).unwrap();
    let trusted = PinRecoveryTenantConfig {
        tenant_id: c.tenant_id.clone(),
        deployment_id: c.deployment_id.clone(),
        control_base_url: c.control_base_url.clone(),
        config_signing_keys: BTreeMap::from([("key".into(), jwk().x)]),
    };
    let response = wire::ConfigResponse {
        config_jws: sign_jws(&c, "awiki-recovery-config+jwt"),
        config_hash: hash.clone(),
    };
    assert!(crypto::verify_config(response, &trusted, false).is_ok());
    c.configuration_version += 1;
    c.iat += 1;
    c.exp += 1;
    c.realms[0].receipt_keys[0].kid = "rotated".into();
    assert_eq!(hash, crypto::config_hash(&c).unwrap());
    c.realms.swap(0, 1);
    assert_ne!(hash, crypto::config_hash(&c).unwrap());
    let response = wire::ConfigResponse {
        config_jws: sign_jws(&c, "awiki-recovery-config+jwt"),
        config_hash: hash,
    };
    assert!(crypto::verify_config(response, &trusted, false).is_err());
}
#[test]
fn forged_configuration_key_and_redirectable_url_fail_closed() {
    let c = config();
    let trusted = PinRecoveryTenantConfig {
        tenant_id: c.tenant_id.clone(),
        deployment_id: c.deployment_id.clone(),
        control_base_url: c.control_base_url.clone(),
        config_signing_keys: BTreeMap::from([("other".into(), jwk().x)]),
    };
    let response = wire::ConfigResponse {
        config_jws: sign_jws(&c, "awiki-recovery-config+jwt"),
        config_hash: crypto::config_hash(&c).unwrap(),
    };
    assert!(crypto::verify_config(response, &trusted, false).is_err());
    for raw in [
        "http://remote.test/",
        "https://u:p@control.test/",
        "https://control.test/?target=x",
        "https://control.test/#fragment",
    ] {
        assert!(crypto::validate_url(raw, false).is_err());
    }
    assert!(crypto::validate_url("http://127.0.0.1:1234/", true).is_ok());
    assert!(crypto::validate_url("http://remote.test/", true).is_err());
}
#[test]
fn backup_aead_binds_header_and_original_root() {
    let s = session();
    let key = [4u8; 32];
    let root = signing_key().to_pkcs8_der().unwrap();
    let backup = crypto::encrypt_root(header(&s), root.as_bytes(), &key).unwrap();
    let recovered = crypto::decrypt_root(&backup, &key, &target()).unwrap();
    assert_eq!(recovered.as_slice(), root.as_bytes());
    let mut tampered = backup.clone();
    tampered.envelope.header.slot_id.push('x');
    assert!(crypto::decrypt_root(&tampered, &key, &target()).is_err());
    let mut other = target();
    other.root_public_jwk.x = crypto::b64(
        &ed25519_dalek::SigningKey::from_bytes(&[1u8; 32])
            .verifying_key()
            .to_bytes(),
    );
    assert!(crypto::decrypt_root(&backup, &key, &other).is_err());
    assert!(crypto::decrypt_root(&backup, &[5u8; 32], &target()).is_err());
}
#[test]
fn dpop_retry_has_new_jti_same_body_token_and_idempotency() {
    let s = session();
    let url = "https://realm-1.test/req";
    let body = b"exact-cbor";
    let ik = uuid();
    let a = http::proof(&s, "POST", url, body, Some("secret-token"), Some(&ik)).unwrap();
    let b = http::proof(&s, "POST", url, body, Some("secret-token"), Some(&ik)).unwrap();
    let pa = jwt_payload(&a).unwrap();
    let pb = jwt_payload(&b).unwrap();
    assert_ne!(pa["jti"], pb["jti"]);
    assert_eq!(pa["awiki_ik"], ik);
    assert_eq!(pa["awiki_bht"], crypto::hash(body));
    assert_eq!(pa["ath"], crypto::hash(b"secret-token"));
    let parts = a.split('.').collect::<Vec<_>>();
    let sig = p256::ecdsa::Signature::from_slice(&crypto::decode(parts[2]).unwrap()).unwrap();
    let private = crypto::decode(&s.dpop_private).unwrap();
    let key = p256::ecdsa::SigningKey::from_slice(&private).unwrap();
    use p256::ecdsa::signature::Verifier;
    assert!(key
        .verifying_key()
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
        .is_ok());
    let get = http::proof(&s, "GET", url, b"", None, None).unwrap();
    let p = jwt_payload(&get).unwrap();
    assert!(p.get("ath").is_none());
    assert!(p.get("awiki_ik").is_none());
}
#[test]
fn exact_body_retry_rejects_replacement_and_is_vault_only() {
    let root = tempfile::tempdir().unwrap();
    let c = core(root.path(), "device-a");
    let store = Store::new(&c).unwrap();
    let mut s = session();
    let first = http::prepare(
        &store,
        &mut s,
        "step",
        "POST",
        "https://control.test/op",
        b"SECRET_RECOVERY_BODY".to_vec(),
    )
    .unwrap();
    let retry = http::prepare(
        &store,
        &mut s,
        "step",
        "POST",
        "https://control.test/op",
        b"SECRET_RECOVERY_BODY".to_vec(),
    )
    .unwrap();
    assert_eq!(first.idempotency_key, retry.idempotency_key);
    assert!(http::prepare(
        &store,
        &mut s,
        "step",
        "POST",
        "https://control.test/op",
        b"changed".to_vec()
    )
    .is_err());
    let reopened = Store::new(&core(root.path(), "device-a")).unwrap();
    assert_eq!(
        reopened.session(&s.id).unwrap().dpop_private,
        s.dpop_private
    );
    assert!(Store::new(&core(root.path(), "device-b"))
        .unwrap()
        .session(&s.id)
        .is_err());
    for file in std::fs::read_dir(root.path().join("vault/records")).unwrap() {
        let bytes = std::fs::read(file.unwrap().path()).unwrap();
        assert!(!bytes
            .windows(b"SECRET_RECOVERY_BODY".len())
            .any(|v| v == b"SECRET_RECOVERY_BODY"));
        assert!(!bytes
            .windows(s.dpop_private.len())
            .any(|v| v == s.dpop_private.as_bytes()));
    }
}
#[test]
fn pending_index_contains_only_public_operation_fields() {
    let root = tempfile::tempdir().unwrap();
    let c = core(root.path(), "device-a");
    let store = Store::new(&c).unwrap();
    let s = session();
    store.save_session(&s).unwrap();
    let mut op = new_operation(&s, PinRecoveryKind::Recover).unwrap();
    op.wrapping_key_b64u = Some("SECRET_WRAPPING_KEY".into());
    op.recovered_root_b64u = Some("SECRET_ROOT".into());
    store.save_operation(&op).unwrap();
    let conn =
        rusqlite::Connection::open(root.path().join("local/im.pin-recovery.sqlite")).unwrap();
    let columns = conn
        .prepare("PRAGMA table_info(pin_recovery_operations_v1)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(columns, vec!["operation_id", "target_id", "did", "phase"]);
    let raw = std::fs::read(root.path().join("local/im.pin-recovery.sqlite")).unwrap();
    for secret in ["SECRET_WRAPPING_KEY", "SECRET_ROOT"] {
        assert!(!raw.windows(secret.len()).any(|b| b == secret.as_bytes()));
    }
    assert_eq!(store.list_operations().unwrap().len(), 1);
    assert!(Store::new(&core(root.path(), "device-b"))
        .unwrap()
        .list_operations()
        .unwrap()
        .is_empty());
}
#[test]
fn uncertain_cannot_reopen_from_instantaneous_noncommit_status() {
    let exp = 1_000;
    for observed in [999, 1_000, 1_001, 1_005] {
        assert_eq!(
            error_code(
                &custody::require_expired_noncommit(exp, observed, Some(false)).unwrap_err()
            ),
            "reconciliation_required"
        );
    }
    assert!(custody::require_expired_noncommit(exp, 1_006, Some(false)).is_ok());
    assert!(custody::require_expired_noncommit(exp, 1_006, Some(true)).is_err());
    assert!(custody::require_expired_noncommit(exp, 1_006, None).is_err());
}
#[test]
fn local_pending_never_becomes_an_active_identity_or_repeats_a_guess() {
    let s = session();
    let mut op = new_operation(&s, PinRecoveryKind::Recover).unwrap();
    op.phase = PinRecoveryPhase::CommitUnknown;
    let progress = lifecycle::progress(&op);
    assert!(progress.recovered_identity.is_none());
    assert!(!progress
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin));
    assert!(!progress
        .allowed_actions
        .contains(&PinRecoveryAction::ConfirmDevice));
    assert!(progress
        .allowed_actions
        .contains(&PinRecoveryAction::Resume));
    op.phase = PinRecoveryPhase::LocalActivationPending;
    let progress = lifecycle::progress(&op);
    assert!(progress
        .allowed_actions
        .contains(&PinRecoveryAction::RetryLocalActivation));
    assert!(!progress
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin));
}
fn remote_fixture(s: &Session, o: &Operation) -> wire::OperationResponse {
    wire::OperationResponse {
        protocol: wire::PROTOCOL.into(),
        operation_id: o.id.clone(),
        kind: if o.kind == PinRecoveryKind::Enable {
            "enable"
        } else {
            "recover"
        }
        .into(),
        state: "authorized".into(),
        operation_version: 1,
        allowed_actions: vec!["upload_backup".into(), "cancel".into()],
        target_id: o.target.target_id.clone(),
        did: o.target.did.clone(),
        binding_generation: o.target.binding_generation.clone(),
        recovery_id: uuid(),
        backup_epoch: 1,
        slot_id: format!("rs_{}", crypto::b64(&[1u8; 32])),
        expected_active_epoch: None,
        registration_version: None,
        backup_hash: None,
        config_hash: s.config_hash.clone(),
        op_jkt: http::jkt(&s.dpop_private).unwrap(),
        authorization_epoch: 1,
        guess_cycle: 1,
        guesses_used: 0,
        guesses_limit: 10,
        registered_realms: vec![],
        verified_realms: vec![],
        cleanup_remaining: 0,
        expires_at: "2099-01-01T00:00:00Z".into(),
        authorization_challenge: None,
        commit_challenge: None,
        device_result: None,
        recovery_checkpoint: None,
        recovered_device_authorization: None,
        last_error_code: None,
    }
}
#[test]
fn operation_never_switches_target_key_slot_or_epoch_on_retry() {
    let s = session();
    let mut o = new_operation(&s, PinRecoveryKind::Enable).unwrap();
    let initial = remote_fixture(&s, &o);
    accept_remote(&mut o, &s, initial.clone()).unwrap();
    for field in [
        "target_id",
        "binding_generation",
        "slot_id",
        "op_jkt",
        "config_hash",
    ] {
        let mut value = serde_json::to_value(&initial).unwrap();
        value[field] = json!("other");
        let changed = serde_json::from_value(value).unwrap();
        assert!(
            accept_remote(&mut o, &s, changed).is_err(),
            "{field} must be fixed"
        );
    }
    let mut changed = initial.clone();
    changed.backup_epoch = 2;
    assert!(accept_remote(&mut o, &s, changed).is_err());
    let mut newer = initial.clone();
    newer.operation_version = 2;
    newer.registration_version = Some("01010101010101010101010101010101".into());
    accept_remote(&mut o, &s, newer).unwrap();
    assert!(
        accept_remote(&mut o, &s, initial).is_err(),
        "version/checkpoint cannot roll back"
    );
}
#[test]
fn public_actions_follow_remote_admission_and_local_uncertainty() {
    let s = session();
    let mut o = new_operation(&s, PinRecoveryKind::Enable).unwrap();
    o.phase = PinRecoveryPhase::AwaitingPin;
    o.remote = Some(remote_fixture(&s, &o));
    assert!(lifecycle::progress(&o)
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin));
    o.remote.as_mut().unwrap().allowed_actions = vec!["cancel".into()];
    assert!(!lifecycle::progress(&o)
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin));
    assert!(lifecycle::progress(&o)
        .allowed_actions
        .contains(&PinRecoveryAction::Cancel));
    o.phase = PinRecoveryPhase::CommitUnknown;
    o.commit_attempted = true;
    assert!(!lifecycle::progress(&o)
        .allowed_actions
        .contains(&PinRecoveryAction::Cancel));
}
struct PersistedPinProvider {
    phase_path: std::path::PathBuf,
    proposal: crate::internal::identity_provider::ProviderPinRecoveryProposal,
}
#[async_trait::async_trait]
impl crate::internal::identity_provider::ProviderPinRootRecoverySession for PersistedPinProvider {
    async fn proposal(
        &self,
    ) -> crate::internal::identity_provider::ProviderResult<
        crate::internal::identity_provider::ProviderPinRecoveryProposal,
    > {
        let mut p = self.proposal.clone();
        p.phase = std::fs::read_to_string(&self.phase_path).unwrap();
        Ok(p)
    }
    async fn sign_pin_recovery_intent(
        &self,
        _: crate::internal::identity_provider::ProviderPinRecoverySignRequest,
    ) -> crate::internal::identity_provider::ProviderResult<
        crate::internal::identity_provider::ProviderPinRecoverySignedIntent,
    > {
        panic!("observation must not sign")
    }
    async fn complete_pin_root_recovery(
        &self,
        _: crate::internal::identity_provider::ProviderPinRecoveryCompletion,
    ) -> crate::internal::identity_provider::ProviderResult<
        Option<std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>>,
    > {
        panic!("observation must not activate")
    }
}
#[tokio::test]
async fn provider_uncertainty_repairs_stale_core_after_two_store_crash_boundary() {
    use crate::internal::identity_provider::*;
    let root = tempfile::tempdir().unwrap();
    let c = core(root.path(), "device-a");
    let store = Store::new(&c).unwrap();
    let s = session();
    store.save_session(&s).unwrap();
    let mut op = new_operation(&s, PinRecoveryKind::Recover).unwrap();
    op.phase = PinRecoveryPhase::AwaitingDeviceConfirmation;
    let binding = ProviderPinRecoveryBinding {
        operation_id: op.id.clone(),
        account_user_id: "account".into(),
        target_id: op.target.target_id.clone(),
        handle: op.target.full_handle.clone(),
        did: op.target.did.clone(),
        domain: "control.test".into(),
        binding_generation: op.target.binding_generation.clone(),
        recovery_id: uuid(),
        backup_epoch: 1,
        slot_id: format!("rs_{}", crypto::b64(&[1u8; 32])),
        op_jkt: http::jkt(&s.dpop_private).unwrap(),
        root_kid: op.target.root_kid.clone(),
        root_public_key_multibase: crypto::root_multibase(&op.target.root_public_jwk).unwrap(),
        encrypted_package_hash: Some(crypto::hash(b"package")),
        config_hash: s.config_hash.clone(),
        audience: s.config.control_base_url.clone(),
    };
    let proposal = ProviderPinRecoveryProposal {
        identity: ProviderIdentityRef {
            store_id: "store".into(),
            identity_id: "identity".into(),
            did: op.target.did.clone(),
        },
        binding,
        device_proposal: json!({"device_id":"new-device"}),
        generation: 1,
        phase: "prepared".into(),
    };
    op.custody_pending = Some(serde_json::to_value(&proposal).unwrap());
    store.save_operation(&op).unwrap();
    let phase_path = root.path().join("provider-phase");
    std::fs::write(&phase_path, "publication_uncertain").unwrap();
    drop(store);
    drop(c); // Reopen Core while Provider has already persisted the signature phase.
    let c = core(root.path(), "device-a");
    let store = Store::new(&c).unwrap();
    let mut reopened = store.operation(&op.id).unwrap();
    assert_eq!(custody::proposal(&reopened).unwrap().phase, "prepared");
    let provider = PersistedPinProvider {
        phase_path,
        proposal,
    };
    custody::capture_proposal(&store, &mut reopened, &provider)
        .await
        .unwrap();
    let repaired = store.operation(&op.id).unwrap();
    assert_eq!(
        custody::proposal(&repaired).unwrap().phase,
        "publication_uncertain"
    );
    let progress = lifecycle::progress(&repaired);
    assert!(!progress
        .allowed_actions
        .contains(&PinRecoveryAction::ConfirmDevice));
    assert!(!progress
        .allowed_actions
        .contains(&PinRecoveryAction::Cancel));
    assert!(progress
        .allowed_actions
        .contains(&PinRecoveryAction::Resume));
}
#[test]
fn delayed_preexpiry_status_cannot_authorize_noncommit_reconciliation() {
    let challenge_exp = 1_000;
    let status_request_started_at = 999;
    let status_returned_at = 1_100;
    assert!(status_returned_at > challenge_exp + 5);
    assert!(custody::require_expired_noncommit(
        challenge_exp,
        status_request_started_at,
        Some(false)
    )
    .is_err());
    assert!(custody::require_expired_noncommit(challenge_exp, 1_006, Some(false)).is_ok());
}
#[test]
fn corrected_otp_is_a_new_submission_while_exact_retry_keeps_its_key() {
    let root = tempfile::tempdir().unwrap();
    let c = core(root.path(), "device-a");
    let store = Store::new(&c).unwrap();
    let mut s = session();
    let wrong = json!({"protocol":wire::PROTOCOL,"challenge_id":uuid(),"code":"000000"});
    let mut corrected = wrong.clone();
    corrected["code"] = json!("000001");
    let url = "https://control.test/phone/sessions";
    let a = http::prepare(
        &store,
        &mut s,
        &otp_submission_step(&wrong).unwrap(),
        "POST",
        url,
        crypto::canonical(&wrong).unwrap(),
    )
    .unwrap();
    let retry = http::prepare(
        &store,
        &mut s,
        &otp_submission_step(&wrong).unwrap(),
        "POST",
        url,
        crypto::canonical(&wrong).unwrap(),
    )
    .unwrap();
    assert_eq!(a.idempotency_key, retry.idempotency_key);
    let b = http::prepare(
        &store,
        &mut s,
        &otp_submission_step(&corrected).unwrap(),
        "POST",
        url,
        crypto::canonical(&corrected).unwrap(),
    )
    .unwrap();
    assert_ne!(a.idempotency_key, b.idempotency_key);
}
struct EmptyCustody {
    lists: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl crate::internal::identity_provider::IdentityCustody for EmptyCustody {
    async fn store_info(
        &self,
    ) -> crate::internal::identity_provider::ProviderResult<
        crate::internal::identity_provider::ProviderStoreInfo,
    > {
        panic!("unexpected store inspection")
    }
    async fn list_identities(
        &self,
    ) -> crate::internal::identity_provider::ProviderResult<
        Vec<crate::internal::identity_provider::ProviderIdentityDescriptor>,
    > {
        self.lists.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(vec![])
    }
    async fn begin_pin_root_recovery(
        &self,
        _: crate::internal::identity_provider::ProviderBeginPinRootRecoveryRequest,
    ) -> crate::internal::identity_provider::ProviderResult<
        std::sync::Arc<dyn crate::internal::identity_provider::ProviderPinRootRecoverySession>,
    > {
        panic!("observation/cancellation must not create a new identity")
    }
    async fn open_identity(
        &self,
        _: &crate::internal::identity_provider::ProviderIdentityRef,
    ) -> crate::internal::identity_provider::ProviderResult<
        std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>,
    > {
        panic!("unexpected open")
    }
    async fn create_identity(
        &self,
        _: crate::internal::identity_provider::ProviderCreateIdentityRequest,
    ) -> crate::internal::identity_provider::ProviderResult<
        std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>,
    > {
        panic!("unexpected create")
    }
    async fn delete_identity(
        &self,
        _: &crate::internal::identity_provider::ProviderIdentityRef,
    ) -> crate::internal::identity_provider::ProviderResult<()> {
        panic!("generic delete must not be used")
    }
    async fn begin_device_enrollment(
        &self,
        _: crate::internal::identity_provider::ProviderDeviceEnrollmentRequest,
    ) -> crate::internal::identity_provider::ProviderResult<
        std::sync::Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>,
    > {
        panic!("unexpected enrollment")
    }
    async fn begin_request_signing_enrollment(
        &self,
        _: crate::internal::identity_provider::ProviderRequestSigningEnrollmentRequest,
    ) -> crate::internal::identity_provider::ProviderResult<
        std::sync::Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>,
    > {
        panic!("unexpected enrollment")
    }
    async fn resume_enrollment(
        &self,
        _: &crate::internal::identity_provider::ProviderIdentityRef,
    ) -> crate::internal::identity_provider::ProviderResult<
        Option<std::sync::Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>>,
    > {
        panic!("unexpected legacy enrollment")
    }
    async fn confirm_root_promotion(
        &self,
        _: &crate::internal::identity_provider::ProviderIdentityRef,
        _: crate::internal::identity_provider::ProviderVerifiedRemoteDocument,
    ) -> crate::internal::identity_provider::ProviderResult<()> {
        panic!("unexpected legacy promotion")
    }
    async fn sign_pending_root_object_proof(
        &self,
        _: &crate::internal::identity_provider::ProviderIdentityRef,
        _: crate::internal::identity_provider::ProviderObjectProofRequest,
    ) -> crate::internal::identity_provider::ProviderResult<serde_json::Value> {
        panic!("unexpected legacy signing")
    }
    async fn recover(&self) -> crate::internal::identity_provider::ProviderResult<()> {
        Ok(())
    }
}
fn core_empty_provider(
    root: &std::path::Path,
    provider: std::sync::Arc<EmptyCustody>,
) -> crate::ImCore {
    let base = core(root, "device-a");
    crate::ImCore::new_with_options(
        base.inner().sdk_config().clone(),
        base.inner().sdk_paths().clone(),
        crate::ImCoreOpenOptions::default()
            .with_pin_recovery_tenants(base.inner().pin_recovery_tenants.clone())
            .with_identity_custody_provider(provider)
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([7u8; 32]),
                    root.join("vault"),
                    "workspace",
                    "device-a",
                ),
            ),
    )
    .unwrap()
}
#[tokio::test]
async fn ingress_marker_survives_restart_without_creating_during_cancel_or_inspect() {
    let root = tempfile::tempdir().unwrap();
    let provider = std::sync::Arc::new(EmptyCustody {
        lists: Default::default(),
    });
    let c = core_empty_provider(root.path(), provider.clone());
    let store = Store::new(&c).unwrap();
    let s = session();
    store.save_session(&s).unwrap();
    let mut o = new_operation(&s, PinRecoveryKind::Recover).unwrap();
    o.custody_ingress_started = true;
    o.recovered_root_b64u = Some(crypto::b64(b"sealed root"));
    o.phase = PinRecoveryPhase::AwaitingPin;
    store.save_operation(&o).unwrap();
    drop(store);
    drop(c);
    let c = core_empty_provider(root.path(), provider.clone());
    let store = Store::new(&c).unwrap();
    let mut o = store.operation(&o.id).unwrap();
    custody::synchronize_pending(&c, &store, &s, &mut o)
        .await
        .unwrap();
    assert!(o.recovered_root_b64u.is_some());
    assert!(o.custody_ingress_started);
    assert!(custody::ingress_root(&o, false).unwrap().is_none());
    assert!(!lifecycle::progress(&o)
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin));
    let mut remote = remote_fixture(&s, &o);
    remote.state = "cancelled".into();
    o.remote = Some(remote);
    custody::cancel_prepared(&c, &store, &s, &mut o)
        .await
        .unwrap();
    assert!(!o.custody_ingress_started);
    assert!(o.custody_pending.is_none());
    clear_ephemeral(&mut o);
    assert!(o.recovered_root_b64u.is_none());
    assert_eq!(provider.lists.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn cancelled_provider_delete_before_core_write_is_idempotent_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let provider = std::sync::Arc::new(EmptyCustody {
        lists: Default::default(),
    });
    let c = core_empty_provider(root.path(), provider.clone());
    let store = Store::new(&c).unwrap();
    let s = session();
    store.save_session(&s).unwrap();
    let mut o = new_operation(&s, PinRecoveryKind::Recover).unwrap();
    o.custody_ingress_started = true;
    o.custody_pending = Some(json!({"phase":"prepared"}));
    let mut remote = remote_fixture(&s, &o);
    remote.state = "cancelled".into();
    o.remote = Some(remote);
    store.save_operation(&o).unwrap();
    drop(store);
    drop(c);
    let c = core_empty_provider(root.path(), provider.clone());
    let store = Store::new(&c).unwrap();
    let mut reopened = store.operation(&o.id).unwrap();
    custody::cancel_prepared(&c, &store, &s, &mut reopened)
        .await
        .unwrap();
    let mut again = store.operation(&o.id).unwrap();
    custody::cancel_prepared(&c, &store, &s, &mut again)
        .await
        .unwrap();
    assert!(!again.custody_ingress_started);
    assert!(again.custody_pending.is_none());
    assert_eq!(provider.lists.load(std::sync::atomic::Ordering::SeqCst), 1);
}
#[test]
fn new_operation_cannot_bypass_an_existing_same_target_pending() {
    let root = tempfile::tempdir().unwrap();
    let c = core(root.path(), "device-a");
    let store = Store::new(&c).unwrap();
    let s = session();
    let mut old = new_operation(&s, PinRecoveryKind::Recover).unwrap();
    store.save_operation(&old).unwrap();
    assert!(ensure_no_pending_target(&store, &target()).is_err());
    old.phase = PinRecoveryPhase::Cancelled;
    store.save_operation(&old).unwrap();
    assert!(ensure_no_pending_target(&store, &target()).is_ok());
}
fn committed_device_fixture() -> (
    serde_json::Value,
    wire::RemoteDeviceResult,
    wire::RecoveredDeviceAuthorization,
) {
    let t = target();
    let signing_kid = format!("{}#device-sign", t.did);
    let agreement_kid = format!("{}#device-agree", t.did);
    let device_key = ed25519_dalek::SigningKey::from_bytes(&[20u8; 32])
        .verifying_key()
        .to_bytes();
    let agree =
        x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from([21u8; 32])).to_bytes();
    let document = json!({"id":t.did,"verificationMethod":[{"id":t.root_kid,"controller":t.did,"type":"JsonWebKey2020","publicKeyJwk":t.root_public_jwk},{"id":signing_kid,"controller":t.did,"type":"JsonWebKey2020","publicKeyJwk":{"kty":"OKP","crv":"Ed25519","x":crypto::b64(&device_key)}},{"id":agreement_kid,"controller":t.did,"type":"JsonWebKey2020","publicKeyJwk":{"kty":"OKP","crv":"X25519","x":crypto::b64(&agree)}}],"authentication":[t.root_kid,signing_kid],"assertionMethod":[t.root_kid,signing_kid],"keyAgreement":[agreement_kid],"deviceManifest":{"type":"ANPDeviceManifest","devices":[{"device_id":"new-device","signing_key_id":signing_kid,"e2ee_key_id":agreement_kid,"profiles":custody::profiles()}]}});
    let result = wire::RemoteDeviceResult {
        operation_id: uuid(),
        did: t.did,
        device_id: "new-device".into(),
        root_kid: t.root_kid,
        signing_kid: signing_kid.clone(),
        agreement_kid: agreement_kid.clone(),
        checkpoint: wire::Checkpoint {
            document_version: 2,
            registry_version: 2,
            document_digest: crypto::hash(&crypto::canonical(&document).unwrap()),
        },
        committed_document: document.clone(),
        committed_at: "2026-10-30T00:00:00Z".into(),
    };
    let auth = wire::RecoveredDeviceAuthorization {
        device_id: result.device_id.clone(),
        signing_key_id: signing_kid,
        e2ee_key_id: agreement_kid,
        status: "active".into(),
        role: "admin".into(),
        management_ready: true,
        auth_generation: 7,
    };
    (document, result, auth)
}
#[test]
fn historical_commit_does_not_authorize_a_currently_revoked_device() {
    let (document, result, auth) = committed_device_fixture();
    custody::verify_current_device(&document, &result, &auth).unwrap();
    let root_before = document["verificationMethod"][0].clone();
    let mut revoked_document = document.clone();
    revoked_document["deviceManifest"]["devices"] = json!([]);
    revoked_document["authentication"] = json!([result.root_kid]);
    revoked_document["assertionMethod"] = json!([result.root_kid]);
    assert_eq!(root_before, revoked_document["verificationMethod"][0]);
    assert_eq!(
        error_code(&custody::verify_current_device(&revoked_document, &result, &auth).unwrap_err()),
        "device_revoked"
    );
    let mut revoked_registry = auth.clone();
    revoked_registry.status = "revoked".into();
    assert_eq!(
        error_code(
            &custody::verify_current_device(&document, &result, &revoked_registry).unwrap_err()
        ),
        "device_revoked"
    );
    let mut demoted = auth;
    demoted.role = "member".into();
    demoted.management_ready = false;
    assert!(custody::verify_current_device(&document, &result, &demoted).is_err());
}
#[test]
fn a_new_explicit_pin_attempt_never_resubmits_old_recovered_receipts() {
    let s = session();
    let mut o = new_operation(&s, PinRecoveryKind::Recover).unwrap();
    let old = uuid();
    let current = uuid();
    o.attempt = Some(wire::AttemptResponse {
        attempt_id: current.clone(),
        operation_id: o.id.clone(),
        action: "recover".into(),
        expires_at: "2099-01-01T00:00:00Z".into(),
        slot_id: format!("rs_{}", crypto::b64(&[1u8; 32])),
        backup_epoch: 1,
        registration_version: Some("01010101010101010101010101010101".into()),
        cleanup_job_id: None,
        realm_tokens: vec![],
    });
    let old_receipt = sign_jws(
        &json!({"phase":"recovered","attempt_id":old,"jti":uuid()}),
        "awiki-realm-receipt+jwt",
    );
    let registered = sign_jws(
        &json!({"phase":"registered","attempt_id":uuid(),"jti":uuid()}),
        "awiki-realm-receipt+jwt",
    );
    let new_receipt = sign_jws(
        &json!({"phase":"recovered","attempt_id":current,"jti":uuid()}),
        "awiki-realm-receipt+jwt",
    );
    o.receipts = vec![old_receipt, registered.clone(), new_receipt.clone()];
    assert_eq!(
        receipts_for_current_attempt(&o).unwrap(),
        vec![registered, new_receipt]
    );
    assert_eq!(
        o.receipts.len(),
        3,
        "audit evidence is retained while only the current attempt is submitted"
    );
}
#[test]
fn control_requests_reuse_host_version_and_realms_use_protocol_headers() {
    let root = tempfile::tempdir().unwrap();
    let baseline = core(root.path(), "device-a");
    let mut config = baseline.inner().sdk_config().clone();
    config.client_version_info =
        Some(crate::ClientVersionInfo::new("awiki-me", "0815", "0.1.41", Some(53)).unwrap());
    let c = crate::ImCore::new(config, baseline.inner().sdk_paths().clone()).unwrap();
    let control = http::base_headers(&c, false);
    assert_eq!(
        control[crate::CLIENT_VERSION_HEADER],
        "awiki-me/0815/0.1.41+53"
    );
    assert_eq!(control["Content-Type"], "application/json");
    assert_eq!(
        http::control_version_headers(&c)[crate::CLIENT_VERSION_HEADER],
        control[crate::CLIENT_VERSION_HEADER]
    );
    let realm = http::base_headers(&c, true);
    assert!(!realm.contains_key(crate::CLIENT_VERSION_HEADER));
    assert_eq!(realm["Content-Type"], "application/octet-stream");
    assert_eq!(realm["X-Juicebox-Version"], "0.3.7");
    assert_eq!(realm["X-Awiki-Recovery-Protocol"], wire::PROTOCOL);
}
#[test]
fn realm_proxy_outage_is_not_a_pin_or_authorization_error() {
    let headers = BTreeMap::new();
    assert_eq!(
        error_code(&adapter::realm_response_error(
            502,
            &headers,
            b"<html>Bad Gateway</html>"
        )),
        "realm_unavailable"
    );
    assert_eq!(
        error_code(&adapter::realm_response_error(
            503,
            &headers,
            b"upstream unavailable"
        )),
        "realm_unavailable"
    );
    assert_eq!(
        error_code(&adapter::realm_response_error(
            409,
            &headers,
            br#"{"error":{"code":"binding_changed"}}"#
        )),
        "binding_changed"
    );
    assert_eq!(
        error_code(&adapter::realm_response_error(
            503,
            &headers,
            br#"{"error":{"code":"reconciliation_required"}}"#
        )),
        "reconciliation_required"
    );
    assert_eq!(
        error_code(&adapter::realm_response_error(
            403,
            &headers,
            br#"{"error":{"code":"operation_not_authorized"}}"#
        )),
        "operation_not_authorized"
    );
    assert_eq!(
        error_code(&adapter::realm_response_error(
            429,
            &headers,
            br#"{"error":{"code":"guess_budget_exhausted"}}"#
        )),
        "guess_budget_exhausted"
    );
}

pub(super) fn cleanup_test_core(root: &std::path::Path) -> crate::ImCore {
    core_empty_provider(
        root,
        std::sync::Arc::new(EmptyCustody {
            lists: Default::default(),
        }),
    )
}
