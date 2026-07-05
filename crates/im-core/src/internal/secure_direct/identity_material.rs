use anp::PrivateKeyMaterial;
use rusqlite::Connection;
use serde_json::Value;

use super::client::DirectSecureClientInput;

pub(crate) struct DirectSecureIdentityMaterial {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) identity_name: String,
    pub(crate) signing_key_id: String,
    pub(crate) agreement_key_id: String,
    pub(crate) signing_private_pem: String,
    pub(crate) agreement_private_pem: String,
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
            signing_private_pem: self.signing_private_pem.clone(),
            agreement_private_pem: self.agreement_private_pem.clone(),
            local_did_document: self.local_did_document.clone(),
            local_state,
        }
    }
}

pub(crate) struct DirectSecureAgreementMaterial {
    pub(crate) agreement_key_id: String,
    pub(crate) agreement_private_pem: String,
}

pub(crate) fn local_identity_material(
    client: &crate::core::ImClient,
) -> crate::ImResult<DirectSecureIdentityMaterial> {
    let runtime = client.runtime();
    let local_did_document = runtime.key_provider.did_document()?;
    let signing_private_pem = runtime.key_provider.default_signing_private_pem()?;
    let agreement_private_pem = runtime.key_provider.e2ee_agreement_private_pem()?;
    let owner_did = client.did().as_str().to_owned();
    Ok(DirectSecureIdentityMaterial {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: owner_did.clone(),
        identity_name: client
            .current_identity()
            .local_alias
            .clone()
            .unwrap_or_else(|| client.current_identity().id.as_str().to_owned()),
        signing_key_id: format!("{owner_did}#key-1"),
        agreement_key_id: format!("{owner_did}#key-3"),
        signing_private_pem,
        agreement_private_pem,
        local_did_document,
    })
}

pub(crate) fn agreement_material(
    client: &crate::core::ImClient,
) -> crate::ImResult<DirectSecureAgreementMaterial> {
    let owner_did = client.did().as_str().to_owned();
    Ok(DirectSecureAgreementMaterial {
        agreement_key_id: format!("{owner_did}#key-3"),
        agreement_private_pem: client.runtime().key_provider.e2ee_agreement_private_pem()?,
    })
}

pub(crate) fn agreement_private_key(
    client: &crate::core::ImClient,
) -> crate::ImResult<PrivateKeyMaterial> {
    let material = agreement_material(client)?;
    PrivateKeyMaterial::from_pem(&material.agreement_private_pem).map_err(|err| {
        crate::ImError::Serialization {
            detail: format!("parse direct E2EE agreement private key: {err}"),
        }
    })
}

pub(crate) fn local_did_document(client: &crate::core::ImClient) -> crate::ImResult<Value> {
    client.runtime().key_provider.did_document()
}

#[cfg(test)]
mod tests {
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
