use awiki_im_core as im_core;

fn config() -> im_core::ImCoreConfig {
    im_core::ImCoreConfig {
        service_base_url: im_core::ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "example.test".to_owned(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        ca_bundle: None,
        transport_policy: im_core::MessageTransportPolicy::HttpOnly,
    }
}

fn paths(root: &std::path::Path) -> im_core::ImCorePaths {
    im_core::ImCorePaths {
        identities: im_core::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities/registry.json"),
            default_identity_path: Some(root.join("identities/default")),
        },
        local_state: im_core::LocalStatePaths {
            sqlite_path: root.join("local/im.sqlite"),
        },
        runtime: im_core::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

#[tokio::test]
async fn root_transfer_gate_defaults_off_before_identity_or_vault_access() {
    let root = tempfile::tempdir().unwrap();
    let core = im_core::ImCore::new(config(), paths(root.path())).unwrap();
    let error = core
        .root_key_transfer()
        .send(im_core::identity::RootKeyTransferSendRequest {
            identity: im_core::IdentitySelector::Default,
            recipient_device_id: im_core::ids::ProtocolDeviceId::parse("dev-recipient").unwrap(),
            message_id: im_core::ids::MessageId::parse("root-message-1").unwrap(),
            user_presence_confirmed: true,
        })
        .await
        .unwrap_err();
    assert_eq!(
        error,
        im_core::ImError::UnsupportedCapability {
            capability: "awiki-root-key-transfer-disabled".to_owned()
        }
    );
    assert!(!root.path().join("identities/registry.json").exists());

    let error = core
        .root_key_transfer()
        .list(im_core::identity::RootKeyTransferListRequest {
            identity: im_core::IdentitySelector::Default,
            include_completed: false,
        })
        .await
        .unwrap_err();
    assert_eq!(
        error,
        im_core::ImError::UnsupportedCapability {
            capability: "awiki-root-key-transfer-disabled".to_owned()
        }
    );
}

#[tokio::test]
async fn user_presence_is_rejected_before_identity_lookup_when_gate_is_enabled() {
    let root = tempfile::tempdir().unwrap();
    let core = im_core::ImCore::new_with_options(
        config(),
        paths(root.path()),
        im_core::ImCoreOpenOptions::default().with_multi_device_root_transfer_enabled(true),
    )
    .unwrap();
    let request = im_core::identity::RootKeyTransferSendRequest {
        identity: im_core::IdentitySelector::Default,
        recipient_device_id: im_core::ids::ProtocolDeviceId::parse("dev-recipient").unwrap(),
        message_id: im_core::ids::MessageId::parse("root-message-1").unwrap(),
        user_presence_confirmed: false,
    };
    let debug = format!("{request:?}");
    assert!(!debug.contains("root_private_key"));
    assert!(!debug.contains("document_hash"));
    assert_eq!(
        core.root_key_transfer().send(request).await.unwrap_err(),
        im_core::ImError::PermissionDenied
    );

    let retry = im_core::identity::RootKeyTransferRetryRequest {
        identity: im_core::IdentitySelector::Default,
        message_id: im_core::ids::MessageId::parse("root-message-1").unwrap(),
        user_presence_confirmed: false,
    };
    let debug = format!("{retry:?}");
    assert!(!debug.contains("root_private_key"));
    assert_eq!(
        core.root_key_transfer().retry(retry).await.unwrap_err(),
        im_core::ImError::PermissionDenied
    );
}
