use super::*;
use crate::internal::identity_handle_recovery_operation::{
    self as operations, RecoveryLifecycleClass as Lifecycle,
};
use crate::internal::identity_store::{IdentityStore, IndexEntry};
use crate::internal::identity_transition_pending::{self as transitions, TransitionPhase};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn committed_operation(core: &crate::ImCore, name: &str) -> PendingHandleRecoveryV4 {
    let id = format!("recovery-multi-{name}");
    let owner = format!("owner-{name}");
    let account = format!("account-{name}");
    let mut pending = v4_awaiting_factor_pending(&id, &owner);
    pending.full_handle = format!("{name}.awiki.test");
    pending.local_alias = name.to_owned();
    pending.local_previous_did = format!("did:wba:awiki.test:users:{name}-old");
    pending.fresh_local_state = true;
    pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
        core,
        "awiki.test",
        name,
    )
    .unwrap();
    pending.validate().unwrap();
    let store = PendingHandleRecoveryStore::from_core(core).unwrap();
    store.create_v4(&pending).unwrap();
    let sqlite = &core.inner().sdk_paths().local_state.sqlite_path;
    operations::insert(
        sqlite,
        &operations::RecoveryOperationRecord::pre_commit(
            id.clone(),
            owner,
            pending.full_handle.clone(),
            crate::internal::identity_handle_recovery_pending::pending_v4_key_id(&id),
            "2026-09-24T00:00:00Z".to_owned(),
        )
        .unwrap(),
    )
    .unwrap();
    let revision = pending.revision;
    pending
        .freeze_exchange(
            crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                account_user_id: account.clone(),
                full_handle: pending.full_handle.clone(),
                current_did: pending.local_previous_did.clone(),
                binding_generation: "7".to_owned(),
            },
            "fixture-grant".to_owned(),
            "2099-09-24T00:00:00Z".to_owned(),
        )
        .unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    operations::record_frozen_intent(
        sqlite,
        &id,
        &account,
        pending.intent_hash.as_deref().unwrap(),
        "2026-09-24T00:00:01Z",
    )
    .unwrap();
    operations::mark_commit_attempted(sqlite, &id, "2026-09-24T00:00:02Z").unwrap();
    let revision = pending.revision;
    pending
        .mark_commit_attempted("2026-09-24T00:00:02Z".to_owned())
        .unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let revision = pending.revision;
    pending
        .record_remote_result(remote_result_for_pending(&pending, &account))
        .unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    pending
}

