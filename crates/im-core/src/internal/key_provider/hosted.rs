use std::fmt;
use std::sync::Mutex;

use serde_json::Value;

pub(crate) struct HostedKeyMaterialProvider {
    did_document: Value,
    default_signing_private_pem: String,
    e2ee_agreement_private_pem: Option<String>,
    auth_state: Mutex<crate::internal::auth::state::AuthStateSnapshot>,
}

impl HostedKeyMaterialProvider {
    pub(crate) fn new(material: &crate::identity::HostedIdentityMaterial) -> crate::ImResult<Self> {
        Ok(Self {
            did_document: material.did_document.clone(),
            default_signing_private_pem: require_non_empty_secret(
                "default_signing_private_key_pem",
                &material.default_signing_private_key_pem,
            )?,
            e2ee_agreement_private_pem: material
                .e2ee_agreement_private_key_pem
                .as_deref()
                .map(|value| require_non_empty_secret("e2ee_agreement_private_key_pem", value))
                .transpose()?,
            auth_state: Mutex::new(auth_state_from_token(material.auth_token.as_deref())?),
        })
    }
}

impl fmt::Debug for HostedKeyMaterialProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostedKeyMaterialProvider")
            .field("backend", &"hosted-memory")
            .field("did_document", &"<redacted-hosted-did-document>")
            .field("default_signing_private_pem", &"<redacted-private-key>")
            .field("e2ee_agreement_private_pem", &"<redacted-private-key>")
            .field("auth_state", &"<redacted-auth-state>")
            .finish_non_exhaustive()
    }
}

impl super::KeyMaterialProvider for HostedKeyMaterialProvider {
    fn did_document(&self) -> crate::ImResult<Value> {
        Ok(self.did_document.clone())
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<Value>> {
        Ok(Some(self.did_document.clone()))
    }

    fn default_signing_private_pem(&self) -> crate::ImResult<String> {
        Ok(self.default_signing_private_pem.clone())
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
            provider.default_signing_private_pem().unwrap(),
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
            provider.default_signing_private_pem().unwrap(),
            "signing-secret"
        );
        assert!(matches!(
            provider.e2ee_agreement_private_pem(),
            Err(crate::ImError::IdentityNotReady { missing, .. })
                if missing == vec!["e2ee_agreement_private_key"]
        ));
    }
}
