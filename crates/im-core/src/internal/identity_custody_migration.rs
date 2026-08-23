//! One-way migration from AWiki identity key storage into ANP Identity.
//!
//! The order is fixed: copy every identity, verify every imported identity,
//! atomically cut over the complete index, then remove legacy identity keys.

use std::fs;
use std::path::Path;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use crate::identity::{
    IdentityCustodyMigrationIdentityReport, IdentityCustodyMigrationPhase,
    IdentityCustodyMigrationReport,
};
use crate::internal::identity_device_state::{DeviceAuthorizationStatus, IdentityDeviceMode};
use crate::internal::identity_store::{
    IdentityCustodyCutoverMarker, IdentityStore, IndexEntry,
    IDENTITY_CUSTODY_CUTOVER_INDEX_SCHEMA_VERSION, IDENTITY_CUSTODY_CUTOVER_MARKER_SCHEMA_VERSION,
};
use crate::internal::secret_vault::record::SecretRef;

const BACKEND: &str = "anp_identity";
const ROOT_PRIVATE_FILES: &[&str] = &["key-1-private.pem", "private.key"];
const SIGNING_PRIVATE_FILE: &str = "e2ee-signing-private.pem";
const AGREEMENT_PRIVATE_FILES: &[&str] = &["e2ee-agreement-private.pem", "key-3-private.pem"];
const DAEMON_PRIVATE_FILES: &[&str] = &["daemon-key-1-private.pem", "daemon-subkey-package.json"];

fn inspect_only(core: &crate::core::ImCore) -> crate::ImResult<IdentityCustodyMigrationReport> {
    block_on_run(core, true, None)
}

#[cfg(feature = "identity-native-anp")]
fn sync_run(
    core: &crate::core::ImCore,
    dry_run: bool,
    failure: Option<FailurePoint>,
) -> crate::ImResult<IdentityCustodyMigrationReport> {
    block_on_run(core, dry_run, failure)
}

fn block_on_run(
    core: &crate::core::ImCore,
    dry_run: bool,
    failure: Option<FailurePoint>,
) -> crate::ImResult<IdentityCustodyMigrationReport> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "synchronous identity migration cannot run inside an async runtime".to_owned(),
        });
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| crate::ImError::Internal {
            message: format!("identity migration runtime failed: {error}"),
        })?
        .block_on(run(core, dry_run, failure))
}

pub(crate) fn inspect(
    core: &crate::core::ImCore,
) -> crate::ImResult<IdentityCustodyMigrationReport> {
    #[cfg(feature = "identity-native-anp")]
    {
        sync_run(core, true, None)
    }
    #[cfg(not(feature = "identity-native-anp"))]
    {
        inspect_only(core)
    }
}

#[cfg(feature = "identity-native-anp")]
pub(crate) fn migrate(
    core: &crate::core::ImCore,
) -> crate::ImResult<IdentityCustodyMigrationReport> {
    sync_run(core, false, None)
}

pub(crate) async fn migrate_async(
    core: &crate::core::ImCore,
) -> crate::ImResult<IdentityCustodyMigrationReport> {
    run(core, false, None).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    AfterCopied(usize),
    AfterCutover,
}

enum MigrationPreparation {
    Complete(IdentityCustodyMigrationReport),
    Ready {
        reports: Vec<IdentityCustodyMigrationIdentityReport>,
        materials: Vec<LegacyIdentityMaterial>,
        existing_bindings: Vec<CutoverBinding>,
        pending_warnings: Vec<String>,
    },
}