// Real Core/Vault/custody and HTTP parsing; only the remote WNS, auth and
// PreKey service responses are fixtures. No phone, OTP or external service.
struct Server {
    stop: Arc<AtomicBool>,
    fail_publish: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn start(listener: std::net::TcpListener, pending: Vec<PendingHandleRecoveryV4>) -> Self {
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let fail_publish = Arc::new(AtomicBool::new(true));
        let stopped = stop.clone();
        let failing = fail_publish.clone();
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("fixture listener: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let raw = read_http_request(&mut stream);
                if raw.starts_with("GET /.well-known/handle/") {
                    let p = pending
                        .iter()
                        .find(|p| {
                            raw.starts_with(&format!("GET /.well-known/handle/{} ", p.local_alias))
                        })
                        .unwrap();
                    write_json_response(
                        &mut stream,
                        &json!({"handle": p.full_handle, "did": p.identity.did.as_str(), "status": "active", "binding_generation": "8"}),
                    );
                    continue;
                }
                let body: serde_json::Value =
                    serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
                let result = if body["method"] == "get_me" {
                    let p = pending
                        .iter()
                        .find(|p| raw.contains(p.identity.did.as_str()))
                        .unwrap();
                    let now = time::OffsetDateTime::now_utc().unix_timestamp();
                    let account = &p.remote_result.as_ref().unwrap().account_user_id;
                    let claims = json!({
                        "iss":"user-service", "aud":["awiki-user-service","awiki-message-service"],
                        "sub":p.identity.did.as_str(), "type":"access", "purpose":"awiki.device.access.v1",
                        "did":p.identity.did.as_str(), "user_id":account, "device_id":p.identity.protocol_device_id.as_str(),
                        "key_id":p.identity.device_signing_key_id, "auth_generation":1,
                        "scopes":["device:manage","device:read","message:connect"],
                        "iat":now,"nbf":now,"exp":now+300,"jti":"multi-recovery-fixture"
                    });
                    json!({"did":p.identity.did.as_str(), "user_id":account,
                        "access_token":format!("e30.{}.fixture",URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()))})
                } else if body["params"]["body"]["prekey_bundle"].is_object() {
                    if failing.load(Ordering::SeqCst) {
                        write_json_response(
                            &mut stream,
                            &json!({"jsonrpc":"2.0", "id":body["id"], "error":{"code":-32000,"message":"fixture PreKey unavailable"}}),
                        );
                        continue;
                    }
                    let publish = &body["params"]["body"];
                    json!({"published":true, "owner_did":publish["prekey_bundle"]["owner_did"],
                        "owner_device_id":publish["prekey_bundle"]["owner_device_id"],
                        "bundle_id":publish["prekey_bundle"]["bundle_id"], "published_at":"2026-09-24T00:00:05Z",
                        "published_opk_count":publish["one_time_prekeys"].as_array().unwrap().len()})
                } else {
                    panic!("unexpected fixture method: {}", body["method"]);
                };
                write_json_response(
                    &mut stream,
                    &json!({"jsonrpc":"2.0", "id":body["id"], "result":result}),
                );
            }
        });
        Self {
            stop,
            fail_publish,
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

#[tokio::test]
async fn two_legacy_handle_recoveries_resume_through_auth_and_prekeys_in_either_order() {
    for reverse in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let core = recovery_test_core(root.path(), &endpoint, [125; 32]);
        let mut pending = [
            committed_operation(&core, "alice"),
            committed_operation(&core, "bob"),
        ];
        let server = Server::start(listener, pending.to_vec());
        for p in &pending {
            // Stop after the real local identity switch, before PreKey publication finishes.
            let error = resume(
                &core,
                HandleRecoveryResumeRequest {
                    operation_id: p.operation_id.clone(),
                },
            )
            .await
            .unwrap_err();
            assert_eq!(
                service_code(&error),
                Some("local_transition_pending"),
                "{error:?}"
            );
            assert_eq!(
                transitions::load(
                    &core.inner().sdk_paths().local_state.sqlite_path,
                    &p.operation_id
                )
                .unwrap()
                .unwrap()
                .phase,
                TransitionPhase::IdentitySwitched
            );
        }
        let paths = &core.inner().sdk_paths().identities;
        let store = IdentityStore::new(paths);
        let mut index = store.load_index().unwrap();
        for p in &pending {
            let alias = format!("{}-old", p.local_alias);
            index.credentials.insert(
                alias.clone(),
                IndexEntry {
                    credential_name: alias,
                    unique_id: format!("prior-{}", p.owner_identity_id),
                    dir_name: format!("prior-{}", p.owner_identity_id),
                    did: p.local_previous_did.clone(),
                    user_id: p.remote_result.as_ref().unwrap().account_user_id.clone(),
                    handle: p.local_alias.clone(),
                    full_handle: p.full_handle.clone(),
                    binding_generation: Some("7".to_owned()),
                    identity_custody_backend: Some("anp_identity".to_owned()),
                    ..IndexEntry::default()
                },
            );
        }
        index.default_credential_name = "alice-old".to_owned();
        std::fs::write(
            &paths.registry_path,
            serde_json::to_vec_pretty(&index).unwrap(),
        )
        .unwrap();
        store.write_default_identity("alice-old").unwrap();
        server.fail_publish.store(false, Ordering::SeqCst);
        if reverse {
            pending.reverse();
        }
        let error = resume(
            &core,
            HandleRecoveryResumeRequest {
                operation_id: pending[0].operation_id.clone(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            service_code(&error),
            Some("identity.local_registry_conflict")
        );
        assert_eq!(store.load_index().unwrap().credentials.len(), 3);
        assert!(core.identities().list().is_err());
        let sqlite = &core.inner().sdk_paths().local_state.sqlite_path;
        assert_eq!(
            operations::load(sqlite, &pending[0].operation_id)
                .unwrap()
                .unwrap()
                .lifecycle_class,
            Lifecycle::LocalTransitionPending
        );
        // Restart with one repaired Handle and one still conflicting Handle.
        // Both exact operation journals must remain reachable without login.
        drop(core);
        let core = recovery_test_core(root.path(), &endpoint, [125; 32]);
        assert!(core.identities().list().is_err());
        let sqlite = &core.inner().sdk_paths().local_state.sqlite_path;
        for p in [&pending[1], &pending[0]] {
            let context = crate::internal::identity_handle_recovery_context::inspect(
                &core,
                crate::identity::HandleRecoveryContextRequest {
                    full_handle: p.full_handle.clone(),
                    identity: None,
                },
            )
            .await
            .unwrap();
            assert!(context
                .allowed_actions
                .contains(&crate::identity::HandleRecoveryAction::Resume));
            let progress = resume(
                &core,
                HandleRecoveryResumeRequest {
                    operation_id: p.operation_id.clone(),
                },
            )
            .await
            .unwrap();
            assert_eq!(
                progress.phase,
                crate::identity::HandleRecoveryPhase::Applied
            );
            assert_eq!(
                operations::load(sqlite, &p.operation_id)
                    .unwrap()
                    .unwrap()
                    .lifecycle_class,
                Lifecycle::Applied
            );
        }
        assert_eq!(core.identities().list().unwrap().len(), 2);
        drop(core);
        let reopened = recovery_test_core(root.path(), &endpoint, [125; 32]);
        assert_eq!(reopened.identities().list().unwrap().len(), 2);
        for p in &pending {
            let client = reopened
                .client_async(crate::identity::IdentitySelector::Did(
                    p.identity.did.clone(),
                ))
                .await
                .unwrap();
            assert!(client
                .runtime()
                .key_provider
                .valid_auth_token()
                .unwrap()
                .is_some());
        }
    }
}
