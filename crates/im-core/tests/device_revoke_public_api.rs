use awiki_im_core::identity::{
    DeviceRevokeRequest, DeviceRevokeResult, DeviceRevokeStatus, IdentitySelector,
};
use awiki_im_core::prelude::{Did, ProtocolDeviceId};
use awiki_im_core::{
    DeviceRevokeOutcomeCategory, ImCore, ImCoreConfig, ImCoreOpenOptions, ImCorePaths,
    LocalStatePaths, MessageTransportPolicy, RuntimePaths, ServiceEndpoint,
};

fn test_core(options: ImCoreOpenOptions) -> (tempfile::TempDir, ImCore) {
    let root = tempfile::tempdir().unwrap();
    let core = ImCore::new_with_options(
        ImCoreConfig {
            service_base_url: ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "awiki.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: MessageTransportPolicy::HttpOnly,
        },
        ImCorePaths {
            identities: awiki_im_core::IdentityRegistryPaths {
                identity_root_dir: root.path().join("identities"),
                registry_path: root.path().join("identities").join("registry.json"),
                default_identity_path: Some(root.path().join("identities").join("default")),
            },
            local_state: LocalStatePaths {
                sqlite_path: root.path().join("local").join("im.sqlite"),
            },
            runtime: RuntimePaths {
                cache_dir: root.path().join("cache"),
                temp_dir: root.path().join("tmp"),
            },
        },
        options,
    )
    .unwrap();
    (root, core)
}

fn request(user_presence_confirmed: bool) -> DeviceRevokeRequest {
    DeviceRevokeRequest {
        identity: IdentitySelector::Default,
        target_device_id: ProtocolDeviceId::parse("dev-target").unwrap(),
        user_presence_confirmed,
    }
}

#[tokio::test]
async fn dedicated_revoke_gate_defaults_off() {
    let defaults = ImCoreOpenOptions::default();
    assert!(!defaults.multi_device_device_revoke_enabled);
    let (_root, core) = test_core(defaults);

    assert_eq!(
        core.device_revoke()
            .revoke(request(true))
            .await
            .unwrap_err(),
        awiki_im_core::ImError::UnsupportedCapability {
            capability: "awiki-device-revoke-disabled".to_owned()
        }
    );
}

#[tokio::test]
async fn user_presence_and_vault_fail_closed_before_identity_or_network_access() {
    let (_root, core) =
        test_core(ImCoreOpenOptions::default().with_multi_device_device_revoke_enabled(true));
    assert_eq!(
        core.device_revoke()
            .revoke(request(false))
            .await
            .unwrap_err(),
        awiki_im_core::ImError::DeviceRevokeOutcome {
            category: DeviceRevokeOutcomeCategory::CancelledBeforeSubmit,
        }
    );
    assert_eq!(
        core.device_revoke()
            .revoke(request(true))
            .await
            .unwrap_err(),
        awiki_im_core::ImError::DeviceRevokeOutcome {
            category: DeviceRevokeOutcomeCategory::RejectedBeforeCommit,
        }
    );
}

#[test]
fn public_result_exposes_only_safe_revocation_projection() {
    let result = DeviceRevokeResult {
        did: Did::parse("did:wba:awiki.test:user:alice:e1_test").unwrap(),
        target_device_id: ProtocolDeviceId::parse("dev-target").unwrap(),
        status: DeviceRevokeStatus::Revoked,
    };
    let value = serde_json::to_value(&result).unwrap();
    let keys = value
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(keys, ["did", "status", "target_device_id"]);
    let rendered = serde_json::to_string(&result).unwrap();
    for forbidden in [
        "operation_id",
        "checkpoint",
        "proof",
        "new_document",
        "private_key",
        "access_token",
        "refresh_token",
    ] {
        assert!(!rendered.contains(forbidden));
    }
}
