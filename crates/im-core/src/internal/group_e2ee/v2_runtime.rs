//! Thin im-core boundary for the persistent ANP P6 v2 OpenMLS operations.
//!
//! Product orchestration remains responsible for P4 membership/owner policy,
//! message-service CAS, KeyPackage leasing, and device-targeted delivery. This
//! wrapper scopes storage from the authoritative active vNext device
//! authorization and maps typed local cryptographic operations into the
//! im-core error model.

use anp::group_e2ee::operations::v2::{
    self, V2AcceptKeyPackagePublishInput, V2AddMemberInput, V2CreateGroupInput, V2DecryptInput,
    V2DecryptOutput, V2FinalizeInput, V2FinalizeOutput, V2GenerateKeyPackageInput,
    V2InspectLocalGroupInput, V2InspectLocalGroupOutput, V2ListLocalGroupMemberEndpointsOutput,
    V2MarkTerminalIntentInput, V2PrepareKeyPackagePublishInput, V2PreparedAdd, V2PreparedCreate,
    V2PreparedKeyPackagePublish, V2PreparedRemove, V2ProcessCommitInput, V2ProcessCommitOutput,
    V2ProcessNoticeInput, V2ProcessNoticeOutput, V2ProcessWelcomeInput, V2ReconcilePendingInput,
    V2ReconcilePendingOutput, V2RemoveMemberInput,
};
use anp::group_e2ee::storage::{GroupMlsOwnerScope, GroupMlsStore, ImCoreSqliteGroupMlsStore};
use anp::group_e2ee::{V2GroupCipherObject, V2GroupKeyPackage};
use anp::PrivateKeyMaterial;
use serde_json::Value;

use super::provider::map_group_mls_error;

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeV2Runtime {
    store: ImCoreSqliteGroupMlsStore,
}

impl GroupE2eeV2Runtime {
    pub(crate) fn new(store: ImCoreSqliteGroupMlsStore) -> Self {
        Self { store }
    }

    pub(crate) fn owner_scope(&self) -> crate::ImResult<GroupMlsOwnerScope> {
        self.store
            .owner_scope()
            .ok_or_else(|| crate::ImError::Internal {
                message: "P6 v2 runtime requires a device-scoped OwnerScope".to_owned(),
            })
    }

    pub(crate) fn generate_key_package(
        &self,
        input: V2GenerateKeyPackageInput,
        did_document: &Value,
        device_signing_private_key: &PrivateKeyMaterial,
    ) -> crate::ImResult<V2GroupKeyPackage> {
        v2::generate_key_package_v2(&self.store, input, did_document, device_signing_private_key)
            .map_err(map_group_mls_error)
    }

    pub(crate) fn prepare_or_resume_key_package_publish(
        &self,
        input: V2PrepareKeyPackagePublishInput,
        did_document: &Value,
        device_signing_private_key: &PrivateKeyMaterial,
    ) -> crate::ImResult<V2PreparedKeyPackagePublish> {
        v2::prepare_or_resume_key_package_publish_v2(
            &self.store,
            input,
            did_document,
            device_signing_private_key,
        )
        .map_err(map_group_mls_error)
    }

