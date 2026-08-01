use std::fmt;
use std::sync::Mutex;

use serde_json::Value;

pub(crate) struct HostedKeyMaterialProvider {
    did_document: Value,
    legacy_key1_private_pem: String,
    request_signing_key_id: Option<String>,
    e2ee_agreement_private_pem: Option<String>,
    auth_state: Mutex<crate::internal::auth::state::AuthStateSnapshot>,
}

pub(crate) struct HostBackedDeviceKeyMaterialProvider {
    did_document: Value,
    device_signing_key_id: String,
    device_signing_private_pem: String,
    root_private_pem: String,
    e2ee_agreement_private_pem: String,
    auth_state: Mutex<crate::internal::auth::state::AuthStateSnapshot>,
}

impl HostedKeyMaterialProvider {
    pub(crate) fn new(material: &crate::identity::HostedIdentityMaterial) -> crate::ImResult<Self> {
        Ok(Self {
            did_document: material.did_document.clone(),
            legacy_key1_private_pem: require_non_empty_secret(
                "default_signing_private_key_pem",
                &material.default_signing_private_key_pem,
            )?,
            request_signing_key_id: None,
            e2ee_agreement_private_pem: material
                .e2ee_agreement_private_key_pem
                .as_deref()
                .map(|value| require_non_empty_secret("e2ee_agreement_private_key_pem", value))
                .transpose()?,
            auth_state: Mutex::new(auth_state_from_token(material.auth_token.as_deref())?),
        })
    }

    pub(crate) fn new_for_request_signing_key(
        material: &crate::identity::HostedIdentityMaterial,
        request_signing_key_id: &str,
    ) -> crate::ImResult<Self> {
        let mut provider = Self::new(material)?;
        provider.request_signing_key_id = Some(validate_request_signing_key(
            material,
            request_signing_key_id,
        )?);
        Ok(provider)
    }
}

impl HostBackedDeviceKeyMaterialProvider {
    pub(crate) fn new(
        material: &crate::identity::HostBackedDeviceIdentityMaterial,
    ) -> crate::ImResult<Self> {
        validate_host_backed_device_material(material)?;
        Ok(Self {
            did_document: material.did_document.clone(),
            device_signing_key_id: material.device_signing_key_id.clone(),
            device_signing_private_pem: material.device_signing_private_key_pem.clone(),
            root_private_pem: material.root_private_key_pem.clone(),
            e2ee_agreement_private_pem: material.device_e2ee_private_key_pem.clone(),
            auth_state: Mutex::new(auth_state_from_token(Some(&material.access_token))?),
        })
    }
}

impl fmt::Debug for HostedKeyMaterialProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostedKeyMaterialProvider")
            .field("backend", &"hosted-memory")
            .field("did_document", &"<redacted-hosted-did-document>")
            .field("legacy_key1_private_pem", &"<redacted-private-key>")
            .field("e2ee_agreement_private_pem", &"<redacted-private-key>")
            .field("auth_state", &"<redacted-auth-state>")
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for HostBackedDeviceKeyMaterialProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostBackedDeviceKeyMaterialProvider")
            .field("backend", &"host-backed-device-memory")
            .field("did_document", &"<redacted-hosted-did-document>")
            .field("device_signing_key_id", &self.device_signing_key_id)
            .field("device_signing_private_pem", &"<redacted-private-key>")
            .field("root_private_pem", &"<redacted-private-key>")
            .field("e2ee_agreement_private_pem", &"<redacted-private-key>")
            .field("auth_state", &"<redacted-auth-state>")
            .finish()
    }
}

