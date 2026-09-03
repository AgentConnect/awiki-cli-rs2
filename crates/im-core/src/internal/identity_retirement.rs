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
    ensure_prepared(core, &input)?;
    let path = retirement_record_path(core, &input.identity_id);
    let raw = fs::read(&path)?;
    let mut record: IdentityRetirementRecord =
        serde_json::from_slice(&raw).map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })?;
    validate_record(core, &record)?;
    advance(core, &path, &mut record)
}

pub(crate) fn ensure_prepared(
    core: &crate::core::ImCore,
    input: &IdentityRetirementInput,
) -> crate::ImResult<()> {
    let record = IdentityRetirementRecord {
        schema_version: RETIREMENT_SCHEMA_VERSION,
        identity_id: input.identity_id.clone(),
        did: input.did.clone(),
        local_alias: input.local_alias.clone(),
        identity_dir_name: input.identity_dir_name.clone(),
        next_default_alias: input.next_default_alias.clone(),
        protocol_device_id: input.protocol_device_id.clone(),
        phase: IdentityRetirementPhase::Prepared,
    };
    validate_record(core, &record)?;
    let path = retirement_record_path(core, &record.identity_id);
    match fs::read(&path) {
        Ok(raw) => {
            let existing: IdentityRetirementRecord =
                serde_json::from_slice(&raw).map_err(|error| crate::ImError::Serialization {
                    detail: error.to_string(),
                })?;
            validate_record(core, &existing)?;
            if existing.identity_id != record.identity_id
                || existing.did != record.did
                || existing.local_alias != record.local_alias
                || existing.identity_dir_name != record.identity_dir_name
                || existing.next_default_alias != record.next_default_alias
                || existing.protocol_device_id != record.protocol_device_id
            {
                return Err(crate::ImError::PermissionDenied);
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => write_record(&path, &record),
        Err(error) => Err(crate::ImError::from(error)),
    }
}

pub(crate) fn is_completed(
    core: &crate::core::ImCore,
    input: &IdentityRetirementInput,
) -> crate::ImResult<bool> {
    let path = retirement_record_path(core, &input.identity_id);
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(crate::ImError::from(error)),
    };
    let record: IdentityRetirementRecord =
        serde_json::from_slice(&raw).map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })?;
    validate_record(core, &record)?;
    Ok(record.phase == IdentityRetirementPhase::Completed
        && record.identity_id == input.identity_id
        && record.did == input.did
        && record.local_alias == input.local_alias
        && record.identity_dir_name == input.identity_dir_name
        && record.next_default_alias == input.next_default_alias
        && record.protocol_device_id == input.protocol_device_id)
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

/// Returns whether the exact historical owner/device tuple was retired by a
/// completed local identity deletion.
///
/// Message projections deliberately retain their stable account binding after
/// credential deletion. Registration may treat that binding as having no live
/// local credential only when the durable retirement marker closes over the
/// same identity, DID, and protocol device. Missing, partial, or mismatched
/// markers never relax the registration continuity fence.
pub(crate) fn matches_completed_binding(
    identity_root_dir: &Path,
    identity_id: &str,
    did: &str,
    protocol_device_id: &str,
) -> crate::ImResult<bool> {
    let path = retirement_record_path_from_root(identity_root_dir, identity_id);
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(crate::ImError::from(error)),
    };
    let record: IdentityRetirementRecord =
        serde_json::from_slice(&raw).map_err(|_| crate::ImError::PermissionDenied)?;
    if record.schema_version != RETIREMENT_SCHEMA_VERSION
        || record.identity_id.trim().is_empty()
        || record.did.trim().is_empty()
        || record.local_alias.trim().is_empty()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::ids::Did::parse(&record.did)?;
    if let Some(record_device_id) = record.protocol_device_id.as_deref() {
        crate::ids::ProtocolDeviceId::parse(record_device_id)?;
    }
    Ok(record.phase == IdentityRetirementPhase::Completed
        && record.identity_id == identity_id
        && record.did == did
        && record.protocol_device_id.as_deref() == Some(protocol_device_id))
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
        #[cfg(test)]
        fail_after_phase_for_test(IdentityRetirementTestCut::Tombstoned)?;
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
        #[cfg(test)]
        fail_after_phase_for_test(IdentityRetirementTestCut::FilesRemoved)?;
    }

    // Repeat this cleanup even for Completed markers. An operation admitted
    // before the host detached the client may finish late; the durable
    // identity-id tombstone guarantees those records are removed on next open.
    //
    // The tombstone is keyed by identity_id, which is the deterministic DID
    // suffix for a handle. Re-registering the same handle reuses the same
    // identity_id, so a stale tombstone must never wipe the vault records of
    // an identity that is currently registered: that would destroy the live
    // identity's secrets and make the next open fail closed with
    // identity_vault_record_open_failed. Only replay cleanup when the retired
    // identity is not present in the registry anymore.
    let rollover_supersedes = match record.protocol_device_id.as_deref() {
        Some(protocol_device_id) => {
            crate::internal::identity_registration_retired_join::completed_rollover_supersedes_retirement(
                core,
                &record.identity_id,
                &record.did,
                protocol_device_id,
            )
            ?
        }
        None => false,
    };
    if identity_is_registered(core, record)? || rollover_supersedes {
        outcome.warnings.push(format!(
            "identity {} is currently registered; skipping retirement vault cleanup",
            record.identity_id
        ));
    } else {
        cleanup_identity_vault(core, &record.identity_id)?;
        cleanup_retired_anp_identity(core, record)?;
    }
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
        #[cfg(test)]
        fail_after_phase_for_test(IdentityRetirementTestCut::Completed)?;
    }
    Ok(outcome)
}