    pub(crate) fn accept_key_package_publish(
        &self,
        input: V2AcceptKeyPackagePublishInput,
    ) -> crate::ImResult<V2PreparedKeyPackagePublish> {
        v2::accept_key_package_publish_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn create_group_prepare(
        &self,
        input: V2CreateGroupInput,
    ) -> crate::ImResult<V2PreparedCreate> {
        v2::create_group_prepare_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn add_member_prepare(
        &self,
        input: V2AddMemberInput,
    ) -> crate::ImResult<V2PreparedAdd> {
        v2::add_member_prepare_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn remove_member_prepare(
        &self,
        input: V2RemoveMemberInput,
    ) -> crate::ImResult<V2PreparedRemove> {
        v2::remove_member_prepare_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn finalize_commit(
        &self,
        input: V2FinalizeInput,
    ) -> crate::ImResult<V2FinalizeOutput> {
        v2::finalize_commit_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn abort_commit(&self, input: V2FinalizeInput) -> crate::ImResult<V2FinalizeOutput> {
        v2::abort_commit_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn process_welcome(
        &self,
        input: V2ProcessWelcomeInput,
    ) -> crate::ImResult<V2ProcessCommitOutput> {
        v2::process_welcome_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn process_commit(
        &self,
        input: V2ProcessCommitInput,
    ) -> crate::ImResult<V2ProcessCommitOutput> {
        v2::process_commit_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn process_notice(
        &self,
        input: V2ProcessNoticeInput,
    ) -> crate::ImResult<V2ProcessNoticeOutput> {
        v2::process_notice_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn mark_terminal_intent(
        &self,
        input: V2MarkTerminalIntentInput,
    ) -> crate::ImResult<()> {
        v2::mark_local_group_terminal_intent_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn reconcile_pending(
        &self,
        input: V2ReconcilePendingInput,
    ) -> crate::ImResult<V2ReconcilePendingOutput> {
        v2::reconcile_pending_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn inspect_local_group(
        &self,
        input: V2InspectLocalGroupInput,
    ) -> crate::ImResult<V2InspectLocalGroupOutput> {
        v2::inspect_local_group_v2(&self.store, input).map_err(map_group_mls_error)
    }

    /// Returns the secret-free endpoint projection of the accepted local MLS tree.
    ///
    /// This is local inspection only. Callers must independently resolve the
    /// current P2 Manifest and P4 business membership before planning changes.
    pub(crate) fn list_local_group_member_endpoints(
        &self,
        input: V2InspectLocalGroupInput,
    ) -> crate::ImResult<V2ListLocalGroupMemberEndpointsOutput> {
        v2::list_local_group_member_endpoints_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn encrypt(
        &self,
        input: v2::V2EncryptInput,
    ) -> crate::ImResult<V2GroupCipherObject> {
        v2::encrypt_v2(&self.store, input).map_err(map_group_mls_error)
    }

    pub(crate) fn decrypt(&self, input: V2DecryptInput) -> crate::ImResult<V2DecryptOutput> {
        v2::decrypt_v2(&self.store, input).map_err(map_group_mls_error)
    }
}

pub(crate) fn mark_terminal_intent_for_client(
    client: &crate::core::ImClient,
    group_did: &str,
    signal: v2::V2TerminalSignal,
) -> crate::ImResult<()> {
    let runtime = runtime_for_client(client)?;
    let scope = runtime.owner_scope()?;
    match runtime.mark_terminal_intent(V2MarkTerminalIntentInput {
        owner_did: scope.owner_did,
        owner_device_id: scope.device_id,
        group_did: group_did.to_owned(),
        signal,
        request_id: format!(
            "p6-v2-terminal-{}",
            crate::internal::wire::common::generate_operation_id()
        ),
    }) {
        Ok(()) => Ok(()),
        Err(crate::ImError::Service {
            code: Some(code), ..
        }) if code == "group.e2ee.state_not_ready" => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn runtime_for_client(
    client: &crate::core::ImClient,
) -> crate::ImResult<GroupE2eeV2Runtime> {
    let identity = client.current_identity();
    let device = super::v2_lifecycle::current_v2_device(client)?;
    let store = ImCoreSqliteGroupMlsStore::from_local_state_sqlite_path(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
        identity.id.as_str(),
        identity.did.as_str(),
        &device.device_id,
    )
    .map_err(|err| crate::ImError::LocalStateUnavailable {
        detail: format!("initialize P6 v2 group MLS store: {err}"),
    })?;
    Ok(GroupE2eeV2Runtime::new(store))
}

#[cfg(test)]
mod tests {
    use anp::authentication::{create_did_wba_document, DidDocumentOptions};
    use anp::group_e2ee::storage::GroupMlsOwnerScope;
    use serde_json::json;

    use super::*;

    #[test]
    fn wrapper_preserves_exact_device_store_scope_errors() {
        let bundle = create_did_wba_document("example.test", DidDocumentOptions::default())
            .expect("test DID");
        let did = bundle.did().expect("test DID id").to_owned();
        let device_id = "device-a";
        let state_path = unique_temp_path("p6-v2-wrapper").join("mls.sqlite");
        let store = ImCoreSqliteGroupMlsStore::new_scoped_state_db(
            &state_path,
            GroupMlsOwnerScope::new("identity-a", did.clone(), device_id).expect("owner scope"),
        );
        let runtime = GroupE2eeV2Runtime::new(store);
        let signing_key = PrivateKeyMaterial::from_pem(&bundle.keys["key-1"].private_key_pem)
            .expect("device signing key");

        let error = runtime
            .generate_key_package(
                V2GenerateKeyPackageInput {
                    owner_did: "did:wba:example.test:agents:other".to_owned(),
                    owner_device_id: device_id.to_owned(),
                    verification_method: format!("{did}#key-1"),
                    key_package_id: "kp-scope-mismatch".to_owned(),
                    issued_at: "2026-07-19T00:00:00Z".to_owned(),
                    expires_at: "2026-08-19T00:00:00Z".to_owned(),
                    now: "2026-07-20T00:00:00Z".to_owned(),
                    draft_extension_negotiated: true,
                    request_id: "req-scope-mismatch".to_owned(),
                },
                &bundle.did_document,
                &signing_key,
            )
            .expect_err("another DID cannot use this device-scoped store");
        assert!(matches!(error, crate::ImError::InvalidInput { .. }));

        if let Some(parent) = state_path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn endpoint_inventory_wrapper_fails_closed_without_local_group_state() {
        let bundle = create_did_wba_document("example.test", DidDocumentOptions::default())
            .expect("test DID");
        let did = bundle.did().expect("test DID id").to_owned();
        let device_id = "device-a";
        let state_path = unique_temp_path("p6-v2-inventory-wrapper").join("mls.sqlite");
        let store = ImCoreSqliteGroupMlsStore::new_scoped_state_db(
            &state_path,
            GroupMlsOwnerScope::new("identity-a", did.clone(), device_id).expect("owner scope"),
        );
        let runtime = GroupE2eeV2Runtime::new(store);

        let error = runtime
            .list_local_group_member_endpoints(V2InspectLocalGroupInput {
                owner_did: did,
                owner_device_id: device_id.to_owned(),
                group_did: "did:wba:example.test:groups:missing".to_owned(),
                request_id: "req-inventory-missing".to_owned(),
            })
            .expect_err("missing local MLS state must not synthesize an endpoint inventory");
        assert!(matches!(
            error,
            crate::ImError::Internal { ref message }
                if message.contains("group.e2ee.state_not_ready")
        ));

        if let Some(parent) = state_path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn runtime_reloads_exact_device_scope_after_client_rebuild() {
        let fixture = crate::internal::secure_direct::v2_product::v2_product_tests::RuntimeP5TestClientFixture::new(
            "p6-runtime-device-reload",
        );
        let first_client = fixture.client();
        assert_eq!(first_client.current_identity().device_id, None);
        let expected_device_id = first_client
            .core_handle()
            .identities()
            .device_summary(crate::identity::IdentitySelector::Default)
            .unwrap()
            .protocol_device_id
            .unwrap();
        let first_scope = runtime_for_client(&first_client)
            .unwrap()
            .owner_scope()
            .unwrap();
        assert_eq!(
            first_scope.device_id,
            expected_device_id.as_str(),
            "the runtime must not depend on the transient IdentitySummary device_id"
        );
        drop(first_client);

        let rebuilt_client = fixture.client();
        assert_eq!(rebuilt_client.current_identity().device_id, None);
        let rebuilt_scope = runtime_for_client(&rebuilt_client)
            .unwrap()
            .owner_scope()
            .unwrap();
        assert_eq!(rebuilt_scope.device_id, expected_device_id.as_str());
    }

    #[test]
    fn runtime_rejects_legacy_or_missing_device_authorization() {
        for (label, device_state) in [
            (
                "legacy",
                json!({
                    "schema_version": 1,
                    "mode": "legacy"
                }),
            ),
            (
                "missing-authorization",
                json!({
                    "schema_version": 1,
                    "mode": "v_next",
                    "checkpoint": {
                        "document_version": 1,
                        "document_hash": "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                        "registry_version": 1
                    }
                }),
            ),
        ] {
            let fixture = crate::internal::secure_direct::v2_product::v2_product_tests::RuntimeP5TestClientFixture::new(
                &format!("p6-runtime-{label}"),
            );
            let client = fixture.client();
            let registry_path = client
                .core_inner()
                .sdk_paths()
                .identities
                .registry_path
                .clone();

            let mut registry: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
            registry["credentials"]["alice"]["device_state"] = device_state;
            std::fs::write(
                &registry_path,
                serde_json::to_vec_pretty(&registry).unwrap(),
            )
            .unwrap();

            assert!(matches!(
                runtime_for_client(&client),
                Err(crate::ImError::PermissionDenied)
            ));
        }
    }

    fn unique_temp_path(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }
}