impl super::KeyMaterialProvider for HostedKeyMaterialProvider {
    fn did_document(&self) -> crate::ImResult<Value> {
        Ok(self.did_document.clone())
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<Value>> {
        Ok(Some(self.did_document.clone()))
    }

    fn device_request_signing_private_pem(&self) -> crate::ImResult<String> {
        Ok(self
            .legacy_key1_role_adapter()
            .device_request_signing_private_pem())
    }

    fn device_request_signing_material(
        &self,
    ) -> crate::ImResult<super::DeviceRequestSigningMaterial> {
        Ok(super::DeviceRequestSigningMaterial {
            key_id: self
                .request_signing_key_id
                .clone()
                .map(Ok)
                .unwrap_or_else(|| super::file::request_signing_key_id(&self.did_document))?,
            private_key_pem: self.device_request_signing_private_pem()?,
        })
    }

    fn did_document_root_private_pem(&self) -> crate::ImResult<String> {
        Ok(self
            .legacy_key1_role_adapter()
            .did_document_root_private_pem())
    }

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String> {
        self.e2ee_agreement_private_pem
            .clone()
            .ok_or_else(|| crate::ImError::IdentityNotReady {
                identity: "hosted-memory".to_owned(),
                missing: vec!["e2ee_agreement_private_key".to_owned()],
            })
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        self.auth_state
            .lock()
            .map_err(|_| crate::ImError::Internal {
                message: "hosted auth state lock poisoned".to_owned(),
            })
            .map(|snapshot| snapshot.clone())
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        Ok(self.auth_state()?.bearer_token)
    }

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        let next = auth_state_from_token(Some(token))?;
        let mut guard = self
            .auth_state
            .lock()
            .map_err(|_| crate::ImError::Internal {
                message: "hosted auth state lock poisoned".to_owned(),
            })?;
        *guard = next;
        Ok(())
    }
}

impl super::KeyMaterialProvider for HostBackedDeviceKeyMaterialProvider {
    fn did_document(&self) -> crate::ImResult<Value> {
        Ok(self.did_document.clone())
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<Value>> {
        Ok(Some(self.did_document.clone()))
    }

    fn device_request_signing_private_pem(&self) -> crate::ImResult<String> {
        Ok(self.device_signing_private_pem.clone())
    }

    fn device_request_signing_material(
        &self,
    ) -> crate::ImResult<super::DeviceRequestSigningMaterial> {
        Ok(super::DeviceRequestSigningMaterial {
            key_id: self.device_signing_key_id.clone(),
            private_key_pem: self.device_signing_private_pem.clone(),
        })
    }

    fn did_document_root_private_pem(&self) -> crate::ImResult<String> {
        Ok(self.root_private_pem.clone())
    }

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String> {
        Ok(self.e2ee_agreement_private_pem.clone())
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        self.auth_state
            .lock()
            .map_err(|_| crate::ImError::Internal {
                message: "host-backed device auth state lock poisoned".to_owned(),
            })
            .map(|snapshot| snapshot.clone())
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        Ok(self.auth_state()?.bearer_token)
    }

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        let next = auth_state_from_token(Some(token))?;
        let mut guard = self
            .auth_state
            .lock()
            .map_err(|_| crate::ImError::Internal {
                message: "host-backed device auth state lock poisoned".to_owned(),
            })?;
        *guard = next;
        Ok(())
    }
}

fn validate_host_backed_device_material(
    material: &crate::identity::HostBackedDeviceIdentityMaterial,
) -> crate::ImResult<()> {
    let did = crate::ids::Did::parse(&material.did)?;
    if material.identity_id.trim().is_empty()
        || material.identity_id.trim() != material.identity_id
        || material.account_id.trim().is_empty()
        || material.account_id.trim() != material.account_id
        || material.did_document.get("id").and_then(Value::as_str) != Some(did.as_str())
        || !anp::authentication::validate_did_document_binding(&material.did_document, true)
        || material.authorization_status
            != crate::identity::IdentityDeviceAuthorizationStatus::Active
        || material.role != crate::identity::IdentityDeviceRole::Admin
        || !material.management_ready
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let binding_generation = anp::wns::BindingGeneration::new(material.binding_generation.clone())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if binding_generation.to_string() != material.binding_generation {
        return Err(crate::ImError::PermissionDenied);
    }
    let auth_generation = material
        .auth_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .filter(|generation| generation.to_string() == material.auth_generation)
        .ok_or(crate::ImError::PermissionDenied)?;

    let manifest = anp::authentication::validate_device_manifest(&material.did_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    let matching_devices = manifest
        .devices
        .iter()
        .filter(|device| device.device_id == material.protocol_device_id.as_str())
        .collect::<Vec<_>>();
    if matching_devices.len() != 1
        || matching_devices[0].signing_key_id != material.device_signing_key_id
        || matching_devices[0].e2ee_key_id != material.device_e2ee_key_id
        || material.root_key_id != format!("{}#key-1", did.as_str())
        || material.root_key_id == material.device_signing_key_id
        || material.root_key_id == material.device_e2ee_key_id
        || material.device_signing_key_id == material.device_e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }

    validate_private_key_binding(
        &material.did_document,
        &material.root_key_id,
        &material.root_private_key_pem,
        PrivateKeyRole::Root,
    )?;
    validate_private_key_binding(
        &material.did_document,
        &material.device_signing_key_id,
        &material.device_signing_private_key_pem,
        PrivateKeyRole::DeviceSigning,
    )?;
    validate_private_key_binding(
        &material.did_document,
        &material.device_e2ee_key_id,
        &material.device_e2ee_private_key_pem,
        PrivateKeyRole::DeviceE2ee,
    )?;
    crate::internal::access_token::validate_device_access_token(
        &material.access_token,
        &crate::internal::access_token::ExpectedDeviceAccess {
            did: did.as_str(),
            user_id: &material.account_id,
            device_id: material.protocol_device_id.as_str(),
            key_id: &material.device_signing_key_id,
            auth_generation,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            management_ready: true,
        },
    )
}

