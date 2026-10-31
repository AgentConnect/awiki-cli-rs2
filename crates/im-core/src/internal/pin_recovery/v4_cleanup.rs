//! Exact old-binding cleanup after a confirmed V4, never a generic delete grant.
use super::{
    crypto, failure,
    pending::{Operation, Store},
    PinRecoveryError, PinRecoveryPhase,
};
use crate::internal::identity_handle_recovery_pending::{
    PendingHandleRecoveryStore, PendingHandleRecoveryV4, PendingRecoveryPhaseV4,
};
use crate::internal::identity_provider::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    Prepared,
    Isolated,
    Deleted,
    Retained,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Claim {
    v4_operation_id: String,
    account_user_id: String,
    previous_did: String,
    previous_generation: String,
    successor_did: String,
    successor_generation: String,
    stage: Stage,
    request: Option<ProviderPinRecoveryV4RetirementRequest>,
    #[serde(default)]
    isolated_generation: Option<u64>,
    retained_reason: Option<String>,
    #[serde(default)]
    core_cleared: bool,
    #[serde(default)]
    session_ids: Vec<String>,
}
impl Claim {
    fn provider_done(&self) -> bool {
        matches!(self.stage, Stage::Deleted | Stage::Retained)
    }
    fn done(&self) -> bool {
        self.provider_done() && self.core_cleared
    }
}

