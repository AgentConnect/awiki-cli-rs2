use std::sync::Arc;

use super::ImClient;
use crate::internal::identity_device_state::DeviceAuthorizationRole;
use crate::internal::identity_runtime::{
    ClientIdentityRuntime, LocalOwnerContext, SyncAccountSeed,
};

struct ClientFixture {
    _root: tempfile::TempDir,
    core: crate::ImCore,
}

impl ClientFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "example.test".to_owned(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::IdentityRegistryPaths {
                    identity_root_dir: root.path().join("identities"),
                    registry_path: root.path().join("identities/registry.json"),
                    default_identity_path: None,
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: root.path().join("state/im_core.sqlite"),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
        )
        .unwrap();
        Self { _root: root, core }
    }

    fn client(&self, spec: ClientSpec<'_>) -> ImClient {
        let identity_dir = self._root.path().join("identity-material");
        let did = crate::ids::Did::parse(spec.did).unwrap();
        ImClient::new(
            self.core.inner.clone(),
            ClientIdentityRuntime {
                summary: crate::identity::IdentitySummary {
                    id: crate::ids::IdentityId::parse(spec.identity_id).unwrap(),
                    did: did.clone(),
                    handle: None,
                    display_name: None,
                    local_alias: Some("owner".to_owned()),
                    device_id: Some(spec.protocol_device_id.to_owned()),
                    is_default: true,
                    readiness: crate::identity::IdentityReadiness {
                        ready_for_auth: true,
                        ready_for_messaging: true,
                        missing: Vec::new(),
                    },
                },
                did_document_path: identity_dir.join("did.json"),
                private_key_path: identity_dir.join("private.key"),
                e2ee_agreement_private_key_path: identity_dir.join("e2ee.key"),
                auth_state_path: identity_dir.join("auth.json"),
                key_provider: Arc::new(
                    crate::internal::key_provider::FileBackedIdentitySigner::new(identity_dir),
                ),
                identity_session: None,
                owner: LocalOwnerContext {
                    identity_id: crate::ids::IdentityId::parse(spec.owner_identity_id).unwrap(),
                    current_did: did,
                    sync_account: Some(SyncAccountSeed::new(
                        spec.account_id.to_owned(),
                        crate::ids::ProtocolDeviceId::parse(spec.protocol_device_id).unwrap(),
                        Some(spec.identity_generation.to_owned()),
                        spec.device_auth_generation.to_owned(),
                        format!("{}#signing", spec.did),
                        format!("{}#e2ee", spec.did),
                        spec.role,
                        spec.management_ready,
                    )),
                },
            },
        )
    }
}

#[derive(Clone, Copy)]
struct ClientSpec<'a> {
    identity_id: &'a str,
    owner_identity_id: &'a str,
    did: &'a str,
    account_id: &'a str,
    protocol_device_id: &'a str,
    identity_generation: &'a str,
    device_auth_generation: &'a str,
    role: DeviceAuthorizationRole,
    management_ready: bool,
}

impl Default for ClientSpec<'static> {
    fn default() -> Self {
        Self {
            identity_id: "identity-1",
            owner_identity_id: "identity-1",
            did: "did:example:owner",
            account_id: "account-1",
            protocol_device_id: "device-1",
            identity_generation: "1",
            device_auth_generation: "1",
            role: DeviceAuthorizationRole::Member,
            management_ready: false,
        }
    }
}

#[test]
fn same_owner_refresh_preserves_all_runtime_stores_and_versions() {
    let fixture = ClientFixture::new();
    let current = fixture.client(ClientSpec::default());
    let conversation_store = current.conversation_store();
    let message_store = current.message_store();
    let notification_store = current.system_notification_store();
    let _ = conversation_store.repair_required_patch("seed_version");
    let _ = message_store.repair_required_patch(
        &crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:daemon", "example.test").unwrap(),
        ),
        100,
        "seed_version",
    );
    let conversation_version = conversation_store.version_for_test();
    let message_version = message_store.version_for_test();
    assert!(conversation_version > 0);
    assert!(message_version > 0);

    let refreshed = fixture.client(ClientSpec {
        device_auth_generation: "2",
        role: DeviceAuthorizationRole::Admin,
        management_ready: true,
        ..ClientSpec::default()
    });
    let (refreshed, authorization_changed) = current.refresh_runtime_from(refreshed).unwrap();

    assert!(authorization_changed);
    assert!(Arc::ptr_eq(
        &conversation_store,
        &refreshed.conversation_store()
    ));
    assert!(Arc::ptr_eq(&message_store, &refreshed.message_store()));
    assert!(Arc::ptr_eq(
        &notification_store,
        &refreshed.system_notification_store()
    ));
    assert_eq!(
        refreshed.conversation_store().version_for_test(),
        conversation_version
    );
    assert_eq!(
        refreshed.message_store().version_for_test(),
        message_version
    );
}

#[test]
fn equivalent_same_owner_refresh_does_not_report_authorization_change() {
    let fixture = ClientFixture::new();
    let current = fixture.client(ClientSpec::default());
    let refreshed = fixture.client(ClientSpec::default());

    let (_, authorization_changed) = current.refresh_runtime_from(refreshed).unwrap();

    assert!(!authorization_changed);
}

#[test]
fn refresh_rejects_different_core_owner_did_account_and_device() {
    let fixture = ClientFixture::new();
    let other_fixture = ClientFixture::new();
    let cases = [
        other_fixture.client(ClientSpec::default()),
        fixture.client(ClientSpec {
            identity_id: "identity-2",
            owner_identity_id: "identity-2",
            ..ClientSpec::default()
        }),
        fixture.client(ClientSpec {
            did: "did:example:other",
            ..ClientSpec::default()
        }),
        fixture.client(ClientSpec {
            account_id: "account-2",
            ..ClientSpec::default()
        }),
        fixture.client(ClientSpec {
            protocol_device_id: "device-2",
            ..ClientSpec::default()
        }),
    ];

    for candidate in cases {
        let current = fixture.client(ClientSpec::default());
        assert!(matches!(
            current.refresh_runtime_from(candidate),
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
    }
}