#[derive(Clone, Copy)]
enum PrivateKeyRole {
    Root,
    DeviceSigning,
    DeviceE2ee,
}

fn validate_private_key_binding(
    document: &Value,
    key_id: &str,
    private_key_pem: &str,
    role: PrivateKeyRole,
) -> crate::ImResult<()> {
    let methods = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut matching = methods
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(key_id));
    let method = matching.next().ok_or(crate::ImError::PermissionDenied)?;
    if matching.next().is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    let private = anp::PrivateKeyMaterial::from_pem(private_key_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let algorithm_matches = match role {
        PrivateKeyRole::Root | PrivateKeyRole::DeviceSigning => {
            matches!(&private, anp::PrivateKeyMaterial::Ed25519(_))
        }
        PrivateKeyRole::DeviceE2ee => {
            matches!(&private, anp::PrivateKeyMaterial::X25519(_))
        }
    };
    let public = crate::internal::identity_wire::document::extract_identity_public_key(method)?;
    if !algorithm_matches || private.public_key().to_pem() != public.to_pem() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_request_signing_key(
    material: &crate::identity::HostedIdentityMaterial,
    request_signing_key_id: &str,
) -> crate::ImResult<String> {
    let request_signing_key_id = request_signing_key_id.trim();
    if request_signing_key_id.is_empty()
        || material.did_document.get("id").and_then(Value::as_str) != Some(material.did.as_str())
        || !material
            .did_document
            .get("authentication")
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry.as_str() == Some(request_signing_key_id)
                        || entry.get("id").and_then(Value::as_str) == Some(request_signing_key_id)
                })
            })
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let method = material
        .did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods.iter().find(|method| {
                method.get("id").and_then(Value::as_str) == Some(request_signing_key_id)
            })
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let private_key = anp::PrivateKeyMaterial::from_pem(&material.default_signing_private_key_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let public_key = crate::internal::identity_wire::document::extract_identity_public_key(method)?;
    if private_key.public_key().to_pem() != public_key.to_pem() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(request_signing_key_id.to_owned())
}

impl HostedKeyMaterialProvider {
    fn legacy_key1_role_adapter(&self) -> super::LegacyKey1RoleAdapter {
        super::LegacyKey1RoleAdapter::new(self.legacy_key1_private_pem.clone())
    }
}

fn require_non_empty_secret(field: &'static str, value: &str) -> crate::ImResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(trimmed.to_owned())
}

