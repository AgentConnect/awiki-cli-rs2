use awiki_im_core::identity::{
    DeviceJoinAccountVerificationGrant, DeviceJoinBeginRequest, DeviceJoinLocalPhase,
    DeviceJoinRole, DeviceJoinSessionView, DeviceJoinSide,
};
use awiki_im_core::{
    IdentityRegistryPaths, ImCore, ImCoreConfig, ImCoreOpenOptions, ImCorePaths, LocalStatePaths,
    RuntimePaths, ServiceEndpoint,
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
async fn device_join_rollout_gate_defaults_off_before_state_or_network_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let core = ImCore::open(config(), paths(root.path())).await.unwrap();
    let grant = DeviceJoinAccountVerificationGrant::from_token(
        "account-verification-token-must-never-appear",
    )
    .unwrap();

    let error = core
        .device_join()
        .begin_new_device_join(DeviceJoinBeginRequest {
            operation_id: "join-start-gate-test".to_owned(),
            did: awiki_im_core::ids::Did::parse(
                "did:wba:awiki.info:user:alice:e1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            )
            .unwrap(),
            ttl_seconds: 300,
            account_verification_grant: grant,
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        awiki_im_core::ImError::UnsupportedCapability { ref capability }
            if capability == "awiki-multi-device-join-disabled"
    ));
    assert!(!root.path().join("identities/.device-join").exists());
}

#[tokio::test]
async fn explicitly_enabled_join_surface_can_resume_an_empty_local_store() {
    let root = tempfile::tempdir().unwrap();
    let core = ImCore::open_with_options(
        config(),
        paths(root.path()),
        ImCoreOpenOptions::default().with_multi_device_join_enabled(true),
    )
    .await
    .unwrap();

    assert!(core.device_join().local_sessions().unwrap().is_empty());
}

#[test]
fn join_grant_and_approval_prompt_debug_are_redacted() {
    let token = "account-verification-token-must-never-appear";
    let grant = DeviceJoinAccountVerificationGrant::from_token(token).unwrap();
    let rendered = format!("{grant:?}");
    assert!(!rendered.contains(token));
    assert!(rendered.contains("redacted"));

    let prompt = awiki_im_core::identity::DeviceJoinApprovalPrompt {
        approval_handle: "approval-handle-must-not-appear".to_owned(),
        join_session_id: "join-safe-id".to_owned(),
        role: DeviceJoinRole::Member,
        sas: "123456".to_owned(),
        expires_at: "2026-07-19T12:00:00Z".to_owned(),
    };
    let rendered = format!("{prompt:?}");
    assert!(!rendered.contains("approval-handle-must-not-appear"));
    assert!(!rendered.contains("123456"));
    assert!(rendered.contains("join-safe-id"));
}

#[test]
fn host_session_projection_excludes_internal_join_checkpoints() {
    let view = DeviceJoinSessionView {
        join_session_id: "join-safe-id".to_owned(),
        did: awiki_im_core::ids::Did::parse("did:wba:example.test:alice").unwrap(),
        protocol_device_id: awiki_im_core::ids::ProtocolDeviceId::parse("device-new").unwrap(),
        side: DeviceJoinSide::NewDevice,
        phase: DeviceJoinLocalPhase::Pending,
        expires_at: "2026-07-19T12:00:00Z".to_owned(),
    };

    let json = serde_json::to_value(view).unwrap();
    for forbidden in [
        "join_request_hash",
        "challenge_id",
        "document_version",
        "document_hash",
        "registry_version",
        "auth_generation",
    ] {
        assert!(
            json.get(forbidden).is_none(),
            "unexpected field {forbidden}"
        );
    }
}
