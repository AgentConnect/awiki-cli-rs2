use super::*;
use crate::internal::identity_custody::{
    controller_custody_provider, provision_handle_recovery_identity_async,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

async fn predecessor(core: &crate::ImCore, path: &[&str]) -> Value {
    let mut create = crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
        "awiki.test",
        "alice",
        None,
        None,
    )
    .unwrap();
    create.spec.path_segments = path.iter().map(|s| (*s).to_owned()).collect();
    controller_custody_provider(core)
        .await
        .unwrap()
        .create_identity(create.spec)
        .await
        .unwrap()
        .public_identity()
        .await
        .unwrap()
        .document
}

// Real Core, custody, Vault, SQLite, WNS parsing and grant exchange. Only the
// remote responses are fixtures; no SMS or real account recovery is performed.
struct Server {
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn start(listener: std::net::TcpListener, binding: Value, exchange_did: String) -> Self {
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = stop.clone();
        let seen = requests.clone();
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("fixture listener: {e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let raw = read_http_request(&mut stream);
                if raw.starts_with("GET /.well-known/handle/alice ") {
                    seen.lock().unwrap().push("wns".into());
                    write_json_response(&mut stream, &binding);
                    continue;
                }
                let body: Value =
                    serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
                if raw.starts_with("POST /user-service/v1/auth/handle-recovery/v4/exchange ") {
                    seen.lock().unwrap().push("exchange".into());
                    write_json_response(
                        &mut stream,
                        &json!({
                            "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                            "recovery_grant": "subject-path-fixture", "purpose": "awiki.identity.handle-recovery.v1",
                            "expires_at": "2099-08-07T00:05:00Z",
                            "current_binding": {"account_user_id": "account-alice", "full_handle": "alice.awiki.test",
                                "current_did": exchange_did, "binding_generation": "7"}
                        }),
                    );
                } else {
                    seen.lock()
                        .unwrap()
                        .push(body["method"].as_str().unwrap().to_owned());
                    write_json_response(
                        &mut stream,
                        &json!({
                            "jsonrpc": "2.0", "id": body["id"],
                            "result": {"ok": true, "retry_after_seconds": 60, "retry_at": "2099-08-07T00:01:00Z"}
                        }),
                    );
                }
            }
        });
        Self {
            stop,
            requests,
            worker: Some(worker),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            if let Err(error) = worker.join() {
                if !std::thread::panicking() {
                    std::panic::resume_unwind(error);
                }
            }
        }
    }
}

fn binding(did: &str) -> Value {
    json!({"handle": "alice.awiki.test", "did": did, "status": "active", "binding_generation": "7"})
}

fn otp_request() -> HandleRecoveryOtpRequest {
    HandleRecoveryOtpRequest {
        identity: None,
        full_handle: "alice.awiki.test".into(),
        phone: "+15555550123".into(),
    }
}

#[tokio::test]
async fn recovery_subject_path_survives_otp_restart_and_grant_exchange() {
    for path in [
        vec!["alice"],
        vec!["user", "alice"],
        vec!["agents", "primary"],
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), &endpoint, [131; 32]);
        let previous = predecessor(&core, &path).await;
        let previous_did = previous["id"].as_str().unwrap();
        let server = Server::start(listener, binding(previous_did), previous_did.to_owned());
        let otp = core
            .handle_recovery()
            .request_handle_recovery_otp(otp_request())
            .await
            .unwrap();
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        let (_, before) = store.load_v4(&otp.operation_id).unwrap().unwrap();
        drop(store);
        drop(core);
        let core = recovery_test_core(root.path(), &endpoint, [131; 32]);
        let resent = core
            .handle_recovery()
            .request_handle_recovery_otp(otp_request())
            .await
            .unwrap();
        assert_eq!(resent.operation_id, otp.operation_id);
        let prepared = core
            .handle_recovery()
            .prepare_handle_recovery(HandleRecoveryPrepareRequest {
                operation_id: otp.operation_id.clone(),
                phone: "+15555550123".into(),
                code: "123456".into(),
            })
            .await
            .unwrap();
        assert_eq!(prepared.phase, HandleRecoveryPhase::ReadyToCommit);
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        let (_, after) = store.load_v4(&otp.operation_id).unwrap().unwrap();
        assert_eq!(after.identity.did, before.identity.did);
        assert_eq!(
            after.identity.device_signing_key_id,
            before.identity.device_signing_key_id
        );
        assert_eq!(after.local_previous_did, previous_did);
        assert!(!after.commit_attempted);
        // Exercise ANP's actual proof and transition validator, the same
        // stable-path contract that rejected the production recovery.
        anp::authentication::verify_active_e1_document(
            after.identity.did.as_str(),
            &after.identity.did_document,
        )
        .unwrap();
        let predecessor = unsigned_recovery_predecessor_document(
            previous.clone(),
            previous_did,
            after.identity.did.as_str(),
        )
        .unwrap();
        let hop = anp::authentication::verify_transition_hop(
            &predecessor,
            &after.identity.did_document,
            Some(&previous),
        )
        .unwrap();
        assert_eq!(
            hop.assurance,
            anp::authentication::TransitionAssurance::Unverified
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.as_str() == "wns")
                .count(),
            1
        );
        assert_eq!(
            controller_custody_provider(&core)
                .await
                .unwrap()
                .list_identities()
                .await
                .unwrap()
                .len(),
            2
        );
    }
}