fn prepare_migration(
    core: &crate::core::ImCore,
    dry_run: bool,
    pending: crate::internal::identity_pending_upgrade::PendingUpgradeOutcome,
) -> crate::ImResult<MigrationPreparation> {
    let paths = &core.inner().sdk_paths().identities;
    let legacy_store = IdentityStore::new(paths);
    let index = legacy_store.load_index()?;
    if let Some(marker) = index.identity_custody_cutover.as_ref() {
        let mut report = report_for_cutover_index(dry_run, &index, marker);
        if !dry_run && !marker.cleanup_complete {
            cleanup_after_cutover(core, &legacy_store)?;
            report.phase = IdentityCustodyMigrationPhase::Cleaned;
            report.cleanup_complete = true;
        }
        return Ok(MigrationPreparation::Complete(report));
    }

    let mut reports = Vec::new();
    let mut materials = Vec::new();
    let mut existing_bindings = Vec::new();
    for (identity_name, entry) in &index.credentials {
        if entry.identity_custody_backend.as_deref() == Some(BACKEND) {
            let store_id = entry
                .anp_identity_store_id
                .clone()
                .filter(|value| !value.trim().is_empty());
            let identity_id = entry
                .anp_identity_id
                .clone()
                .filter(|value| !value.trim().is_empty());
            match (store_id, identity_id) {
                (Some(store_id), Some(identity_id)) => {
                    let document = legacy_store.load_did_document(&entry.dir_name)?;
                    let document_digest = canonical_document_digest(&document)?;
                    reports.push(IdentityCustodyMigrationIdentityReport {
                        identity_name: identity_name.clone(),
                        did: entry.did.clone(),
                        eligible: true,
                        already_managed: true,
                        root_capability_present: entry.device_state.as_ref().is_some_and(|state| {
                            state
                                .authorization
                                .as_ref()
                                .is_some_and(|authorization| authorization.management_ready)
                        }),
                        reason: None,
                    });
                    existing_bindings.push(CutoverBinding {
                        identity_name: identity_name.clone(),
                        did: entry.did.clone(),
                        source_unique_id: entry.unique_id.clone(),
                        source_dir_name: entry.dir_name.clone(),
                        store_id,
                        identity_id,
                        auth_ref: entry.anp_identity_auth_ref.clone(),
                        document_digest,
                        root_capability_present: entry
                            .device_state
                            .as_ref()
                            .and_then(|state| state.authorization.as_ref())
                            .is_some_and(|authorization| authorization.management_ready),
                    });
                }
                _ => reports.push(ineligible_report(
                    identity_name,
                    entry,
                    "ANP Identity projection is incomplete",
                )),
            }
            continue;
        }
        match prepare_legacy_identity(core, &legacy_store, identity_name, entry) {
            Ok(material) => {
                reports.push(material.report.clone());
                materials.push(material);
            }
            Err(error) => reports.push(ineligible_report(identity_name, entry, &error.to_string())),
        }
    }

    let mut blockers = pending.blockers;
    let pending_warnings = pending.warnings;
    blockers.extend(
        reports
            .iter()
            .filter(|report| !report.eligible)
            .map(|report| {
                format!(
                    "identity {} is not eligible: {}",
                    report.identity_name,
                    report.reason.as_deref().unwrap_or("unknown reason")
                )
            }),
    );
    if index.credentials.is_empty()
        || (materials.is_empty() && existing_bindings.len() == reports.len())
    {
        return Ok(MigrationPreparation::Complete(
            IdentityCustodyMigrationReport {
                dry_run,
                phase: IdentityCustodyMigrationPhase::NotRequired,
                store_id: common_store_id(&existing_bindings),
                marker_written: false,
                cleanup_complete: false,
                copied_count: 0,
                verified_count: existing_bindings.len(),
                identities: reports,
                blockers,
                warnings: pending_warnings,
            },
        ));
    }
    if dry_run || !blockers.is_empty() {
        return Ok(MigrationPreparation::Complete(
            IdentityCustodyMigrationReport {
                dry_run,
                phase: if blockers.is_empty() {
                    IdentityCustodyMigrationPhase::Eligible
                } else {
                    IdentityCustodyMigrationPhase::Blocked
                },
                store_id: common_store_id(&existing_bindings),
                marker_written: false,
                cleanup_complete: false,
                copied_count: 0,
                verified_count: existing_bindings.len(),
                identities: reports,
                blockers,
                warnings: pending_warnings
                    .into_iter()
                    .chain(std::iter::once(
                        "dry-run and pre-cutover inspection never remove legacy identity records"
                            .to_owned(),
                    ))
                    .collect(),
            },
        ));
    }
    Ok(MigrationPreparation::Ready {
        reports,
        materials,
        existing_bindings,
        pending_warnings,
    })
}

