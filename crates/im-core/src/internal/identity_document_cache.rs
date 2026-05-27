use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub(crate) fn load_local_did_document(
    paths: &crate::paths::IdentityRegistryPaths,
    did: &str,
) -> crate::ImResult<Option<Value>> {
    let did = did.trim();
    if did.is_empty() || !did.starts_with("did:") {
        return Ok(None);
    }
    let raw = match fs::read(&paths.registry_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "identity_registry".to_owned(),
                detail: err.to_string(),
            });
        }
    };
    let registry: Value =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    let Some(dir_name) =
        sdk_identity_dir_name(&registry, did).or_else(|| legacy_identity_dir_name(&registry, did))
    else {
        return Ok(None);
    };
    read_did_document(&paths.identity_root_dir.join(dir_name), did)
}

fn sdk_identity_dir_name(registry: &Value, did: &str) -> Option<String> {
    registry
        .get("identities")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|entry| {
            let candidate = entry.get("did").and_then(Value::as_str)?.trim();
            if candidate != did {
                return None;
            }
            first_nonempty([
                string_field(entry, "dir_name"),
                string_field(entry, "local_alias"),
                string_field(entry, "id"),
            ])
        })
}

fn legacy_identity_dir_name(registry: &Value, did: &str) -> Option<String> {
    registry
        .get("credentials")
        .and_then(Value::as_object)?
        .iter()
        .find_map(|(alias, entry)| {
            let candidate = entry.get("did").and_then(Value::as_str)?.trim();
            if candidate != did {
                return None;
            }
            first_nonempty([
                string_field(entry, "dir_name"),
                string_field(entry, "unique_id"),
                string_field(entry, "credential_name"),
                Some(alias.as_str()),
            ])
        })
}

fn read_did_document(identity_dir: &Path, did: &str) -> crate::ImResult<Option<Value>> {
    for path in did_document_paths(identity_dir) {
        let raw = match fs::read(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "did_document".to_owned(),
                    detail: err.to_string(),
                });
            }
        };
        let document: Value =
            serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        if document.get("id").and_then(Value::as_str) == Some(did)
            && document.get("verificationMethod").is_some()
        {
            return Ok(Some(document));
        }
    }
    Ok(None)
}

fn did_document_paths(identity_dir: &Path) -> [PathBuf; 2] {
    [
        identity_dir.join("did.json"),
        identity_dir.join("did_document.json"),
    ]
}

fn string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn first_nonempty<const N: usize>(values: [Option<&str>; N]) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    #[test]
    fn load_local_did_document_reads_sdk_registry_layout() {
        let root = unique_temp_root("im-core-local-did-sdk");
        let paths = paths(&root);
        let did = "did:example:alice";
        write_registry(
            &paths,
            json!({
                "default_identity": "alice",
                "identities": [{
                    "id": "alice-id",
                    "did": did,
                    "local_alias": "alice"
                }]
            }),
        );
        write_document(&paths.identity_root_dir.join("alice"), did, "did.json");

        let document = load_local_did_document(&paths, did).unwrap().unwrap();

        assert_eq!(document["id"], did);
    }

    #[test]
    fn load_local_did_document_reads_legacy_index_layout() {
        let root = unique_temp_root("im-core-local-did-legacy");
        let paths = paths(&root);
        let did = "did:example:bob";
        write_registry(
            &paths,
            json!({
                "default_credential_name": "bob",
                "credentials": {
                    "bob": {
                        "credential_name": "bob",
                        "dir_name": "bob-dir",
                        "did": did
                    }
                }
            }),
        );
        write_document(
            &paths.identity_root_dir.join("bob-dir"),
            did,
            "did_document.json",
        );

        let document = load_local_did_document(&paths, did).unwrap().unwrap();

        assert_eq!(document["id"], did);
    }

    #[test]
    fn load_local_did_document_ignores_unmatched_did() {
        let root = unique_temp_root("im-core-local-did-miss");
        let paths = paths(&root);
        write_registry(
            &paths,
            json!({
                "default_identity": "alice",
                "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice"
                }]
            }),
        );

        assert!(load_local_did_document(&paths, "did:example:bob")
            .unwrap()
            .is_none());
    }

    fn paths(root: &std::path::Path) -> crate::paths::IdentityRegistryPaths {
        crate::paths::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("index.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        }
    }

    fn write_registry(paths: &crate::paths::IdentityRegistryPaths, value: serde_json::Value) {
        fs::create_dir_all(&paths.identity_root_dir).unwrap();
        fs::write(
            &paths.registry_path,
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }

    fn write_document(identity_dir: &std::path::Path, did: &str, filename: &str) {
        fs::create_dir_all(identity_dir).unwrap();
        fs::write(
            identity_dir.join(filename),
            serde_json::to_vec_pretty(&json!({
                "id": did,
                "verificationMethod": []
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn unique_temp_root(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }
}
