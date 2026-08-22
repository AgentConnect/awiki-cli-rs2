//! Converges identity pending records written before ANP Identity custody.

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::internal::identity_join_activation_pending::{
    JoinEnrollmentRef, PendingJoinActivation, PendingJoinActivationStore,
};
use crate::internal::identity_legacy_upgrade_pending::{
    LegacyUpgradeIdentityRef, PendingLegacyUpgrade, PendingLegacyUpgradeAttempt,
    PendingLegacyUpgradePhase, PendingLegacyUpgradeStore,
};
use crate::internal::identity_registration_pending::{
    PendingRegistration, PendingRegistrationIdentity, PendingRegistrationPhase,
    PendingRegistrationRemoteResult, PendingRegistrationStore,
};
use crate::internal::secret_vault::record::{SecretKind, SecretRef};

#[derive(Debug, Default)]
pub(crate) struct PendingUpgradeOutcome {
    pub(crate) blockers: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn converge(
    core: &crate::core::ImCore,
    dry_run: bool,
) -> crate::ImResult<PendingUpgradeOutcome> {
    let Some(context) = core.inner().identity_vault() else {
        return Ok(PendingUpgradeOutcome::default());
    };
    let vault = context.vault();
    let mut outcome = PendingUpgradeOutcome::default();
    let records = vault
        .list()?
        .into_iter()
        .filter(|record| pending_kind(&record.kind))
        .collect::<Vec<_>>();
    for reference in records {
        let opened = vault.open(&reference)?;
        let probe: SchemaProbe = match serde_json::from_slice(opened.expose_secret()) {
            Ok(probe) => probe,
            Err(_) => {
                outcome.blockers.push(blocker(&reference, "unreadable"));
                continue;
            }
        };
        if probe.schema_version != 1 {
            outcome.blockers.push(blocker(&reference, "active"));
            continue;
        }
        match reference.kind {
            SecretKind::IdentityRegistrationPending => {
                converge_registration(
                    core,
                    vault.as_ref(),
                    &reference,
                    opened.expose_secret(),
                    dry_run,
                    &mut outcome,
                )?;
            }
            SecretKind::IdentityHandleRecoveryPending => {
                converge_handle_recovery(
                    core,
                    vault.as_ref(),
                    &reference,
                    opened.expose_secret(),
                    dry_run,
                    &mut outcome,
                )?;
            }
            SecretKind::IdentityJoinActivationPending => {
                converge_join(
                    core,
                    &reference,
                    opened.expose_secret(),
                    dry_run,
                    &mut outcome,
                )?;
            }
            SecretKind::IdentityLegacyUpgradePending => {
                converge_legacy_upgrade(
                    core,
                    &reference,
                    opened.expose_secret(),
                    dry_run,
                    &mut outcome,
                )?;
            }
            SecretKind::IdentityRootImportPending => outcome.warnings.push(
                "legacy root-import pending will be imported directly after custody cutover"
                    .to_owned(),
            ),
            _ => outcome
                .blockers
                .push(blocker(&reference, "requires_reconcile")),
        }
    }
    outcome.blockers.sort();
    outcome.blockers.dedup();
    outcome.warnings.sort();
    outcome.warnings.dedup();
    Ok(outcome)
}

#[derive(Deserialize)]
struct SchemaProbe {
    schema_version: u32,
}

#[derive(Deserialize)]
struct LegacyRegistrationProbe {
    remote_attempted: bool,
    remote_result: Option<serde_json::Value>,
    phase: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyRegistrationPending {
    schema_version: u32,
    target_handle: String,
    target_domain: String,
    local_alias: String,
    display_name: String,
    make_default: bool,
    verification_kind: String,
    verification_target: Option<String>,
    invite_code: Option<String>,
    generated: crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    document_hash: String,
    phase: PendingRegistrationPhase,
    remote_attempted: bool,
    remote_result: Option<PendingRegistrationRemoteResult>,
}

fn converge_registration(
    core: &crate::core::ImCore,
    vault: &dyn crate::internal::secret_vault::SecretVault,
    reference: &SecretRef,
    raw: &[u8],
    dry_run: bool,
    outcome: &mut PendingUpgradeOutcome,
) -> crate::ImResult<()> {
    let pending: LegacyRegistrationProbe =
        serde_json::from_slice(raw).map_err(|_| crate::ImError::PermissionDenied)?;
    if !pending.remote_attempted && pending.remote_result.is_none() && pending.phase == "prepared" {
        if !dry_run {
            vault.delete(reference)?;
        }
        outcome.warnings.push(format!(
            "{} legacy registration pending is safe to discard before remote mutation",
            if dry_run { "eligible:" } else { "discarded:" }
        ));
    } else {
        if dry_run {
            outcome.warnings.push(
                "eligible: attempted legacy registration can import its exact identity before reconcile"
                    .to_owned(),
            );
        } else {
            let legacy: LegacyRegistrationPending = match serde_json::from_slice(raw) {
                Ok(legacy) => legacy,
                Err(_) => {
                    outcome.blockers.push(blocker(reference, "unreadable"));
                    return Ok(());
                }
            };
            upgrade_registration(core, reference, legacy)?;
            outcome.warnings.push(
                "upgraded attempted legacy registration for exact remote reconciliation".to_owned(),
            );
        }
        outcome.blockers.push(blocker(reference, "active"));
    }
    Ok(())
}

fn upgrade_registration(
    core: &crate::core::ImCore,
    legacy_ref: &SecretRef,
    legacy: LegacyRegistrationPending,
) -> crate::ImResult<()> {
    if legacy.schema_version != 1
        || legacy.document_hash
            != crate::internal::identity_wire::document::document_hash(
                &legacy.generated.did_document,
            )?
        || !legacy.remote_attempted
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let generated = legacy.generated;
    use anp_identity::host::MigrationPort;
    let mut manager = crate::internal::identity_custody::open_controller_manager(core)?;
    let existing = manager
        .list()
        .map_err(crate::internal::identity_custody::map_facade_error)?
        .into_iter()
        .find(|descriptor| descriptor.reference.did == generated.did.as_str());
    let identity = match existing {
        Some(descriptor) => manager
            .get(&descriptor.reference)
            .map_err(crate::internal::identity_custody::map_facade_error)?,
        None => manager
            .import_full_identity(anp_identity::host::FullIdentityImportRequest {
                remote: anp_identity::VerifiedRemoteDocument {
                    document: anp_identity::DidDocument::from_value(generated.did_document.clone()),
                    evidence: anp_identity::VerifiedPublicationEvidence {
                        document_version: 1,
                        registry_version: 1,
                        document_digest: anp_identity::canonical_document_digest(
                            &generated.did_document,
                        )
                        .map_err(crate::internal::identity_custody::map_error)?,
                    },
                },
                did_wba: true,
                private_keys: vec![
                    imported_key(
                        &generated.did_document,
                        &generated.root_key_id,
                        anp_identity::host::MigrationKeyPurpose::RootControl,
                        Zeroizing::new(generated.root_private_pem.clone()),
                    )?,
                    imported_key(
                        &generated.did_document,
                        &generated.device_signing_key_id,
                        anp_identity::host::MigrationKeyPurpose::DeviceAssertion,
                        Zeroizing::new(generated.device_signing_private_pem.clone()),
                    )?,
                    imported_key(
                        &generated.did_document,
                        &generated.device_e2ee_key_id,
                        anp_identity::host::MigrationKeyPurpose::KeyAgreement,
                        Zeroizing::new(generated.device_e2ee_private_pem.clone()),
                    )?,
                ],
            })
            .map_err(crate::internal::identity_custody::map_facade_error)?,
    };
    let public = identity
        .public_identity()
        .map_err(crate::internal::identity_custody::map_facade_error)?;
    if public.state != anp_identity::PublicIdentityState::Active
        || anp_identity::canonical_document_digest(public.document.as_value())
            .map_err(crate::internal::identity_custody::map_error)?
            != anp_identity::canonical_document_digest(&generated.did_document)
                .map_err(crate::internal::identity_custody::map_error)?
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let registration_identity = PendingRegistrationIdentity {
        controller_store_id: public.reference.store_id,
        controller_identity_id: public.reference.identity_id,
        did: generated.did,
        did_document: generated.did_document,
        protocol_device_id: generated.protocol_device_id,
        root_key_id: generated.root_key_id,
        device_signing_key_id: generated.device_signing_key_id,
        device_e2ee_key_id: generated.device_e2ee_key_id,
        legacy_daemon_authorization: true,
        controller_revision_id: None,
    };
    let mut current = PendingRegistration::new(
        legacy.target_handle,
        legacy.target_domain,
        legacy.local_alias,
        legacy.display_name,
        legacy.make_default,
        legacy.verification_kind,
        legacy.verification_target,
        legacy.invite_code,
        registration_identity,
    )?;
    current.phase = legacy.phase;
    current.remote_attempted = legacy.remote_attempted;
    current.remote_result = legacy.remote_result;
    current.validate()?;
    let pending_store = PendingRegistrationStore::from_core(core)?;
    let current_ref = pending_store.save(&current)?;
    if &current_ref != legacy_ref {
        pending_store.delete(legacy_ref)?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct LegacyRecoveryProbe {
    commit_attempted: bool,
    remote_result: Option<serde_json::Value>,
}

fn converge_handle_recovery(
    core: &crate::core::ImCore,
    vault: &dyn crate::internal::secret_vault::SecretVault,
    reference: &SecretRef,
    raw: &[u8],
    dry_run: bool,
    outcome: &mut PendingUpgradeOutcome,
) -> crate::ImResult<()> {
    let pending: LegacyRecoveryProbe =
        serde_json::from_slice(raw).map_err(|_| crate::ImError::PermissionDenied)?;
    if !pending.commit_attempted && pending.remote_result.is_none() {
        if !dry_run {
            vault.delete(reference)?;
        }
        outcome.warnings.push(format!(
            "{} legacy Handle Recovery pending is safe to discard before Commit",
            if dry_run { "eligible:" } else { "discarded:" }
        ));
    } else {
        if dry_run {
            outcome.warnings.push(
                "eligible: attempted legacy Handle Recovery can import its exact identity before result-get"
                    .to_owned(),
            );
        } else {
            crate::internal::identity_handle_recovery_pending::PendingHandleRecoveryStore::from_core(core)?
                .upgrade_legacy_v4(core, reference, raw)?;
            outcome.warnings.push(
                "upgraded attempted legacy Handle Recovery for exact result-get recovery"
                    .to_owned(),
            );
        }
        outcome.blockers.push(blocker(reference, "active"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyJoinPending {
    schema_version: u32,
    join_session_id: String,
    did: crate::ids::Did,
    resolved_document: serde_json::Value,
    authorization: crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
    signing_private_pem: String,
    e2ee_private_pem: String,
    access_result: Option<crate::internal::identity_device_join_runtime::DeviceJoinAccessResult>,
}

fn converge_join(
    core: &crate::core::ImCore,
    reference: &SecretRef,
    raw: &[u8],
    dry_run: bool,
    outcome: &mut PendingUpgradeOutcome,
) -> crate::ImResult<()> {
    let legacy: LegacyJoinPending =
        serde_json::from_slice(raw).map_err(|_| crate::ImError::PermissionDenied)?;
    if legacy.schema_version != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    if dry_run {
        outcome.warnings.push(
            "eligible: legacy Join pending can import the exact authorized device keys".to_owned(),
        );
        outcome.blockers.push(blocker(reference, "active"));
        return Ok(());
    }
    let custody = import_active_device(
        core,
        &legacy.did,
        &legacy.resolved_document,
        legacy.authorization.checkpoint.document_version,
        legacy.authorization.checkpoint.registry_version,
        &legacy.authorization.device.signing_key_id,
        Zeroizing::new(legacy.signing_private_pem),
        &legacy.authorization.device.e2ee_key_id,
        Zeroizing::new(legacy.e2ee_private_pem),
    )?;
    let mut current = PendingJoinActivation::new(
        legacy.join_session_id,
        legacy.did,
        legacy.resolved_document,
        legacy.authorization,
        custody,
    )?;
    current.access_result = legacy.access_result;
    let store = PendingJoinActivationStore::from_core(core)?;
    let current_ref = store.save(&current)?;
    if &current_ref != reference {
        store.delete(reference)?;
    }
    outcome
        .warnings
        .push("upgraded legacy Join pending into active ANP Identity custody".to_owned());
    outcome.blockers.push(blocker(reference, "active"));
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyUpgradePending {
    schema_version: u32,
    local_alias: String,
    source_document_hash: String,
    root_ref: SecretRef,
    generated: crate::internal::identity_legacy_upgrade::GeneratedLegacyUpgrade,
    phase: PendingLegacyUpgradePhase,
    attempt: PendingLegacyUpgradeAttempt,
    last_attempt_at: String,
    failure_code: Option<String>,
    checkpoint: Option<crate::internal::identity_device_state::IdentityInternalCheckpoint>,
    access_token: Option<String>,
}

fn converge_legacy_upgrade(
    core: &crate::core::ImCore,
    reference: &SecretRef,
    raw: &[u8],
    dry_run: bool,
    outcome: &mut PendingUpgradeOutcome,
) -> crate::ImResult<()> {
    let legacy: LegacyUpgradePending =
        serde_json::from_slice(raw).map_err(|_| crate::ImError::PermissionDenied)?;
    if legacy.schema_version != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    if legacy.phase != PendingLegacyUpgradePhase::Prepared
        || legacy.checkpoint.is_some()
        || legacy.access_token.is_some()
    {
        outcome
            .blockers
            .push(blocker(reference, "remote_outcome_requires_reconcile"));
        return Ok(());
    }
    if dry_run {
        outcome.warnings.push(
            "eligible: legacy upgrade pending can import the exact bootstrap device keys"
                .to_owned(),
        );
        outcome.blockers.push(blocker(reference, "active"));
        return Ok(());
    }
    let generated = legacy.generated;
    let custody = import_active_device(
        core,
        &generated.did,
        &generated.target_document,
        1,
        1,
        &generated.signing_key_id,
        Zeroizing::new(generated.signing_private_pem),
        &generated.e2ee_key_id,
        Zeroizing::new(generated.e2ee_private_pem),
    )?;
    let identity = LegacyUpgradeIdentityRef {
        custody,
        did: generated.did,
        protocol_device_id: generated.protocol_device_id,
        signing_public_key_multibase: public_multibase(
            &generated.target_document,
            &generated.signing_key_id,
        )?,
        signing_key_id: generated.signing_key_id,
        e2ee_public_key_multibase: public_multibase(
            &generated.target_document,
            &generated.e2ee_key_id,
        )?,
        e2ee_key_id: generated.e2ee_key_id,
        target_document: generated.target_document,
        target_document_hash: generated.target_document_hash,
    };
    let mut current = PendingLegacyUpgrade::new(
        legacy.local_alias,
        legacy.source_document_hash,
        legacy.root_ref,
        identity,
    )?;
    current.attempt = legacy.attempt;
    current.last_attempt_at = legacy.last_attempt_at;
    current.failure_code = legacy.failure_code;
    let store = PendingLegacyUpgradeStore::from_core(core)?;
    let current_ref = store.save(&current)?;
    if &current_ref != reference {
        store.delete(reference)?;
    }
    outcome
        .warnings
        .push("upgraded Legacy Upgrade pending into active ANP Identity custody".to_owned());
    outcome.blockers.push(blocker(reference, "active"));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn import_active_device(
    core: &crate::core::ImCore,
    did: &crate::ids::Did,
    document: &serde_json::Value,
    document_version: u64,
    registry_version: u64,
    signing_kid: &str,
    signing_pem: Zeroizing<String>,
    e2ee_kid: &str,
    e2ee_pem: Zeroizing<String>,
) -> crate::ImResult<JoinEnrollmentRef> {
    use anp_identity::host::MigrationPort;
    let mut manager = crate::internal::identity_custody::open_controller_manager(core)?;
    let existing = manager
        .list()
        .map_err(crate::internal::identity_custody::map_facade_error)?
        .into_iter()
        .find(|descriptor| descriptor.reference.did == did.as_str());
    let identity = match existing {
        Some(descriptor) => manager
            .get(&descriptor.reference)
            .map_err(crate::internal::identity_custody::map_facade_error)?,
        None => manager
            .import_device_identity(anp_identity::host::DeviceIdentityImportRequest {
                remote: anp_identity::VerifiedRemoteDocument {
                    document: anp_identity::DidDocument::from_value(document.clone()),
                    evidence: anp_identity::VerifiedPublicationEvidence {
                        document_version,
                        registry_version,
                        document_digest: anp_identity::canonical_document_digest(document)
                            .map_err(crate::internal::identity_custody::map_error)?,
                    },
                },
                did_wba: true,
                signing_key: imported_key(
                    document,
                    signing_kid,
                    anp_identity::host::MigrationKeyPurpose::DeviceAssertion,
                    signing_pem,
                )?,
                agreement_key: imported_key(
                    document,
                    e2ee_kid,
                    anp_identity::host::MigrationKeyPurpose::KeyAgreement,
                    e2ee_pem,
                )?,
            })
            .map_err(crate::internal::identity_custody::map_facade_error)?,
    };
    let public = identity
        .public_identity()
        .map_err(crate::internal::identity_custody::map_facade_error)?;
    if public.state != anp_identity::PublicIdentityState::Active
        || anp_identity::canonical_document_digest(public.document.as_value())
            .map_err(crate::internal::identity_custody::map_error)?
            != anp_identity::canonical_document_digest(document)
                .map_err(crate::internal::identity_custody::map_error)?
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(JoinEnrollmentRef {
        store_id: public.reference.store_id,
        identity_id: public.reference.identity_id,
        enrollment_id: crate::internal::identity_custody::LEGACY_IMPORTED_ACTIVE_ENROLLMENT_ID
            .to_owned(),
    })
}

fn imported_key(
    document: &serde_json::Value,
    kid: &str,
    purpose: anp_identity::host::MigrationKeyPurpose,
    pem: Zeroizing<String>,
) -> crate::ImResult<anp_identity::host::MigrationPrivateKey> {
    let material =
        anp::PrivateKeyMaterial::from_pem(&pem).map_err(|_| crate::ImError::PermissionDenied)?;
    let expected = anp::authentication::find_verification_method(document, kid)
        .and_then(|method| anp::authentication::extract_public_key(&method).ok())
        .ok_or(crate::ImError::PermissionDenied)?;
    if material.public_key().to_pem() != expected.to_pem() {
        return Err(crate::ImError::PermissionDenied);
    }
    let raw = match (purpose, material) {
        (
            anp_identity::host::MigrationKeyPurpose::RootControl
            | anp_identity::host::MigrationKeyPurpose::DeviceAssertion,
            anp::PrivateKeyMaterial::Ed25519(key),
        ) => key.to_bytes().to_vec(),
        (
            anp_identity::host::MigrationKeyPurpose::KeyAgreement,
            anp::PrivateKeyMaterial::X25519(key),
        ) => key.to_bytes().to_vec(),
        _ => return Err(crate::ImError::PermissionDenied),
    };
    Ok(anp_identity::host::MigrationPrivateKey {
        kid: kid.to_owned(),
        purpose,
        encoding: anp_identity::host::MigrationPrivateKeyEncoding::Raw32,
        secret: Zeroizing::new(raw),
    })
}

fn public_multibase(document: &serde_json::Value, kid: &str) -> crate::ImResult<String> {
    anp::authentication::find_verification_method(document, kid)
        .and_then(|method| {
            method
                .get("publicKeyMultibase")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .ok_or(crate::ImError::PermissionDenied)
}

fn pending_kind(kind: &SecretKind) -> bool {
    matches!(
        kind,
        SecretKind::IdentityJoinPairingPrivate
            | SecretKind::IdentityJoinSessionToken
            | SecretKind::IdentityJoinActivationPending
            | SecretKind::IdentityRegistrationPending
            | SecretKind::IdentityHandleRecoveryPending
            | SecretKind::IdentityLegacyUpgradePending
            | SecretKind::IdentityRootImportPending
            | SecretKind::IdentityAuthCommitPending
            | SecretKind::IdentityDeviceRevokePending
    )
}

fn blocker(reference: &SecretRef, reason: &str) -> String {
    format!(
        "unresolved identity pending record: {} ({reason})",
        reference.kind.as_str()
    )
}

#[cfg(test)]
mod tests;
