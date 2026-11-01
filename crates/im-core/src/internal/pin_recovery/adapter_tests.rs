use super::*;

#[test]
fn registration_receipt_continuation_requires_known_attempt_and_exact_binding() {
    let session = super::super::tests::session();
    let mut operation =
        super::super::new_operation(&session, crate::pin_recovery::PinRecoveryKind::Enable)
            .unwrap();
    let mut remote = super::super::tests::remote_fixture(&session, &operation);
    remote.backup_hash = Some(crypto::hash(b"envelope"));
    remote.registration_version = Some("a1".repeat(16));
    let attempt = wire::AttemptResponse {
        attempt_id: super::super::uuid(),
        operation_id: operation.id.clone(),
        action: "register".into(),
        expires_at: "2099-01-01T00:00:00Z".into(),
        slot_id: remote.slot_id.clone(),
        backup_epoch: remote.backup_epoch,
        registration_version: remote.registration_version.clone(),
        cleanup_job_id: None,
        realm_tokens: vec![],
    };
    let mut realm = session.config.realms[0].clone();
    realm.receipt_keys[0].kid = "key".into();
    let old = super::super::uuid();
    operation.registration_attempt_ids.insert(old.clone());
    operation.remote = Some(remote.clone());
    let exact = ExactRequest {
        method: "POST".into(),
        operation_id: Some(operation.id.clone()),
        url: format!("{}req", realm.address),
        body_b64u: crypto::b64(b"sealed registration"),
        idempotency_key: "unchanged-original-key".into(),
    };
    let claims = serde_json::json!({
        "iss":format!("urn:awiki:realm:{}",realm.id),
        "aud":session.config.control_audience,
        "iat":crypto::now(), "exp":crypto::now()+600, "jti":super::super::uuid(),
        "realm_id":realm.id, "tenant_id":session.tenant_id,
        "operation_id":operation.id, "attempt_id":old,
        "slot_id":remote.slot_id, "backup_epoch":remote.backup_epoch,
        "phase":"registered", "registration_version":remote.registration_version,
        "backup_hash":remote.backup_hash, "request_sha256":crypto::hash(b"sealed registration"),
        "record_revision":1,
    });
    let verify = |value: &serde_json::Value, phase: &str| {
        verify_receipt(
            &super::super::tests::sign_jws(value, "awiki-realm-receipt+jwt"),
            &realm,
            &session,
            &operation,
            &attempt,
            &exact,
            phase,
        )
    };
    assert!(verify(&claims, "Register2").is_ok());
    let mut current = claims.clone();
    current["attempt_id"] = attempt.attempt_id.clone().into();
    assert!(verify(&current, "Register2").is_ok());
    let mut unrelated = claims.clone();
    unrelated["attempt_id"] = super::super::uuid().into();
    assert!(verify(&unrelated, "Register2").is_err());
    let mut recovered = claims.clone();
    recovered["phase"] = "recovered".into();
    assert!(verify(&recovered, "Recover3").is_err());
    for (field, replacement) in [
        ("operation_id", serde_json::json!(super::super::uuid())),
        ("slot_id", serde_json::json!("different-slot")),
        (
            "backup_hash",
            serde_json::json!(crypto::hash(b"different-envelope")),
        ),
        (
            "request_sha256",
            serde_json::json!(crypto::hash(b"changed-body")),
        ),
        ("registration_version", serde_json::json!("b2".repeat(16))),
    ] {
        let mut changed = claims.clone();
        changed[field] = replacement;
        assert!(verify(&changed, "Register2").is_err(), "{field}");
    }
}

#[test]
fn old_pending_without_register_attempt_history_remains_readable() {
    let session = super::super::tests::session();
    let operation =
        super::super::new_operation(&session, crate::pin_recovery::PinRecoveryKind::Enable)
            .unwrap();
    let mut value = serde_json::to_value(&operation).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("registration_attempt_ids");
    let restored: Operation = serde_json::from_value(value).unwrap();
    assert!(restored.registration_attempt_ids.is_empty());
}
