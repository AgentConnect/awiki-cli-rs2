use super::*;

fn expired_prepared(core: &crate::ImCore, operation_id: &str) -> PendingHandleRecoveryV4 {
    let mut pending = v4_awaiting_factor_pending(operation_id, "unprojected-refresh-owner");
    pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
        core,
        "awiki.test",
        "alice",
    )
    .unwrap();
    let previous = pending.local_previous_did.clone();
    pending.freeze_fresh_local_owner(&previous).unwrap();
    let store = PendingHandleRecoveryStore::from_core(core).unwrap();
    store.create_v4(&pending).unwrap();
    let revision = pending.revision;
    pending
        .freeze_exchange(
            crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                account_user_id: "refresh-account".to_owned(),
                full_handle: pending.full_handle.clone(),
                current_did: previous,
                binding_generation: "7".to_owned(),
            },
            "expired-grant".to_owned(),
            "2020-01-01T00:00:00Z".to_owned(),
        )
        .unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let db = &core.inner().sdk_paths().local_state.sqlite_path;
    crate::internal::identity_handle_recovery_operation::insert(
        db,
        &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            operation_id.to_owned(),
            pending.owner_identity_id.clone(),
            pending.full_handle.clone(),
            crate::internal::identity_handle_recovery_pending::pending_v4_key_id(operation_id),
            "2026-09-08T00:00:00Z".to_owned(),
        )
        .unwrap(),
    )
    .unwrap();
    crate::internal::identity_handle_recovery_operation::record_frozen_intent(
        db,
        operation_id,
        "refresh-account",
        pending.intent_hash.as_deref().unwrap(),
        "2026-09-08T00:00:01Z",
    )
    .unwrap();
    pending
}

#[tokio::test]
async fn expired_prepared_refreshes_same_operation_and_survives_reopen() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), &endpoint, [79; 32]);
    let pending = expired_prepared(&core, "recover-v4-precommit-refresh");
    assert_eq!(
        status(&core, &pending.operation_id).unwrap().failure_code,
        Some(HandleRecoveryErrorCode::FactorRetryRequired)
    );
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        write_json_response(
            &mut stream,
            &json!({"jsonrpc":"2.0","id":body["id"],"result":{
                "ok":true,"retry_after_seconds":60,"retry_at":"2099-09-08T00:01:00Z"
            }}),
        );
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        assert!(request.contains("/handle-recovery/v4/exchange"));
        write_json_response(
            &mut stream,
            &json!({
                "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                "recovery_grant":"renewed-grant", "purpose":"awiki.identity.handle-recovery.v1",
                "expires_at":"2099-09-08T00:05:00Z", "current_binding":{
                    "account_user_id":"refresh-account","full_handle":"alice.awiki.test",
                    "current_did":"did:wba:awiki.test:users:alice-old","binding_generation":"7"
                }
            }),
        );
    });
    let otp = request_otp(
        &core,
        HandleRecoveryOtpRequest {
            identity: None,
            full_handle: pending.full_handle.clone(),
            phone: "+15555550100".to_owned(),
        },
    )
    .await
    .unwrap();
    assert_eq!(otp.operation_id, pending.operation_id);
    let refreshed = prepare(
        &core,
        HandleRecoveryPrepareRequest {
            operation_id: pending.operation_id.clone(),
            phone: "+15555550100".to_owned(),
            code: "123456".to_owned(),
        },
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(refreshed.phase, HandleRecoveryPhase::ReadyToCommit);
    assert_eq!(refreshed.failure_code, None);
    drop(core);
    let reopened = recovery_test_core(root.path(), &endpoint, [79; 32]);
    let store = PendingHandleRecoveryStore::from_core(&reopened).unwrap();
    let (_, result) = store.load_v4(&pending.operation_id).unwrap().unwrap();
    assert!(!result.commit_attempted);
    assert_eq!(result.owner_identity_id, pending.owner_identity_id);
    assert_eq!(result.identity.did, pending.identity.did);
    assert_eq!(result.intent_hash, pending.intent_hash);
    assert_eq!(result.authoritative_binding, pending.authoritative_binding);
    assert_eq!(
        status(&reopened, &pending.operation_id)
            .unwrap()
            .failure_code,
        None
    );
}

#[tokio::test]
async fn prepared_refresh_rejects_changed_binding_and_preserves_intent() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), &endpoint, [78; 32]);
    let pending = expired_prepared(&core, "recover-v4-changed-binding");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_http_request(&mut stream);
        write_json_response(
            &mut stream,
            &json!({
                "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                "recovery_grant":"changed-grant", "purpose":"awiki.identity.handle-recovery.v1",
                "expires_at":"2099-09-08T00:05:00Z", "current_binding":{
                    "account_user_id":"refresh-account","full_handle":"alice.awiki.test",
                    "current_did":"did:wba:awiki.test:users:alice-other","binding_generation":"8"
                }
            }),
        );
    });
    let result = prepare(
        &core,
        HandleRecoveryPrepareRequest {
            operation_id: pending.operation_id.clone(),
            phone: "+15555550100".to_owned(),
            code: "123456".to_owned(),
        },
    )
    .await;
    server.join().unwrap();
    assert!(result.is_err());
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let (_, result) = store.load_v4(&pending.operation_id).unwrap().unwrap();
    assert_eq!(result.intent_hash, pending.intent_hash);
    assert_eq!(result.authoritative_binding, pending.authoritative_binding);
    assert!(!result.commit_attempted);
    assert_eq!(
        status(&core, &pending.operation_id).unwrap().failure_code,
        Some(HandleRecoveryErrorCode::UnknownEpoch)
    );
}
