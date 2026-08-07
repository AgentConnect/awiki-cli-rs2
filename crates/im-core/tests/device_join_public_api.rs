use awiki_im_core::identity::{
    DeviceJoinAccountVerificationGrant, DeviceJoinAuthorizationStatus,
    DeviceJoinAuthorizedDeviceSummary, DeviceJoinLocalPhase, DeviceJoinRole, DeviceJoinSessionView,
    DeviceJoinSide, DeviceRegistryAuthorizedDeviceSummary,
};
use awiki_im_core::{
    IdentityRegistryPaths, ImCore, ImCoreConfig, ImCorePaths, LocalStatePaths, RuntimePaths,
    ServiceEndpoint,
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
async fn join_surface_can_resume_an_empty_local_store() {
    let root = tempfile::tempdir().unwrap();
    let core = ImCore::open(config(), paths(root.path())).await.unwrap();

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

#[test]
fn registry_generation_uses_a_dedicated_public_projection() {
    let did = awiki_im_core::ids::Did::parse("did:wba:example.test:alice").unwrap();
    let protocol_device_id = awiki_im_core::ids::ProtocolDeviceId::parse("device-current").unwrap();
    let join_device = DeviceJoinAuthorizedDeviceSummary {
        protocol_device_id: protocol_device_id.clone(),
        signing_key_id: format!("{}#device-current-sign", did.as_str()),
        e2ee_key_id: format!("{}#device-current-e2ee", did.as_str()),
        status: DeviceJoinAuthorizationStatus::Active,
        role: DeviceJoinRole::Admin,
        management_ready: true,
        is_current: true,
    };
    let registry_device = DeviceRegistryAuthorizedDeviceSummary {
        protocol_device_id,
        signing_key_id: format!("{}#device-current-sign", did.as_str()),
        e2ee_key_id: format!("{}#device-current-e2ee", did.as_str()),
        status: DeviceJoinAuthorizationStatus::Active,
        role: DeviceJoinRole::Admin,
        management_ready: true,
        is_current: true,
        auth_generation: u64::MAX.to_string(),
    };

    let join_json = serde_json::to_value(join_device).unwrap();
    let registry_json = serde_json::to_value(registry_device).unwrap();
    assert!(join_json.get("auth_generation").is_none());
    assert_eq!(
        registry_json
            .get("auth_generation")
            .and_then(serde_json::Value::as_str),
        Some("18446744073709551615")
    );
    assert!(registry_json
        .get("auth_generation")
        .is_some_and(serde_json::Value::is_string));
}

#[test]
fn low_level_join_state_machine_is_not_a_public_rollout_bypass() {
    let identity_module = include_str!("../src/identity/mod.rs");
    let prelude = include_str!("../src/prelude.rs");
    let service = include_str!("../src/identity/join.rs");

    assert!(identity_module.contains("pub(crate) use self::join"));
    assert!(!prelude.contains("DeviceJoinSessionSummary"));
    for method in [
        "start",
        "prepare_admin_challenge",
        "respond_as_new_device",
        "verify_response_as_admin",
        "session",
    ] {
        assert!(
            service.contains(&format!("pub(crate) fn {method}")),
            "low-level method {method} must remain crate-internal"
        );
    }
}
