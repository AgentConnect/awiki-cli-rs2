use super::*;
use crate::internal::identity_device_revoke::tests::web_scenario;

struct TestRemote {
    registry: DeviceJoinRemoteRegistry,
    document: Value,
    calls: Vec<wire::PreparedDeviceDocumentUpdate>,
    lose_response: bool,
    reject: bool,
    conflict: bool,
    original_result: Option<IdentityInternalCheckpoint>,
}

impl Remote for TestRemote {
    async fn current(
        &mut self,
        _: &crate::ids::Did,
    ) -> crate::ImResult<(DeviceJoinRemoteRegistry, Value)> {
        Ok((self.registry.clone(), self.document.clone()))
    }
    async fn submit(
        &mut self,
        request: &wire::PreparedDeviceDocumentUpdate,
        _: &crate::ids::Did,
        expected: &IdentityInternalCheckpoint,
    ) -> crate::ImResult<IdentityInternalCheckpoint> {
        self.calls.push(request.clone());
        if self.conflict {
            return Err(crate::ImError::Service {
                status_code: None,
                code: Some("device.document_version_conflict".into()),
                message: "conflict".into(),
                data: None,
            });
        }
        if self.reject {
            return Err(crate::ImError::PermissionDenied);
        }
        if self.original_result.is_none() {
            self.document = request.new_document.clone();
            self.registry.checkpoint = expected.clone();
            self.original_result = Some(expected.clone());
        }
        if self.lose_response {
            self.lose_response = false;
            return Err(crate::ImError::TransportUnavailable {
                detail: "response lost".into(),
            });
        }
        Ok(self.original_result.clone().unwrap())
    }
}

#[tokio::test]
async fn web_service_update_recovers_same_operation_after_response_loss_and_later_updates() {
    let scenario = web_scenario().await;
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    let mut desired = services(&scenario.document, &scenario.did).unwrap();
    desired.push(DidDocumentService {
        id: format!("{}#profile", scenario.did.as_str()),
        service_type: "ProfileService".into(),
        service_endpoint: "https://profile.example/first".into(),
        service_did: None,
        profiles: vec![],
        security_profiles: vec![],
    });
    let mut remote = TestRemote {
        registry: scenario.registry.clone(),
        document: scenario.document.clone(),
        calls: vec![],
        lose_response: true,
        reject: false,
        conflict: false,
        original_result: None,
    };
    assert!(execute(&core, &client, Some(desired.clone()), &mut remote)
        .await
        .is_err());
    assert_eq!(remote.calls.len(), 1);
    assert_eq!(scenario.local_document(), scenario.document);
    let pending = Store::new(&core)
        .unwrap()
        .load(&scenario.did)
        .unwrap()
        .unwrap();
    assert!(!pending.committed);
    assert!(pending.candidate.is_some());
    assert!(remote.calls[0].new_document.get("proof").is_none());
    assert_eq!(
        remote.calls[0].new_document["deviceManifest"],
        scenario.document["deviceManifest"]
    );
    let mut different = desired;
    different.last_mut().unwrap().service_endpoint = "https://profile.example/other".into();
    assert!(execute(&core, &client, Some(different), &mut remote)
        .await
        .is_err());
    assert_eq!(remote.calls.len(), 1);

    // A later legitimate update is different from the original candidate.
    remote.document["alsoKnownAs"] = json!(["https://profile.example/later"]);
    remote.registry.checkpoint.document_version += 1;
    remote.registry.checkpoint.document_hash = document::document_hash(&remote.document).unwrap();
    drop(client);
    drop(core);
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    remote.reject = true;
    assert!(execute(&core, &client, None, &mut remote).await.is_err());
    assert!(has_pending(&core, &scenario.did).unwrap());
    assert_eq!(scenario.local_document(), scenario.document);
    remote.reject = false;
    remote
        .registry
        .devices
        .iter_mut()
        .find(|d| d.device_id == scenario.authorizing.device_id)
        .unwrap()
        .status = DeviceAuthorizationStatus::Revoked;
    assert!(execute(&core, &client, None, &mut remote).await.is_err());
    assert!(
        Store::new(&core)
            .unwrap()
            .load(&scenario.did)
            .unwrap()
            .unwrap()
            .committed
    );
    assert_eq!(scenario.local_document(), scenario.document);
    // Receipt alone cannot activate a revoked device. The current admin check
    // is exercised above; restoring this test authority permits convergence.
    remote
        .registry
        .devices
        .iter_mut()
        .find(|d| d.device_id == scenario.authorizing.device_id)
        .unwrap()
        .status = DeviceAuthorizationStatus::Active;
    // Crash after custody commits but before the local business projection.
    let saved = Store::new(&core)
        .unwrap()
        .load(&scenario.did)
        .unwrap()
        .unwrap();
    crate::internal::identity_device_join::complete_provider_document_change(
        &client,
        saved.candidate.as_ref().unwrap(),
        &saved.result_checkpoint().unwrap(),
    )
    .await
    .unwrap();
    drop(client);
    drop(core);
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    let calls = remote.calls.len();
    let current = execute(&core, &client, None, &mut remote).await.unwrap();
    assert_eq!(remote.calls.len(), calls);
    assert_eq!(current, remote.document);
    assert_eq!(scenario.local_document(), current);
    assert!(!has_pending(&core, &scenario.did).unwrap());
    let public = client
        .runtime()
        .identity_session
        .as_ref()
        .unwrap()
        .public_identity()
        .await
        .unwrap();
    assert_eq!(public.document, current);
    assert_eq!(public.state, ProviderIdentityState::Active);
    assert_eq!(remote.calls[0].operation_id, remote.calls[1].operation_id);
    assert_eq!(remote.calls[0].new_document, remote.calls[1].new_document);
    assert_ne!(
        remote.calls[0].authorizing_device_proof.nonce,
        remote.calls[1].authorizing_device_proof.nonce
    );
}

