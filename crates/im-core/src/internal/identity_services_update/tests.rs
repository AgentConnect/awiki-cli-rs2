use super::*;
use crate::internal::identity_device_revoke::tests::web_scenario;

struct TestRemote {
    registry: DeviceJoinRemoteRegistry,
    document: Value,
    calls: Vec<wire::PreparedDeviceDocumentUpdate>,
    lose_response: bool,
    reject: bool,
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