#[tokio::test]
async fn recovery_subject_path_skips_old_wrong_path_candidate_and_reuses_matching_candidate() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [132; 32]);
    let previous = predecessor(&core, &["alice"]).await;
    let previous_did = previous["id"].as_str().unwrap();
    // Reproduce an unprojected candidate left by the buggy registration-based generator.
    let wrong = crate::internal::identity_custody::provision_handle_recovery_identity(
        &core,
        "awiki.test",
        "alice",
    )
    .unwrap();
    let candidate =
        provision_handle_recovery_identity_async(&core, "awiki.test", "alice", previous_did)
            .await
            .unwrap();
    assert_ne!(candidate.did, wrong.did);
    assert_ne!(candidate.did.as_str(), previous_did);
    let reused =
        provision_handle_recovery_identity_async(&core, "awiki.test", "alice", previous_did)
            .await
            .unwrap();
    assert_eq!(candidate, reused);
    let predecessor =
        unsigned_recovery_predecessor_document(previous.clone(), previous_did, wrong.did.as_str())
            .unwrap();
    assert_eq!(
        anp::authentication::verify_transition_hop(
            &predecessor,
            &wrong.did_document,
            Some(&previous)
        )
        .unwrap_err()
        .kind,
        anp::authentication::TransitionErrorKind::StablePathMismatch
    );
    assert_eq!(
        controller_custody_provider(&core)
            .await
            .unwrap()
            .list_identities()
            .await
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn recovery_subject_path_binding_change_stops_before_freezing_or_committing() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), &endpoint, [133; 32]);
    let previous = predecessor(&core, &["alice"]).await;
    let changed = predecessor(&core, &["user", "alice"]).await;
    let server = Server::start(
        listener,
        binding(previous["id"].as_str().unwrap()),
        changed["id"].as_str().unwrap().to_owned(),
    );
    let otp = core
        .handle_recovery()
        .request_handle_recovery_otp(otp_request())
        .await
        .unwrap();
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let (_, before) = store.load_v4(&otp.operation_id).unwrap().unwrap();
    let error = core
        .handle_recovery()
        .prepare_handle_recovery(HandleRecoveryPrepareRequest {
            operation_id: otp.operation_id.clone(),
            phone: "+15555550123".into(),
            code: "123456".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("state_changed_requires_new_operation")
    );
    let (_, after) = store.load_v4(&otp.operation_id).unwrap().unwrap();
    assert_eq!(before.identity, after.identity);
    assert_eq!(before.local_previous_did, after.local_previous_did);
    assert!(after.intent.is_none());
    assert!(!after.commit_attempted);
    assert_eq!(
        status(&core, &otp.operation_id).unwrap().allowed_actions,
        vec![crate::identity::HandleRecoveryAction::DiscardPreAttempt]
    );
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn recovery_subject_path_invalid_discovery_creates_no_keys_or_operation() {
    for invalid in ["inactive", "wrong_handle", "wrong_domain", "malformed_e1"] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), &endpoint, [134; 32]);
        let did = format!("did:wba:awiki.test:alice:e1_{}", "A".repeat(43));
        let mut doc = binding(&did);
        match invalid {
            "inactive" => doc["status"] = json!("revoked"),
            "wrong_handle" => doc["handle"] = json!("bob.awiki.test"),
            "wrong_domain" => doc["did"] = json!(did.replace("awiki.test", "other.test")),
            _ => doc["did"] = json!("did:wba:awiki.test:alice:e1_invalid"),
        }
        let server = Server::start(listener, doc, did);
        assert!(
            core.handle_recovery()
                .request_handle_recovery_otp(otp_request())
                .await
                .is_err(),
            "{invalid}"
        );
        assert!(PendingHandleRecoveryStore::from_core(&core)
            .unwrap()
            .list_v4_for_handle("alice.awiki.test")
            .unwrap()
            .is_empty());
        assert!(controller_custody_provider(&core)
            .await
            .unwrap()
            .list_identities()
            .await
            .unwrap()
            .is_empty());
        assert_eq!(*server.requests.lock().unwrap(), vec!["wns"]);
    }
}

