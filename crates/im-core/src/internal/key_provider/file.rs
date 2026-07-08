use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

const DID_DOCUMENT_FILES: &[&str] = &["did.json", "did_document.json"];
const DEFAULT_SIGNING_PRIVATE_FILES: &[&str] = &["private.key", "key-1-private.pem"];
const E2EE_AGREEMENT_PRIVATE_FILES: &[&str] = &["e2ee-agreement-private.pem", "key-3-private.pem"];
const AUTH_STATE_FILE: &str = "auth.json";

pub(crate) struct FileBackedKeyMaterialProvider {
    identity_dir: PathBuf,
}

impl FileBackedKeyMaterialProvider {
    pub(crate) fn new(identity_dir: PathBuf) -> Self {
        Self { identity_dir }
    }

    pub(crate) fn did_document_path(&self) -> PathBuf {
        first_existing_path(&self.identity_dir, DID_DOCUMENT_FILES)
    }

    pub(crate) fn default_signing_private_key_path(&self) -> PathBuf {
        first_existing_path(&self.identity_dir, DEFAULT_SIGNING_PRIVATE_FILES)
    }

    pub(crate) fn e2ee_agreement_private_key_path(&self) -> PathBuf {
        first_existing_path(&self.identity_dir, E2EE_AGREEMENT_PRIVATE_FILES)
    }

    pub(crate) fn auth_state_path(&self) -> PathBuf {
        self.identity_dir.join(AUTH_STATE_FILE)
    }
}

impl fmt::Debug for FileBackedKeyMaterialProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileBackedKeyMaterialProvider")
            .field("identity_dir", &self.identity_dir)
            .field("backend", &"file-backed")
            .finish_non_exhaustive()
    }
}

impl super::KeyMaterialProvider for FileBackedKeyMaterialProvider {
    fn did_document(&self) -> crate::ImResult<Value> {
        read_json_file(&self.did_document_path(), "did_document")
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<Value>> {
        read_optional_json_file(&self.did_document_path(), "did_document")
    }

    fn default_signing_private_pem(&self) -> crate::ImResult<String> {
        read_non_empty_text_file(
            &self.default_signing_private_key_path(),
            "default_signing_private_key",
        )
    }

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String> {
        read_non_empty_text_file(
            &self.e2ee_agreement_private_key_path(),
            "e2ee_agreement_private_key",
        )
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        crate::internal::auth::state::read_auth_state(&self.auth_state_path())
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        crate::internal::auth::state::read_jwt_token(&self.auth_state_path())
    }

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        crate::internal::auth::state::persist_jwt_token(&self.auth_state_path(), token)
    }
}

fn first_existing_path(identity_dir: &Path, names: &[&str]) -> PathBuf {
    names
        .iter()
        .map(|name| identity_dir.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| identity_dir.join(names[0]))
}

fn read_json_file(path: &Path, path_kind: &str) -> crate::ImResult<Value> {
    let raw = std::fs::read(path).map_err(|err| crate::ImError::CredentialFileUnreadable {
        path_kind: path_kind.to_string(),
        detail: err.to_string(),
    })?;
    serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn read_optional_json_file(path: &Path, path_kind: &str) -> crate::ImResult<Option<Value>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: path_kind.to_string(),
                detail: err.to_string(),
            });
        }
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
}

fn read_non_empty_text_file(path: &Path, path_kind: &str) -> crate::ImResult<String> {
    let value =
        std::fs::read_to_string(path).map_err(|err| crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_string(),
            detail: err.to_string(),
        })?;
    if value.trim().is_empty() {
        return Err(crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_string(),
            detail: "file is empty".to_string(),
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::key_provider::KeyMaterialProvider;
    use serde_json::json;

    const TEST_SIGNING_PEM: &str =
        "-----BEGIN PRIVATE KEY-----\nprovider-signing-secret\n-----END PRIVATE KEY-----\n";
    const TEST_AGREEMENT_PEM: &str =
        "-----BEGIN PRIVATE KEY-----\nprovider-agreement-secret\n-----END PRIVATE KEY-----\n";

    #[test]
    fn identity_key_provider_reads_current_file_backed_material() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did_document.json"),
            serde_json::to_vec(&json!({"id": "did:wba:alice.example"})).unwrap(),
        )
        .unwrap();
        std::fs::write(identity_dir.join("key-1-private.pem"), TEST_SIGNING_PEM).unwrap();
        std::fs::write(
            identity_dir.join("e2ee-agreement-private.pem"),
            TEST_AGREEMENT_PEM,
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("auth.json"),
            br#"{"jwt_token":"token-current"}"#,
        )
        .unwrap();

        let provider = FileBackedKeyMaterialProvider::new(identity_dir);

        assert_eq!(
            provider
                .did_document()
                .unwrap()
                .get("id")
                .and_then(Value::as_str),
            Some("did:wba:alice.example")
        );
        assert_eq!(
            provider.default_signing_private_pem().unwrap(),
            TEST_SIGNING_PEM
        );
        assert_eq!(
            provider.e2ee_agreement_private_pem().unwrap(),
            TEST_AGREEMENT_PEM
        );
        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("token-current")
        );
    }

    #[test]
    fn identity_key_provider_prefers_legacy_fallback_files_when_present() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec(&json!({"id": "did:wba:legacy.example"})).unwrap(),
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("did_document.json"),
            serde_json::to_vec(&json!({"id": "did:wba:current.example"})).unwrap(),
        )
        .unwrap();
        std::fs::write(identity_dir.join("private.key"), "legacy-signing").unwrap();
        std::fs::write(identity_dir.join("key-1-private.pem"), "current-signing").unwrap();
        std::fs::write(identity_dir.join("key-3-private.pem"), "legacy-agreement").unwrap();

        let provider = FileBackedKeyMaterialProvider::new(identity_dir);

        assert_eq!(
            provider
                .did_document()
                .unwrap()
                .get("id")
                .and_then(Value::as_str),
            Some("did:wba:legacy.example")
        );
        assert_eq!(
            provider.default_signing_private_pem().unwrap(),
            "legacy-signing"
        );
        assert_eq!(
            provider.e2ee_agreement_private_pem().unwrap(),
            "legacy-agreement"
        );
    }

    #[test]
    fn identity_key_provider_optional_did_document_preserves_missing_file_compatibility() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();

        let provider = FileBackedKeyMaterialProvider::new(identity_dir);

        assert_eq!(provider.optional_did_document().unwrap(), None);
    }

    #[test]
    fn identity_key_provider_persists_auth_token_without_debug_leak() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();

        let provider = FileBackedKeyMaterialProvider::new(identity_dir);
        provider.persist_auth_token("token-secret-value").unwrap();

        assert_eq!(
            provider.valid_auth_token().unwrap().as_deref(),
            Some("token-secret-value")
        );
        let debug = format!("{provider:?}");
        assert!(!debug.contains("token-secret-value"));
        assert!(!debug.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn identity_key_provider_reports_empty_private_key_without_secret_material() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(identity_dir.join("private.key"), "   \n").unwrap();

        let err = FileBackedKeyMaterialProvider::new(identity_dir)
            .default_signing_private_pem()
            .unwrap_err()
            .to_string();

        assert!(err.contains("default_signing_private_key"));
        assert!(err.contains("file is empty"));
        assert!(!err.contains("BEGIN PRIVATE KEY"));
    }
}
