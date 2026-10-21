use super::*;
use crate::api::client::DartImClient;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use im_core::prelude::*;
use serde_json::json;
use std::time::Duration;

struct Fixture {
    _root: tempfile::TempDir,
    core: ImCore,
    material: VNextAgentBootstrapMaterial,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let core = ImCore::new(
            ImCoreConfig::new(
                ServiceEndpoint::parse("http://127.0.0.1:9").unwrap(),
                "awiki.test",
            )
            .unwrap(),
            ImCorePaths {
                identities: IdentityRegistryPaths {
                    identity_root_dir: root.path().join("identities"),
                    registry_path: root.path().join("registry.json"),
                    default_identity_path: None,
                },
                local_state: LocalStatePaths {
                    sqlite_path: root.path().join("state.sqlite"),
                },
                runtime: RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
        )
        .unwrap();
        let material = core
            .generate_vnext_agent_bootstrap(AgentIdentityKind::Runtime, "capability-subscription")
            .unwrap();
        Self {
            _root: root,
            core,
            material,
        }
    }

    fn client(&self, generation: u64) -> ImClient {
        let b = &self.material;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = json!({
            "iss": "user-service", "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": b.did.as_str(), "type": "access", "purpose": "awiki.device.access.v1",
            "did": b.did.as_str(), "user_id": "test-account", "device_id": b.protocol_device_id.as_str(),
            "key_id": b.device_signing_key_id, "auth_generation": generation,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now - 1, "nbf": now - 1, "exp": now + 3600, "jti": "test-only",
        });
        self.core
            .client_with_device_identity_material(HostBackedDeviceIdentityMaterial {
                identity_id: b.identity_id.clone(),
                did: b.did.as_str().to_owned(),
                handle: Some(format!("{}.awiki.test", b.handle_local_part)),
                display_name: None,
                account_id: "test-account".to_owned(),
                binding_generation: "1".to_owned(),
                did_document: b.did_document.clone(),
                protocol_device_id: b.protocol_device_id.clone(),
                device_signing_key_id: b.device_signing_key_id.clone(),
                device_signing_private_key_pem: b.device_signing_private_key_pem.clone(),
                device_e2ee_key_id: b.device_e2ee_key_id.clone(),
                device_e2ee_private_key_pem: b.device_e2ee_private_key_pem.clone(),
                root_key_id: b.root_key_id.clone(),
                root_private_key_pem: b.root_private_key_pem.clone(),
                authorization_status: IdentityDeviceAuthorizationStatus::Active,
                role: IdentityDeviceRole::Admin,
                management_ready: true,
                auth_generation: generation.to_string(),
                access_token: format!(
                    "e30.{}.test-signature",
                    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
                ),
            })
            .unwrap()
    }
}
async fn next(subscription: &Arc<DartCapabilitySubscription>) -> serde_json::Value {
    let state = tokio::time::timeout(Duration::from_secs(1), next_capability_state(subscription))
        .await
        .expect("subscription must wake without a discovery request")
        .unwrap()
        .unwrap();
    serde_json::from_str(&state).unwrap()
}

#[tokio::test]
async fn existing_subscribers_follow_same_device_authorization_replacement() {
    let fixture = Fixture::new();
    let client = Arc::new(DartImClient::new(fixture.client(1)));
    let identity_id = client.clone_inner().unwrap().current_identity().id.clone();
    let first = subscribe_local_capabilities(&client).unwrap();
    let second = subscribe_local_capabilities(&client).unwrap();
    next(&first).await;
    next(&second).await;
    // Identical authorization (including an ordinary token refresh) keeps the source.
    assert!(!client
        .replace_inner(&identity_id, fixture.client(1))
        .unwrap());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), next_capability_state(&first))
            .await
            .is_err()
    );
    let waiting = tokio::spawn({
        let first = first.clone();
        async move { next(&first).await }
    });
    tokio::task::yield_now().await;
    assert!(client
        .replace_inner(&identity_id, fixture.client(2))
        .unwrap());
    let expected = serde_json::to_value(
        client
            .clone_inner()
            .unwrap()
            .subscribe_backend_capabilities()
            .borrow()
            .clone(),
    )
    .unwrap();
    assert_eq!(waiting.await.unwrap(), expected);
    assert_eq!(next(&second).await, expected);
    // A late response/runtime from the old authorization cannot switch back.
    assert!(client
        .replace_inner(&identity_id, fixture.client(1))
        .is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), next_capability_state(&second))
            .await
            .is_err()
    );
    client.close().unwrap();
    for subscription in [&first, &second] {
        assert!(
            tokio::time::timeout(Duration::from_secs(1), next_capability_state(subscription))
                .await
                .unwrap()
                .unwrap()
                .is_none()
        );
    }
    assert!(subscribe_local_capabilities(&client).is_err());
}

#[tokio::test]
async fn cancelling_one_subscription_does_not_close_other_subscribers() {
    let fixture = Fixture::new();
    let client = Arc::new(DartImClient::new(fixture.client(1)));
    let first = subscribe_local_capabilities(&client).unwrap();
    let second = subscribe_local_capabilities(&client).unwrap();
    next(&first).await;
    next(&second).await;
    close_capability_subscription(&first);
    assert!(next_capability_state(&first).await.unwrap().is_none());
    let id = client.clone_inner().unwrap().current_identity().id.clone();
    client.replace_inner(&id, fixture.client(2)).unwrap();
    next(&second).await;
    client.close().unwrap();
}

#[tokio::test]
async fn rebind_discards_queued_old_results_and_follows_new_source() {
    let fixture = Fixture::new();
    let state = fixture
        .client(1)
        .subscribe_backend_capabilities()
        .borrow()
        .clone();
    let (old, old_rx) = tokio::sync::watch::channel(state.clone());
    let (source, source_rx) = tokio::sync::watch::channel(Some(old_rx));
    let subscription = DartCapabilitySubscription::new_local(source_rx);
    next(&subscription).await;
    let mut stale = state.clone();
    stale.persistence_failed = true;
    old.send_replace(stale.clone());
    let (new, new_rx) = tokio::sync::watch::channel(state.clone());
    source.send_replace(Some(new_rx));
    assert_eq!(
        next(&subscription).await,
        serde_json::to_value(&state).unwrap()
    );
    old.send_replace(stale);
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        next_capability_state(&subscription)
    )
    .await
    .is_err());
    let mut changed = state;
    changed.refresh_error =
        Some(im_core::backend_capabilities::CapabilityRefreshError::Unavailable);
    new.send_replace(changed.clone());
    assert_eq!(
        next(&subscription).await,
        serde_json::to_value(&changed).unwrap()
    );
    drop(source);
    assert!(next_capability_state(&subscription)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn public_subscription_keeps_initial_and_live_updates_and_can_cancel_pending_read() {
    let fixture = Fixture::new();
    let mut state = fixture
        .client(1)
        .subscribe_backend_capabilities()
        .borrow()
        .clone();
    let (sender, receiver) = tokio::sync::watch::channel(state.clone());
    let subscription = DartCapabilitySubscription::new(receiver);
    assert_eq!(
        next(&subscription).await,
        serde_json::to_value(&state).unwrap()
    );
    state.persistence_failed = true;
    sender.send_replace(state.clone());
    assert_eq!(
        next(&subscription).await,
        serde_json::to_value(&state).unwrap()
    );
    let pending = tokio::spawn({
        let subscription = subscription.clone();
        async move { next_capability_state(&subscription).await }
    });
    tokio::task::yield_now().await;
    close_capability_subscription(&subscription);
    assert!(tokio::time::timeout(Duration::from_secs(1), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .is_none());
}
