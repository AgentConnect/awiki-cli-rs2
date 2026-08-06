//! Crash-safe, offline retirement of one local identity.
//!
//! The durable marker is intentionally secret-free. It makes the identity
//! index tombstone authoritative before files and identity-scoped Vault
//! records are removed, and lets Core finish interrupted cleanup on open.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const RETIREMENT_SCHEMA_VERSION: u32 = 1;
const RETIREMENT_DIR_NAME: &str = ".identity-retirements";

#[derive(Debug, Clone)]
pub(crate) struct IdentityRetirementInput {
    pub(crate) identity_id: String,
    pub(crate) did: String,
    pub(crate) local_alias: String,
    pub(crate) identity_dir_name: Option<String>,
    pub(crate) next_default_alias: Option<String>,
    pub(crate) protocol_device_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IdentityRetirementOutcome {
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IdentityRetirementPhase {
    Prepared,
    Tombstoned,
    FilesRemoved,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRetirementRecord {
    schema_version: u32,
    identity_id: String,
    did: String,
    local_alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity_dir_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_default_alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol_device_id: Option<String>,
    phase: IdentityRetirementPhase,
}

pub(crate) fn retire(
    core: &crate::core::ImCore,
    input: IdentityRetirementInput,
) -> crate::ImResult<IdentityRetirementOutcome> {
    let mut record = IdentityRetirementRecord {
        schema_version: RETIREMENT_SCHEMA_VERSION,
        identity_id: input.identity_id,
        did: input.did,
        local_alias: input.local_alias,
        identity_dir_name: input.identity_dir_name,
        next_default_alias: input.next_default_alias,
        protocol_device_id: input.protocol_device_id,
        phase: IdentityRetirementPhase::Prepared,
    };
    validate_record(core, &record)?;
    let path = retirement_record_path(core, &record.identity_id);
    write_record(&path, &record)?;
    advance(core, &path, &mut record)
}

pub(crate) fn recover_all(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let directory = retirement_dir(core);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(crate::ImError::from(error)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    for path in paths {
        let raw = fs::read(&path)?;
        let mut record: IdentityRetirementRecord =
            serde_json::from_slice(&raw).map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        validate_record(core, &record)?;
        if path != retirement_record_path(core, &record.identity_id) {
            return Err(crate::ImError::PermissionDenied);
        }
        advance(core, &path, &mut record)?;
    }
    Ok(())
}

fn advance(
    core: &crate::core::ImCore,
    path: &Path,
    record: &mut IdentityRetirementRecord,
) -> crate::ImResult<IdentityRetirementOutcome> {
    let paths = &core.inner().sdk_paths().identities;
    let store = crate::internal::identity_store::IdentityStore::new(paths);
    let mut outcome = IdentityRetirementOutcome::default();

    if record.phase == IdentityRetirementPhase::Prepared {
        let lock = store.lock_index_mutation()?;
        let mut index = store.load_index()?;
        let removed_name = index
            .credentials
            .iter()
            .find(|(_, entry)| entry.unique_id == record.identity_id && entry.did == record.did)
            .map(|(name, _)| name.clone());
        if let Some(name) = removed_name.as_deref() {
            index.credentials.remove(name);
        }
        if removed_name.as_deref() == Some(index.default_credential_name.as_str())
            || !index
                .credentials
                .contains_key(&index.default_credential_name)
        {
            index.default_credential_name = record
                .next_default_alias
                .as_ref()
                .filter(|alias| index.credentials.contains_key(alias.as_str()))
                .cloned()
                .or_else(|| index.credentials.keys().next().cloned())
                .unwrap_or_default();
        }
        let next_default = (!index.default_credential_name.is_empty())
            .then_some(index.default_credential_name.clone());
        store.save_index_locked(&lock, index)?;
        store.sync_default_identity(next_default.as_deref())?;
        record.phase = IdentityRetirementPhase::Tombstoned;
        write_record(path, record)?;
    }

    if record.phase == IdentityRetirementPhase::Tombstoned {
        if let Some(dir_name) = record.identity_dir_name.as_deref() {
            let identity_dir = store.local_identity_dir(dir_name)?;
            match identity_directory_matches(&identity_dir, record)? {
                IdentityDirectoryMatch::Exact => match fs::remove_dir_all(&identity_dir) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        outcome.warnings.push(format!(
                            "local identity directory was already missing: {}",
                            identity_dir.display()
                        ));
                    }
                    Err(error) => return Err(crate::ImError::from(error)),
                },
                IdentityDirectoryMatch::Missing => outcome.warnings.push(format!(
                    "local identity directory was already missing: {}",
                    identity_dir.display()
                )),
                IdentityDirectoryMatch::Different => {
                    outcome.warnings.push(format!(
                        "local identity directory is now owned by another identity: {}",
                        identity_dir.display()
                    ));
                }
            }
        } else {
            outcome.warnings.push(format!(
                "local identity {} did not include a usable directory name",
                record.identity_id
            ));
        }
        record.phase = IdentityRetirementPhase::FilesRemoved;
        write_record(path, record)?;
    }

    // Repeat this cleanup even for Completed markers. An operation admitted
    // before the host detached the client may finish late; the durable
    // identity-id tombstone guarantees those records are removed on next open.
    cleanup_identity_vault(core, &record.identity_id)?;
    if let Some(protocol_device_id) = record.protocol_device_id.as_deref() {
        crate::internal::identity_device_join::retire_authorized_new_device_sessions(
            core,
            &record.did,
            protocol_device_id,
        )?;
    }
    if record.phase != IdentityRetirementPhase::Completed {
        record.phase = IdentityRetirementPhase::Completed;
        write_record(path, record)?;
    }
    Ok(outcome)
}

fn cleanup_identity_vault(core: &crate::core::ImCore, identity_id: &str) -> crate::ImResult<()> {
    let Some(context) = core.inner().identity_vault() else {
        return Ok(());
    };
    let vault = context.vault();
    for secret_ref in vault.list()? {
        if secret_ref.identity_id.as_deref() == Some(identity_id) {
            vault.delete(&secret_ref)?;
        }
    }
    Ok(())
}

enum IdentityDirectoryMatch {
    Exact,
    Missing,
    Different,
}

fn identity_directory_matches(
    identity_dir: &Path,
    record: &IdentityRetirementRecord,
) -> crate::ImResult<IdentityDirectoryMatch> {
    let identity_path = identity_dir.join("identity.json");
    let raw = match fs::read(&identity_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !identity_dir.exists() {
                return Ok(IdentityDirectoryMatch::Missing);
            }
            return legacy_identity_directory_matches(identity_dir, record);
        }
        Err(error) => return Err(crate::ImError::from(error)),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })?;
    let identity_id = value.get("unique_id").and_then(serde_json::Value::as_str);
    let did = value.get("did").and_then(serde_json::Value::as_str);
    Ok(
        if identity_id == Some(record.identity_id.as_str()) && did == Some(record.did.as_str()) {
            IdentityDirectoryMatch::Exact
        } else {
            IdentityDirectoryMatch::Different
        },
    )
}

