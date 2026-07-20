use awiki_im_core::identity::{
    HandleRecoveryBeginGrant, HandleRecoveryBeginRequest, HandleRecoveryPhase,
    HandleRecoveryProgress, HandleRecoverySide,
};
use awiki_im_core::{
    IdentityRegistryPaths, IdentitySecretStoragePolicy, ImCore, ImCoreConfig, ImCoreOpenOptions,
    ImCorePaths, LocalStatePaths, RuntimePaths, ServiceEndpoint,
};

fn paths(root: &std::path::Path) -> ImCorePaths {
    ImCorePaths {
        identities: IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities.json"),
            default_identity_path: Some(root.join("default-identity")),
        },
        local_state: LocalStatePaths {
            sqlite_path: root.join("im-core.sqlite3"),
        },
        runtime: RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn config() -> ImCoreConfig {
    ImCoreConfig::new(
        ServiceEndpoint::parse("https://awiki.info").unwrap(),
        "awiki.info",
    )
    .unwrap()
}

#[tokio::test]
async fn handle_recovery_rollout_gate_defaults_off_before_vault_or_network_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let core = ImCore::open(config(), paths(root.path())).await.unwrap();

    let error = core
        .handle_recovery()
        .begin(HandleRecoveryBeginRequest {
            handle: awiki_im_core::ids::Handle::parse("alice.awiki.info", "").unwrap(),
            account_verification_grant: HandleRecoveryBeginGrant::from_token(
                "account-verification-token-must-never-appear",
            )
            .unwrap(),
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        awiki_im_core::ImError::UnsupportedCapability { ref capability }
            if capability == "awiki-handle-recovery-disabled"
    ));
    assert!(!root.path().join("identities").exists());
}

#[test]
fn enabled_handle_recovery_requires_vault_required_before_local_state_access() {
    let root = tempfile::tempdir().unwrap();
    let core = ImCore::new_with_options(
        config(),
        paths(root.path()),
        ImCoreOpenOptions::default().with_multi_device_handle_recovery_enabled(true),
    )
    .unwrap();

    let error = core.handle_recovery().local_sessions().unwrap_err();
    assert!(matches!(
        error,
        awiki_im_core::ImError::LocalStateUnavailable { ref detail }
            if detail.contains("VaultRequired")
    ));
    assert_eq!(
        IdentitySecretStoragePolicy::default(),
        IdentitySecretStoragePolicy::FileCompat
    );
}

#[test]
fn host_progress_projection_excludes_tokens_proofs_keys_and_internal_checkpoints() {
    let progress = HandleRecoveryProgress {
        recovery_session_id: "recovery-safe-id".to_owned(),
        handle: awiki_im_core::ids::Handle::parse("alice.awiki.info", "").unwrap(),
        old_did: awiki_im_core::ids::Did::parse("did:wba:awiki.info:user:alice:e1_old").unwrap(),
        side: HandleRecoverySide::Requester,
        phase: HandleRecoveryPhase::Cooling,
        cooling_until: "2026-07-21T00:00:00Z".to_owned(),
        expires_at: "2026-07-22T00:00:00Z".to_owned(),
        can_cancel_from_this_device: false,
        new_did: None,
        local_activation_pending: false,
    };

    let json = serde_json::to_value(progress).unwrap();
    for forbidden in [
        "account_verification_token",
        "reconfirmation_token",
        "recovery_session_token",
        "bootstrap_device_proof",
        "new_did_document",
        "root_private_key",
        "document_version",
        "document_hash",
        "registry_version",
        "mapping_generation",
    ] {
        assert!(
            json.get(forbidden).is_none(),
            "unexpected field {forbidden}"
        );
    }
}
