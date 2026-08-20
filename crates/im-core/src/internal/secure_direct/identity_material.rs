use rusqlite::Connection;
use serde_json::Value;
use std::sync::Arc;

use super::client::DirectSecureClientInput;

pub(crate) struct DirectSecureIdentityMaterial {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) identity_name: String,
    pub(crate) signing_key_id: String,
    pub(crate) agreement_key_id: String,
    pub(crate) identity_signer: Arc<dyn crate::internal::key_provider::IdentitySigner>,
    pub(crate) local_did_document: Value,
}

impl DirectSecureIdentityMaterial {
    pub(crate) fn client_input<'a>(
        &self,
        local_state: &'a Connection,
    ) -> DirectSecureClientInput<'a> {
        DirectSecureClientInput {
            owner_identity_id: self.owner_identity_id.clone(),
            owner_did: self.owner_did.clone(),
            identity_name: self.identity_name.clone(),
            signing_key_id: self.signing_key_id.clone(),
            agreement_key_id: self.agreement_key_id.clone(),
            identity_signer: Arc::clone(&self.identity_signer),
            local_did_document: self.local_did_document.clone(),
            local_state,
        }
    }
}

pub(crate) struct DirectSecureAgreementMaterial {
    pub(crate) agreement_key_id: String,
    pub(crate) identity_signer: Arc<dyn crate::internal::key_provider::IdentitySigner>,
}

pub(crate) fn local_identity_material(
    client: &crate::core::ImClient,
) -> crate::ImResult<DirectSecureIdentityMaterial> {
    let runtime = client.runtime();
    let local_did_document = runtime.key_provider.did_document()?;
    let owner_did = client.did().as_str().to_owned();
    let (signing_key_id, agreement_key_id) =
        direct_secure_key_ids(&owner_did, runtime.owner.sync_account.as_ref());
    Ok(DirectSecureIdentityMaterial {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: owner_did.clone(),
        identity_name: client
            .current_identity()
            .local_alias
            .clone()
            .unwrap_or_else(|| client.current_identity().id.as_str().to_owned()),
        signing_key_id,
        agreement_key_id,
        identity_signer: Arc::clone(&runtime.key_provider),
        local_did_document,
    })
}

pub(crate) fn agreement_material(
    client: &crate::core::ImClient,
) -> crate::ImResult<DirectSecureAgreementMaterial> {
    let owner_did = client.did().as_str().to_owned();
    let (_, agreement_key_id) =
        direct_secure_key_ids(&owner_did, client.runtime().owner.sync_account.as_ref());
    Ok(DirectSecureAgreementMaterial {
        agreement_key_id,
        identity_signer: Arc::clone(&client.runtime().key_provider),
    })
}

fn direct_secure_key_ids(
    owner_did: &str,
    sync_account: Option<&crate::internal::identity_runtime::SyncAccountSeed>,
) -> (String, String) {
    sync_account
        .map(|account| {
            (
                account.device_signing_key_id.clone(),
                account.device_e2ee_key_id.clone(),
            )
        })
        .unwrap_or_else(|| (format!("{owner_did}#key-1"), format!("{owner_did}#key-3")))
}

pub(crate) fn local_did_document(client: &crate::core::ImClient) -> crate::ImResult<Value> {
    client.runtime().key_provider.did_document()
}

#[cfg(test)]
mod tests {
    use super::direct_secure_key_ids;

    #[test]
    fn direct_secure_key_ids_use_exact_vnext_device_authorization() {
        let did = "did:wba:awiki.test:user:alice:e1_demo";
        let seed = crate::internal::identity_runtime::SyncAccountSeed::new(
            "account-alice".to_owned(),
            crate::ids::ProtocolDeviceId::parse("dev-device-a").unwrap(),
            Some("1".to_owned()),
            "1".to_owned(),
            format!("{did}#dev-device-a-sign"),
            format!("{did}#dev-device-a-e2ee"),
            crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            true,
        );

        assert_eq!(
            direct_secure_key_ids(did, Some(&seed)),
            (
                format!("{did}#dev-device-a-sign"),
                format!("{did}#dev-device-a-e2ee"),
            )
        );
    }

    #[test]
    fn direct_secure_key_ids_keep_legacy_key_roles_without_device_binding() {
        let did = "did:wba:awiki.test:user:alice:e1_legacy";
        assert_eq!(
            direct_secure_key_ids(did, None),
            (format!("{did}#key-1"), format!("{did}#key-3"))
        );
    }

    #[test]
    fn secure_direct_key_material_scanner_rejects_runtime_secret_path_reads() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let files = [
            "src/internal/secure_direct/prepare.rs",
            "src/internal/secure_direct/send.rs",
            "src/internal/secure_direct/incoming.rs",
            "src/internal/secure_direct/async_send.rs",
            "src/internal/secure_direct/async_receive.rs",
        ];
        let forbidden = [
            "runtime.private_key_path",
            "runtime.e2ee_agreement_private_key_path",
            "runtime.did_document_path",
            ".private_key_path.clone()",
            ".e2ee_agreement_private_key_path.clone()",
            ".did_document_path.clone()",
        ];
        for file in files {
            let path = manifest_dir.join(file);
            let source = std::fs::read_to_string(&path).expect("secure direct source should read");
            for pattern in forbidden {
                assert!(
                    !source.contains(pattern),
                    "{file} must not contain `{pattern}`; use secure_direct::identity_material"
                );
            }
        }
    }
}