#[cfg(feature = "identity-native-anp")]
fn cleanup_retired_anp_identity(
    core: &crate::core::ImCore,
    record: &IdentityRetirementRecord,
) -> crate::ImResult<()> {
    let Some(protocol_device_id) = record.protocol_device_id.as_deref() else {
        return Ok(());
    };
    let mut manager = crate::internal::identity_custody::open_controller_manager(core)?;
    let mut matches = manager
        .list()
        .map_err(crate::internal::identity_custody::map_facade_error)?
        .into_iter()
        .filter(|descriptor| {
            descriptor.reference.did == record.did
                && descriptor.state == anp_identity::PublicIdentityState::Active
        })
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let Some(descriptor) = matches.pop() else {
        return Ok(());
    };
    let identity = manager
        .get(&descriptor.reference)
        .map_err(crate::internal::identity_custody::map_facade_error)?;
    let public = identity
        .public_identity()
        .map_err(crate::internal::identity_custody::map_facade_error)?;
    let manifest = anp::authentication::validate_device_manifest(public.document.as_value())
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest
        .devices
        .iter()
        .filter(|device| device.device_id == protocol_device_id)
        .count()
        != 1
    {
        return Err(crate::ImError::PermissionDenied);
    }
    manager
        .delete(
            &descriptor.reference,
            anp_identity::DeleteIdentityRequest {
                discard_pending_changes: true,
            },
        )
        .map_err(crate::internal::identity_custody::map_facade_error)?;
    Ok(())
}

#[cfg(not(feature = "identity-native-anp"))]
fn cleanup_retired_anp_identity(
    _core: &crate::core::ImCore,
    _record: &IdentityRetirementRecord,
) -> crate::ImResult<()> {
    Ok(())
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

/// Returns whether the retired identity is currently registered in the local
/// identity index.
///
/// The retirement tombstone is keyed by `identity_id`, which is the
/// deterministic DID suffix for a handle. A re-registration of the same
/// handle reuses the same `identity_id`, so the index must be consulted
/// before replaying vault cleanup: wiping the records of a registered
/// identity destroys its live secrets (next open fails closed with
/// `identity_vault_record_open_failed`).
fn identity_is_registered(
    core: &crate::core::ImCore,
    record: &IdentityRetirementRecord,
) -> crate::ImResult<bool> {
    let store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let index = store.load_index()?;
    Ok(index
        .credentials
        .values()
        .any(|entry| entry.unique_id == record.identity_id && entry.did == record.did))
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
    retirement_dir_from_root(&core.inner().sdk_paths().identities.identity_root_dir)
}

fn retirement_record_path(core: &crate::core::ImCore, identity_id: &str) -> PathBuf {
    retirement_record_path_from_root(
        &core.inner().sdk_paths().identities.identity_root_dir,
        identity_id,
    )
}

fn retirement_dir_from_root(identity_root_dir: &Path) -> PathBuf {
    identity_root_dir.join(RETIREMENT_DIR_NAME)
}

fn retirement_record_path_from_root(identity_root_dir: &Path, identity_id: &str) -> PathBuf {
    let digest = Sha256::digest(identity_id.as_bytes());
    retirement_dir_from_root(identity_root_dir)
        .join(format!("{}.json", URL_SAFE_NO_PAD.encode(digest)))
}

fn write_record(path: &Path, record: &IdentityRetirementRecord) -> crate::ImResult<()> {
    let raw = serde_json::to_vec_pretty(record).map_err(|error| crate::ImError::Serialization {
        detail: error.to_string(),
    })?;
    crate::internal::identity_store::write_secure_bytes_atomic(path, &raw)
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentityRetirementTestCut {
    Tombstoned,
    FilesRemoved,
    Completed,
}

#[cfg(test)]
std::thread_local! {
    static TEST_CUT: std::cell::Cell<Option<IdentityRetirementTestCut>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(test)]
pub(crate) fn set_test_cut(cut: IdentityRetirementTestCut) {
    TEST_CUT.with(|value| value.set(Some(cut)));
}

#[cfg(test)]
fn fail_after_phase_for_test(cut: IdentityRetirementTestCut) -> crate::ImResult<()> {
    if TEST_CUT.with(|value| value.get() == Some(cut)) {
        TEST_CUT.with(|value| value.set(None));
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!("injected identity retirement cut after {cut:?}"),
        });
    }
    Ok(())
}
