use super::record::{SecretRef, VaultSecretRecord};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct FileSecretVaultStore {
    root_dir: PathBuf,
}

impl FileSecretVaultStore {
    pub(crate) fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    pub(crate) fn put(&self, record: &VaultSecretRecord) -> crate::ImResult<SecretRef> {
        let secret_ref = record.secret_ref();
        let path = self.record_path(&secret_ref);
        let raw =
            serde_json::to_vec_pretty(record).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        write_secure_file(&path, &raw)?;
        Ok(secret_ref)
    }

    pub(crate) fn get(&self, secret_ref: &SecretRef) -> crate::ImResult<VaultSecretRecord> {
        let path = self.record_path(secret_ref);
        let raw = fs::read(&path).map_err(|err| crate::ImError::CredentialFileUnreadable {
            path_kind: "secret_vault_record".to_owned(),
            detail: err.to_string(),
        })?;
        let record: VaultSecretRecord =
            serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        if record.secret_ref() != *secret_ref {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(record)
    }

    pub(crate) fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        let path = self.record_path(secret_ref);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(crate::ImError::Io {
                detail: format!("delete secret vault record {}: {err}", path.display()),
            }),
        }
    }

    pub(crate) fn list(&self) -> crate::ImResult<Vec<SecretRef>> {
        let records_dir = self.records_dir();
        let entries = match fs::read_dir(&records_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(crate::ImError::Io {
                    detail: format!("read secret vault records {}: {err}", records_dir.display()),
                });
            }
        };
        let mut refs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(crate::ImError::from)?;
            if !is_record_file(&entry.path()) {
                continue;
            }
            if entry.file_type().map_err(crate::ImError::from)?.is_file() {
                let raw = fs::read(entry.path()).map_err(crate::ImError::from)?;
                let record: VaultSecretRecord =
                    serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
                        detail: err.to_string(),
                    })?;
                refs.push(record.secret_ref());
            }
        }
        refs.sort_by(|left, right| {
            record_storage_key(left)
                .as_str()
                .cmp(record_storage_key(right).as_str())
        });
        Ok(refs)
    }

    pub(crate) fn record_path(&self, secret_ref: &SecretRef) -> PathBuf {
        self.records_dir()
            .join(format!("{}.json", record_storage_key(secret_ref)))
    }

    fn records_dir(&self) -> PathBuf {
        self.root_dir.join("records")
    }
}

fn is_record_file(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("json")
}

fn record_storage_key(secret_ref: &SecretRef) -> String {
    let mut hasher = Sha256::new();
    push_hash_field(&mut hasher, "workspace_id", &secret_ref.workspace_id);
    push_hash_field(&mut hasher, "device_id", &secret_ref.device_id);
    push_optional_hash_field(
        &mut hasher,
        "identity_id",
        secret_ref.identity_id.as_deref(),
    );
    push_optional_hash_field(&mut hasher, "did", secret_ref.did.as_deref());
    push_hash_field(&mut hasher, "kind", secret_ref.kind.as_str());
    push_hash_field(&mut hasher, "key_id", &secret_ref.key_id);
    push_hash_field(
        &mut hasher,
        "key_version",
        &secret_ref.key_version.to_string(),
    );
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn push_optional_hash_field(hasher: &mut Sha256, name: &str, value: Option<&str>) {
    match value {
        Some(value) => push_hash_field(hasher, name, value),
        None => push_hash_field(hasher, name, "<none>"),
    }
}

fn push_hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(value.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}

fn write_secure_file(path: &Path, raw: &[u8]) -> crate::ImResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_private_dir_mode(parent)?;
    }
    let temp = temp_path(path);
    {
        let mut file = create_private_file(&temp)?;
        file.write_all(raw).map_err(crate::ImError::from)?;
        file.sync_all().map_err(crate::ImError::from)?;
    }
    fs::rename(&temp, path).map_err(|err| crate::ImError::Io {
        detail: format!(
            "rename secret vault temp file {} to {}: {err}",
            temp.display(),
            path.display()
        ),
    })?;
    set_private_file_mode(path)?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("record.json");
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nanos))
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}
