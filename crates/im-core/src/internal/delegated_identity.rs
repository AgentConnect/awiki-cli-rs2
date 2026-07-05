use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::Value;

const VAULT_KEY_REF_PREFIX: &str = "vault:";

pub(crate) fn did_from_verification_method(method: &str) -> Option<&str> {
    let method = method.trim();
    let (did, fragment) = method.split_once('#')?;
    if did.trim().is_empty() || fragment.trim().is_empty() {
        return None;
    }
    Some(did)
}

pub(crate) fn require_method_owner(
    owner_did: &str,
    verification_method: &str,
    owner_field: &str,
    method_field: &str,
) -> crate::ImResult<()> {
    let method_owner = did_from_verification_method(verification_method).ok_or_else(|| {
        crate::ImError::invalid_input(
            Some(method_field.to_owned()),
            "verification method must be a DID URL with a fragment",
        )
    })?;
    if owner_did.trim() != method_owner.trim() {
        return Err(crate::ImError::invalid_input(
            Some(owner_field.to_owned()),
            format!("{owner_field} must match the DID part of {method_field}"),
        ));
    }
    Ok(())
}

pub(crate) fn require_authentication_method(
    did_document: &Value,
    verification_method: &str,
    field: &str,
) -> crate::ImResult<()> {
    let method = verification_method.trim();
    let present = did_document
        .get("authentication")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.as_str()
                    .is_some_and(|candidate| candidate.trim() == method)
                    || item
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|candidate| candidate.trim() == method)
            })
        });
    if present {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some(field.to_owned()),
        "verification method must be present in DID Document authentication",
    ))
}

pub(crate) fn load_did_document_for_owner(
    client: &crate::core::ImClient,
    owner_did: &str,
    fallback_current_document: Option<Value>,
) -> crate::ImResult<Value> {
    let owner_did = owner_did.trim();
    if owner_did == client.did().as_str() {
        if let Some(document) = fallback_current_document {
            return Ok(document);
        }
        return client.runtime().key_provider.did_document();
    }
    crate::internal::identity_document_cache::load_local_did_document(
        &client.core_inner().sdk_paths().identities,
        owner_did,
    )?
    .ok_or_else(|| crate::ImError::CredentialFileUnreadable {
        path_kind: "did_document".to_owned(),
        detail: format!("DID Document for {owner_did} is not available locally"),
    })
}

pub(crate) async fn load_did_document_for_owner_async(
    client: &crate::core::ImClient,
    owner_did: &str,
    fallback_current_document: Option<Value>,
) -> crate::ImResult<Value> {
    let owner_did = owner_did.trim();
    if owner_did == client.did().as_str() {
        if let Some(document) = fallback_current_document {
            return Ok(document);
        }
        return client.runtime().key_provider.did_document();
    }
    crate::internal::identity_document_cache::load_local_did_document_async(
        &client.core_inner().sdk_paths().identities,
        owner_did,
    )
    .await?
    .ok_or_else(|| crate::ImError::CredentialFileUnreadable {
        path_kind: "did_document".to_owned(),
        detail: format!("DID Document for {owner_did} is not available locally"),
    })
}

pub(crate) fn load_private_key_ref(
    client: &crate::core::ImClient,
    key_ref: &str,
) -> crate::ImResult<String> {
    if let Some(secret_ref) = vault_key_ref(key_ref)? {
        return open_vault_private_key_ref(client, &secret_ref);
    }
    let path = key_ref_path(client, key_ref)?;
    std::fs::read_to_string(&path).map_err(|err| crate::ImError::CredentialFileUnreadable {
        path_kind: "delegated_private_key".to_owned(),
        detail: format!("{}: {err}", path.display()),
    })
}

pub(crate) async fn load_private_key_ref_async(
    client: &crate::core::ImClient,
    key_ref: &str,
) -> crate::ImResult<String> {
    if let Some(secret_ref) = vault_key_ref(key_ref)? {
        return open_vault_private_key_ref(client, &secret_ref);
    }
    let path = key_ref_path(client, key_ref)?;
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|err| crate::ImError::CredentialFileUnreadable {
            path_kind: "delegated_private_key".to_owned(),
            detail: format!("{}: {err}", path.display()),
        })
}

pub(crate) fn encode_vault_key_ref(
    secret_ref: &crate::vault::SecretRef,
) -> crate::ImResult<String> {
    let raw = serde_json::to_vec(secret_ref).map_err(|err| crate::ImError::Serialization {
        detail: format!("serialize vault key_ref: {err}"),
    })?;
    Ok(format!(
        "{VAULT_KEY_REF_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(raw)
    ))
}

pub fn encode_vault_key_ref_public(
    secret_ref: &crate::vault::SecretRef,
) -> crate::ImResult<String> {
    encode_vault_key_ref(secret_ref)
}

pub(crate) fn delegated_vault_dir_for_client(client: &crate::core::ImClient) -> PathBuf {
    let sqlite_path = &client.core_inner().sdk_paths().local_state.sqlite_path;
    sqlite_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("secrets")
        .join("vault")
}

