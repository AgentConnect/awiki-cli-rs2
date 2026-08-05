//! Durable local-only convergence for access tokens returned by a successful
//! business response. Retrying this record never retries the business RPC.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata};
use crate::internal::secret_vault::SealSecretRequest;

#[derive(Serialize, Deserialize)]
struct PendingAuthCommit {
    origin: String,
    access_token: String,
}

pub(crate) fn stage(
    client: &crate::core::ImClient,
    origin: &str,
    access_token: &str,
) -> crate::ImResult<()> {
    let context = client.core_inner().identity_vault().ok_or_else(|| {
        crate::ImError::LocalStateUnavailable {
            detail: "auth convergence requires Vault storage".to_owned(),
        }
    })?;
    let identity = client.current_identity();
    let key_id = key_id(identity.id.as_str(), origin);
    context.vault().seal(SealSecretRequest {
        metadata: SecretMetadata {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            identity_id: Some(identity.id.as_str().to_owned()),
            did: Some(identity.did.as_str().to_owned()),
            kind: SecretKind::IdentityAuthCommitPending,
            key_id,
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        },
        plaintext: crate::internal::platform_secret::SecretBytes::from_vec(
            serde_json::to_vec(&PendingAuthCommit {
                origin: origin.to_owned(),
                access_token: access_token.to_owned(),
            })
            .map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?,
        ),
    })?;
    Ok(())
}

pub(crate) fn drain(client: &crate::core::ImClient) -> crate::ImResult<Vec<(String, String)>> {
    let Some(context) = client.core_inner().identity_vault() else {
        return Ok(Vec::new());
    };
    let identity_id = client.current_identity().id.as_str();
    let vault = context.vault();
    let refs = vault
        .list()?
        .into_iter()
        .filter(|secret_ref| {
            secret_ref.kind == SecretKind::IdentityAuthCommitPending
                && secret_ref.identity_id.as_deref() == Some(identity_id)
        })
        .collect::<Vec<_>>();
    let mut committed = Vec::new();
    for secret_ref in refs {
        let opened = vault.open(&secret_ref)?;
        let pending: PendingAuthCommit = serde_json::from_slice(opened.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        if client
            .runtime()
            .key_provider
            .persist_auth_token(&pending.access_token)
            .is_ok()
        {
            vault.delete(&secret_ref)?;
            committed.push((pending.origin, pending.access_token));
        }
    }
    Ok(committed)
}

fn key_id(identity_id: &str, origin: &str) -> String {
    let digest = Sha256::digest(format!("{identity_id}\0{origin}").as_bytes());
    format!("auth-commit-{}", URL_SAFE_NO_PAD.encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_auth_commit_survives_client_restart_and_drains_without_business_rpc() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::core::ImCore::new_with_options(
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
                    registry_path: root.path().join("identities").join("registry.json"),
                    default_identity_path: Some(root.path().join("identities").join("default")),
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: root.path().join("local").join("im.sqlite"),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
            crate::core::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::core::IdentitySecretStoragePolicy::VaultRequired,
                crate::core::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([61_u8; 32]),
                    root.path().join("vault"),
                    "workspace-1",
                    "vault-device-1",
                ),
            ),
        )
        .unwrap();
        let material = crate::identity::HostedIdentityMaterial {
            identity_id: "identity-1".to_owned(),
            did: "did:example:alice".to_owned(),
            handle: None,
            display_name: Some("Alice".to_owned()),
            did_document: serde_json::json!({"id": "did:example:alice"}),
            default_signing_private_key_pem: "unused-signing-secret".to_owned(),
            e2ee_agreement_private_key_pem: None,
            auth_token: None,
        };
        let first_client = core
            .client_with_identity_material(material.clone())
            .unwrap();

        stage(
            &first_client,
            "https://example.test/user-service/v1/did-auth/rpc",
            "fresh-access-token",
        )
        .unwrap();
        drop(first_client);

        let restarted_client = core.client_with_identity_material(material).unwrap();
        let committed = drain(&restarted_client).unwrap();

        assert_eq!(
            committed,
            vec![(
                "https://example.test/user-service/v1/did-auth/rpc".to_owned(),
                "fresh-access-token".to_owned()
            )]
        );
        assert_eq!(
            restarted_client
                .runtime()
                .key_provider
                .valid_auth_token()
                .unwrap()
                .as_deref(),
            Some("fresh-access-token")
        );
        assert!(core
            .inner()
            .identity_vault()
            .unwrap()
            .vault()
            .list()
            .unwrap()
            .is_empty());
    }
}