async fn run(
    core: &crate::core::ImCore,
    dry_run: bool,
    failure: Option<FailurePoint>,
) -> crate::ImResult<IdentityCustodyMigrationReport> {
    let pending = crate::internal::identity_pending_upgrade::converge(core, dry_run).await?;
    let prepare_core = core.clone();
    let prepared = crate::internal::runtime::worker::run_blocking(move || {
        prepare_migration(&prepare_core, dry_run, pending)
    })
    .await
    .map_err(|error| crate::ImError::Internal {
        message: error.to_string(),
    })??;
    let (reports, materials, existing_bindings, pending_warnings) = match prepared {
        MigrationPreparation::Complete(report) => return Ok(report),
        MigrationPreparation::Ready {
            reports,
            materials,
            existing_bindings,
            pending_warnings,
        } => (reports, materials, existing_bindings, pending_warnings),
    };

    let custody = crate::internal::identity_custody::controller_custody_provider(core).await?;
    let store_id = custody
        .store_info()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?
        .store_id;
    for binding in &existing_bindings {
        if binding.store_id != store_id {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    let mut bindings = existing_bindings;
    let mut copied_count = 0;
    for material in materials {
        let binding = copy_one_identity(custody.as_ref(), material).await?;
        copied_count += 1;
        bindings.push(binding);
        if failure == Some(FailurePoint::AfterCopied(copied_count)) {
            return Err(test_failure("after_copy"));
        }
    }

    for binding in &bindings {
        verify_binding(custody.as_ref(), binding).await?;
    }
    let verified_count = bindings.len();
    let commit_core = core.clone();
    let commit_store_id = store_id.clone();
    let commit_bindings = bindings.clone();
    crate::internal::runtime::worker::run_blocking(move || {
        let legacy_store = IdentityStore::new(&commit_core.inner().sdk_paths().identities);
        commit_cutover(
            &commit_core,
            &legacy_store,
            &commit_store_id,
            &commit_bindings,
        )?;
        if failure == Some(FailurePoint::AfterCutover) {
            return Err(test_failure("after_cutover"));
        }
        cleanup_after_cutover(&commit_core, &legacy_store)
    })
    .await
    .map_err(|error| crate::ImError::Internal {
        message: error.to_string(),
    })??;

    Ok(IdentityCustodyMigrationReport {
        dry_run: false,
        phase: IdentityCustodyMigrationPhase::Cleaned,
        store_id: Some(store_id),
        marker_written: true,
        cleanup_complete: true,
        copied_count,
        verified_count,
        identities: reports,
        blockers: Vec::new(),
        warnings: pending_warnings
            .into_iter()
            .chain(std::iter::once(
                "legacy identity keys were removed only after the schema-v6 cutover marker committed"
                    .to_owned(),
            ))
            .collect(),
    })
}

struct LegacyIdentityMaterial {
    report: IdentityCustodyMigrationIdentityReport,
    identity_name: String,
    source_unique_id: String,
    source_dir_name: String,
    did: crate::ids::Did,
    document: serde_json::Value,
    evidence: crate::internal::identity_provider::ProviderPublicationEvidence,
    root_key: Option<crate::internal::identity_provider::ProviderIdentityMaterialKey>,
    signing_key: crate::internal::identity_provider::ProviderIdentityMaterialKey,
    e2ee_key: crate::internal::identity_provider::ProviderIdentityMaterialKey,
    auth_ref: Option<SecretRef>,
}

#[derive(Clone)]
struct CutoverBinding {
    identity_name: String,
    did: String,
    source_unique_id: String,
    source_dir_name: String,
    store_id: String,
    identity_id: String,
    auth_ref: Option<SecretRef>,
    document_digest: String,
    root_capability_present: bool,
}

fn prepare_legacy_identity(
    core: &crate::core::ImCore,
    store: &IdentityStore<'_>,
    identity_name: &str,
    entry: &IndexEntry,
) -> crate::ImResult<LegacyIdentityMaterial> {
    let did = crate::ids::Did::parse(&entry.did)?;
    let state = entry
        .device_state
        .as_ref()
        .ok_or_else(|| not_ready(&entry.did, "identity_device_state"))?;
    if state.mode != IdentityDeviceMode::VNext {
        return Err(not_ready(
            &entry.did,
            "legacy_identity_requires_vnext_upgrade",
        ));
    }
    state.validate_for_did(&did)?;
    let authorization = state
        .authorization
        .as_ref()
        .filter(|authorization| authorization.status == DeviceAuthorizationStatus::Active)
        .ok_or_else(|| not_ready(&entry.did, "active_device_authorization"))?;
    let checkpoint = state
        .checkpoint
        .as_ref()
        .ok_or_else(|| not_ready(&entry.did, "identity_checkpoint"))?;
    let document = store.load_did_document(&entry.dir_name)?;
    if document.get("id").and_then(serde_json::Value::as_str) != Some(did.as_str())
        || !anp::authentication::validate_did_document_binding(&document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(&document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if !manifest.devices.iter().any(|device| {
        device.device_id == authorization.protocol_device_id.as_str()
            && device.signing_key_id == authorization.signing_key_id
            && device.e2ee_key_id == authorization.e2ee_key_id
    }) {
        return Err(crate::ImError::PermissionDenied);
    }
    let root_kid = document
        .get("proof")
        .and_then(serde_json::Value::as_object)
        .and_then(|proof| proof.get("verificationMethod"))
        .and_then(serde_json::Value::as_str)
        .ok_or(crate::ImError::PermissionDenied)?
        .to_owned();
    let identity_dir = store.local_identity_dir(&entry.dir_name)?;
    let (root_pem, signing_pem, e2ee_pem, auth_ref) =
        load_legacy_key_material(core, entry, &identity_dir)?;
    if authorization.management_ready != root_pem.is_some() {
        return Err(not_ready(
            &entry.did,
            "root_capability_and_device_authorization_match",
        ));
    }
    let root_key = root_pem
        .map(|pem| {
            imported_key(
                &document,
                &root_kid,
                crate::internal::identity_provider::ProviderKeyPurpose::RootControl,
                pem,
            )
        })
        .transpose()?;
    let signing_key = imported_key(
        &document,
        &authorization.signing_key_id,
        crate::internal::identity_provider::ProviderKeyPurpose::DeviceAssertion,
        signing_pem,
    )?;
    let e2ee_key = imported_key(
        &document,
        &authorization.e2ee_key_id,
        crate::internal::identity_provider::ProviderKeyPurpose::KeyAgreement,
        e2ee_pem,
    )?;
    let digest = canonical_document_digest(&document)?;
    Ok(LegacyIdentityMaterial {
        report: IdentityCustodyMigrationIdentityReport {
            identity_name: identity_name.to_owned(),
            did: did.as_str().to_owned(),
            eligible: true,
            already_managed: false,
            root_capability_present: root_key.is_some(),
            reason: None,
        },
        identity_name: identity_name.to_owned(),
        source_unique_id: entry.unique_id.clone(),
        source_dir_name: entry.dir_name.clone(),
        did,
        document,
        evidence: crate::internal::identity_provider::ProviderPublicationEvidence {
            document_version: checkpoint.document_version,
            registry_version: checkpoint.registry_version,
            document_digest: digest,
        },
        root_key,
        signing_key,
        e2ee_key,
        auth_ref,
    })
}

fn load_legacy_key_material(
    core: &crate::core::ImCore,
    entry: &IndexEntry,
    identity_dir: &Path,
) -> crate::ImResult<(
    Option<Zeroizing<String>>,
    Zeroizing<String>,
    Zeroizing<String>,
    Option<SecretRef>,
)> {
    if let Some(metadata) = entry.vault_migration.as_ref() {
        let context = core
            .inner()
            .identity_vault()
            .ok_or_else(|| not_ready(&entry.did, "identity_secret_vault"))?;
        if metadata.workspace_id != context.workspace_id()
            || metadata.device_id != context.vault_context_device_id().as_str()
        {
            return Err(not_ready(&entry.did, "identity_vault_context_match"));
        }
        let refs = metadata
            .vnext_refs
            .as_ref()
            .ok_or_else(|| not_ready(&entry.did, "vnext_vault_key_refs"))?;
        let vault = context.vault();
        let root = refs
            .did_document_root_private
            .as_ref()
            .map(|reference| open_vault_utf8(vault.as_ref(), reference, "root_private"))
            .transpose()?;
        let signing = open_vault_utf8(
            vault.as_ref(),
            &refs.device_request_signing_private,
            "device_signing_private",
        )?;
        let e2ee = open_vault_utf8(
            vault.as_ref(),
            &refs.e2ee_agreement_private,
            "e2ee_agreement_private",
        )?;
        return Ok((root, signing, e2ee, Some(refs.auth_jwt.clone())));
    }
    Ok((
        read_optional_utf8(identity_dir, ROOT_PRIVATE_FILES)?,
        read_required_utf8(
            identity_dir,
            &[SIGNING_PRIVATE_FILE],
            "device_signing_private",
        )?,
        read_required_utf8(
            identity_dir,
            AGREEMENT_PRIVATE_FILES,
            "e2ee_agreement_private",
        )?,
        None,
    ))
}

fn imported_key(
    document: &serde_json::Value,
    kid: &str,
    purpose: crate::internal::identity_provider::ProviderKeyPurpose,
    pem: Zeroizing<String>,
) -> crate::ImResult<crate::internal::identity_provider::ProviderIdentityMaterialKey> {
    let material = anp::PrivateKeyMaterial::from_pem(&pem).map_err(|_| {
        crate::ImError::CredentialFileUnreadable {
            path_kind: "identity_private_key".to_owned(),
            detail: "legacy identity key is invalid".to_owned(),
        }
    })?;
    let method = anp::authentication::find_verification_method(document, kid)
        .ok_or(crate::ImError::PermissionDenied)?;
    let expected = anp::authentication::extract_public_key(&method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if material.public_key().to_pem() != expected.to_pem() {
        return Err(crate::ImError::PermissionDenied);
    }
    let raw = match (purpose, material) {
        (
            crate::internal::identity_provider::ProviderKeyPurpose::RootControl
            | crate::internal::identity_provider::ProviderKeyPurpose::DeviceAssertion,
            anp::PrivateKeyMaterial::Ed25519(key),
        ) => key.to_bytes().to_vec(),
        (
            crate::internal::identity_provider::ProviderKeyPurpose::KeyAgreement,
            anp::PrivateKeyMaterial::X25519(key),
        ) => key.to_bytes().to_vec(),
        _ => return Err(crate::ImError::PermissionDenied),
    };
    Ok(
        crate::internal::identity_provider::ProviderIdentityMaterialKey {
            kid: kid.to_owned(),
            purpose,
            encoding: crate::internal::identity_provider::ProviderPrivateKeyEncoding::Raw32,
            secret: Zeroizing::new(raw),
        },
    )
}

async fn copy_one_identity(
    custody: &dyn crate::internal::identity_provider::IdentityCustody,
    material: LegacyIdentityMaterial,
) -> crate::ImResult<CutoverBinding> {
    let did = material.did.as_str().to_owned();
    let expected_document_digest = material.evidence.document_digest.clone();
    let root_capability_present = material.root_key.is_some();
    let existing = custody
        .list_identities()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?
        .into_iter()
        .find(|descriptor| descriptor.reference.did == did);
    let session = match existing {
        Some(descriptor) => custody
            .open_identity(&descriptor.reference)
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?,
        None => {
            let mut keys = Vec::with_capacity(if material.root_key.is_some() { 3 } else { 2 });
            if let Some(root_key) = material.root_key {
                keys.push(root_key);
            }
            keys.push(material.signing_key);
            keys.push(material.e2ee_key);
            custody
                .import_identity_material(
                    crate::internal::identity_provider::ProviderIdentityMaterialImportRequest {
                        remote:
                            crate::internal::identity_provider::ProviderVerifiedRemoteDocument {
                                document: material.document,
                                evidence: material.evidence,
                            },
                        did_wba: true,
                        keys,
                        request_id: format!(
                            "legacy-migration:{}:{}",
                            material.source_unique_id, expected_document_digest
                        ),
                    },
                )
                .await
                .map_err(crate::internal::identity_provider::map_provider_error)?
        }
    };
    let public = session
        .public_identity()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    if public.state != crate::internal::identity_provider::ProviderIdentityState::Active
        || canonical_document_digest(&public.document)? != expected_document_digest
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(CutoverBinding {
        identity_name: material.identity_name,
        did,
        source_unique_id: material.source_unique_id,
        source_dir_name: material.source_dir_name,
        store_id: public.reference.store_id,
        identity_id: public.reference.identity_id,
        auth_ref: material.auth_ref,
        document_digest: expected_document_digest,
        root_capability_present,
    })
}

async fn verify_binding(
    custody: &dyn crate::internal::identity_provider::IdentityCustody,
    binding: &CutoverBinding,
) -> crate::ImResult<()> {
    let info = custody
        .store_info()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    if info.store_id != binding.store_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let identity = custody
        .open_identity(&crate::internal::identity_provider::ProviderIdentityRef {
            store_id: binding.store_id.clone(),
            identity_id: binding.identity_id.clone(),
            did: binding.did.clone(),
        })
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    let public = identity
        .public_identity()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    let status = identity
        .host_status()
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    if public.state != crate::internal::identity_provider::ProviderIdentityState::Active
        || identity
            .resume_document_change()
            .await
            .map_err(crate::internal::identity_provider::map_provider_error)?
            .is_some()
        || canonical_document_digest(&public.document)? != binding.document_digest
        || status.root_capability
            != if binding.root_capability_present {
                crate::internal::identity_provider::ProviderRootCapability::Active
            } else {
                crate::internal::identity_provider::ProviderRootCapability::Absent
            }
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let signing = public
        .active_keys
        .iter()
        .find(|key| {
            key.purposes
                .contains(&crate::internal::identity_provider::ProviderKeyPurpose::DeviceAssertion)
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let agreement = public
        .active_keys
        .iter()
        .find(|key| {
            key.purposes
                .contains(&crate::internal::identity_provider::ProviderKeyPurpose::KeyAgreement)
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    identity
        .sign(crate::internal::identity_provider::ProviderSignRequest {
            purpose: crate::internal::identity_provider::ProviderSigningPurpose::DeviceAssertion,
            key: crate::internal::identity_provider::ProviderKeySelector::Kid(signing.kid.clone()),
            payload: b"awiki identity custody migration verify".to_vec(),
        })
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    let peer = x25519_dalek::StaticSecret::from([97_u8; 32]);
    identity
        .derive_shared_secret(
            crate::internal::identity_provider::ProviderKeyAgreementRequest {
                key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                    agreement.kid.clone(),
                ),
                peer_public: x25519_dalek::PublicKey::from(&peer).to_bytes(),
            },
        )
        .await
        .map_err(crate::internal::identity_provider::map_provider_error)?;
    Ok(())
}

fn commit_cutover(
    core: &crate::core::ImCore,
    store: &IdentityStore<'_>,
    store_id: &str,
    bindings: &[CutoverBinding],
) -> crate::ImResult<()> {
    let lock = store.lock_index_mutation()?;
    let mut index = store.load_index()?;
    if index.identity_custody_cutover.is_some() || index.credentials.len() != bindings.len() {
        return Err(crate::ImError::PermissionDenied);
    }
    for binding in bindings {
        let entry = index
            .credentials
            .get_mut(&binding.identity_name)
            .ok_or(crate::ImError::PermissionDenied)?;
        if entry.did != binding.did
            || entry.unique_id != binding.source_unique_id
            || entry.dir_name != binding.source_dir_name
            || binding.store_id != store_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if entry.identity_custody_backend.as_deref() != Some(BACKEND) {
            let current = prepare_legacy_identity(core, store, &binding.identity_name, entry)?;
            if current.did.as_str() != binding.did
                || current.source_unique_id != binding.source_unique_id
                || current.source_dir_name != binding.source_dir_name
                || current.root_key.is_some() != binding.root_capability_present
                || current.evidence.document_digest != binding.document_digest
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        entry.identity_custody_backend = Some(BACKEND.to_owned());
        entry.anp_identity_store_id = Some(store_id.to_owned());
        entry.anp_identity_id = Some(binding.identity_id.clone());
        entry.anp_identity_auth_ref = binding.auth_ref.clone();
    }
    index.schema_version = IDENTITY_CUSTODY_CUTOVER_INDEX_SCHEMA_VERSION;
    index.identity_custody_cutover = Some(IdentityCustodyCutoverMarker {
        schema_version: IDENTITY_CUSTODY_CUTOVER_MARKER_SCHEMA_VERSION,
        backend: BACKEND.to_owned(),
        store_id: store_id.to_owned(),
        cutover_at: chrono::Utc::now().to_rfc3339(),
        cleanup_complete: false,
    });
    store.save_index_locked(&lock, index)
}

fn cleanup_after_cutover(
    core: &crate::core::ImCore,
    store: &IdentityStore<'_>,
) -> crate::ImResult<()> {
    let lock = store.lock_index_mutation()?;
    let mut index = store.load_index()?;
    let marker = index
        .identity_custody_cutover
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if marker.cleanup_complete {
        return Ok(());
    }
    let vault = core.inner().identity_vault().map(|context| context.vault());
    for entry in index.credentials.values_mut() {
        let identity_dir = store.local_identity_dir(&entry.dir_name)?;
        for name in ROOT_PRIVATE_FILES
            .iter()
            .chain([SIGNING_PRIVATE_FILE].iter())
            .chain(AGREEMENT_PRIVATE_FILES.iter())
            .chain(DAEMON_PRIVATE_FILES.iter())
        {
            remove_file_if_exists(&identity_dir.join(name))?;
        }
        if let Some(metadata) = entry.vault_migration.as_ref() {
            let vault = vault
                .as_ref()
                .ok_or_else(|| not_ready(&entry.did, "identity_secret_vault"))?;
            let mut refs = Vec::new();
            if let Some(vnext) = metadata.vnext_refs.as_ref() {
                refs.push(vnext.device_request_signing_private.clone());
                refs.push(vnext.e2ee_agreement_private.clone());
                refs.extend(vnext.did_document_root_private.clone());
            }
            refs.extend(metadata.refs.daemon_subkey_private.clone());
            refs.dedup();
            for reference in refs {
                vault.delete(&reference)?;
            }
        }
        entry.vault_migration = None;
    }
    index
        .identity_custody_cutover
        .as_mut()
        .ok_or(crate::ImError::PermissionDenied)?
        .cleanup_complete = true;
    index.schema_version = IDENTITY_CUSTODY_CUTOVER_INDEX_SCHEMA_VERSION;
    store.save_index_locked(&lock, index)
}

fn report_for_cutover_index(
    dry_run: bool,
    index: &crate::internal::identity_store::IndexPayload,
    marker: &IdentityCustodyCutoverMarker,
) -> IdentityCustodyMigrationReport {
    IdentityCustodyMigrationReport {
        dry_run,
        phase: if marker.cleanup_complete {
            IdentityCustodyMigrationPhase::Cleaned
        } else {
            IdentityCustodyMigrationPhase::Cutover
        },
        store_id: Some(marker.store_id.clone()),
        marker_written: true,
        cleanup_complete: marker.cleanup_complete,
        copied_count: 0,
        verified_count: index.credentials.len(),
        identities: index
            .credentials
            .iter()
            .map(|(name, entry)| IdentityCustodyMigrationIdentityReport {
                identity_name: name.clone(),
                did: entry.did.clone(),
                eligible: true,
                already_managed: true,
                root_capability_present: entry
                    .device_state
                    .as_ref()
                    .and_then(|state| state.authorization.as_ref())
                    .is_some_and(|authorization| authorization.management_ready),
                reason: None,
            })
            .collect(),
        blockers: Vec::new(),
        warnings: Vec::new(),
    }
}

fn ineligible_report(
    identity_name: &str,
    entry: &IndexEntry,
    reason: &str,
) -> IdentityCustodyMigrationIdentityReport {
    IdentityCustodyMigrationIdentityReport {
        identity_name: identity_name.to_owned(),
        did: entry.did.clone(),
        eligible: false,
        already_managed: false,
        root_capability_present: false,
        reason: Some(reason.to_owned()),
    }
}

fn common_store_id(bindings: &[CutoverBinding]) -> Option<String> {
    let first = bindings.first()?.store_id.clone();
    bindings
        .iter()
        .all(|binding| binding.store_id == first)
        .then_some(first)
}

fn canonical_document_digest(document: &serde_json::Value) -> crate::ImResult<String> {
    let canonical = serde_json_canonicalizer::to_vec(document).map_err(|_| {
        crate::ImError::invalid_input(
            Some("did_document".to_owned()),
            "DID document cannot be canonicalized",
        )
    })?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

fn read_optional_utf8(root: &Path, names: &[&str]) -> crate::ImResult<Option<Zeroizing<String>>> {
    for name in names {
        match fs::read(root.join(name)) {
            Ok(bytes) if bytes.is_empty() => continue,
            Ok(bytes) => {
                return String::from_utf8(bytes)
                    .map(Zeroizing::new)
                    .map(Some)
                    .map_err(|_| crate::ImError::CredentialFileUnreadable {
                        path_kind: "identity_private_key".to_owned(),
                        detail: "legacy identity key is not UTF-8".to_owned(),
                    });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(crate::ImError::from(error)),
        }
    }
    Ok(None)
}

fn read_required_utf8(
    root: &Path,
    names: &[&str],
    path_kind: &str,
) -> crate::ImResult<Zeroizing<String>> {
    read_optional_utf8(root, names)?.ok_or_else(|| crate::ImError::CredentialFileUnreadable {
        path_kind: path_kind.to_owned(),
        detail: "legacy identity key is missing".to_owned(),
    })
}

fn open_vault_utf8(
    vault: &dyn crate::internal::secret_vault::SecretVault,
    reference: &SecretRef,
    path_kind: &str,
) -> crate::ImResult<Zeroizing<String>> {
    let opened = vault.open(reference)?;
    String::from_utf8(opened.expose_secret().to_vec())
        .map(Zeroizing::new)
        .map_err(|_| crate::ImError::CredentialFileUnreadable {
            path_kind: path_kind.to_owned(),
            detail: "vault identity key is not UTF-8".to_owned(),
        })
}

fn remove_file_if_exists(path: &Path) -> crate::ImResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(crate::ImError::from(error)),
    }
}

fn not_ready(identity: &str, missing: &str) -> crate::ImError {
    crate::ImError::IdentityNotReady {
        identity: identity.to_owned(),
        missing: vec![missing.to_owned()],
    }
}

fn test_failure(point: &str) -> crate::ImError {
    crate::ImError::Internal {
        message: format!("identity custody migration fault at {point}"),
    }
}

#[cfg(test)]
mod tests;