fn legacy_identity_directory_matches(
    identity_dir: &Path,
    record: &IdentityRetirementRecord,
) -> crate::ImResult<IdentityDirectoryMatch> {
    for file_name in ["did_document.json", "did.json"] {
        let raw = match fs::read(identity_dir.join(file_name)) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(crate::ImError::from(error)),
        };
        let value: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        return Ok(
            if value.get("id").and_then(serde_json::Value::as_str) == Some(record.did.as_str()) {
                IdentityDirectoryMatch::Exact
            } else {
                IdentityDirectoryMatch::Different
            },
        );
    }
    Ok(IdentityDirectoryMatch::Different)
}

fn validate_record(
    core: &crate::core::ImCore,
    record: &IdentityRetirementRecord,
) -> crate::ImResult<()> {
    if record.schema_version != RETIREMENT_SCHEMA_VERSION
        || record.identity_id.trim().is_empty()
        || record.did.trim().is_empty()
        || record.local_alias.trim().is_empty()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::ids::Did::parse(&record.did)?;
    if let Some(protocol_device_id) = record.protocol_device_id.as_deref() {
        crate::ids::ProtocolDeviceId::parse(protocol_device_id)?;
    }
    if let Some(dir_name) = record.identity_dir_name.as_deref() {
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .local_identity_dir(dir_name)?;
    }
    Ok(())
}

fn retirement_dir(core: &crate::core::ImCore) -> PathBuf {
    core.inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(RETIREMENT_DIR_NAME)
}

fn retirement_record_path(core: &crate::core::ImCore, identity_id: &str) -> PathBuf {
    let digest = Sha256::digest(identity_id.as_bytes());
    retirement_dir(core).join(format!("{}.json", URL_SAFE_NO_PAD.encode(digest)))
}

fn write_record(path: &Path, record: &IdentityRetirementRecord) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(record).map_err(|error| crate::ImError::Serialization {
        detail: error.to_string(),
    })?;
    crate::internal::identity_store::write_secure_bytes_atomic(path, &raw)
}
