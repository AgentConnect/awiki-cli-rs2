use super::*;
use crate::internal::identity_handle_recovery_pending::*;
use crate::internal::pin_recovery::pending::{ExactRequest, Session};
use crate::internal::pin_recovery::tests::{cleanup_test_core, session, sign_jws, target};
use serde_json::json;
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn v4_applied_fixture() -> PendingHandleRecoveryV4 {
    let generated = crate::internal::identity_generation::generate_handle_recovery_identity(
        "control.test",
        "alice",
        None,
        None,
    )
    .unwrap();
    let identity = HandleRecoveryIdentityRef {
        store_id: "v4-fixture-store".into(),
        identity_id: id(),
        did: generated.did,
        did_document: generated.did_document,
        protocol_device_id: generated.protocol_device_id,
        root_key_id: generated.root_key_id,
        device_signing_key_id: generated.device_signing_key_id,
        device_e2ee_key_id: generated.device_e2ee_key_id,
    };
    let old = target();
    let mut p = PendingHandleRecoveryV4::new_pre_otp(
        format!("recover_v4_{}", id()),
        id(),
        "alice".into(),
        "Alice".into(),
        true,
        true,
        old.full_handle.clone(),
        old.did.clone(),
        identity,
    )
    .unwrap();
    p.freeze_exchange(
        RecoveryAuthoritativeBindingV4 {
            account_user_id: "account".into(),
            full_handle: old.full_handle,
            current_did: old.did,
            binding_generation: old.binding_generation,
        },
        "fixture-grant".into(),
        "2099-01-01T00:00:00Z".into(),
    )
    .unwrap();
    p.mark_commit_attempted("2026-10-30T00:00:00Z".into())
        .unwrap();
    let result = RecoveryRemoteResultV4 {
        state: "recovered".into(),
        operation_id: p.operation_id.clone(),
        intent_hash: p.intent_hash.clone().unwrap(),
        intent_schema_version: "1".into(),
        contract_version: V4_CONTRACT_VERSION.into(),
        account_user_id: "account".into(),
        full_handle: p.full_handle.clone(),
        previous_did: p.local_previous_did.clone(),
        current_did: p.identity.did.as_str().into(),
        binding_generation: "184467440737095516161".into(),
        checkpoint: RecoveryCheckpointV4 {
            document_version: 1,
            registry_version: 1,
            document_hash: crate::internal::identity_wire::document::document_hash(
                &p.identity.did_document,
            )
            .unwrap(),
        },
        bootstrap_device: RecoveryBootstrapDeviceV4 {
            device_id: p.identity.protocol_device_id.as_str().into(),
            status: "active".into(),
            role: "admin".into(),
            management_ready: true,
            auth_generation: 1,
        },
        committed_at: "2026-10-30T00:00:01Z".into(),
    };
    p.record_remote_result(result).unwrap();
    p.mark_local_transition_pending().unwrap();
    p.mark_applied().unwrap();
    p
}
fn bound_session() -> Session {
    let mut s = session();
    s.pst = Some(super::super::wire::PhoneSessionResponse {
        access_token: sign_jws(&json!({"sub":"account"}), "awiki-phone-session+jwt"),
        token_type: "DPoP".into(),
        expires_in: 600,
        session_id: id(),
        expires_at: "2099-01-01T00:00:00Z".into(),
    });
    s
}
fn retained_claim(v4: &PendingHandleRecoveryV4, sessions: Vec<String>) -> Claim {
    let old = v4.authoritative_binding.as_ref().unwrap();
    let r = v4.remote_result.as_ref().unwrap();
    Claim {
        v4_operation_id: v4.operation_id.clone(),
        account_user_id: old.account_user_id.clone(),
        previous_did: old.current_did.clone(),
        previous_generation: old.binding_generation.clone(),
        successor_did: r.current_did.clone(),
        successor_generation: r.binding_generation.clone(),
        stage: Stage::Retained,
        request: None,
        isolated_generation: None,
        retained_reason: Some("projected_fixture".into()),
        core_cleared: false,
        session_ids: sessions,
    }
}
#[test]
fn applied_flag_is_in_same_persisted_state_before_first_cleanup() {
    let p = v4_applied_fixture();
    assert!(p.pin_recovery_cleanup_pending);
    let reopened: PendingHandleRecoveryV4 =
        serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    assert!(reopened.pin_recovery_cleanup_pending);
    let mut record =
        crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            p.operation_id.clone(),
            p.owner_identity_id.clone(),
            p.full_handle.clone(),
            "vault-fixture".into(),
            "2026-10-30T00:00:00Z".into(),
        )
        .unwrap();
    record.lifecycle_class =
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::Applied;
    let actions = crate::internal::identity_handle_recovery_context::operation_actions(
        &record,
        &reopened,
        time::OffsetDateTime::now_utc(),
    )
    .unwrap();
    assert!(actions.contains(&crate::identity::HandleRecoveryAction::Resume));
}
#[test]
fn sessions_include_original_resume_and_keep_only_explicit_new_binding() {
    let root = tempfile::tempdir().unwrap();
    let core = cleanup_test_core(root.path());
    let store = Store::new(&core).unwrap();
    let v4 = v4_applied_fixture();
    let mut original = bound_session();
    let mut first =
        super::super::new_operation(&original, crate::pin_recovery::PinRecoveryKind::Recover)
            .unwrap();
    first.account_user_id = Some("account".into());
    let old_request = ExactRequest {
        method: "POST".into(),
        operation_id: Some(first.id.clone()),
        url: "https://control.test/req".into(),
        body_b64u: crypto::b64(b"old-register-payload"),
        idempotency_key: id(),
    };
    original
        .exact_requests
        .insert("realm:old-attempt:Register2".into(), old_request.clone());
    store.save_session(&original).unwrap();
    let mut resumed = original.clone();
    resumed.id = id();
    resumed.resume_operation_id = Some(first.id.clone());
    store.save_session(&resumed).unwrap();
    first.session_id = resumed.id.clone();
    let mut second =
        super::super::new_operation(&resumed, crate::pin_recovery::PinRecoveryKind::Recover)
            .unwrap();
    second.account_user_id = Some("account".into());
    let mut newer = resumed.clone();
    newer.id = id();
    newer.selected.as_mut().unwrap().did = v4.identity.did.as_str().into();
    newer.selected.as_mut().unwrap().binding_generation = v4
        .remote_result
        .as_ref()
        .unwrap()
        .binding_generation
        .clone();
    let current_key = newer.dpop_private.clone();
    let new_id = id();
    newer.exact_requests.insert(
        "realm:new-attempt:Register2".into(),
        ExactRequest {
            method: "POST".into(),
            operation_id: Some(new_id),
            url: "https://control.test/req".into(),
            body_b64u: crypto::b64(b"new-active-material"),
            idempotency_key: id(),
        },
    );
    store.save_session(&newer).unwrap();
    let old_ids = [first.id.clone(), second.id.clone()].into_iter().collect();
    let old = v4.authoritative_binding.as_ref().unwrap();
    let sessions = associated_sessions(
        &store,
        &[first.clone(), second.clone()],
        &old_ids,
        "account",
        &old.current_did,
        &old.binding_generation,
    )
    .unwrap();
    assert!(
        sessions.contains(&original.id)
            && sessions.contains(&resumed.id)
            && sessions.contains(&newer.id)
    );
    first.v4_cleanup = Some(retained_claim(&v4, sessions.clone()));
    second.v4_cleanup = Some(retained_claim(&v4, sessions));
    clear_old(&mut first);
    clear_old(&mut second);
    store.save_operation(&first).unwrap();
    store.save_operation(&second).unwrap();
    clean_sessions(
        &store,
        &old_ids,
        "account",
        &old.current_did,
        &old.binding_generation,
    )
    .unwrap();
    assert!(store.historical_session_for_cleanup(&original.id).is_err());
    assert!(store.historical_session_for_cleanup(&resumed.id).is_err());
    let kept = store.historical_session_for_cleanup(&newer.id).unwrap();
    assert_eq!(kept.dpop_private, current_key);
    assert!(kept.pst.is_some());
    assert_eq!(kept.exact_requests.len(), 1);
    assert!(kept
        .exact_requests
        .contains_key("realm:new-attempt:Register2"));
}
#[test]
fn ambiguous_session_never_gets_a_false_core_cleared_claim() {
    let root = tempfile::tempdir().unwrap();
    let core = cleanup_test_core(root.path());
    let store = Store::new(&core).unwrap();
    let v4 = v4_applied_fixture();
    let mut s = bound_session();
    let mut op =
        super::super::new_operation(&s, crate::pin_recovery::PinRecoveryKind::Recover).unwrap();
    op.account_user_id = Some("account".into());
    s.pst = None;
    store.save_session(&s).unwrap();
    op.v4_cleanup = Some(retained_claim(&v4, vec![s.id.clone()]));
    clear_old(&mut op);
    store.save_operation(&op).unwrap();
    let ids = [op.id.clone()].into_iter().collect();
    let old = v4.authoritative_binding.as_ref().unwrap();
    assert!(clean_sessions(
        &store,
        &ids,
        "account",
        &old.current_did,
        &old.binding_generation
    )
    .is_err());
    assert!(
        !store
            .operation(&op.id)
            .unwrap()
            .v4_cleanup
            .as_ref()
            .unwrap()
            .core_cleared
    );
    assert!(store.historical_session_for_cleanup(&s.id).is_ok());
}
#[tokio::test]
async fn durable_claim_outlives_deleted_v4_journal_and_never_deletes_historical_provider() {
    let root = tempfile::tempdir().unwrap();
    let core = cleanup_test_core(root.path());
    let store = Store::new(&core).unwrap();
    let v4 = v4_applied_fixture();
    let s = bound_session();
    store.save_session(&s).unwrap();
    let mut op =
        super::super::new_operation(&s, crate::pin_recovery::PinRecoveryKind::Recover).unwrap();
    op.account_user_id = Some("account".into());
    op.wrapping_key_b64u = Some(crypto::b64(b"old wrapping material"));
    store.save_operation(&op).unwrap();
    handoff_for_local_deletion(&store, &v4).unwrap();
    assert!(store.operation(&op.id).unwrap().v4_cleanup.is_some());
    // No V4 record or current identity exists; the independent maintenance entry
    // uses only the confirmed handoff. EmptyCustody panics on broad deletion.
    recover(&core).await.unwrap();
    let cleaned = store.operation(&op.id).unwrap();
    assert_eq!(cleaned.phase, PinRecoveryPhase::Cancelled);
    assert!(cleaned.wrapping_key_b64u.is_none());
    assert!(cleaned.v4_cleanup.as_ref().unwrap().done());
    assert!(store.historical_session_for_cleanup(&s.id).is_err());
    recover(&core).await.unwrap();
}
#[test]
fn provider_done_is_not_core_done_before_session_and_vault_write() {
    let v4 = v4_applied_fixture();
    let mut c = retained_claim(&v4, vec![id()]);
    assert!(c.provider_done());
    assert!(!c.done());
    c.core_cleared = true;
    assert!(c.done());
}
#[test]
fn current_history_and_previous_custody_protect_used_namespace() {
    let root = tempfile::tempdir().unwrap();
    let core = cleanup_test_core(root.path());
    let store = Store::new(&core).unwrap();
    let s = bound_session();
    let mut op =
        super::super::new_operation(&s, crate::pin_recovery::PinRecoveryKind::Recover).unwrap();
    let mut v4 = v4_applied_fixture();
    let reference = ProviderIdentityRef {
        store_id: "old-store".into(),
        identity_id: "old-identity".into(),
        did: op.target.did.clone(),
    };
    let paths = &core.inner().sdk_paths().identities;
    std::fs::create_dir_all(&paths.identity_root_dir).unwrap();
    assert!(never_projected(&core, Some(&v4), &op, &reference).unwrap());
    op.recovered_identity = Some(crate::IdentitySelector::Id(
        crate::ids::IdentityId::parse(&id()).unwrap(),
    ));
    assert!(!never_projected(&core, Some(&v4), &op, &reference).unwrap());
    op.recovered_identity = None;
    v4.previous_custody = Some(reference.clone());
    assert!(!never_projected(&core, Some(&v4), &op, &reference).unwrap());
    v4.previous_custody = None;
    let mut index = crate::internal::identity_store::IdentityStore::new(paths)
        .load_index()
        .unwrap();
    let entry:crate::internal::identity_store::IndexEntry=serde_json::from_value(json!({"credential_name":"old","dir_name":op.id,"unique_id":op.id,"did":reference.did,"full_handle":op.target.full_handle,"identity_custody_backend":"anp_identity","anp_identity_store_id":reference.store_id,"anp_identity_id":reference.identity_id})).unwrap();
    index.credentials.insert("old".into(), entry);
    std::fs::write(&paths.registry_path, serde_json::to_vec(&index).unwrap()).unwrap();
    assert!(!never_projected(&core, Some(&v4), &op, &reference).unwrap());
    let backups = paths.identity_root_dir.join(".recovery-registry-backups");
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::write(
        backups.join("history.json"),
        serde_json::to_vec(&index).unwrap(),
    )
    .unwrap();
    index.credentials.clear();
    std::fs::write(&paths.registry_path, serde_json::to_vec(&index).unwrap()).unwrap();
    assert!(!never_projected(&core, Some(&v4), &op, &reference).unwrap());
    std::fs::write(backups.join("history.json"), b"broken historical evidence").unwrap();
    assert!(!never_projected(&core, Some(&v4), &op, &reference).unwrap());
    drop(store);
}