fn vault_key_ref(key_ref: &str) -> crate::ImResult<Option<crate::vault::SecretRef>> {
    let key_ref = key_ref.trim();
    let Some(encoded) = key_ref.strip_prefix(VAULT_KEY_REF_PREFIX) else {
        return Ok(None);
    };
    if encoded.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("key_ref".to_owned()),
            "vault key_ref must include an encoded SecretRef",
        ));
    }
    let raw = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
        crate::ImError::invalid_input(
            Some("key_ref".to_owned()),
            "vault key_ref must be base64url encoded SecretRef JSON",
        )
    })?;
    let secret_ref: crate::vault::SecretRef =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: format!("parse vault key_ref: {err}"),
        })?;
    validate_delegated_vault_secret_ref(&secret_ref)?;
    Ok(Some(secret_ref))
}

fn open_vault_private_key_ref(
    client: &crate::core::ImClient,
    secret_ref: &crate::vault::SecretRef,
) -> crate::ImResult<String> {
    use crate::vault::{FileSecretVault, FileSecretVaultStore, SecretVault};

    let root_key = crate::internal::secure_direct::secret_store::im_core_vault_root_key_from_env()
        .map_err(|err| crate::ImError::LocalStateUnavailable {
            detail: format!(
                "vault delegated key_ref requires {}; {err}",
                crate::internal::secure_direct::secret_store::IM_CORE_VAULT_ROOT_KEY_ENV
            ),
        })?;
    let vault = FileSecretVault::new(
        root_key,
        FileSecretVaultStore::new(delegated_vault_dir_for_client(client)),
    );
    let opened = vault.open(secret_ref)?;
    let pem = String::from_utf8(opened.expose_secret().to_vec()).map_err(|err| {
        crate::ImError::Serialization {
            detail: format!("vault delegated private key is not valid UTF-8 PEM: {err}"),
        }
    })?;
    anp::PrivateKeyMaterial::from_pem(&pem).map_err(|err| crate::ImError::Serialization {
        detail: format!("vault delegated private key is not valid PEM: {err}"),
    })?;
    Ok(pem)
}

fn validate_delegated_vault_secret_ref(
    secret_ref: &crate::vault::SecretRef,
) -> crate::ImResult<()> {
    match &secret_ref.kind {
        crate::vault::SecretKind::IdentityDaemonPrivate => Ok(()),
        _ => Err(crate::ImError::invalid_input(
            Some("key_ref".to_owned()),
            "vault key_ref must point to a delegated private key secret",
        )),
    }
}

fn key_ref_path(client: &crate::core::ImClient, key_ref: &str) -> crate::ImResult<PathBuf> {
    let key_ref = key_ref.trim();
    if key_ref.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("key_ref".to_owned()),
            "key_ref must not be empty",
        ));
    }
    if let Some(rest) = key_ref.strip_prefix("file:") {
        let path = rest.trim();
        if path.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("key_ref".to_owned()),
                "file key_ref must include a path",
            ));
        }
        return Ok(PathBuf::from(path));
    }
    if let Some(name) = key_ref.strip_prefix("local:") {
        return local_key_ref_path(client, name.trim());
    }
    Ok(PathBuf::from(key_ref))
}

fn local_key_ref_path(client: &crate::core::ImClient, name: &str) -> crate::ImResult<PathBuf> {
    if name.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("key_ref".to_owned()),
            "local key_ref must include a key name",
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(crate::ImError::invalid_input(
            Some("key_ref".to_owned()),
            "local key_ref must not contain path separators",
        ));
    }
    let base = client.runtime().private_key_path.parent().ok_or_else(|| {
        crate::ImError::PathUnavailable {
            path_kind: "private_key".to_owned(),
            detail: "private key path has no parent directory".to_owned(),
        }
    })?;
    let candidates = [
        base.join(name),
        base.join(format!("{name}.pem")),
        base.join(format!("{name}-private.pem")),
        base.join(format!("{name}_private.pem")),
        base.join(format!("{name}.key")),
    ];
    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| crate::ImError::CredentialFileUnreadable {
            path_kind: "delegated_private_key".to_owned(),
            detail: format!("local key_ref {name} was not found near {}", base.display()),
        })
}

fn read_did_document_file(path: &Path, expected_did: &str) -> crate::ImResult<Value> {
    let raw = std::fs::read(path).map_err(|err| crate::ImError::CredentialFileUnreadable {
        path_kind: "did_document".to_owned(),
        detail: err.to_string(),
    })?;
    let document: Value =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    require_document_owner(&document, expected_did)?;
    Ok(document)
}

async fn read_did_document_file_async(path: PathBuf, expected_did: &str) -> crate::ImResult<Value> {
    let raw =
        tokio::fs::read(path)
            .await
            .map_err(|err| crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_owned(),
                detail: err.to_string(),
            })?;
    let document: Value =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    require_document_owner(&document, expected_did)?;
    Ok(document)
}

fn require_document_owner(document: &Value, expected_did: &str) -> crate::ImResult<()> {
    if document.get("id").and_then(Value::as_str) == Some(expected_did) {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some("did_document".to_owned()),
        format!("DID Document id must match {expected_did}"),
    ))
}