pub(crate) async fn finish(
    core: &crate::ImCore,
    v4_store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
) -> crate::ImResult<()> {
    if pending.phase != PendingRecoveryPhaseV4::Applied {
        return Ok(());
    }
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let old = pending
        .authoritative_binding
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?
        .clone();
    let result = pending
        .remote_result
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?
        .clone();
    result.validate_against(
        &pending.operation_id,
        pending
            .intent_hash
            .as_deref()
            .ok_or_else(|| failure("v4_evidence_required", false))?,
    )?;
    let mut candidates = Vec::new();
    for mut operation in store.list_operations()? {
        if operation.target.did != old.current_did
            || operation.target.full_handle != old.full_handle
            || operation.target.binding_generation != old.binding_generation
        {
            continue;
        }
        let account = account_for(&store, &operation)?;
        if account != result.account_user_id || account != old.account_user_id {
            continue;
        }
        operation.account_user_id = Some(account);
        if operation
            .v4_cleanup
            .as_ref()
            .is_some_and(|c| c.v4_operation_id != pending.operation_id)
        {
            return Err(failure("v4_cleanup_conflict", false));
        }
        candidates.push(operation);
    }
    if candidates.is_empty() {
        return set_pending(v4_store, pending, false);
    }
    if candidates
        .iter()
        .all(|o| o.v4_cleanup.as_ref().is_some_and(Claim::done))
    {
        return set_pending(v4_store, pending, false);
    }
    set_pending(v4_store, pending, true)?;
    let needs_authority = candidates.iter().any(|o| {
        o.v4_cleanup
            .as_ref()
            .is_none_or(|c| c.stage == Stage::Prepared)
    });
    let evidence = if needs_authority {
        Some(verified_evidence(core, pending).await?)
    } else {
        None
    };
    let provider = crate::internal::identity_custody::controller_custody_provider(core).await?;
    let old_ids = candidates
        .iter()
        .map(|o| o.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let session_ids = associated_sessions(
        &store,
        &candidates,
        &old_ids,
        &result.account_user_id,
        &old.current_did,
        &old.binding_generation,
    )?;
    for mut operation in candidates {
        if operation.v4_cleanup.as_ref().is_some_and(Claim::done) {
            continue;
        }
        if operation.v4_cleanup.is_none() {
            let mut claim = Claim {
                v4_operation_id: pending.operation_id.clone(),
                account_user_id: result.account_user_id.clone(),
                previous_did: old.current_did.clone(),
                previous_generation: old.binding_generation.clone(),
                successor_did: result.current_did.clone(),
                successor_generation: result.binding_generation.clone(),
                stage: Stage::Prepared,
                request: None,
                isolated_generation: None,
                retained_reason: None,
                core_cleared: false,
                session_ids: session_ids.clone(),
            };
            if operation.kind != crate::pin_recovery::PinRecoveryKind::Recover
                || !super::custody::must_observe_pending(&operation)
            {
                claim.stage = Stage::Retained;
                claim.retained_reason = Some("no_pin_ingress".into());
            } else {
                let reference = if let Some(proposal) = operation.custody_pending.as_ref() {
                    Some(
                        serde_json::from_value::<ProviderPinRecoveryProposal>(proposal.clone())
                            .map_err(|_| failure("corrupt_pending", false))?
                            .identity,
                    )
                } else {
                    let identities = provider
                        .list_identities()
                        .await
                        .map_err(map_provider_error)?;
                    let matches = identities
                        .into_iter()
                        .filter(|i| i.reference.did == operation.target.did)
                        .collect::<Vec<_>>();
                    if matches.len() > 1 {
                        return Err(failure("v4_cleanup_conflict", false));
                    }
                    matches.into_iter().next().map(|i| i.reference)
                };
                if let Some(reference) = reference {
                    if never_projected(core, Some(pending), &operation, &reference)? {
                        let identity = provider
                            .open_identity(&reference)
                            .await
                            .map_err(map_provider_error)?;
                        let generation = identity
                            .pin_root_generation()
                            .await
                            .map_err(map_provider_error)?;
                        claim.request = Some(ProviderPinRecoveryV4RetirementRequest {
                            identity: reference,
                            expected_generation: generation,
                            binding: saved_binding(&store, &operation, &result.account_user_id)?,
                            scope: ProviderPinRecoveryV4RetirementScope::UnprojectedPinRecovery {
                                never_projected: true,
                                absent_from_previous_custody: true,
                            },
                            evidence: evidence
                                .clone()
                                .ok_or_else(|| failure("v4_evidence_required", false))?,
                        });
                    } else {
                        claim.stage = Stage::Retained;
                        claim.retained_reason = Some("projected_or_historical_custody".into());
                    }
                } else {
                    claim.stage = Stage::Deleted;
                }
            }
            operation.v4_cleanup = Some(claim);
            store.save_operation(&operation)?; // claim precedes isolation/deletion
        }
        advance_claim(
            core,
            &store,
            provider.as_ref(),
            Some(pending),
            &mut operation,
            evidence.as_ref(),
        )
        .await?;
    }
    clean_sessions(
        &store,
        &old_ids,
        &result.account_user_id,
        &old.current_did,
        &old.binding_generation,
    )?;
    for id in &old_ids {
        let mut operation = store.operation(id)?;
        if operation
            .v4_cleanup
            .as_ref()
            .is_some_and(Claim::provider_done)
        {
            operation.v4_cleanup.as_mut().unwrap().core_cleared = true;
            store.save_operation(&operation)?;
        }
    }
    set_pending(v4_store, pending, false)
}
async fn advance_claim(
    core: &crate::ImCore,
    store: &Store,
    provider: &dyn IdentityCustody,
    pending: Option<&PendingHandleRecoveryV4>,
    operation: &mut Operation,
    evidence: Option<&ProviderVerifiedPinRecoveryV4Supersession>,
) -> crate::ImResult<()> {
    let claim = operation
        .v4_cleanup
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?
        .clone();
    if claim.stage == Stage::Prepared {
        let mut request = claim
            .request
            .clone()
            .ok_or_else(|| failure("corrupt_pending", false))?;
        if let Some(evidence) = evidence {
            request.evidence.current_document = evidence.current_document.clone();
        }
        operation.v4_cleanup.as_mut().unwrap().request = Some(request.clone());
        store.save_operation(operation)?;
        // Recheck the host-only unprojected scope under the same operation lock.
        if !never_projected(core, pending, operation, &request.identity)? {
            operation.v4_cleanup.as_mut().unwrap().stage = Stage::Retained;
            operation.v4_cleanup.as_mut().unwrap().retained_reason =
                Some("projection_observed_before_isolation".into());
        } else {
            let outcome = provider
                .retire_pin_root_recovery_after_v4(request.clone())
                .await
                .map_err(map_provider_error)?;
            if outcome.identity != request.identity
                || outcome.generation < request.expected_generation
            {
                return Err(failure("v4_cleanup_conflict", false));
            }
            operation.v4_cleanup.as_mut().unwrap().stage = Stage::Isolated;
            operation.v4_cleanup.as_mut().unwrap().isolated_generation = Some(outcome.generation);
        }
        store.save_operation(operation)?;
    }
    if operation
        .v4_cleanup
        .as_ref()
        .is_some_and(|c| c.stage == Stage::Isolated)
    {
        let reference = &operation
            .v4_cleanup
            .as_ref()
            .unwrap()
            .request
            .as_ref()
            .ok_or_else(|| failure("corrupt_pending", false))?
            .identity;
        match provider.open_identity(reference).await {
            Ok(identity) => {
                let public = identity
                    .public_identity()
                    .await
                    .map_err(map_provider_error)?;
                let host = identity.host_status().await.map_err(map_provider_error)?;
                let generation = identity
                    .pin_root_generation()
                    .await
                    .map_err(map_provider_error)?;
                if public.reference != *reference
                    || public.state != ProviderIdentityState::Revoked
                    || host.root_capability != ProviderRootCapability::Absent
                    || Some(generation)
                        != operation.v4_cleanup.as_ref().unwrap().isolated_generation
                {
                    return Err(failure("v4_cleanup_conflict", false));
                }
            }
            Err(e) if e.code == IdentityProviderErrorCode::IdentityNotFound => {
                operation.v4_cleanup.as_mut().unwrap().stage = Stage::Deleted;
                clear_old(operation);
                store.save_operation(operation)?;
                return Ok(());
            }
            Err(e) => return Err(map_provider_error(e)),
        }
        match provider.delete_identity(reference).await {
            Ok(()) => {}
            Err(e) if e.code == IdentityProviderErrorCode::IdentityNotFound => {}
            Err(e) => return Err(map_provider_error(e)),
        }
        operation.v4_cleanup.as_mut().unwrap().stage = Stage::Deleted;
        store.save_operation(operation)?;
    }
    if operation
        .v4_cleanup
        .as_ref()
        .is_some_and(Claim::provider_done)
    {
        clear_old(operation);
        store.save_operation(operation)?;
    }
    Ok(())
}
fn set_pending(
    store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
    value: bool,
) -> crate::ImResult<()> {
    if pending.pin_recovery_cleanup_pending == value {
        return Ok(());
    }
    let revision = pending.revision;
    pending.pin_recovery_cleanup_pending = value;
    pending.revision = revision
        .checked_add(1)
        .ok_or_else(|| failure("corrupt_pending", false))?;
    store.save_v4_cas(pending, revision).map(|_| ())
}
fn account_for(store: &Store, operation: &Operation) -> crate::ImResult<String> {
    if let Some(account) = &operation.account_user_id {
        return Ok(account.clone());
    }
    let session = store.historical_session_for_cleanup(&operation.session_id)?;
    let token = session
        .pst
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    super::jwt_payload(&token.access_token)?["sub"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| failure("corrupt_pending", false))
}
fn saved_binding(
    store: &Store,
    o: &Operation,
    account: &str,
) -> crate::ImResult<ProviderPinRecoveryBinding> {
    let s = store.historical_session_for_cleanup(&o.session_id)?;
    let r = o
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    if account_for(store, o)? != account {
        return Err(failure("v4_cleanup_conflict", false));
    }
    let expected = ProviderPinRecoveryBinding {
        operation_id: o.id.clone(),
        account_user_id: account.into(),
        target_id: o.target.target_id.clone(),
        handle: o.target.full_handle.clone(),
        did: o.target.did.clone(),
        domain: super::trusted_domain(&crate::ids::Did::parse(&o.target.did)?)?,
        binding_generation: o.target.binding_generation.clone(),
        recovery_id: r.recovery_id.clone(),
        backup_epoch: r.backup_epoch,
        slot_id: r.slot_id.clone(),
        op_jkt: r.op_jkt.clone(),
        root_kid: o.target.root_kid.clone(),
        root_public_key_multibase: crypto::root_multibase(&o.target.root_public_jwk)?,
        encrypted_package_hash: r.backup_hash.clone(),
        config_hash: r.config_hash.clone(),
        audience: s.config.control_base_url.clone(),
    };
    if let Some(proposal) = o.custody_pending.as_ref() {
        let p: ProviderPinRecoveryProposal = serde_json::from_value(proposal.clone())
            .map_err(|_| failure("corrupt_pending", false))?;
        if p.binding != expected {
            return Err(failure("v4_cleanup_conflict", false));
        }
    }
    Ok(expected)
}
fn clear_old(o: &mut Operation) {
    super::clear_ephemeral(o);
    o.recovered_identity = None;
    o.phase = PinRecoveryPhase::Cancelled;
    o.last_error = Some(PinRecoveryError {
        code: "superseded_by_v4".into(),
        retryable: false,
        retry_after_seconds: None,
    });
    if o.v4_cleanup
        .as_ref()
        .is_some_and(|c| c.stage == Stage::Deleted)
    {
        o.custody_pending = None;
        o.custody_ingress_started = false;
    }
}
fn never_projected(
    core: &crate::ImCore,
    v4: Option<&PendingHandleRecoveryV4>,
    o: &Operation,
    reference: &ProviderIdentityRef,
) -> crate::ImResult<bool> {
    if o.recovered_identity.is_some()
        || o.phase == PinRecoveryPhase::Completed
        || v4.is_some_and(|v| v.previous_custody.as_ref() == Some(reference))
    {
        return Ok(false);
    }
    let paths = &core.inner().sdk_paths().identities;
    let has_ref = |index: &crate::internal::identity_store::IndexPayload| {
        index.credentials.values().any(|e| {
            e.anp_identity_store_id.as_deref() == Some(reference.store_id.as_str())
                && e.anp_identity_id.as_deref() == Some(reference.identity_id.as_str())
                && e.did == reference.did
        })
    };
    if has_ref(&crate::internal::identity_store::IdentityStore::new(paths).load_index()?) {
        return Ok(false);
    }
    let old =
        PendingHandleRecoveryStore::from_core(core)?.list_v4_for_handle(&o.target.full_handle)?;
    if old
        .iter()
        .any(|(_, p)| p.previous_custody.as_ref() == Some(reference))
    {
        return Ok(false);
    }
    let backups = paths.identity_root_dir.join(".recovery-registry-backups");
    if backups.try_exists()? {
        if !std::fs::symlink_metadata(&backups)?.file_type().is_dir() {
            return Ok(false);
        }
        for entry in std::fs::read_dir(backups)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Ok(false);
            }
            if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let mut backup_paths = paths.clone();
            backup_paths.registry_path = entry.path();
            match crate::internal::identity_store::IdentityStore::new(&backup_paths).load_index() {
                Ok(index) if has_ref(&index) => return Ok(false),
                Ok(_) => {}
                Err(_) => return Ok(false),
            }
        }
    }
    // Public projection remnants and retirement records conservatively preserve custody.
    let entries = match std::fs::read_dir(&paths.identity_root_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(false),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if kind.is_symlink() {
            return Ok(false);
        }
        if !kind.is_dir() {
            continue;
        }
        let file = entry.path().join("identity.json");
        if !file.try_exists()? {
            continue;
        }
        match std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        {
            Some(value) if value["did"].as_str() == Some(reference.did.as_str()) => {
                return Ok(false)
            }
            Some(_) => {}
            None => return Ok(false),
        }
    }
    let retirements = paths.identity_root_dir.join(".identity-retirements");
    if retirements.try_exists()? {
        if !std::fs::symlink_metadata(&retirements)?
            .file_type()
            .is_dir()
        {
            return Ok(false);
        }
        for entry in std::fs::read_dir(retirements)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Ok(false);
            }
            match std::fs::read(entry.path())
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            {
                Some(value) if value["did"].as_str() == Some(reference.did.as_str()) => {
                    return Ok(false)
                }
                Some(_) => {}
                None => return Ok(false),
            }
        }
    }
    Ok(true)
}
async fn verified_evidence(
    core: &crate::ImCore,
    pending: &PendingHandleRecoveryV4,
) -> crate::ImResult<ProviderVerifiedPinRecoveryV4Supersession> {
    let result = pending
        .remote_result
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?;
    let old = pending
        .authoritative_binding
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?;
    let intent = pending
        .intent
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?;
    if intent.hash()?.as_str() != pending.intent_hash.as_deref().unwrap_or_default()
        || intent.expected_previous_did != old.current_did
        || intent.expected_binding_generation != old.binding_generation
        || intent.account_user_id != result.account_user_id
        || intent.new_did != result.current_did
        || result.previous_did != old.current_did
        || result.account_user_id != old.account_user_id
        || result.full_handle != old.full_handle
        || pending.local_previous_did != old.current_did
        || pending.full_handle != old.full_handle
    {
        return Err(failure("v4_evidence_required", false));
    }
    let marker = crate::internal::identity_transition_pending::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
    )?
    .ok_or_else(|| failure("v4_evidence_required", false))?;
    if marker.phase != crate::internal::identity_transition_pending::TransitionPhase::Completed
        || marker.source_kind
            != crate::internal::identity_transition_pending::TransitionSourceKind::Initiator
        || marker.source_id != pending.operation_id
        || marker.recovery_id != pending.operation_id
        || marker.owner_identity_id != pending.owner_identity_id
        || marker.current_device_id.as_deref() != Some(result.bootstrap_device.device_id.as_str())
        || marker.device_auth_generation.as_deref()
            != Some(result.bootstrap_device.auth_generation.to_string().as_str())
        || marker.registry_version.as_deref()
            != Some(result.checkpoint.registry_version.to_string().as_str())
        || marker.previous_did != old.current_did
        || marker.current_did != result.current_did
        || marker.account_user_id != result.account_user_id
        || marker.binding_generation != result.binding_generation
        || marker.handle != result.full_handle
    {
        return Err(failure("v4_evidence_required", false));
    }
    crate::internal::identity_handle_recovery_authority::require_current_binding(
        core, pending, &marker,
    )
    .await?;
    let client = core
        .client_async(crate::IdentitySelector::Did(crate::ids::Did::parse(
            &result.current_did,
        )?))
        .await?;
    let mut runtime =
        crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production(
            core, &client,
        );
    let registry = runtime.registry().await?;
    let mut transport = crate::internal::transport::CorePlainTransport::new_no_redirect(core);
    let document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut transport,
        &result.current_did,
    )
    .await?;
    if crate::internal::identity_wire::document::document_hash(&document)?
        != registry.checkpoint.document_hash
    {
        return Err(failure("document_changed", true));
    }
    Ok(ProviderVerifiedPinRecoveryV4Supersession {
        v4_operation_id: pending.operation_id.clone(),
        v4_intent_hash: pending.intent_hash.clone().unwrap(),
        account_user_id: result.account_user_id.clone(),
        full_handle: result.full_handle.clone(),
        previous_did: old.current_did.clone(),
        previous_binding_generation: old.binding_generation.clone(),
        successor_did: result.current_did.clone(),
        successor_binding_generation: result.binding_generation.clone(),
        committed_checkpoint: ProviderDocumentCheckpoint {
            document_version: result.checkpoint.document_version,
            registry_version: result.checkpoint.registry_version,
            document_digest: result.checkpoint.document_hash.clone(),
        },
        current_binding: ProviderPinRecoveryCurrentBinding {
            full_handle: result.full_handle.clone(),
            did: result.current_did.clone(),
            binding_generation: result.binding_generation.clone(),
        },
        current_document: ProviderVerifiedRemoteDocument {
            document,
            evidence: ProviderPublicationEvidence {
                document_version: registry.checkpoint.document_version,
                registry_version: registry.checkpoint.registry_version,
                document_digest: registry.checkpoint.document_hash,
            },
        },
    })
}