fn auth_state_from_token(
    token: Option<&str>,
) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
    let Some(token) = token.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(crate::internal::auth::state::AuthStateSnapshot::default());
    };
    let raw = crate::internal::auth::state::auth_state_json_for_token(token)?;
    crate::internal::auth::state::parse_auth_state(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::key_provider::KeyMaterialProvider;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::json;

    #[test]
    fn hosted_key_provider_keeps_secret_material_in_memory_without_debug_leak() {
        let provider = HostedKeyMaterialProvider::new(&crate::identity::HostedIdentityMaterial {
            identity_id: "daemon-agent".to_owned(),
            did: "did:example:daemon".to_owned(),
            handle: Some("daemon.example".to_owned()),
            display_name: None,
            did_document: json!({"id": "did:example:daemon"}),
            default_signing_private_key_pem: "signing-secret".to_owned(),
            e2ee_agreement_private_key_pem: Some("agreement-secret".to_owned()),
            auth_token: Some("token-secret".to_owned()),
        })
        .unwrap();

        assert_eq!(
            provider.device_request_signing_private_pem().unwrap(),
            "signing-secret"
        );
        assert_eq!(
            provider.did_document_root_private_pem().unwrap(),
            "signing-secret"
        );
        assert_eq!(
            provider.e2ee_agreement_private_pem().unwrap(),
            "agreement-secret"
        );
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("token-secret")
        );
        provider.persist_auth_token("fresh-secret").unwrap();
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("fresh-secret")
        );
        let debug = format!("{provider:?}");
        assert!(!debug.contains("signing-secret"));
        assert!(!debug.contains("agreement-secret"));
        assert!(!debug.contains("token-secret"));
        assert!(!debug.contains("fresh-secret"));
        let adapter_debug = format!("{:?}", provider.legacy_key1_role_adapter());
        assert!(!adapter_debug.contains("signing-secret"));
        assert!(adapter_debug.contains("<redacted-private-key>"));
    }

    #[test]
    fn signing_only_hosted_provider_fails_closed_for_e2ee_material() {
        let provider = HostedKeyMaterialProvider::new(&crate::identity::HostedIdentityMaterial {
            identity_id: "delegated-inbox".to_owned(),
            did: "did:example:alice".to_owned(),
            handle: None,
            display_name: None,
            did_document: json!({"id": "did:example:alice"}),
            default_signing_private_key_pem: "signing-secret".to_owned(),
            e2ee_agreement_private_key_pem: None,
            auth_token: None,
        })
        .unwrap();

        assert_eq!(
            provider.device_request_signing_private_pem().unwrap(),
            "signing-secret"
        );
        assert!(matches!(
            provider.e2ee_agreement_private_pem(),
            Err(crate::ImError::IdentityNotReady { missing, .. })
                if missing == vec!["e2ee_agreement_private_key"]
        ));
    }

    #[test]
    fn explicit_request_signing_key_binds_the_matching_hosted_private_key() {
        let generated =
            crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
                "awiki.test",
                "hosted",
                None,
                None,
            )
            .unwrap();
        let material = crate::identity::HostedIdentityMaterial {
            identity_id: generated.unique_id,
            did: generated.did.as_str().to_owned(),
            handle: None,
            display_name: None,
            did_document: generated.did_document,
            default_signing_private_key_pem: generated.device_signing_private_pem,
            e2ee_agreement_private_key_pem: Some(generated.device_e2ee_private_pem),
            auth_token: None,
        };

        let provider = HostedKeyMaterialProvider::new_for_request_signing_key(
            &material,
            &generated.device_signing_key_id,
        )
        .unwrap();
        let signing = provider.device_request_signing_material().unwrap();

        assert_eq!(signing.key_id, generated.device_signing_key_id);
    }

    #[test]
    fn explicit_request_signing_key_accepts_vnext_okp_jwk() {
        let did = "did:wba:awiki.test:user:hosted:e1_root";
        let key_id = format!("{did}#dev-new-sign");
        let private =
            anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[42; 32]));
        let anp::PublicKeyMaterial::Ed25519(public) = private.public_key() else {
            panic!("test requires Ed25519");
        };
        let material = crate::identity::HostedIdentityMaterial {
            identity_id: "e1_root".to_owned(),
            did: did.to_owned(),
            handle: None,
            display_name: None,
            did_document: json!({
                "id": did,
                "verificationMethod": [{
                    "id": key_id,
                    "type": "JsonWebKey2020",
                    "controller": did,
                    "publicKeyJwk": {
                        "kty": "OKP",
                        "crv": "Ed25519",
                        "x": URL_SAFE_NO_PAD.encode(public.to_bytes()),
                    },
                }],
                "authentication": [key_id],
            }),
            default_signing_private_key_pem: private.to_pem(),
            e2ee_agreement_private_key_pem: None,
            auth_token: None,
        };

        let provider =
            HostedKeyMaterialProvider::new_for_request_signing_key(&material, &key_id).unwrap();

        assert_eq!(
            provider.device_request_signing_material().unwrap().key_id,
            key_id,
        );
    }

    #[test]
    fn explicit_request_signing_key_rejects_a_different_hosted_private_key() {
        let generated =
            crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
                "awiki.test",
                "hosted-mismatch",
                None,
                None,
            )
            .unwrap();
        let material = crate::identity::HostedIdentityMaterial {
            identity_id: generated.unique_id,
            did: generated.did.as_str().to_owned(),
            handle: None,
            display_name: None,
            did_document: generated.did_document,
            default_signing_private_key_pem: generated.root_private_pem,
            e2ee_agreement_private_key_pem: Some(generated.device_e2ee_private_pem),
            auth_token: None,
        };

        assert!(matches!(
            HostedKeyMaterialProvider::new_for_request_signing_key(
                &material,
                &generated.device_signing_key_id,
            ),
            Err(crate::ImError::PermissionDenied)
        ));
    }
}