#[tokio::test]
async fn wba_service_update_preserves_root_and_device_authority() {
    let scenario = crate::internal::identity_device_revoke::tests::provider_scenario(
        crate::identity::DidMethod::Wba,
    )
    .await;
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    let mut desired = services(&scenario.document, &scenario.did).unwrap();
    desired.push(DidDocumentService {
        id: format!("{}#profile", scenario.did.as_str()),
        service_type: "ProfileService".into(),
        service_endpoint: "https://profile.example/wba".into(),
        service_did: None,
        profiles: vec![],
        security_profiles: vec![],
    });
    let mut remote = TestRemote {
        registry: scenario.registry.clone(),
        document: scenario.document.clone(),
        calls: vec![],
        lose_response: false,
        reject: false,
        conflict: false,
        original_result: None,
    };
    let current = execute(&core, &client, Some(desired), &mut remote)
        .await
        .unwrap();
    assert!(current.get("proof").is_some());
    assert!(document::validate_control_document_method(&current));
    for field in [
        "deviceManifest",
        "verificationMethod",
        "authentication",
        "assertionMethod",
        "keyAgreement",
    ] {
        assert_eq!(current[field], scenario.document[field]);
    }
    assert!(!has_pending(&core, &scenario.did).unwrap());
}

#[tokio::test]
async fn service_update_member_or_protected_service_change_does_not_prepare_an_operation() {
    let scenario = web_scenario().await;
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    let desired = services(&scenario.document, &scenario.did).unwrap();
    let mut remote = TestRemote {
        registry: scenario.registry.clone(),
        document: scenario.document.clone(),
        calls: vec![],
        lose_response: false,
        reject: false,
        conflict: false,
        original_result: None,
    };
    remote
        .registry
        .devices
        .iter_mut()
        .find(|d| d.device_id == scenario.authorizing.device_id)
        .unwrap()
        .role = DeviceAuthorizationRole::Member;
    assert!(execute(&core, &client, Some(desired.clone()), &mut remote)
        .await
        .is_err());
    assert!(!has_pending(&core, &scenario.did).unwrap());
    remote.registry = scenario.registry.clone();
    let mut bad = desired;
    bad.push(DidDocumentService {
        id: format!("{}#handle", scenario.did.as_str()),
        service_type: "ANPHandleService".into(),
        service_endpoint: "https://attacker.example/.well-known/handle/other".into(),
        service_did: None,
        profiles: vec![],
        security_profiles: vec![],
    });
    assert!(execute(&core, &client, Some(bad), &mut remote)
        .await
        .is_err());
    assert!(!has_pending(&core, &scenario.did).unwrap());
    assert!(remote.calls.is_empty());
    assert!(client
        .runtime()
        .identity_session
        .as_ref()
        .unwrap()
        .resume_document_change()
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn web_service_conflict_survives_reopen_then_releases_both_pending_records() {
    let scenario = web_scenario().await;
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    let desired = services(&scenario.document, &scenario.did).unwrap();
    let mut remote = TestRemote {
        registry: scenario.registry.clone(),
        document: scenario.document.clone(),
        calls: vec![],
        lose_response: false,
        reject: false,
        conflict: true,
        original_result: None,
    };
    // A conflict without a forward current checkpoint cannot discard custody.
    assert!(execute(&core, &client, Some(desired.clone()), &mut remote)
        .await
        .is_err());
    let pending = Store::new(&core)
        .unwrap()
        .load(&scenario.did)
        .unwrap()
        .unwrap();
    assert!(pending.rejected);
    assert!(client
        .runtime()
        .identity_session
        .as_ref()
        .unwrap()
        .resume_document_change()
        .await
        .unwrap()
        .is_some());
    drop(client);
    drop(core);
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    remote.document["alsoKnownAs"] = json!(["https://example.test/winner"]);
    remote.registry.checkpoint.document_version += 1;
    remote.registry.checkpoint.document_hash = document::document_hash(&remote.document).unwrap();
    // The persisted terminal result skips the old RPC even after restart.
    assert!(execute(&core, &client, None, &mut remote).await.is_err());
    assert_eq!(remote.calls.len(), 1);
    assert!(!has_pending(&core, &scenario.did).unwrap());
    assert!(client
        .runtime()
        .identity_session
        .as_ref()
        .unwrap()
        .resume_document_change()
        .await
        .unwrap()
        .is_none());
    assert_eq!(scenario.local_document(), remote.document);
    remote.conflict = false;
    execute(&core, &client, Some(desired), &mut remote)
        .await
        .unwrap();
    assert_ne!(remote.calls[0].operation_id, remote.calls[1].operation_id);
    assert_eq!(
        remote.calls[1].expected_checkpoint.document_version,
        remote.calls[0].expected_checkpoint.document_version + 1
    );
}

#[tokio::test]
async fn capability_upgrade_keeps_old_profiles_keys_and_resumes_after_response_loss() {
    let scenario =
        crate::internal::identity_device_revoke::tests::provider_scenario_with_old_profiles(
            crate::identity::DidMethod::Wba,
            true,
        )
        .await;
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    let target = scenario.authorizing.device_id.clone();
    let mut remote = TestRemote {
        registry: scenario.registry.clone(),
        document: scenario.document.clone(),
        calls: vec![],
        lose_response: true,
        reject: false,
        conflict: false,
        original_result: None,
    };
    assert!(
        execute_with_target(&core, &client, None, Some(target.clone()), &mut remote)
            .await
            .is_err()
    );
    assert_eq!(scenario.local_document(), scenario.document);
    let request = remote.calls[0].clone();
    if let Ok(path) = std::env::var("AWIKI_CAPABILITY_FIXTURE_OUT") {
        let wire = wire::build_update_call(&request).unwrap();
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({
                "current_document": scenario.document,
                "method": wire.method,
                "params": wire.params,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        wire::build_update_call(&request).unwrap().method,
        "device_capability_update"
    );
    assert_eq!(
        request.capability_target_device_id.as_deref(),
        Some(target.as_str())
    );
    let expected = capability_candidate(&scenario.document, &target).unwrap();
    let mut candidate = request.new_document.clone();
    candidate["proof"] = expected["proof"].clone();
    assert_eq!(candidate, expected);
    // A different operation cannot reuse or erase this uncertain publication.
    assert!(execute(&core, &client, None, &mut remote).await.is_err());
    assert!(execute_with_target(
        &core,
        &client,
        None,
        Some("other-device".into()),
        &mut remote
    )
    .await
    .is_err());
    drop(client);
    drop(core);
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    Store::new(&core)
        .unwrap()
        .load(&scenario.did)
        .expect("pending validates after restart")
        .unwrap();
    let result = execute_with_target(&core, &client, None, Some(target), &mut remote).await;
    assert!(
        result.is_ok(),
        "resume result {result:?}, calls {}, committed {}",
        remote.calls.len(),
        Store::new(&core)
            .unwrap()
            .load(&scenario.did)
            .unwrap()
            .map(|p| p.committed)
            .unwrap_or(false)
    );
    let current = result.unwrap();
    assert_eq!(scenario.local_document(), current);
    assert!(!has_pending(&core, &scenario.did).unwrap());
    assert_eq!(remote.calls[0].operation_id, remote.calls[1].operation_id);
    assert_eq!(
        current["verificationMethod"],
        scenario.document["verificationMethod"]
    );
    assert_eq!(current["service"], scenario.document["service"]);
    assert_eq!(
        remote.registry.checkpoint.registry_version,
        scenario.registry.checkpoint.registry_version
    );
}

#[tokio::test]
async fn capability_upgrade_rejects_stale_cas_without_local_profile_changes() {
    let scenario =
        crate::internal::identity_device_revoke::tests::provider_scenario_with_old_profiles(
            crate::identity::DidMethod::Wba,
            true,
        )
        .await;
    let core = scenario.open_core(false);
    let client = core.client_async(IdentitySelector::Default).await.unwrap();
    let mut remote = TestRemote {
        registry: scenario.registry.clone(),
        document: scenario.document.clone(),
        calls: vec![],
        lose_response: false,
        reject: false,
        conflict: true,
        original_result: None,
    };
    assert!(execute_with_target(
        &core,
        &client,
        None,
        Some(scenario.authorizing.device_id.clone()),
        &mut remote
    )
    .await
    .is_err());
    assert_eq!(scenario.local_document(), scenario.document);
    assert!(has_pending(&core, &scenario.did).unwrap());
    // Only a definitive CAS rejection plus a forward verified checkpoint can
    // release the old candidate. A network error cannot take this path.
    remote.registry.checkpoint.registry_version += 1;
    assert!(execute_with_target(
        &core,
        &client,
        None,
        Some(scenario.authorizing.device_id.clone()),
        &mut remote
    )
    .await
    .is_err());
    assert!(!has_pending(&core, &scenario.did).unwrap());
    assert_eq!(remote.calls.len(), 1);
    remote.conflict = false;
    execute_with_target(
        &core,
        &client,
        None,
        Some(scenario.authorizing.device_id.clone()),
        &mut remote,
    )
    .await
    .unwrap();
}