#[tokio::test]
async fn recovery_subject_path_old_bad_intent_never_resubmits_or_rekeys() {
    for attempted in [false, true] {
        let root = tempfile::tempdir().unwrap();
        // No listener: the guard must complete without any remote request.
        let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [135; 32]);
        let mut pending = create_v4_operation_with_transition_identities(
            &core,
            "recover-bad-subject",
            "owner-bad-subject",
        );
        let previous = predecessor(&core, &["alice"]).await;
        pending
            .freeze_fresh_local_owner(previous["id"].as_str().unwrap())
            .unwrap();
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        let revision = pending.revision;
        pending
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "account-alice".into(),
                    full_handle: pending.full_handle.clone(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "7".into(),
                },
                "fixture-grant".into(),
                "2099-08-07T00:05:00Z".into(),
            )
            .unwrap();
        store.save_v4_cas(&pending, revision).unwrap();
        let sqlite = &core.inner().sdk_paths().local_state.sqlite_path;
        let record = crate::internal::identity_handle_recovery_operation::load(
            sqlite,
            &pending.operation_id,
        )
        .unwrap()
        .unwrap();
        reconcile_frozen_intent_index(sqlite, &record, &pending, &now_second_z().unwrap()).unwrap();
        if attempted {
            let revision = pending.revision;
            pending
                .mark_commit_attempted(now_second_z().unwrap())
                .unwrap();
            store.save_v4_cas(&pending, revision).unwrap();
            crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
                sqlite,
                &pending.operation_id,
                &now_second_z().unwrap(),
            )
            .unwrap();
        }
        let before = pending.clone();
        assert!(!send_commit_v4(&core, &store, &mut pending).await.unwrap());
        let (_, after) = store.load_v4(&pending.operation_id).unwrap().unwrap();
        assert_eq!(after.identity, before.identity);
        assert_eq!(after.intent, before.intent);
        assert_eq!(after.intent_hash, before.intent_hash);
        assert_eq!(after.commit_attempted, attempted);
        assert_eq!(after.phase, before.phase);
        assert_eq!(
            after.last_error_code.as_deref(),
            Some(if attempted {
                "unknown_epoch"
            } else {
                "state_changed_requires_new_operation"
            })
        );
    }
}

#[tokio::test]
async fn recovery_subject_path_local_owner_keeps_alias_and_avoids_discovery() {
    use crate::internal::identity_store::{IndexEntry, IndexPayload};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), &endpoint, [136; 32]);
    let previous = predecessor(&core, &["alice"]).await;
    let previous_did = previous["id"].as_str().unwrap();
    let mut index = IndexPayload::default();
    index.credentials.insert(
        "my-account".into(),
        IndexEntry {
            credential_name: "my-account".into(),
            unique_id: "owner-local-alice".into(),
            did: previous_did.into(),
            full_handle: "alice.awiki.test".into(),
            ..IndexEntry::default()
        },
    );
    let registry = &core.inner().sdk_paths().identities.registry_path;
    std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
    let index_bytes = serde_json::to_vec(&index).unwrap();
    std::fs::write(registry, &index_bytes).unwrap();
    let server = Server::start(listener, binding(previous_did), previous_did.into());
    let otp = core
        .handle_recovery()
        .request_handle_recovery_otp(otp_request())
        .await
        .unwrap();
    let (_, pending) = PendingHandleRecoveryStore::from_core(&core)
        .unwrap()
        .load_v4(&otp.operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(pending.owner_identity_id, "owner-local-alice");
    assert_eq!(pending.local_alias, "my-account");
    assert!(!pending.fresh_local_state);
    assert!(
        crate::internal::identity_generation::handle_recovery_subject_matches(
            previous_did,
            pending.identity.did.as_str()
        )
        .unwrap()
    );
    assert!(!server.requests.lock().unwrap().iter().any(|s| s == "wns"));
    assert_eq!(std::fs::read(registry).unwrap(), index_bytes);
}

#[tokio::test]
async fn recovery_subject_path_preserves_origin_port_and_nested_segments() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [137; 32]);
    let mut create = crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
        "awiki.test",
        "alice",
        None,
        None,
    )
    .unwrap();
    create.spec.port = Some(8443);
    create.spec.path_segments = vec!["agents".into(), "archive".into()];
    let provider = controller_custody_provider(&core).await.unwrap();
    let previous = provider
        .create_identity(create.spec)
        .await
        .unwrap()
        .public_identity()
        .await
        .unwrap();
    let candidate = provision_handle_recovery_identity_async(
        &core,
        "awiki.test",
        "alice",
        &previous.reference.did,
    )
    .await
    .unwrap();
    let old_profile = anp::authentication::parse_did_wba_e1(&previous.reference.did).unwrap();
    let new_profile = anp::authentication::parse_did_wba_e1(candidate.did.as_str()).unwrap();
    assert_eq!(
        old_profile.stable_subject_path,
        new_profile.stable_subject_path
    );
    assert_eq!(old_profile.origin, "awiki.test%3A8443");
    assert!(candidate.did_document["service"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["type"] == "ANPHandleService"
            && s["serviceEndpoint"] == "https://awiki.test/.well-known/handle/alice"));
    anp::authentication::verify_active_e1_document(candidate.did.as_str(), &candidate.did_document)
        .unwrap();
}