fn session_account(session: &super::pending::Session) -> Option<String> {
    session
        .pst
        .as_ref()
        .and_then(|p| super::jwt_payload(&p.access_token).ok())
        .and_then(|p| p["sub"].as_str().map(str::to_owned))
}
fn exact_is_old(
    key: &str,
    request: &super::pending::ExactRequest,
    old: &std::collections::BTreeSet<String>,
) -> bool {
    request
        .operation_id
        .as_ref()
        .is_some_and(|id| old.contains(id))
        || old.iter().any(|id| {
            key.starts_with(&format!("{id}:"))
                || request.url.ends_with(&format!("/operations/{id}"))
                || request.url.contains(&format!("/operations/{id}/"))
        })
}
fn associated_sessions(
    store: &Store,
    operations: &[Operation],
    old: &std::collections::BTreeSet<String>,
    account: &str,
    did: &str,
    generation: &str,
) -> crate::ImResult<Vec<String>> {
    let mut ids = operations
        .iter()
        .map(|o| o.session_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for session in store.list_sessions_for_cleanup()? {
        let selected = session
            .selected
            .as_ref()
            .is_some_and(|t| t.did == did && t.binding_generation == generation)
            && session_account(&session).as_deref() == Some(account);
        if selected
            || session
                .resume_operation_id
                .as_ref()
                .is_some_and(|id| old.contains(id))
            || session
                .exact_requests
                .iter()
                .any(|(k, r)| exact_is_old(k, r, old))
        {
            ids.insert(session.id.clone());
        }
    }
    Ok(ids.into_iter().collect())
}
fn clean_sessions(
    store: &Store,
    old: &std::collections::BTreeSet<String>,
    account: &str,
    did: &str,
    generation: &str,
) -> crate::ImResult<()> {
    let operations = store.list_operations()?;
    let mut ids = std::collections::BTreeSet::new();
    for o in &operations {
        if old.contains(&o.id) {
            if let Some(c) = o.v4_cleanup.as_ref() {
                ids.extend(c.session_ids.clone());
            }
        }
    }
    for mut session in store.list_sessions_for_cleanup()? {
        if !ids.contains(&session.id) {
            continue;
        }
        let foreign_active = operations.iter().any(|o| {
            o.session_id == session.id
                && !old.contains(&o.id)
                && !matches!(
                    o.phase,
                    PinRecoveryPhase::Completed | PinRecoveryPhase::Cancelled
                )
        });
        let old_selected = session
            .selected
            .as_ref()
            .is_some_and(|t| t.did == did && t.binding_generation == generation)
            && session_account(&session).as_deref() == Some(account);
        let old_unselected = session.selected.is_none()
            && session
                .resume_operation_id
                .as_ref()
                .is_some_and(|id| old.contains(id));
        if old_selected || old_unselected {
            if foreign_active {
                return Err(failure("v4_session_scope_unproven", false));
            }
            store.delete_session_for_cleanup(&session.id)?;
        } else {
            let explicit_new = session
                .selected
                .as_ref()
                .is_some_and(|t| t.did != did || t.binding_generation != generation)
                && session_account(&session).is_some();
            if !explicit_new {
                return Err(failure("v4_session_scope_unproven", false));
            }
            // A new binding can legitimately reuse the DPoP key after fresh OTP.
            // Retain its credentials and requests, pruning only exact old owners.
            session
                .exact_requests
                .retain(|key, request| !exact_is_old(key, request, old));
            if session
                .exact_requests
                .iter()
                .any(|(key, request)| key.starts_with("realm:") && request.operation_id.is_none())
            {
                return Err(failure("v4_session_scope_unproven", false));
            }
            if session
                .resume_operation_id
                .as_ref()
                .is_some_and(|id| old.contains(id))
            {
                session.resume_operation_id = None;
            }
            store.save_session(&session)?;
        }
    }
    Ok(())
}

/// Preserve exact cleanup independently before an explicitly deleted V4 journal
/// disappears. This transfers no network authority and never widens provider deletion.
pub(crate) fn handoff_for_local_deletion(
    store: &Store,
    pending: &PendingHandleRecoveryV4,
) -> crate::ImResult<()> {
    if pending.phase != PendingRecoveryPhaseV4::Applied || !pending.pin_recovery_cleanup_pending {
        return Ok(());
    }
    let _lock = store.lock_sync()?;
    let old = pending
        .authoritative_binding
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?;
    let result = pending
        .remote_result
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?;
    result.validate_against(
        &pending.operation_id,
        pending
            .intent_hash
            .as_deref()
            .ok_or_else(|| failure("v4_evidence_required", false))?,
    )?;
    let intent = pending
        .intent
        .as_ref()
        .ok_or_else(|| failure("v4_evidence_required", false))?;
    if intent.hash()?.as_str() != pending.intent_hash.as_deref().unwrap_or_default()
        || result.previous_did != old.current_did
        || result.account_user_id != old.account_user_id
        || result.full_handle != old.full_handle
        || intent.expected_previous_did != old.current_did
        || intent.expected_binding_generation != old.binding_generation
        || intent.new_did != result.current_did
    {
        return Err(failure("v4_evidence_required", false));
    }
    let mut candidates = Vec::new();
    for mut operation in store.list_operations()? {
        if operation.target.did != old.current_did
            || operation.target.binding_generation != old.binding_generation
            || operation.target.full_handle != old.full_handle
            || account_for(store, &operation)? != old.account_user_id
        {
            continue;
        }
        operation.account_user_id = Some(old.account_user_id.clone());
        candidates.push(operation);
    }
    let ids = candidates
        .iter()
        .map(|o| o.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let sessions = associated_sessions(
        store,
        &candidates,
        &ids,
        &old.account_user_id,
        &old.current_did,
        &old.binding_generation,
    )?;
    for mut operation in candidates {
        if let Some(claim) = operation.v4_cleanup.as_ref() {
            if claim.v4_operation_id != pending.operation_id {
                return Err(failure("v4_cleanup_conflict", false));
            }
        } else {
            // No previously verified retirement request exists. Preserve custody
            // rather than inventing fresh authority after identity deletion.
            operation.v4_cleanup = Some(Claim {
                v4_operation_id: pending.operation_id.clone(),
                account_user_id: old.account_user_id.clone(),
                previous_did: old.current_did.clone(),
                previous_generation: old.binding_generation.clone(),
                successor_did: result.current_did.clone(),
                successor_generation: result.binding_generation.clone(),
                stage: Stage::Retained,
                request: None,
                isolated_generation: None,
                retained_reason: Some("owner_deleted_before_retirement_authority_capture".into()),
                core_cleared: false,
                session_ids: sessions.clone(),
            });
        }
        store.save_operation(&operation)?;
    }
    Ok(())
}

/// Resumes only durable exact V4 cleanup claims. It never signs, guesses a PIN,
/// recreates an identity, or derives a grant from a service rejection.
pub(crate) async fn recover(core: &crate::ImCore) -> crate::ImResult<()> {
    if core.inner().identity_secret_storage_policy()
        != crate::IdentitySecretStoragePolicy::VaultRequired
        || core.inner().identity_vault().is_none()
    {
        return Ok(());
    }
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut groups = std::collections::BTreeMap::<String, Vec<Operation>>::new();
    for operation in store.list_operations()? {
        if let Some(claim) = operation.v4_cleanup.as_ref() {
            groups
                .entry(claim.v4_operation_id.clone())
                .or_default()
                .push(operation);
        }
    }
    if groups.is_empty() {
        return Ok(());
    }
    let provider = crate::internal::identity_custody::controller_custody_provider(core).await?;
    for operations in groups.into_values() {
        if operations
            .iter()
            .all(|o| o.v4_cleanup.as_ref().is_some_and(Claim::done))
        {
            continue;
        }
        let first = operations[0].v4_cleanup.as_ref().unwrap().clone();
        let old_ids = operations
            .iter()
            .map(|o| o.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for mut operation in operations {
            let claim = operation.v4_cleanup.as_ref().unwrap();
            if claim.account_user_id != first.account_user_id
                || claim.previous_did != first.previous_did
                || claim.previous_generation != first.previous_generation
                || operation.target.did != claim.previous_did
                || operation.target.binding_generation != claim.previous_generation
                || operation.account_user_id.as_deref() != Some(claim.account_user_id.as_str())
            {
                return Err(failure("v4_cleanup_conflict", false));
            }
            if !claim.done() {
                advance_claim(core, &store, provider.as_ref(), None, &mut operation, None).await?;
            }
        }
        clean_sessions(
            &store,
            &old_ids,
            &first.account_user_id,
            &first.previous_did,
            &first.previous_generation,
        )?;
        for id in old_ids {
            let mut operation = store.operation(&id)?;
            if operation
                .v4_cleanup
                .as_ref()
                .is_some_and(Claim::provider_done)
            {
                operation.v4_cleanup.as_mut().unwrap().core_cleared = true;
                store.save_operation(&operation)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
