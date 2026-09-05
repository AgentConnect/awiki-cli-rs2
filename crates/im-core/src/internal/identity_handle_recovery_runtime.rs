//! Host-neutral orchestration for Manifest Handle Recovery V4.0.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde_json::{json, Value};

use crate::identity::{
    AuthorizedJoinActivationProgress, AuthorizedJoinActivationRequest,
    HandleRecoveryAccountEpochReceipt, HandleRecoveryActivateRequest, HandleRecoveryDiscardRequest,
    HandleRecoveryErrorCode, HandleRecoveryImpact, HandleRecoveryKeyState,
    HandleRecoveryOperationLifecycle, HandleRecoveryOperationSummary, HandleRecoveryOtpRequest,
    HandleRecoveryOtpResult, HandleRecoveryPhase, HandleRecoveryPrepareRequest,
    HandleRecoveryProgress, HandleRecoveryQuarantineRequest, HandleRecoveryResetReference,
    HandleRecoveryResumeRequest, HandleRecoveryTransitionSourceKind,
};
use crate::internal::identity_handle_recovery_pending::{
    PendingHandleRecoveryStore, PendingHandleRecoveryV4, PendingRecoveryPhaseV4,
};
use crate::internal::transport::{AsyncRestTransport as _, AsyncRpcTransport as _};

struct RecoveryLocalContext {
    owner_identity_id: String,
    local_alias: String,
    display_name: String,
    make_default: bool,
    local_previous_did: String,
    fresh_local_state: bool,
    identity: Option<crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef>,
}

pub(crate) async fn request_otp(
    core: &crate::core::ImCore,
    request: HandleRecoveryOtpRequest,
) -> crate::ImResult<HandleRecoveryOtpResult> {
    require_enabled(core)?;
    let canonical =
        crate::internal::identity_wire::handle_recovery::canonical_handle(&request.full_handle)?;
    let request_lock_scope = format!("handle:{}", canonical.full);
    let lock = core.inner().handle_recovery_lock(&request_lock_scope);
    let _guard = lock.lock().await;
    crate::internal::identity_local_deletion::ensure_no_active_deletion_at_path(
        &core.inner().sdk_paths().local_state.sqlite_path,
        "",
        Some(&canonical.full),
    )?;
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let store = PendingHandleRecoveryStore::from_core(core)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    let context = if let Some(selector) = request.identity {
        require_explicit_identity(&selector)?;
        let identity = core.identities().resolve_async(selector).await?;
        if identity.handle.as_ref().map(|handle| handle.as_str()) != Some(canonical.full.as_str()) {
            return Err(recovery_error(
                HandleRecoveryErrorCode::LocalMigrationUnsupported,
            ));
        }
        let entry = index
            .credentials
            .values()
            .find(|entry| entry.unique_id == identity.id.as_str())
            .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalMigrationUnsupported))?;
        RecoveryLocalContext {
            owner_identity_id: identity.id.as_str().to_owned(),
            local_alias: (!entry.credential_name.trim().is_empty())
                .then(|| entry.credential_name.clone())
                .or_else(|| identity.local_alias.clone())
                .ok_or_else(|| {
                    recovery_error(HandleRecoveryErrorCode::LocalMigrationUnsupported)
                })?,
            display_name: identity
                .display_name
                .clone()
                .unwrap_or_else(|| canonical.local_part.clone()),
            make_default: identity.is_default,
            local_previous_did: identity.did.as_str().to_owned(),
            fresh_local_state: false,
            identity: None,
        }
    } else {
        let local_matches = index
            .credentials
            .values()
            .filter(|entry| entry.full_handle == canonical.full)
            .collect::<Vec<_>>();
        if local_matches.len() > 1 {
            return Err(recovery_error(
                HandleRecoveryErrorCode::LocalMigrationUnsupported,
            ));
        }
        if let Some(entry) = local_matches.first() {
            RecoveryLocalContext {
                owner_identity_id: entry.unique_id.clone(),
                local_alias: entry.credential_name.clone(),
                display_name: if entry.name.trim().is_empty() {
                    canonical.local_part.clone()
                } else {
                    entry.name.clone()
                },
                make_default: entry.is_default,
                local_previous_did: entry.did.clone(),
                fresh_local_state: false,
                identity: None,
            }
        } else if let Some(existing) = crate::internal::identity_handle_recovery_operation::list_handle(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &canonical.full,
        )?
        .into_iter()
        .find(|record| {
            matches!(
                record.lifecycle_class,
                crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
                    | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved
                    | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted
                    | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending
            )
        }) {
            let (_, pending) = store
                .load_v4(&existing.operation_id)
                .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
                .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
            RecoveryLocalContext {
                owner_identity_id: pending.owner_identity_id,
                local_alias: pending.local_alias,
                display_name: pending.display_name,
                make_default: pending.make_default,
                local_previous_did: pending.local_previous_did,
                fresh_local_state: pending.fresh_local_state,
                identity: None,
            }
        } else {
            let mut vault_only = store
                .list_v4_for_handle(&canonical.full)?
                .into_iter()
                .map(|(_, pending)| pending)
                .filter(|pending| {
                    pending.phase == PendingRecoveryPhaseV4::AwaitingFactor
                        && !pending.commit_attempted
                })
                .collect::<Vec<_>>();
            if vault_only.len() > 1 {
                return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
            }
            if let Some(pending) = vault_only.pop() {
                RecoveryLocalContext {
                    owner_identity_id: pending.owner_identity_id,
                    local_alias: pending.local_alias,
                    display_name: pending.display_name,
                    make_default: pending.make_default,
                    local_previous_did: pending.local_previous_did,
                    fresh_local_state: pending.fresh_local_state,
                    identity: None,
                }
            } else {
            let identity =
                crate::internal::identity_custody::provision_handle_recovery_identity_async(
                    core,
                    &canonical.domain,
                    &canonical.local_part,
                )
                .await?;
            let unique_id = identity.unique_id()?;
            RecoveryLocalContext {
                owner_identity_id: unique_id.clone(),
                local_alias: unique_id,
                display_name: canonical.local_part.clone(),
                make_default: index.default_credential_name.is_empty(),
                local_previous_did: format!("{}:unbound", identity.did.as_str()),
                fresh_local_state: true,
                identity: Some(identity),
            }
            }
        }
    };
    reconcile_vault_only_awaiting_factor_operation(
        core,
        &store,
        &context.owner_identity_id,
        &canonical.full,
        &context.local_previous_did,
        context.fresh_local_state,
    )?;
    let existing = reusable_awaiting_factor_operation(
        core,
        &store,
        &context.owner_identity_id,
        &canonical.full,
        &context.local_previous_did,
        context.fresh_local_state,
    )?;
    let operation_id = if let Some(operation_id) = existing {
        operation_id
    } else {
        let operation_id = random_reference("recover_v4")?;
        let identity = match context.identity.clone() {
            Some(identity) => identity,
            None => {
                crate::internal::identity_custody::provision_handle_recovery_identity_async(
                    core,
                    &canonical.domain,
                    &canonical.local_part,
                )
                .await?
            }
        };
        let pending = PendingHandleRecoveryV4::new_pre_otp(
            operation_id.clone(),
            context.owner_identity_id.clone(),
            context.local_alias.clone(),
            context.display_name.clone(),
            context.make_default,
            context.fresh_local_state,
            canonical.full.clone(),
            context.local_previous_did.clone(),
            identity,
        )?;
        store.create_v4(&pending)?;
        let now = format_timestamp(
            time::OffsetDateTime::now_utc()
                .replace_nanosecond(0)
                .map_err(|_| crate::ImError::PermissionDenied)?,
        )?;
        let operation = crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
                operation_id.clone(),
                context.owner_identity_id.clone(),
                canonical.full.clone(),
                crate::internal::identity_handle_recovery_pending::pending_v4_key_id(&operation_id),
                now,
            )?;
        insert_precommit_operation_or_cleanup(core, &store, &pending, &operation).await?;
        operation_id
    };
    let call = crate::internal::identity_wire::handle_recovery::build_send_otp_call(
        &request.phone,
        &canonical.full,
        &operation_id,
    )?;
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let raw = transport
        .rpc(call.endpoint, call.method, call.params)
        .await?;
    let (accepted, retry_after_seconds, retry_at) = parse_otp_send_boundary(&raw)?;
    Ok(HandleRecoveryOtpResult {
        owner_identity_id: crate::ids::IdentityId::parse(&context.owner_identity_id)?,
        full_handle: canonical.full,
        operation_id,
        accepted,
        retry_after_seconds,
        retry_at,
    })
}

fn reconcile_vault_only_awaiting_factor_operation(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    owner_identity_id: &str,
    full_handle: &str,
    local_previous_did: &str,
    fresh_local_state: bool,
) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let vault_only = store
        .list_v4_for_owner(owner_identity_id)?
        .into_iter()
        .filter_map(
            |(_, pending)| match crate::internal::identity_handle_recovery_operation::load(
                sqlite_path,
                &pending.operation_id,
            ) {
                Ok(None) => Some(Ok(pending)),
                Ok(Some(_)) => None,
                Err(error) => Some(Err(error)),
            },
        )
        .collect::<crate::ImResult<Vec<_>>>()?;
    if vault_only.is_empty() {
        return Ok(());
    }
    if vault_only.len() != 1 {
        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
    }
    let pending = &vault_only[0];
    if pending.phase != PendingRecoveryPhaseV4::AwaitingFactor
        || pending.commit_attempted
        || pending.owner_identity_id != owner_identity_id
        || pending.full_handle != full_handle
        || pending.local_previous_did != local_previous_did
        || pending.fresh_local_state != fresh_local_state
    {
        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
    }
    let has_actionable_index =
        crate::internal::identity_handle_recovery_operation::list_owner(
            sqlite_path,
            owner_identity_id,
        )?
        .into_iter()
        .any(|record| {
            matches!(
                record.lifecycle_class,
                crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
                    | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved
                    | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted
                    | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending
            )
        });
    if has_actionable_index {
        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
    }
    let operation =
        crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            pending.operation_id.clone(),
            pending.owner_identity_id.clone(),
            pending.full_handle.clone(),
            crate::internal::identity_handle_recovery_pending::pending_v4_key_id(
                &pending.operation_id,
            ),
            now_second_z()?,
        )?;
    crate::internal::identity_handle_recovery_operation::insert(sqlite_path, &operation)
}

fn reusable_awaiting_factor_operation(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    owner_identity_id: &str,
    full_handle: &str,
    local_previous_did: &str,
    fresh_local_state: bool,
) -> crate::ImResult<Option<String>> {
    let existing = crate::internal::identity_handle_recovery_operation::list_owner(
        &core.inner().sdk_paths().local_state.sqlite_path,
        owner_identity_id,
    )?
    .into_iter()
    .find(|record| {
        matches!(
            record.lifecycle_class,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
                | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved
                | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted
                | crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending
        )
    });
    let Some(existing) = existing else {
        return Ok(None);
    };
    if existing.key_state
        != crate::internal::identity_handle_recovery_operation::RecoveryKeyState::Available
        || existing.full_handle != full_handle
        || existing.vault_key_id
            != crate::internal::identity_handle_recovery_pending::pending_v4_key_id(
                &existing.operation_id,
            )
    {
        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
    }
    let (_, pending) = store
        .load_v4(&existing.operation_id)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    let pre_attempt = existing.lifecycle_class
        == crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
        && !existing.commit_attempted
        && pending.phase == PendingRecoveryPhaseV4::AwaitingFactor
        && !pending.commit_attempted;
    let post_attempt_refresh = existing.lifecycle_class
        == crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved
        && existing.commit_attempted
        && pending.phase == PendingRecoveryPhaseV4::RemoteOutcomeUnknown
        && pending.commit_attempted
        && pending.authoritative_binding.is_some()
        && pending.intent.is_some()
        && pending.intent_hash.is_some();
    if (!pre_attempt && !post_attempt_refresh)
        || pending.owner_identity_id != owner_identity_id
        || pending.full_handle != full_handle
        || pending.local_previous_did != local_previous_did
        || pending.fresh_local_state != fresh_local_state
    {
        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
    }
    Ok(Some(existing.operation_id))
}

fn parse_otp_send_boundary(raw: &Value) -> crate::ImResult<(bool, u32, String)> {
    let accepted =
        raw.get("ok")
            .and_then(Value::as_bool)
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "handle recovery OTP response is missing ok".to_owned(),
            })?;
    if !accepted {
        return Err(crate::ImError::Serialization {
            detail: "handle recovery OTP response was not accepted".to_owned(),
        });
    }
    let seconds = raw
        .get("retry_after_seconds")
        .and_then(Value::as_u64)
        .filter(|seconds| (1..=3600).contains(seconds))
        .and_then(|seconds| u32::try_from(seconds).ok())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "handle recovery OTP retry_after_seconds is invalid".to_owned(),
        })?;
    let retry_at = raw
        .get("retry_at")
        .and_then(Value::as_str)
        .filter(|value| value.ends_with('Z'))
        .filter(|value| {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .is_ok()
        })
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "handle recovery OTP retry_at is invalid".to_owned(),
        })?
        .to_owned();
    Ok((accepted, seconds, retry_at))
}

pub(crate) async fn list_operations(
    core: &crate::core::ImCore,
    identity: crate::identity::IdentitySelector,
) -> crate::ImResult<Vec<HandleRecoveryOperationSummary>> {
    require_enabled(core)?;
    require_explicit_identity(&identity)?;
    let owner_identity_id = match identity {
        crate::identity::IdentitySelector::Id(identity_id) => identity_id.as_str().to_owned(),
        selector => core
            .identities()
            .resolve_async(selector)
            .await?
            .id
            .as_str()
            .to_owned(),
    };
    crate::internal::identity_handle_recovery_operation::list_owner(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &owner_identity_id,
    )?
    .into_iter()
    .map(operation_summary)
    .collect()
}

pub(crate) async fn discard_pre_attempt(
    core: &crate::core::ImCore,
    request: HandleRecoveryDiscardRequest,
) -> crate::ImResult<HandleRecoveryOperationSummary> {
    require_enabled(core)?;
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let index = crate::internal::identity_handle_recovery_operation::load(
        sqlite_path,
        &request.operation_id,
    )?
    .ok_or_else(operation_not_found_error)?;
    let store = PendingHandleRecoveryStore::from_core(core)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    if index.lifecycle_class
        == crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::DiscardedPreAttempt
        && !index.commit_attempted
        && index.key_state
            == crate::internal::identity_handle_recovery_operation::RecoveryKeyState::DestroyedPreAttempt
    {
        if let Some((_, pending)) = store.load_v4(&request.operation_id)? {
            crate::internal::identity_custody::discard_unpublished_handle_recovery_async(
                core,
                &pending.identity,
            )
            .await?;
        }
        store.delete_v4_pre_attempt(&request.operation_id)?;
        return operation_summary(index);
    }
    if index.lifecycle_class
        != crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
        || index.commit_attempted
    {
        return Err(recovery_error(HandleRecoveryErrorCode::OutcomeUnknown));
    }
    let (_, pending) = store
        .load_v4(&request.operation_id)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    if pending.commit_attempted {
        return Err(recovery_error(HandleRecoveryErrorCode::OutcomeUnknown));
    }
    // SQLite is the non-secret destructive authority. Claim the pre-attempt
    // operation atomically before deleting Vault material so an in-flight
    // activate/Commit and discard cannot both win. A crash after this write is
    // safe: a repeated discard idempotently finishes Vault cleanup.
    crate::internal::identity_handle_recovery_operation::discard_pre_attempt(
        sqlite_path,
        &request.operation_id,
        &now_second_z()?,
    )?;
    crate::internal::identity_custody::discard_unpublished_handle_recovery_async(
        core,
        &pending.identity,
    )
    .await?;
    store.delete_v4_pre_attempt(&request.operation_id)?;
    operation_summary(
        crate::internal::identity_handle_recovery_operation::load(
            sqlite_path,
            &request.operation_id,
        )?
        .ok_or(crate::ImError::PermissionDenied)?,
    )
}

pub(crate) async fn quarantine_key_unavailable(
    core: &crate::core::ImCore,
    request: HandleRecoveryQuarantineRequest,
) -> crate::ImResult<HandleRecoveryOperationSummary> {
    require_enabled(core)?;
    if !request.user_presence_confirmed {
        return Err(user_presence_required_error());
    }
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let operation = crate::internal::identity_handle_recovery_operation::load(
        sqlite_path,
        &request.operation_id,
    )?
    .ok_or_else(operation_not_found_error)?;
    if operation.lifecycle_class
        == crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::Applied
        || operation.key_state
            == crate::internal::identity_handle_recovery_operation::RecoveryKeyState::DestroyedPreAttempt
    {
        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
    }
    if operation.key_state
        != crate::internal::identity_handle_recovery_operation::RecoveryKeyState::PermanentlyUnavailable
    {
        if let Ok(store) = PendingHandleRecoveryStore::from_core(core) {
            if let Ok(Some((_, pending))) = store.load_v4(&request.operation_id) {
                match crate::internal::identity_custody::handle_recovery_identity_async(
                    core,
                    &pending.identity,
                )
                .await
                {
                    Ok(_) => {
                        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
                    }
                    Err(crate::ImError::IdentityNotFound { .. }) => {}
                    Err(_) => {
                        return Err(recovery_error(
                            HandleRecoveryErrorCode::LocalKeyUnavailable,
                        ));
                    }
                }
            }
        }
    }
    crate::internal::identity_handle_recovery_operation::quarantine_key_unavailable(
        sqlite_path,
        &request.operation_id,
        &now_second_z()?,
    )?;
    crate::internal::identity_handle_recovery_metrics::record_key_unavailable();
    operation_summary(
        crate::internal::identity_handle_recovery_operation::load(
            sqlite_path,
            &request.operation_id,
        )?
        .ok_or(crate::ImError::PermissionDenied)?,
    )
}

pub(crate) async fn authorized_receipt(
    core: &crate::core::ImCore,
    identity: crate::identity::IdentitySelector,
) -> crate::ImResult<Option<HandleRecoveryAccountEpochReceipt>> {
    require_enabled(core)?;
    require_explicit_identity(&identity)?;
    let identity = core.identities().resolve_async(identity).await?;
    let marker = crate::internal::identity_transition_pending::load_latest_applied_for_owner(
        &core.inner().sdk_paths().local_state.sqlite_path,
        identity.id.as_str(),
    )?;
    let Some(marker) = marker else {
        return Ok(None);
    };
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let entry = index
        .credentials
        .values()
        .find(|entry| entry.unique_id == identity.id.as_str())
        .ok_or(crate::ImError::PermissionDenied)?;
    if marker.contract_version
        != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION
        || marker.account_user_id != entry.user_id
        || marker.handle != entry.full_handle
        || marker.current_did != entry.did
        || Some(marker.binding_generation.as_str()) != entry.binding_generation.as_deref()
    {
        return Ok(None);
    }
    Ok(Some(receipt_projection(&marker)?))
}

fn operation_summary(
    record: crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord,
) -> crate::ImResult<HandleRecoveryOperationSummary> {
    use crate::internal::identity_handle_recovery_operation::{
        RecoveryKeyState as InternalKeyState, RecoveryLifecycleClass as InternalLifecycle,
    };
    Ok(HandleRecoveryOperationSummary {
        operation_id: record.operation_id,
        owner_identity_id: crate::ids::IdentityId::parse(&record.owner_identity_id)?,
        account_user_id: record.account_user_id,
        full_handle: record.full_handle,
        lifecycle_class: match record.lifecycle_class {
            InternalLifecycle::PreCommit => HandleRecoveryOperationLifecycle::PreCommit,
            InternalLifecycle::RemoteUnresolved => {
                HandleRecoveryOperationLifecycle::RemoteUnresolved
            }
            InternalLifecycle::RemoteCommitted => HandleRecoveryOperationLifecycle::RemoteCommitted,
            InternalLifecycle::LocalTransitionPending => {
                HandleRecoveryOperationLifecycle::LocalTransitionPending
            }
            InternalLifecycle::Applied => HandleRecoveryOperationLifecycle::Applied,
            InternalLifecycle::DiscardedPreAttempt => {
                HandleRecoveryOperationLifecycle::DiscardedPreAttempt
            }
            InternalLifecycle::QuarantinedKeyUnavailable => {
                HandleRecoveryOperationLifecycle::QuarantinedKeyUnavailable
            }
            InternalLifecycle::SupersededByStateChange => {
                HandleRecoveryOperationLifecycle::SupersededByStateChange
            }
            InternalLifecycle::FailedTerminal => HandleRecoveryOperationLifecycle::FailedTerminal,
        },
        commit_attempted: record.commit_attempted,
        key_state: match record.key_state {
            InternalKeyState::Available => HandleRecoveryKeyState::Available,
            InternalKeyState::TemporarilyLocked => HandleRecoveryKeyState::TemporarilyLocked,
            InternalKeyState::PermanentlyUnavailable => {
                HandleRecoveryKeyState::PermanentlyUnavailable
            }
            InternalKeyState::DestroyedPreAttempt => HandleRecoveryKeyState::DestroyedPreAttempt,
        },
        intent_hash: record.intent_hash,
        state_root_fingerprint: record.state_root_fingerprint,
        superseded_by_operation_id: record.superseded_by_operation_id,
        last_error_code: record.last_error_code,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn receipt_projection(
    marker: &crate::internal::identity_transition_pending::IdentityTransitionMarker,
) -> crate::ImResult<HandleRecoveryAccountEpochReceipt> {
    Ok(HandleRecoveryAccountEpochReceipt {
        receipt_schema_version: marker.schema_version.to_string(),
        source_kind: match marker.source_kind {
            crate::internal::identity_transition_pending::TransitionSourceKind::Initiator => {
                HandleRecoveryTransitionSourceKind::Initiator
            }
            crate::internal::identity_transition_pending::TransitionSourceKind::JoinedDevice => {
                HandleRecoveryTransitionSourceKind::JoinedDevice
            }
        },
        source_id: marker.source_id.clone(),
        account_user_id: marker.account_user_id.clone(),
        owner_identity_id: crate::ids::IdentityId::parse(&marker.owner_identity_id)?,
        full_handle: marker.handle.clone(),
        local_previous_did: crate::ids::Did::parse(&marker.previous_did)?,
        current_did: crate::ids::Did::parse(&marker.current_did)?,
        binding_generation: marker.binding_generation.clone(),
        current_device_id: crate::ids::ProtocolDeviceId::parse(
            marker
                .current_device_id
                .as_deref()
                .ok_or(crate::ImError::PermissionDenied)?,
        )?,
        device_auth_generation: marker
            .device_auth_generation
            .as_deref()
            .and_then(|value| value.parse().ok())
            .ok_or(crate::ImError::PermissionDenied)?,
        registry_version: marker
            .registry_version
            .as_deref()
            .and_then(|value| value.parse().ok())
            .ok_or(crate::ImError::PermissionDenied)?,
        state_root_fingerprint: marker.state_root_fingerprint.clone(),
        applied_at: marker
            .applied_at
            .clone()
            .ok_or(crate::ImError::PermissionDenied)?,
        metadata_json: marker.metadata_json.clone(),
    })
}

pub(crate) async fn prepare(
    core: &crate::core::ImCore,
    request: HandleRecoveryPrepareRequest,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    let operation_id = crate::internal::identity_wire::handle_recovery::validate_operation_id(
        &request.operation_id,
    )?;
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let operation =
        crate::internal::identity_handle_recovery_operation::load(sqlite_path, &operation_id)?
            .ok_or_else(operation_not_found_error)?;
    let lock = core
        .inner()
        .handle_recovery_lock(&operation.owner_identity_id);
    let _guard = lock.lock().await;
    let operation =
        crate::internal::identity_handle_recovery_operation::load(sqlite_path, &operation_id)?
            .ok_or_else(operation_not_found_error)?;
    let store = PendingHandleRecoveryStore::from_core(core)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    let (_, mut pending) = store
        .load_v4(&operation_id)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    let pre_attempt = operation.lifecycle_class
        == crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
        && !operation.commit_attempted
        && pending.phase == PendingRecoveryPhaseV4::AwaitingFactor
        && !pending.commit_attempted;
    let post_attempt_refresh = operation.lifecycle_class
        == crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved
        && operation.commit_attempted
        && pending.phase == PendingRecoveryPhaseV4::RemoteOutcomeUnknown
        && pending.commit_attempted;
    if pending.owner_identity_id != operation.owner_identity_id
        || operation.key_state
            != crate::internal::identity_handle_recovery_operation::RecoveryKeyState::Available
        || (!pre_attempt && !post_attempt_refresh)
    {
        return Err(recovery_error(HandleRecoveryErrorCode::OutcomeUnknown));
    }
    if post_attempt_refresh {
        match reconcile_result_v4(core, &store, &mut pending).await? {
            ReconcileV4Outcome::Committed => return progress_v4(core, &pending),
            ReconcileV4Outcome::ResultAbsent => {}
            ReconcileV4Outcome::FactorRetryRequired => {
                return Err(recovery_error(HandleRecoveryErrorCode::FactorRetryRequired));
            }
            ReconcileV4Outcome::Unavailable => {
                return Err(recovery_error(HandleRecoveryErrorCode::OutcomeUnknown));
            }
        }
    }
    let bootstrap_signing_public_key = pending.bootstrap_signing_public_key()?;
    let exchange = crate::internal::identity_wire::handle_recovery::build_grant_exchange_call_v4(
        &request.phone,
        &request.code,
        &pending.full_handle,
        &operation_id,
        &pending.identity.device_signing_key_id,
        &bootstrap_signing_public_key,
    )?;
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let result = match transport
        .rest_post(exchange.endpoint, exchange.method, exchange.body)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let Some(code) = service_code(&error) else {
                return Err(error);
            };
            let Some(code) =
                crate::internal::identity_wire::handle_recovery::RecoveryExchangeErrorCodeV4::parse(
                    code,
                )
            else {
                return Err(error);
            };
            let public = exchange_error_projection(code);
            let revision = pending.revision;
            pending.record_retryable_error(public.as_str().to_owned())?;
            store.save_v4_cas(&pending, revision)?;
            crate::internal::identity_handle_recovery_operation::record_nonterminal_error(
                sqlite_path,
                &operation_id,
                Some(public.as_str()),
                &now_second_z()?,
            )?;
            return Err(recovery_error(public));
        }
    };
    let grant = crate::internal::identity_wire::handle_recovery::parse_grant_exchange_result_v4(
        result,
        &pending.full_handle,
    )?;
    let authoritative_binding =
        crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
            account_user_id: grant.current_binding.account_user_id.clone(),
            full_handle: grant.current_binding.full_handle.clone(),
            current_did: grant.current_binding.current_did.clone(),
            binding_generation: grant.current_binding.binding_generation.clone(),
        };
    if post_attempt_refresh
        && pending.authoritative_binding.as_ref() != Some(&authoritative_binding)
    {
        // A previously absent Commit may have completed between the first
        // Result Get and this fresh binding read. Reconcile once more before
        // any direct-previous or break-glass classification. The Handle change
        // and committed-result insert share one server transaction, so a
        // second absent result is the safe state-change discriminator.
        match reconcile_result_v4(core, &store, &mut pending).await? {
            ReconcileV4Outcome::Committed => return progress_v4(core, &pending),
            ReconcileV4Outcome::ResultAbsent => {
                mark_post_attempt_state_changed(core, &pending)?;
                return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
            }
            ReconcileV4Outcome::FactorRetryRequired => {
                return Err(recovery_error(HandleRecoveryErrorCode::FactorRetryRequired));
            }
            ReconcileV4Outcome::Unavailable => {
                return Err(recovery_error(HandleRecoveryErrorCode::OutcomeUnknown));
            }
        }
    }
    let local_index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    if pending.factor_state
        == crate::internal::identity_handle_recovery_pending::RecoveryFactorStateV4::AwaitingOtp
    {
        let expected_committed_generation =
            crate::internal::identity_handle_recovery_pending::increment_canonical_generation(
                &grant.current_binding.binding_generation,
            )
            .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalMigrationUnsupported))?;
        let owner_match = crate::internal::identity_local_owner_matcher::match_stable_owner(
            sqlite_path,
            &local_index,
            crate::internal::identity_local_owner_matcher::StableOwnerAuthority {
                account_user_id: &grant.current_binding.account_user_id,
                full_handle: &grant.current_binding.full_handle,
                previous_did: &grant.current_binding.current_did,
                binding_generation: &expected_committed_generation,
            },
            Some(&operation_id),
            None,
        )?;
        match owner_match {
            crate::internal::identity_local_owner_matcher::StableOwnerMatch::Exact(candidate) => {
                pending.freeze_local_owner(&candidate, &grant.current_binding.current_did)?;
            }
            crate::internal::identity_local_owner_matcher::StableOwnerMatch::None
            | crate::internal::identity_local_owner_matcher::StableOwnerMatch::Conflict => {
                pending.freeze_fresh_local_owner(&grant.current_binding.current_did)?;
            }
        }
    }

    let direct_local_transition = grant.current_binding.current_did == pending.local_previous_did;
    // The only V4.0 exception to a direct local transition is the explicitly
    // confirmed key-unavailable break-glass path. It creates a fresh local
    // crypto/control epoch; transparent N-k history adoption remains V4.1.
    let fresh_break_glass = if !direct_local_transition && !pending.fresh_local_state {
        let authorized = break_glass_authority_for_exchange(
            sqlite_path,
            &pending,
            pre_attempt,
            post_attempt_refresh,
            &now_second_z()?,
        )?;
        crate::internal::identity_handle_recovery_metrics::record_break_glass(if authorized {
            crate::internal::identity_handle_recovery_metrics::BreakGlassResult::Authorized
        } else {
            crate::internal::identity_handle_recovery_metrics::BreakGlassResult::Rejected
        });
        authorized
    } else {
        false
    };
    if !direct_local_transition && !fresh_break_glass && !pending.fresh_local_state {
        if post_attempt_refresh {
            mark_post_attempt_state_changed(core, &pending)?;
        }
        return Err(recovery_error(if post_attempt_refresh {
            HandleRecoveryErrorCode::UnknownEpoch
        } else {
            HandleRecoveryErrorCode::LocalMigrationUnsupported
        }));
    }
    let local_migration_supported = if pending.fresh_local_state {
        true
    } else {
        let local_entry = local_index
            .credentials
            .values()
            .find(|entry| entry.unique_id == pending.owner_identity_id)
            .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalMigrationUnsupported))?;
        local_migration_mode_v4(
            &pending,
            &grant.current_binding.account_user_id,
            &grant.current_binding.full_handle,
            &grant.current_binding.current_did,
            &local_entry.user_id,
            &local_entry.full_handle,
            &local_entry.did,
            fresh_break_glass,
        )
        .is_some()
    };
    if !local_migration_supported {
        if post_attempt_refresh {
            mark_post_attempt_state_changed(core, &pending)?;
        }
        return Err(recovery_error(if post_attempt_refresh {
            HandleRecoveryErrorCode::UnknownEpoch
        } else {
            HandleRecoveryErrorCode::LocalMigrationUnsupported
        }));
    }
    if pending.fresh_local_state {
        let refreshed =
            crate::internal::identity_custody::refresh_fresh_handle_recovery_document_async(
                core,
                &pending.identity,
            )
            .await?;
        pending.replace_identity_document_proof(refreshed)?;
    }
    let recovery_grant = String::from_utf8(grant.recovery_grant.expose_secret().to_vec())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let expected_revision = pending.revision;
    let previous_operation_owner = operation.owner_identity_id.clone();
    match pending.factor_state {
        crate::internal::identity_handle_recovery_pending::RecoveryFactorStateV4::AwaitingOtp => {
            pending.freeze_exchange(authoritative_binding, recovery_grant, grant.expires_at)?;
        }
        crate::internal::identity_handle_recovery_pending::RecoveryFactorStateV4::Exchanged => {
            pending.refresh_grant(&authoritative_binding, recovery_grant, grant.expires_at)?;
        }
    }
    store.save_v4_cas(&pending, expected_revision)?;
    let frozen_account_user_id = &pending
        .authoritative_binding
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?
        .account_user_id;
    let frozen_intent_hash = pending
        .intent_hash
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if pre_attempt {
        crate::internal::identity_handle_recovery_operation::record_frozen_owner_and_intent(
            sqlite_path,
            &operation_id,
            &previous_operation_owner,
            &pending.owner_identity_id,
            frozen_account_user_id,
            frozen_intent_hash,
            &now_second_z()?,
        )?;
    } else {
        crate::internal::identity_handle_recovery_operation::record_frozen_intent(
            sqlite_path,
            &operation_id,
            frozen_account_user_id,
            frozen_intent_hash,
            &now_second_z()?,
        )?;
    }
    progress_v4(core, &pending)
}

fn break_glass_authority_for_exchange(
    sqlite_path: &std::path::Path,
    pending: &PendingHandleRecoveryV4,
    pre_attempt: bool,
    post_attempt_refresh: bool,
    now: &str,
) -> crate::ImResult<bool> {
    if pre_attempt {
        return crate::internal::identity_handle_recovery_operation::claim_quarantined_replacement(
            sqlite_path,
            &pending.operation_id,
            &pending.owner_identity_id,
            &pending.full_handle,
            now,
        );
    }
    if post_attempt_refresh {
        return crate::internal::identity_handle_recovery_operation::is_quarantined_replacement(
            sqlite_path,
            &pending.operation_id,
            &pending.owner_identity_id,
            &pending.full_handle,
        );
    }
    Ok(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalMigrationModeV4 {
    Direct,
    ConfirmedFreshBreakGlass,
}

#[allow(clippy::too_many_arguments)]
fn local_migration_mode_v4(
    pending: &PendingHandleRecoveryV4,
    authoritative_account_user_id: &str,
    authoritative_full_handle: &str,
    authoritative_current_did: &str,
    local_account_user_id: &str,
    local_full_handle: &str,
    local_current_did: &str,
    fresh_break_glass: bool,
) -> Option<LocalMigrationModeV4> {
    if local_full_handle != authoritative_full_handle {
        return None;
    }
    if local_current_did == authoritative_current_did
        && pending.local_previous_did == authoritative_current_did
    {
        return Some(LocalMigrationModeV4::Direct);
    }
    (fresh_break_glass
        && pending.local_previous_did == local_current_did
        && local_account_user_id == authoritative_account_user_id)
        .then_some(LocalMigrationModeV4::ConfirmedFreshBreakGlass)
}

fn mark_post_attempt_state_changed(
    core: &crate::core::ImCore,
    pending: &PendingHandleRecoveryV4,
) -> crate::ImResult<()> {
    crate::internal::identity_handle_recovery_operation::update_lifecycle(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::SupersededByStateChange,
        None,
        Some(
            crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4::StateChangedRequiresNewOperation
                .as_str(),
        ),
        &now_second_z()?,
    )
}

pub(crate) async fn activate(
    core: &crate::core::ImCore,
    request: HandleRecoveryActivateRequest,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    if !request.user_presence_confirmed {
        return Err(user_presence_required_error());
    }
    require_v4_journal(core, &request.operation_id)?;
    advance_v4(core, &request.operation_id).await
}

pub(crate) async fn resume(
    core: &crate::core::ImCore,
    request: HandleRecoveryResumeRequest,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    require_v4_journal(core, &request.operation_id)?;
    advance_v4(core, &request.operation_id).await
}

pub(crate) fn status(
    core: &crate::core::ImCore,
    operation_id: &str,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    let store = PendingHandleRecoveryStore::from_core(core)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    if let Some((_, pending)) = store
        .load_v4(operation_id)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
    {
        return progress_v4(core, &pending);
    }
    if crate::internal::identity_handle_recovery_operation::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        operation_id,
    )?
    .is_some()
    {
        return Err(recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable));
    }
    Err(operation_not_found_error())
}

fn require_v4_journal(core: &crate::core::ImCore, operation_id: &str) -> crate::ImResult<()> {
    let store = PendingHandleRecoveryStore::from_core(core)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    if store
        .load_v4(operation_id)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
        .is_some()
    {
        return Ok(());
    }
    if crate::internal::identity_handle_recovery_operation::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        operation_id,
    )?
    .is_some()
    {
        return Err(recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable));
    }
    Err(operation_not_found_error())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileV4Outcome {
    Committed,
    ResultAbsent,
    FactorRetryRequired,
    Unavailable,
}

async fn advance_v4(
    core: &crate::core::ImCore,
    operation_id: &str,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let store = PendingHandleRecoveryStore::from_core(core)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    let (_, before_lock) = store
        .load_v4(operation_id)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    let lock = core
        .inner()
        .handle_recovery_lock(&before_lock.owner_identity_id);
    let _guard = lock.lock().await;
    let (_, mut pending) = store
        .load_v4(operation_id)
        .map_err(|_| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalKeyUnavailable))?;
    let operation =
        crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)?
            .ok_or_else(operation_not_found_error)?;
    reconcile_frozen_intent_index(sqlite_path, &operation, &pending, &now_second_z()?)?;
    let operation =
        crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)?
            .ok_or_else(operation_not_found_error)?;
    if pending.owner_identity_id != operation.owner_identity_id {
        return Err(crate::ImError::PermissionDenied);
    }
    merge_commit_attempted_authorities(
        sqlite_path,
        &store,
        &operation,
        &mut pending,
        &now_second_z()?,
    )?;
    let operation =
        crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
    require_v4_local_migration_authority(sqlite_path, &pending)?;
    reconcile_v4_lifecycle_index(sqlite_path, &operation, &pending, &now_second_z()?)?;
    if pending.phase == PendingRecoveryPhaseV4::AwaitingFactor {
        return Err(recovery_error(HandleRecoveryErrorCode::FactorRetryRequired));
    }
    if pending.phase == PendingRecoveryPhaseV4::ReadyToCommit {
        let _ = send_commit_v4(core, &store, &mut pending).await?;
    } else if pending.phase == PendingRecoveryPhaseV4::RemoteOutcomeUnknown {
        match reconcile_result_v4(core, &store, &mut pending).await? {
            ReconcileV4Outcome::Committed => {}
            ReconcileV4Outcome::ResultAbsent => {
                if grant_is_fresh(&pending)? {
                    let _ = send_commit_v4(core, &store, &mut pending).await?;
                } else {
                    persist_nonterminal_error_v4(
                        core,
                        &store,
                        &mut pending,
                        HandleRecoveryErrorCode::FactorRetryRequired,
                    )?;
                }
            }
            ReconcileV4Outcome::FactorRetryRequired => {}
            ReconcileV4Outcome::Unavailable => return progress_v4(core, &pending),
        }
    }
    if pending.phase == PendingRecoveryPhaseV4::RemoteCommitted {
        let result = pending
            .remote_result
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let marker =
            crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator_v4(
                sqlite_path,
                &pending,
                result,
            )?;
        crate::internal::identity_transition_pending::persist(sqlite_path, &marker)?;
        crate::internal::identity_handle_recovery_operation::update_lifecycle(
            sqlite_path,
            operation_id,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending,
            Some(&marker.state_root_fingerprint),
            None,
            &now_second_z()?,
        )?;
        let revision = pending.revision;
        pending.mark_local_transition_pending()?;
        store.save_v4_cas(&pending, revision)?;
    }
    if pending.phase == PendingRecoveryPhaseV4::LocalTransitionPending {
        if let Err(error) = apply_local_transition_v4(core, &store, &mut pending).await {
            let Some(code) = local_transition_retry_code(&error) else {
                return Err(error);
            };
            persist_nonterminal_error_code_v4(core, &store, &mut pending, code.as_str())?;
            return Err(recovery_error(code));
        }
    }
    progress_v4(core, &pending)
}

fn require_v4_local_migration_authority(
    sqlite_path: &std::path::Path,
    pending: &PendingHandleRecoveryV4,
) -> crate::ImResult<()> {
    let Some(authoritative) = pending.authoritative_binding.as_ref() else {
        return Ok(());
    };
    if pending.fresh_local_state || pending.local_previous_did == authoritative.current_did {
        return Ok(());
    }
    if crate::internal::identity_handle_recovery_operation::is_quarantined_replacement(
        sqlite_path,
        &pending.operation_id,
        &pending.owner_identity_id,
        &pending.full_handle,
    )? {
        return Ok(());
    }
    Err(crate::ImError::PermissionDenied)
}

fn reconcile_frozen_intent_index(
    sqlite_path: &std::path::Path,
    operation: &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord,
    pending: &PendingHandleRecoveryV4,
    now: &str,
) -> crate::ImResult<()> {
    let Some(binding) = pending.authoritative_binding.as_ref() else {
        if pending.intent.is_some() || pending.intent_hash.is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        return Ok(());
    };
    let intent = pending
        .intent
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let intent_hash = pending
        .intent_hash
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if intent.account_user_id != binding.account_user_id
        || operation
            .account_user_id
            .as_deref()
            .is_some_and(|account| account != binding.account_user_id)
        || operation
            .intent_hash
            .as_deref()
            .is_some_and(|stored| stored != intent_hash)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if operation.owner_identity_id == pending.owner_identity_id
        && operation.account_user_id.as_deref() == Some(binding.account_user_id.as_str())
        && operation.intent_hash.as_deref() == Some(intent_hash)
    {
        return Ok(());
    }
    crate::internal::identity_handle_recovery_operation::record_frozen_owner_and_intent(
        sqlite_path,
        &operation.operation_id,
        &operation.owner_identity_id,
        &pending.owner_identity_id,
        &binding.account_user_id,
        intent_hash,
        now,
    )
}

fn merge_commit_attempted_authorities(
    sqlite_path: &std::path::Path,
    store: &PendingHandleRecoveryStore,
    operation: &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord,
    pending: &mut PendingHandleRecoveryV4,
    now: &str,
) -> crate::ImResult<()> {
    match (operation.commit_attempted, pending.commit_attempted) {
        (false, false) | (true, true) => Ok(()),
        (true, false) => {
            let revision = pending.revision;
            pending.mark_commit_attempted(operation.updated_at.clone())?;
            store.save_v4_cas(pending, revision)?;
            Ok(())
        }
        (false, true) => {
            if operation.lifecycle_class
                != crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
                || operation.account_user_id.is_none()
                || operation.intent_hash.as_deref() != pending.intent_hash.as_deref()
                || pending.intent.is_none()
                || pending.authoritative_binding.is_none()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
                sqlite_path,
                &pending.operation_id,
                now,
            )
        }
    }
}

fn reconcile_v4_lifecycle_index(
    sqlite_path: &std::path::Path,
    operation: &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord,
    pending: &PendingHandleRecoveryV4,
    now: &str,
) -> crate::ImResult<()> {
    use crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass as Lifecycle;
    if pending.phase == PendingRecoveryPhaseV4::RemoteCommitted {
        let result = pending
            .remote_result
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        result.validate_against(
            &pending.operation_id,
            pending
                .intent_hash
                .as_deref()
                .ok_or(crate::ImError::PermissionDenied)?,
        )?;
        if operation.lifecycle_class == Lifecycle::RemoteUnresolved {
            crate::internal::identity_handle_recovery_operation::update_lifecycle(
                sqlite_path,
                &pending.operation_id,
                Lifecycle::RemoteUnresolved,
                Lifecycle::RemoteCommitted,
                None,
                None,
                now,
            )?;
        } else if !matches!(
            operation.lifecycle_class,
            Lifecycle::RemoteCommitted | Lifecycle::LocalTransitionPending
        ) {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    if pending.phase == PendingRecoveryPhaseV4::Applied {
        let result = pending
            .remote_result
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let marker =
            crate::internal::identity_transition_pending::load(sqlite_path, &pending.operation_id)?
                .ok_or(crate::ImError::PermissionDenied)?;
        let expected_auth_generation = result.bootstrap_device.auth_generation.to_string();
        let expected_registry_version = result.checkpoint.registry_version.to_string();
        if marker.phase != crate::internal::identity_transition_pending::TransitionPhase::Completed
            || marker.contract_version
                != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION
            || marker.contract_hash
                != crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
            || marker.source_kind
                != crate::internal::identity_transition_pending::TransitionSourceKind::Initiator
            || marker.source_id != pending.operation_id
            || marker.owner_identity_id != pending.owner_identity_id
            || marker.account_user_id != result.account_user_id
            || marker.handle != result.full_handle
            || marker.previous_did != pending.local_previous_did
            || marker.current_did != result.current_did
            || marker.binding_generation != result.binding_generation
            || marker.current_device_id.as_deref()
                != Some(result.bootstrap_device.device_id.as_str())
            || marker.device_auth_generation.as_deref() != Some(expected_auth_generation.as_str())
            || marker.registry_version.as_deref() != Some(expected_registry_version.as_str())
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if operation.lifecycle_class == Lifecycle::LocalTransitionPending {
            crate::internal::identity_handle_recovery_operation::update_lifecycle(
                sqlite_path,
                &pending.operation_id,
                Lifecycle::LocalTransitionPending,
                Lifecycle::Applied,
                Some(&marker.state_root_fingerprint),
                None,
                now,
            )?;
        } else if operation.lifecycle_class != Lifecycle::Applied {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(())
}

async fn send_commit_v4(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
) -> crate::ImResult<bool> {
    let now = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let grant_expires = pending
        .grant_expires_at
        .as_deref()
        .map(parse_timestamp)
        .transpose()?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::FactorRetryRequired))?;
    if grant_expires <= now {
        persist_nonterminal_error_v4(
            core,
            store,
            pending,
            HandleRecoveryErrorCode::FactorRetryRequired,
        )?;
        return Ok(false);
    }
    let expires = std::cmp::min(now + time::Duration::seconds(120), grant_expires);
    let created_at = format_timestamp(now)?;
    let expires_at = format_timestamp(expires)?;
    let mut nonce = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| crate::ImError::Internal {
            message: "generate Handle Recovery V4 proof nonce failed".to_owned(),
        })?;
    let intent = pending
        .intent
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let intent_hash = pending
        .intent_hash
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let predecessor_document = if pending.fresh_local_state {
        let mut transport = crate::internal::transport::CorePlainTransport::new(core);
        let current = crate::internal::discovery::did_document::resolve_did_document_async(
            &mut transport,
            &pending.local_previous_did,
        )
        .await?;
        unsigned_recovery_predecessor_document(
            current,
            &pending.local_previous_did,
            pending.identity.did.as_str(),
        )?
    } else {
        crate::internal::identity_custody::prepare_handle_recovery_transition_candidate(
            core, pending,
        )
        .await?
        .predecessor_document
    };
    let audience = core
        .inner()
        .multi_device_audience()
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalMigrationUnsupported))?;
    let prepared = crate::internal::identity_wire::handle_recovery::prepare_commit_v4(
        crate::internal::identity_wire::handle_recovery::CommitProofInputV4 {
            proof: crate::internal::identity_wire::handle_recovery::KeyPossessionProofInputV4 {
                intent: &intent,
                intent_hash: &intent_hash,
                audience,
                created_at: &created_at,
                expires_at: &expires_at,
                nonce: &nonce,
            },
            recovery_grant: pending.recovery_grant()?,
            predecessor_did_document: predecessor_document,
            new_did_document: pending.identity.did_document.clone(),
        },
    )?;
    let signature =
        crate::internal::identity_custody::handle_recovery_identity_async(core, &pending.identity)
            .await?
            .sign(crate::internal::identity_provider::ProviderSignRequest {
                purpose:
                    crate::internal::identity_provider::ProviderSigningPurpose::DeviceAssertion,
                key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                    pending.identity.device_signing_key_id.clone(),
                ),
                payload: prepared.signing_input().to_vec(),
            })
            .await
            .map(|signature| signature.bytes)
            .map_err(crate::internal::identity_provider::map_provider_error)?;
    let prepared =
        crate::internal::identity_wire::handle_recovery::complete_commit_v4(prepared, &signature)?;
    if !pending.fresh_local_state {
        crate::internal::identity_custody::begin_handle_recovery_transition_publication(
            core, pending,
        )
        .await?;
    }
    if !pending.commit_attempted {
        let attempted_at = now_second_z()?;
        crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &pending.operation_id,
            &attempted_at,
        )?;
        let revision = pending.revision;
        pending.mark_commit_attempted(attempted_at)?;
        store.save_v4_cas(pending, revision)?;
    }
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let raw = match transport
        .rpc(
            prepared.call.endpoint,
            prepared.call.method,
            prepared.call.params,
        )
        .await
    {
        Ok(raw) => raw,
        Err(error) => {
            if !pending.fresh_local_state {
                crate::internal::identity_custody::mark_handle_recovery_transition_unknown(
                    core, pending,
                )
                .await?;
            }
            if let Some(raw_code) = service_code(&error) {
                let Some(code) = crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4::parse(raw_code) else {
                    return Err(error);
                };
                match server_error_projection(code) {
                    ServerErrorProjectionV4::FactorRetryRequired => {
                        persist_nonterminal_server_error_v4(core, store, pending, code)?;
                        return Ok(false);
                    }
                    ServerErrorProjectionV4::OutcomeUnknown => {
                        persist_nonterminal_server_error_v4(core, store, pending, code)?;
                        return Ok(false);
                    }
                    ServerErrorProjectionV4::SupersededByStateChange
                    | ServerErrorProjectionV4::FailedTerminal => {
                        project_terminal_server_error_v4(core, pending, code)?;
                        return Err(error);
                    }
                }
            }
            persist_nonterminal_error_v4(
                core,
                store,
                pending,
                HandleRecoveryErrorCode::OutcomeUnknown,
            )?;
            return Ok(false);
        }
    };
    let result = match crate::internal::identity_wire::handle_recovery::parse_commit_result_v4(
        raw,
        &pending.operation_id,
        &intent_hash,
    ) {
        Ok(result) => result,
        Err(error) => {
            if !pending.fresh_local_state {
                crate::internal::identity_custody::mark_handle_recovery_transition_unknown(
                    core, pending,
                )
                .await?;
            }
            return Err(error);
        }
    };
    if !pending.fresh_local_state {
        crate::internal::identity_custody::confirm_handle_recovery_transition_published(
            core, pending,
        )
        .await?;
    }
    let revision = pending.revision;
    pending.record_remote_result(result)?;
    store.save_v4_cas(pending, revision)?;
    crate::internal::identity_handle_recovery_operation::update_lifecycle(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted,
        None,
        None,
        &now_second_z()?,
    )?;
    Ok(true)
}

fn unsigned_recovery_predecessor_document(
    mut document: Value,
    expected_previous_did: &str,
    successor_did: &str,
) -> crate::ImResult<Value> {
    let object = document
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    if object.get("id").and_then(Value::as_str) != Some(expected_previous_did)
        || object.get("deactivated").and_then(Value::as_bool) == Some(true)
        || object.contains_key("successorDid")
        || expected_previous_did == successor_did
    {
        return Err(crate::ImError::PermissionDenied);
    }
    object.remove("proof");
    object.insert("deactivated".to_owned(), Value::Bool(true));
    object.insert(
        "successorDid".to_owned(),
        Value::String(successor_did.to_owned()),
    );
    Ok(document)
}

async fn reconcile_result_v4(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
) -> crate::ImResult<ReconcileV4Outcome> {
    let now = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let created_at = format_timestamp(now)?;
    let expires_at = format_timestamp(now + time::Duration::seconds(120))?;
    let mut nonce = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| crate::ImError::Internal {
            message: "generate Handle Recovery V4 result proof nonce failed".to_owned(),
        })?;
    let intent = pending
        .intent
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let intent_hash = pending
        .intent_hash
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let audience = core
        .inner()
        .multi_device_audience()
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::LocalMigrationUnsupported))?;
    let prepared = crate::internal::identity_wire::handle_recovery::prepare_result_get_v4(
        crate::internal::identity_wire::handle_recovery::KeyPossessionProofInputV4 {
            intent: &intent,
            intent_hash: &intent_hash,
            audience,
            created_at: &created_at,
            expires_at: &expires_at,
            nonce: &nonce,
        },
    )?;
    let signature =
        crate::internal::identity_custody::handle_recovery_identity_async(core, &pending.identity)
            .await?
            .sign(crate::internal::identity_provider::ProviderSignRequest {
                purpose:
                    crate::internal::identity_provider::ProviderSigningPurpose::DeviceAssertion,
                key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                    pending.identity.device_signing_key_id.clone(),
                ),
                payload: prepared.signing_input().to_vec(),
            })
            .await
            .map(|signature| signature.bytes)
            .map_err(crate::internal::identity_provider::map_provider_error)?;
    let prepared = crate::internal::identity_wire::handle_recovery::complete_result_get_v4(
        prepared, &signature,
    )?;
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let raw = match transport
        .rpc(
            prepared.call.endpoint,
            prepared.call.method,
            prepared.call.params,
        )
        .await
    {
        Ok(raw) => raw,
        Err(error) => {
            if let Some(raw_code) = service_code(&error) {
                let Some(code) = crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4::parse(raw_code) else {
                    return Err(error);
                };
                match server_error_projection(code) {
                    ServerErrorProjectionV4::FactorRetryRequired => {
                        let revision = pending.revision;
                        pending.record_result_get(
                            now_second_z()?,
                            None,
                            Some(code.as_str().to_owned()),
                        )?;
                        store.save_v4_cas(pending, revision)?;
                        crate::internal::identity_handle_recovery_operation::record_nonterminal_error(
                            &core.inner().sdk_paths().local_state.sqlite_path,
                            &pending.operation_id,
                            Some(code.as_str()),
                            &now_second_z()?,
                        )?;
                        return Ok(ReconcileV4Outcome::FactorRetryRequired);
                    }
                    ServerErrorProjectionV4::OutcomeUnknown => {
                        let revision = pending.revision;
                        pending.record_result_get(
                            now_second_z()?,
                            Some(format_timestamp(now + time::Duration::seconds(10))?),
                            Some(code.as_str().to_owned()),
                        )?;
                        store.save_v4_cas(pending, revision)?;
                        crate::internal::identity_handle_recovery_operation::record_nonterminal_error(
                            &core.inner().sdk_paths().local_state.sqlite_path,
                            &pending.operation_id,
                            Some(code.as_str()),
                            &now_second_z()?,
                        )?;
                        return Ok(ReconcileV4Outcome::Unavailable);
                    }
                    ServerErrorProjectionV4::SupersededByStateChange
                    | ServerErrorProjectionV4::FailedTerminal => {
                        project_terminal_server_error_v4(core, pending, code)?;
                        return Err(error);
                    }
                }
            }
            let revision = pending.revision;
            pending.record_result_get(
                now_second_z()?,
                Some(format_timestamp(now + time::Duration::seconds(10))?),
                Some(HandleRecoveryErrorCode::OutcomeUnknown.as_str().to_owned()),
            )?;
            store.save_v4_cas(pending, revision)?;
            crate::internal::identity_handle_recovery_operation::record_nonterminal_error(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &pending.operation_id,
                Some(HandleRecoveryErrorCode::OutcomeUnknown.as_str()),
                &now_second_z()?,
            )?;
            return Ok(ReconcileV4Outcome::Unavailable);
        }
    };
    let result = crate::internal::identity_wire::handle_recovery::parse_result_get_v4(
        raw,
        &pending.operation_id,
        &intent_hash,
    )?;
    let revision = pending.revision;
    pending.record_result_get(
        now_second_z()?,
        Some(format_timestamp(now + time::Duration::seconds(10))?),
        Some(match &result {
            crate::internal::identity_wire::handle_recovery::RecoveryResultGetV4::Committed(_) => {
                "committed"
            }
            crate::internal::identity_wire::handle_recovery::RecoveryResultGetV4::ResultAbsent => {
                HandleRecoveryErrorCode::ResultAbsent.as_str()
            }
        }
        .to_owned()),
    )?;
    store.save_v4_cas(pending, revision)?;
    if matches!(
        &result,
        crate::internal::identity_wire::handle_recovery::RecoveryResultGetV4::ResultAbsent
    ) {
        crate::internal::identity_handle_recovery_operation::record_nonterminal_error(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &pending.operation_id,
            Some(HandleRecoveryErrorCode::ResultAbsent.as_str()),
            &now_second_z()?,
        )?;
    }
    match result {
        crate::internal::identity_wire::handle_recovery::RecoveryResultGetV4::Committed(result) => {
            if !pending.fresh_local_state {
                crate::internal::identity_custody::confirm_handle_recovery_transition_published(
                    core, pending,
                )
                .await?;
            }
            let revision = pending.revision;
            pending.record_remote_result(result)?;
            store.save_v4_cas(pending, revision)?;
            crate::internal::identity_handle_recovery_operation::update_lifecycle(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &pending.operation_id,
                crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved,
                crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted,
                None,
                None,
                &now_second_z()?,
            )?;
            Ok(ReconcileV4Outcome::Committed)
        }
        crate::internal::identity_wire::handle_recovery::RecoveryResultGetV4::ResultAbsent => {
            if !pending.fresh_local_state {
                crate::internal::identity_custody::reconcile_handle_recovery_transition_remote_old(
                    core, pending,
                )
                .await?;
            }
            Ok(ReconcileV4Outcome::ResultAbsent)
        }
    }
}

fn grant_is_fresh(pending: &PendingHandleRecoveryV4) -> crate::ImResult<bool> {
    Ok(pending
        .grant_expires_at
        .as_deref()
        .map(parse_timestamp)
        .transpose()?
        .is_some_and(|expires| expires > time::OffsetDateTime::now_utc()))
}

fn service_code(error: &crate::ImError) -> Option<&str> {
    match error {
        crate::ImError::Service { code, .. } => code.as_deref(),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerErrorProjectionV4 {
    FactorRetryRequired,
    OutcomeUnknown,
    SupersededByStateChange,
    FailedTerminal,
}

const fn server_error_projection(
    code: crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4,
) -> ServerErrorProjectionV4 {
    use crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4 as Code;
    match code {
        Code::GrantExpired => ServerErrorProjectionV4::FactorRetryRequired,
        Code::InvalidRequest
        | Code::CapabilityDisabled
        | Code::GrantInvalid
        | Code::ProofInvalid
        | Code::TemporarilyUnavailable => ServerErrorProjectionV4::OutcomeUnknown,
        Code::StateChangedRequiresNewOperation => ServerErrorProjectionV4::SupersededByStateChange,
        Code::IntentConflict => ServerErrorProjectionV4::FailedTerminal,
    }
}

const fn exchange_error_projection(
    code: crate::internal::identity_wire::handle_recovery::RecoveryExchangeErrorCodeV4,
) -> HandleRecoveryErrorCode {
    use crate::internal::identity_wire::handle_recovery::RecoveryExchangeErrorCodeV4 as Code;
    match code {
        Code::InvalidRequest | Code::FactorInvalid | Code::RateLimited => {
            HandleRecoveryErrorCode::FactorRetryRequired
        }
        Code::TemporarilyUnavailable => HandleRecoveryErrorCode::OutcomeUnknown,
        Code::CapabilityDisabled => HandleRecoveryErrorCode::LocalMigrationUnsupported,
    }
}

fn persist_nonterminal_error_v4(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
    code: HandleRecoveryErrorCode,
) -> crate::ImResult<()> {
    persist_nonterminal_error_code_v4(core, store, pending, code.as_str())
}

fn persist_nonterminal_server_error_v4(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
    code: crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4,
) -> crate::ImResult<()> {
    persist_nonterminal_error_code_v4(core, store, pending, code.as_str())
}

fn persist_nonterminal_error_code_v4(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
    code: &str,
) -> crate::ImResult<()> {
    let revision = pending.revision;
    pending.record_retryable_error(code.to_owned())?;
    store.save_v4_cas(pending, revision)?;
    crate::internal::identity_handle_recovery_operation::record_nonterminal_error(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
        Some(code),
        &now_second_z()?,
    )
}

fn project_terminal_server_error_v4(
    core: &crate::core::ImCore,
    pending: &PendingHandleRecoveryV4,
    code: crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4,
) -> crate::ImResult<()> {
    use crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass as Lifecycle;
    let next = match server_error_projection(code) {
        ServerErrorProjectionV4::SupersededByStateChange => Lifecycle::SupersededByStateChange,
        ServerErrorProjectionV4::FailedTerminal => Lifecycle::FailedTerminal,
        ServerErrorProjectionV4::FactorRetryRequired | ServerErrorProjectionV4::OutcomeUnknown => {
            return Err(crate::ImError::PermissionDenied)
        }
    };
    crate::internal::identity_handle_recovery_operation::update_lifecycle(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
        Lifecycle::RemoteUnresolved,
        next,
        None,
        Some(code.as_str()),
        &now_second_z()?,
    )
}

async fn apply_local_transition_v4(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    pending: &mut PendingHandleRecoveryV4,
) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let result = pending
        .remote_result
        .clone()
        .ok_or(crate::ImError::PermissionDenied)?;
    let marker =
        crate::internal::identity_transition_pending::load(sqlite_path, &pending.operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
    if marker.phase == crate::internal::identity_transition_pending::TransitionPhase::Pending {
        if pending.fresh_local_state {
            crate::internal::identity_custody::adopt_controller_document_async(
                core,
                crate::internal::identity_custody::ControllerDocumentAdoption::HandleRecovery {
                    pending_operation_id: &pending.operation_id,
                },
                &pending.identity.did,
                &pending.identity.store_id,
                &pending.identity.identity_id,
                &pending.identity.did_document,
                &crate::internal::identity_device_state::IdentityInternalCheckpoint {
                    document_version: result.checkpoint.document_version,
                    document_hash: result.checkpoint.document_hash.clone(),
                    registry_version: result.checkpoint.registry_version,
                },
            )
            .await?;
        }
        if pending.fresh_local_state {
            crate::internal::identity_transition_pending::migrate_initiator_new_local_state(
                sqlite_path,
                &marker,
                &result.bootstrap_device.device_id,
                result.bootstrap_device.auth_generation,
            )?;
        } else if crate::internal::identity_handle_recovery_operation::is_quarantined_replacement(
            sqlite_path,
            &pending.operation_id,
            &pending.owner_identity_id,
            &pending.full_handle,
        )? {
            crate::internal::identity_transition_pending::migrate_initiator_fresh_local_state(
                sqlite_path,
                &marker,
                &result.bootstrap_device.device_id,
                result.bootstrap_device.auth_generation,
            )?;
            crate::internal::identity_handle_recovery_metrics::record_break_glass(
                crate::internal::identity_handle_recovery_metrics::BreakGlassResult::Applied,
            );
        } else {
            crate::internal::identity_transition_pending::migrate_local_state(
                sqlite_path,
                &marker,
                &result.bootstrap_device.device_id,
                result.bootstrap_device.auth_generation,
            )?;
        }
        let generated = &pending.identity;
        let device_state = crate::internal::identity_device_state::IdentityDeviceState {
            schema_version:
                crate::internal::identity_device_state::IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: crate::internal::identity_device_state::IdentityDeviceMode::VNext,
            authorization: Some(
                crate::internal::identity_device_state::DeviceAuthorizationProjection {
                    protocol_device_id: generated.protocol_device_id.clone(),
                    signing_key_id: generated.device_signing_key_id.clone(),
                    e2ee_key_id: generated.device_e2ee_key_id.clone(),
                    status:
                        crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                    role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                    management_ready: true,
                    auth_generation: result.bootstrap_device.auth_generation,
                },
            ),
            checkpoint: Some(
                crate::internal::identity_device_state::IdentityInternalCheckpoint {
                    document_version: result.checkpoint.document_version,
                    document_hash: result.checkpoint.document_hash.clone(),
                    registry_version: result.checkpoint.registry_version,
                },
            ),
        };
        let projection_storage =
            crate::internal::identity_store::AnpIdentityProjectionStorage::from_core_pending_auth(
                core,
                generated.store_id.clone(),
                generated.identity_id.clone(),
            )?;
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .save_anp_identity_transition_projection(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias: pending.local_alias.clone(),
                did: generated.did.clone(),
                unique_id: pending.owner_identity_id.clone(),
                user_id: result.account_user_id.clone(),
                display_name: pending.display_name.clone(),
                handle: crate::internal::identity_wire::handle_recovery::canonical_handle(
                    &pending.full_handle,
                )?
                .local_part,
                full_handle: pending.full_handle.clone(),
                binding_generation: Some(result.binding_generation.clone()),
                jwt_token: String::new(),
                did_document: Some(generated.did_document.clone()),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                    root_key_id: generated.root_key_id.clone(),
                    device_signing_key_id: generated.device_signing_key_id.clone(),
                    device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
                },
                device_state: Some(device_state),
                key1_private_pem: String::new(),
                key1_public_pem: String::new(),
                e2ee_signing_private_pem: String::new(),
                e2ee_agreement_private_pem: String::new(),
                daemon_subkey_package: None,
                make_default: pending.make_default,
            },
            projection_storage,
            crate::internal::identity_store::AnpIdentityProjectionReplacement {
                expected_did: &pending.local_previous_did,
                expected_unique_id: &pending.owner_identity_id,
            },
        )?;
        crate::internal::identity_transition_pending::update_phase(
            sqlite_path,
            &pending.operation_id,
            crate::internal::identity_transition_pending::TransitionPhase::Pending,
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
        )?;
    } else if marker.phase
        == crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched
    {
        validate_switched_identity_v4(core, pending, &result)?;
    }
    let marker =
        crate::internal::identity_transition_pending::load(sqlite_path, &pending.operation_id)?
            .ok_or(crate::ImError::PermissionDenied)?;
    if marker.phase
        == crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched
    {
        let recovered_did = crate::ids::Did::parse(&result.current_did)?;
        let client = core
            .client_async(crate::identity::IdentitySelector::Did(
                recovered_did.clone(),
            ))
            .await?;
        let mut auth = crate::internal::transport::CoreHttpTransport::new_signature_only(&client);
        auth.refresh_jwt_async().await?;
        crate::internal::identity_registration_runtime::publish_v2_prekeys_after_registration_async(
            core,
            &recovered_did,
        )
        .await?;
        if core.inner().group_e2ee_v2_enabled() {
            crate::internal::identity_registration_runtime::publish_v2_group_key_package_after_registration_async(
                core,
                &recovered_did,
            )
            .await?;
        }
        crate::internal::identity_transition_pending::mark_applied(
            sqlite_path,
            &pending.operation_id,
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
            &result.bootstrap_device.device_id,
            &result.bootstrap_device.auth_generation.to_string(),
            &result.checkpoint.registry_version.to_string(),
            "{}",
        )?;
    }
    let applied =
        crate::internal::identity_transition_pending::load(sqlite_path, &pending.operation_id)?
            .filter(|marker| {
                marker.phase
                    == crate::internal::identity_transition_pending::TransitionPhase::Completed
            })
            .ok_or(crate::ImError::PermissionDenied)?;
    if pending.phase != PendingRecoveryPhaseV4::Applied {
        let revision = pending.revision;
        pending.mark_applied()?;
        store.save_v4_cas(pending, revision)?;
    }
    crate::internal::identity_handle_recovery_operation::update_lifecycle(
        sqlite_path,
        &pending.operation_id,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::Applied,
        Some(&applied.state_root_fingerprint),
        None,
        &now_second_z()?,
    )?;
    Ok(())
}

fn validate_switched_identity_v4(
    core: &crate::core::ImCore,
    pending: &PendingHandleRecoveryV4,
    result: &crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4,
) -> crate::ImResult<()> {
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let matches = index.credentials.values().filter(|entry| {
        entry.unique_id == pending.owner_identity_id
            && entry.user_id == result.account_user_id
            && entry.did == result.current_did
            && entry.full_handle == pending.full_handle
            && entry.binding_generation.as_deref() == Some(result.binding_generation.as_str())
            && entry.identity_custody_backend.as_deref() == Some("anp_identity")
            && entry.anp_identity_store_id.as_deref() == Some(pending.identity.store_id.as_str())
            && entry.anp_identity_id.as_deref() == Some(pending.identity.identity_id.as_str())
    });
    if matches.count() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(crate) async fn activate_authorized_join(
    core: &crate::core::ImCore,
    request: AuthorizedJoinActivationRequest,
) -> crate::ImResult<AuthorizedJoinActivationProgress> {
    require_enabled(core)?;
    require_explicit_identity(&request.identity)?;
    if !request.user_presence_confirmed {
        return Err(user_presence_required_error());
    }
    let canonical =
        crate::internal::identity_wire::handle_recovery::canonical_handle(&request.handle)?;
    let operation_id = crate::internal::identity_wire::handle_recovery::validate_operation_id(
        &request.operation_id,
    )?;
    let body = json!({
        "provider": "sms",
        "purpose": "awiki.device.join.v1",
        "phone": crate::internal::identity_wire::normalize_phone(&request.phone)?,
        "code": crate::internal::identity_wire::sanitize_otp(&request.code),
        "target_handle": canonical.local_part,
        "target_handle_domain": canonical.domain,
        "idempotency_scope": operation_id,
    });
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let raw = transport
        .rest_post(
            crate::internal::identity_wire::ACCOUNT_VERIFICATION_EXCHANGE_ENDPOINT,
            "POST",
            body,
        )
        .await?;
    let exchanged =
        crate::internal::identity_wire::handle_recovery::parse_account_verification_result(raw)?;
    if let Some(transition) = &exchanged.identity_transition {
        let owner = core.identities().resolve_async(request.identity).await?;
        let account_user_id = exchanged
            .account_user_id
            .clone()
            .ok_or(crate::ImError::PermissionDenied)?;
        let index = crate::internal::identity_store::IdentityStore::new(
            &core.inner().sdk_paths().identities,
        )
        .load_index()?;
        let authority_closed = request.did.as_str() == transition.current_did
            && exchanged.did.as_deref() == Some(request.did.as_str())
            && exchanged.handle.as_deref() == Some(canonical.full.as_str());
        if !authority_closed {
            return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
        }
        let owner_identity_id =
            match crate::internal::identity_local_owner_matcher::match_stable_owner(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &index,
                crate::internal::identity_local_owner_matcher::StableOwnerAuthority {
                    account_user_id: &account_user_id,
                    full_handle: &canonical.full,
                    previous_did: &transition.previous_did,
                    binding_generation: &transition.binding_generation,
                },
                None,
                None,
            )? {
                crate::internal::identity_local_owner_matcher::StableOwnerMatch::Exact(
                    candidate,
                ) if candidate.owner_identity_id == owner.id.as_str() => {
                    candidate.owner_identity_id
                }
                crate::internal::identity_local_owner_matcher::StableOwnerMatch::Conflict => {
                    return Err(
                        crate::internal::identity_registration_join_preparation::continuity_error(
                            "handle_recovery.local_state_conflict",
                        ),
                    );
                }
                crate::internal::identity_local_owner_matcher::StableOwnerMatch::Exact(_)
                | crate::internal::identity_local_owner_matcher::StableOwnerMatch::None => {
                    return Err(recovery_error(
                        HandleRecoveryErrorCode::LocalMigrationUnsupported,
                    ));
                }
            };
        let handle = canonical.full.clone();
        let previous_did = transition.previous_did.clone();
        let current_did = transition.current_did.clone();
        let binding_generation = transition.binding_generation.clone();
        let token = crate::identity::DeviceJoinAccountVerificationGrant::from_bytes(
            exchanged
                .account_verification_token
                .expose_secret()
                .to_vec(),
        )?;
        let sqlite_path = core.inner().sdk_paths().local_state.sqlite_path.clone();
        let join = core
            .device_join()
            .begin_new_device_join_with_local_hook(
                crate::identity::DeviceJoinBeginRequest {
                    operation_id,
                    did: request.did,
                    ttl_seconds: request.ttl_seconds.unwrap_or(600),
                    account_verification_grant: token,
                },
                move |session| {
                    let marker = crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
                        &sqlite_path,
                        &session.join_session_id,
                        &account_user_id,
                        &owner_identity_id,
                        &handle,
                        &previous_did,
                        &current_did,
                        &binding_generation,
                    )?;
                    crate::internal::identity_transition_pending::persist(&sqlite_path, &marker)
                },
            )
            .await?;
        advance_joined_transition(core, &join.session.join_session_id).await?;
        let marker = crate::internal::identity_transition_pending::load_joined_device(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &join.session.join_session_id,
        )?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::UnknownEpoch))?;
        return Ok(AuthorizedJoinActivationProgress {
            join,
            reset_reference: Some(reset_reference_from_marker(&marker)?),
        });
    }
    let token = crate::identity::DeviceJoinAccountVerificationGrant::from_bytes(
        exchanged
            .account_verification_token
            .expose_secret()
            .to_vec(),
    )?;
    let join = core
        .device_join()
        .begin_new_device_join(crate::identity::DeviceJoinBeginRequest {
            operation_id,
            did: request.did,
            ttl_seconds: request.ttl_seconds.unwrap_or(600),
            account_verification_grant: token,
        })
        .await?;
    Ok(AuthorizedJoinActivationProgress {
        join,
        reset_reference: None,
    })
}

pub(crate) async fn begin_prepared_registration_device_join(
    core: &crate::core::ImCore,
    request: crate::identity::BeginPreparedRegistrationDeviceJoinRequest,
) -> crate::ImResult<AuthorizedJoinActivationProgress> {
    let operation_id = crate::internal::identity_wire::handle_recovery::validate_operation_id(
        &request.operation_id,
    )?;
    let begin_input_hash =
        crate::internal::identity_registration_join_preparation::begin_input_hash(
            &operation_id,
            request.ttl_seconds,
            request.user_presence_confirmed,
        )?;
    let operation_lock = core
        .inner()
        .registration_join_preparations
        .operation_lock(&request.preparation_id)?;
    let _guard = operation_lock.lock().await;
    let snapshot = core
        .inner()
        .registration_join_preparations
        .bind_and_snapshot(&request.preparation_id, &operation_id, &begin_input_hash)?;
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    if snapshot.state_root_fingerprint
        != crate::internal::identity_transition_pending::state_root_fingerprint(sqlite_path)
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let already_authorized = snapshot
        .join_session_id
        .as_deref()
        .and_then(|join_session_id| {
            core.device_join()
                .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)
                .ok()
        })
        .is_some_and(|session| session.phase == crate::identity::DeviceJoinLocalPhase::Authorized);
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    if !already_authorized
        && snapshot.identity_index_fingerprint
            != crate::internal::identity_registration_join_preparation::identity_index_fingerprint(
                &index,
            )?
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let exact_owner = if already_authorized {
        snapshot.owner_identity_id.clone()
    } else {
        revalidate_prepared_registration_owner(core, &snapshot, &index)?
    };
    let is_rebind = matches!(
        snapshot.mode,
        crate::identity::HandleRegistrationJoinMode::HandleRecoveryRebind
    );
    if is_rebind {
        require_enabled(core)?;
        if !request.user_presence_confirmed {
            return Err(user_presence_required_error());
        }
    }

    let join = if snapshot.remote_started {
        let join_session_id = snapshot
            .join_session_id
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        core.device_join()
            .poll_new_device_join(join_session_id)
            .await?
    } else if let Some(resume_join_session_id) = snapshot.resume_join_session_id.as_deref() {
        let transition = snapshot
            .transition
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let continuation = crate::internal::identity_registration_join_continuation::resolve(
            core,
            transition,
            &snapshot.full_handle,
        )?;
        let crate::internal::identity_registration_join_continuation::RegistrationJoinContinuation::Resume(
            evidence,
        ) = continuation
        else {
            return Err(crate::ImError::PermissionDenied);
        };
        if evidence.join_session_id != resume_join_session_id
            || Some(evidence.owner_identity_id.as_str()) != exact_owner.as_deref()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let join = if evidence.remote_create_state
            == crate::internal::identity_device_join::RemoteCreateState::Bound
        {
            core.device_join()
                .poll_new_device_join(resume_join_session_id)
                .await?
        } else {
            let token = crate::identity::DeviceJoinAccountVerificationGrant::from_bytes(
                snapshot.account_verification_token.clone(),
            )?;
            core.device_join()
                .resume_new_device_remote_create(resume_join_session_id, token)
                .await?
        };
        core.inner()
            .registration_join_preparations
            .mark_remote_started(
                &request.preparation_id,
                &operation_id,
                resume_join_session_id,
            )?;
        join
    } else {
        let preparation_id = request.preparation_id.clone();
        let marker_transition = snapshot.transition.clone();
        let marker_owner = exact_owner.clone();
        let retired_owner_evidence = snapshot.retired_owner_evidence.clone();
        let marker_handle = snapshot.full_handle.as_str().to_owned();
        let marker_operation = operation_id.clone();
        let token = crate::identity::DeviceJoinAccountVerificationGrant::from_bytes(
            snapshot.account_verification_token.clone(),
        )?;
        let join = core
            .device_join()
            .begin_new_device_join_with_local_hook(
                crate::identity::DeviceJoinBeginRequest {
                    operation_id: operation_id.clone(),
                    did: snapshot.expected_did.clone(),
                    ttl_seconds: request.ttl_seconds,
                    account_verification_grant: token,
                },
                move |session| {
                    if let Some(evidence) = retired_owner_evidence.as_ref() {
                        let transition = marker_transition
                            .as_ref()
                            .ok_or(crate::ImError::PermissionDenied)?;
                        if marker_owner.as_deref() != Some(evidence.owner_identity_id.as_str()) {
                            return Err(crate::ImError::PermissionDenied);
                        }
                        let journal = crate::internal::identity_registration_retired_join::RetiredJoinRollover::prepared(
                            &session.join_session_id,
                            &transition.account_user_id,
                            &marker_handle,
                            transition,
                            evidence,
                            session.protocol_device_id.as_str(),
                            &session.expires_at,
                        )?;
                        crate::internal::identity_registration_retired_join::insert_prepared(
                            sqlite_path,
                            &journal,
                        )?;
                    } else if let (Some(transition), Some(owner_identity_id)) =
                        (marker_transition.as_ref(), marker_owner.as_deref())
                    {
                        let marker = crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
                            sqlite_path,
                            &session.join_session_id,
                            &transition.account_user_id,
                            owner_identity_id,
                            &marker_handle,
                            &transition.previous_did,
                            &transition.current_did,
                            &transition.binding_generation,
                        )?;
                        crate::internal::identity_transition_pending::persist(sqlite_path, &marker)?;
                    }
                    core.inner()
                        .registration_join_preparations
                        .bind_local_session(
                            &preparation_id,
                            &marker_operation,
                            &session.join_session_id,
                        )
                },
            )
            .await?;
        core.inner()
            .registration_join_preparations
            .mark_remote_started(
                &request.preparation_id,
                &operation_id,
                &join.session.join_session_id,
            )?;
        join
    };
    if let Some(cleanup) = snapshot.pending_registration_cleanup.as_ref() {
        crate::internal::identity_custody::discard_unpublished_registration_async(
            core,
            &cleanup.identity,
        )
        .await?;
        crate::internal::identity_registration_pending::PendingRegistrationStore::from_core(core)?
            .delete(&cleanup.secret_ref)?;
    }
    if is_rebind {
        advance_joined_transition(core, &join.session.join_session_id).await?;
    }
    let reset_reference = if is_rebind {
        crate::internal::identity_transition_pending::load_joined_device(
            sqlite_path,
            &join.session.join_session_id,
        )?
        .as_ref()
        .map(reset_reference_from_marker)
        .transpose()?
    } else {
        None
    };
    Ok(AuthorizedJoinActivationProgress {
        join,
        reset_reference,
    })
}

fn revalidate_prepared_registration_owner(
    core: &crate::core::ImCore,
    snapshot: &crate::internal::identity_registration_join_preparation::RegistrationJoinPreparationSnapshot,
    index: &crate::internal::identity_store::IndexPayload,
) -> crate::ImResult<Option<String>> {
    use crate::internal::identity_local_owner_matcher::{
        RegistrationOwnerAuthority, RegistrationOwnerDisposition, StableOwnerMatch,
    };

    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    match snapshot.transition.as_ref() {
        Some(transition) => match crate::internal::identity_local_owner_matcher::classify_registration_owner(
            sqlite_path,
            &core.inner().sdk_paths().identities.identity_root_dir,
            index,
            RegistrationOwnerAuthority {
                account_user_id: &transition.account_user_id,
                full_handle: snapshot.full_handle.as_str(),
                previous_did: &transition.previous_did,
                current_did: &transition.current_did,
                binding_generation: &transition.binding_generation,
            },
            snapshot.join_session_id.as_deref(),
        )? {
            RegistrationOwnerDisposition::ExactLivePredecessor(owner)
                if snapshot.mode
                    == crate::identity::HandleRegistrationJoinMode::HandleRecoveryRebind
                    && snapshot.owner_identity_id.as_deref()
                        == Some(owner.owner_identity_id.as_str())
                    && snapshot.retired_owner_evidence.is_none() =>
            {
                Ok(Some(owner.owner_identity_id))
            }
            RegistrationOwnerDisposition::RetiredNoLiveCredential(evidence)
                if snapshot.mode == crate::identity::HandleRegistrationJoinMode::Ordinary
                    && snapshot.owner_identity_id.as_deref()
                        == Some(evidence.owner_identity_id.as_str())
                    && snapshot.retired_owner_evidence.as_ref() == Some(&evidence) =>
            {
                Ok(Some(evidence.owner_identity_id))
            }
            RegistrationOwnerDisposition::FreshNone
                if snapshot.mode == crate::identity::HandleRegistrationJoinMode::Ordinary
                    && snapshot.owner_identity_id.is_none()
                    && snapshot.retired_owner_evidence.is_none() =>
            {
                Ok(None)
            }
            RegistrationOwnerDisposition::Conflict => Err(
                crate::internal::identity_registration_join_preparation::continuity_error(
                    "handle_recovery.local_state_conflict",
                ),
            ),
            RegistrationOwnerDisposition::ExactLivePredecessor(_)
            | RegistrationOwnerDisposition::RetiredNoLiveCredential(_)
            | RegistrationOwnerDisposition::FreshNone => Err(crate::ImError::PermissionDenied),
        },
        None => match crate::internal::identity_local_owner_matcher::match_stable_owner_without_transition(
            sqlite_path,
            &core.inner().sdk_paths().identities.identity_root_dir,
            index,
            snapshot.full_handle.as_str(),
            snapshot.expected_did.as_str(),
        )? {
            StableOwnerMatch::None
                if snapshot.mode == crate::identity::HandleRegistrationJoinMode::Ordinary
                    && snapshot.owner_identity_id.is_none()
                    && snapshot.retired_owner_evidence.is_none() =>
            {
                Ok(None)
            }
            StableOwnerMatch::Exact(_) | StableOwnerMatch::Conflict => Err(
                crate::internal::identity_registration_join_preparation::continuity_error(
                    "handle_recovery.transition_missing",
                ),
            ),
            StableOwnerMatch::None => Err(crate::ImError::PermissionDenied),
        },
    }
}

pub(crate) async fn resume_authorized_join_activation(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<AuthorizedJoinActivationProgress> {
    let recovery_marker = crate::internal::identity_transition_pending::load_joined_device(
        &core.inner().sdk_paths().local_state.sqlite_path,
        join_session_id,
    )?;
    if recovery_marker.is_some() {
        require_enabled(core)?;
    }
    let join = core
        .device_join()
        .poll_new_device_join(join_session_id)
        .await?;
    if recovery_marker.is_some() {
        advance_joined_transition(core, join_session_id).await?;
    }
    let reset_reference = crate::internal::identity_transition_pending::load_joined_device(
        &core.inner().sdk_paths().local_state.sqlite_path,
        join_session_id,
    )?
    .as_ref()
    .map(reset_reference_from_marker)
    .transpose()?;
    Ok(AuthorizedJoinActivationProgress {
        join,
        reset_reference,
    })
}

async fn advance_joined_transition(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let Some(marker) = crate::internal::identity_transition_pending::load_joined_device(
        sqlite_path,
        join_session_id,
    )?
    else {
        return Ok(());
    };
    if marker.phase == crate::internal::identity_transition_pending::TransitionPhase::Pending {
        return Ok(());
    }
    if marker.phase
        == crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched
    {
        mark_joined_transition_applied(core, &marker)?;
    }
    Ok(())
}

pub(crate) fn mark_joined_transition_applied(
    core: &crate::core::ImCore,
    marker: &crate::internal::identity_transition_pending::IdentityTransitionMarker,
) -> crate::ImResult<()> {
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let matches = index
        .credentials
        .values()
        .filter(|entry| {
            entry.unique_id == marker.owner_identity_id
                && entry.user_id == marker.account_user_id
                && entry.did == marker.current_did
                && entry.full_handle == marker.handle
                && entry.binding_generation.as_deref() == Some(marker.binding_generation.as_str())
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(recovery_error(HandleRecoveryErrorCode::UnknownEpoch));
    }
    let device_state = matches[0]
        .device_state
        .as_ref()
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::UnknownEpoch))?;
    let authorization = device_state
        .authorization
        .as_ref()
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::UnknownEpoch))?;
    let checkpoint = device_state
        .checkpoint
        .as_ref()
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::UnknownEpoch))?;
    crate::internal::identity_transition_pending::mark_applied(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &marker.recovery_id,
        crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
        authorization.protocol_device_id.as_str(),
        &authorization.auth_generation.to_string(),
        &checkpoint.registry_version.to_string(),
        "{}",
    )
}

fn reset_reference_from_marker(
    marker: &crate::internal::identity_transition_pending::IdentityTransitionMarker,
) -> crate::ImResult<HandleRecoveryResetReference> {
    Ok(HandleRecoveryResetReference {
        account_user_id: marker.account_user_id.clone(),
        owner_identity_id: marker.owner_identity_id.clone(),
        previous_did: crate::ids::Did::parse(&marker.previous_did)?,
        current_did: crate::ids::Did::parse(&marker.current_did)?,
        binding_generation: marker.binding_generation.clone(),
        handle: marker.handle.clone(),
        source_kind: match marker.source_kind {
            crate::internal::identity_transition_pending::TransitionSourceKind::Initiator => {
                HandleRecoveryTransitionSourceKind::Initiator
            }
            crate::internal::identity_transition_pending::TransitionSourceKind::JoinedDevice => {
                HandleRecoveryTransitionSourceKind::JoinedDevice
            }
        },
        source_id: marker.source_id.clone(),
    })
}

fn progress_v4(
    core: &crate::core::ImCore,
    pending: &PendingHandleRecoveryV4,
) -> crate::ImResult<HandleRecoveryProgress> {
    if let Ok(Some(operation)) = crate::internal::identity_handle_recovery_operation::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
    ) {
        let age = metric_age_seconds(&operation.updated_at);
        match pending.phase {
            PendingRecoveryPhaseV4::RemoteOutcomeUnknown => {
                crate::internal::identity_handle_recovery_metrics::record_remote_unresolved_age(
                    age,
                );
            }
            PendingRecoveryPhaseV4::LocalTransitionPending => {
                crate::internal::identity_handle_recovery_metrics::record_local_transition_pending_age(
                    age,
                );
            }
            _ => {}
        }
    }
    let marker = crate::internal::identity_transition_pending::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
    )?;
    let applied_marker = marker.as_ref().filter(|marker| {
        marker.phase == crate::internal::identity_transition_pending::TransitionPhase::Completed
    });
    let reset_reference = applied_marker
        .map(reset_reference_from_marker)
        .transpose()?;
    let result = pending.remote_result.as_ref();
    let (unsupported_e2ee_group_count, unsupported_did_only_group_count) =
        if !pending.fresh_local_state {
            crate::internal::group_rebind_recovery::recovery_impact_counts(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &pending.owner_identity_id,
                &pending.local_previous_did,
            )?
        } else {
            (0, 0)
        };
    Ok(HandleRecoveryProgress {
        operation_id: pending.operation_id.clone(),
        owner_identity_id: crate::ids::IdentityId::parse(&pending.owner_identity_id)?,
        account_user_id: pending
            .authoritative_binding
            .as_ref()
            .map(|binding| binding.account_user_id.clone()),
        full_handle: pending.full_handle.clone(),
        local_previous_did: Some(crate::ids::Did::parse(&pending.local_previous_did)?),
        current_did: pending.identity.did.clone(),
        binding_generation: result.map(|result| result.binding_generation.clone()),
        state_root_fingerprint: applied_marker.map(|marker| marker.state_root_fingerprint.clone()),
        phase: match pending.phase {
            PendingRecoveryPhaseV4::AwaitingFactor => HandleRecoveryPhase::AwaitingFactor,
            PendingRecoveryPhaseV4::ReadyToCommit => HandleRecoveryPhase::ReadyToCommit,
            PendingRecoveryPhaseV4::RemoteOutcomeUnknown => {
                HandleRecoveryPhase::RemoteOutcomeUnknown
            }
            PendingRecoveryPhaseV4::RemoteCommitted => HandleRecoveryPhase::RemoteCommitted,
            PendingRecoveryPhaseV4::LocalTransitionPending => {
                HandleRecoveryPhase::IdentityTransitionPending
            }
            PendingRecoveryPhaseV4::Applied => HandleRecoveryPhase::Applied,
            PendingRecoveryPhaseV4::QuarantinedKeyUnavailable => {
                HandleRecoveryPhase::QuarantinedKeyUnavailable
            }
        },
        impact: HandleRecoveryImpact {
            local_ordinary_data_will_migrate: !pending.fresh_local_state,
            other_devices_must_rejoin: true,
            unsupported_e2ee_group_count,
            unsupported_did_only_group_count,
        },
        reset_reference,
        failure_code: pending
            .last_error_code
            .as_deref()
            .and_then(public_error_code),
    })
}

fn metric_age_seconds(updated_at: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(updated_at)
        .ok()
        .map(|updated| {
            chrono::Utc::now()
                .signed_duration_since(updated.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0) as u64
        })
        .unwrap_or(0)
}

fn require_enabled(core: &crate::core::ImCore) -> crate::ImResult<()> {
    if !core.inner().handle_recovery_enabled() {
        return Err(crate::ImError::unsupported("handle-recovery-v4"));
    }
    Ok(())
}

fn recovery_error(code: HandleRecoveryErrorCode) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.as_str().to_owned()),
        message: code.as_str().to_owned(),
        data: None,
    }
}

fn is_local_deletion_conflict(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if code == "identity.local_deletion_conflict"
    )
}

async fn insert_precommit_operation_or_cleanup(
    core: &crate::core::ImCore,
    store: &PendingHandleRecoveryStore,
    pending: &PendingHandleRecoveryV4,
    operation: &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord,
) -> crate::ImResult<()> {
    let result = crate::internal::identity_handle_recovery_operation::insert(
        &core.inner().sdk_paths().local_state.sqlite_path,
        operation,
    );
    if let Err(error) = result {
        if is_local_deletion_conflict(&error) {
            crate::internal::identity_custody::discard_unpublished_handle_recovery_async(
                core,
                &pending.identity,
            )
            .await?;
            store.delete_v4_pre_attempt(&pending.operation_id)?;
        }
        return Err(error);
    }
    Ok(())
}

fn operation_not_found_error() -> crate::ImError {
    crate::ImError::invalid_input(
        Some("operation_id".to_owned()),
        "Handle Recovery V4 operation was not found",
    )
}

fn user_presence_required_error() -> crate::ImError {
    crate::ImError::invalid_input(
        Some("user_presence_confirmed".to_owned()),
        "explicit user presence is required",
    )
}

fn public_error_code(value: &str) -> Option<HandleRecoveryErrorCode> {
    [
        HandleRecoveryErrorCode::FactorRetryRequired,
        HandleRecoveryErrorCode::ResultAbsent,
        HandleRecoveryErrorCode::OutcomeUnknown,
        HandleRecoveryErrorCode::LocalKeyUnavailable,
        HandleRecoveryErrorCode::LocalTransitionPending,
        HandleRecoveryErrorCode::LocalMigrationUnsupported,
        HandleRecoveryErrorCode::UnknownEpoch,
    ]
    .into_iter()
    .find(|code| code.as_str() == value)
}

fn local_transition_retry_code(error: &crate::ImError) -> Option<HandleRecoveryErrorCode> {
    matches!(
        error,
        crate::ImError::TransportUnavailable { .. }
            | crate::ImError::AuthRequired
            | crate::ImError::SessionExpired
            | crate::ImError::Service { .. }
            | crate::ImError::Serialization { .. }
    )
    .then_some(HandleRecoveryErrorCode::LocalTransitionPending)
}

fn random_reference(prefix: &str) -> crate::ImResult<String> {
    let mut bytes = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| crate::ImError::Internal {
            message: format!("generate {prefix} reference failed"),
        })?;
    Ok(format!("{prefix}_{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn format_timestamp(value: time::OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

fn now_second_z() -> crate::ImResult<String> {
    format_timestamp(
        time::OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .map_err(|_| crate::ImError::PermissionDenied)?,
    )
}

fn parse_timestamp(value: &str) -> crate::ImResult<time::OffsetDateTime> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn require_explicit_identity(selector: &crate::identity::IdentitySelector) -> crate::ImResult<()> {
    if matches!(selector, crate::identity::IdentitySelector::Default) {
        return Err(crate::ImError::invalid_input(
            Some("identity".to_owned()),
            "Handle Recovery requires an explicit identity selector",
        ));
    }
    Ok(())
}

fn authorized_join_transition_error_code(
    account_handle_owner_closed: bool,
    local_matches_previous_did: bool,
    _has_local_transition_marker: bool,
) -> HandleRecoveryErrorCode {
    if !account_handle_owner_closed {
        HandleRecoveryErrorCode::UnknownEpoch
    } else if !local_matches_previous_did {
        HandleRecoveryErrorCode::LocalMigrationUnsupported
    } else {
        HandleRecoveryErrorCode::UnknownEpoch
    }
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.len()
            <= crate::internal::identity_wire::handle_recovery::MAX_BINDING_GENERATION_DIGITS
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.as_bytes()[0] != b'0'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_recovery_builds_unsigned_predecessor_without_old_private_key() {
        let old_did = "did:wba:example.invalid:users:alice:e1_old";
        let new_did = "did:wba:example.invalid:users:alice:e1_new";
        let document = json!({
            "id": old_did,
            "verificationMethod": [{
                "id": format!("{old_did}#key-1"),
                "controller": old_did,
                "type": "Multikey",
                "publicKeyMultibase": "z6Mkfixture"
            }],
            "proof": {"proofValue": "active-document-proof"}
        });

        let predecessor =
            unsigned_recovery_predecessor_document(document, old_did, new_did).unwrap();
        assert_eq!(predecessor["id"], old_did);
        assert_eq!(predecessor["deactivated"], true);
        assert_eq!(predecessor["successorDid"], new_did);
        assert!(predecessor.get("proof").is_none());
        assert!(predecessor.get("verificationMethod").is_some());
    }

    #[test]
    fn fresh_recovery_rejects_an_already_superseded_predecessor() {
        let old_did = "did:wba:example.invalid:users:alice:e1_old";
        let document = json!({
            "id": old_did,
            "deactivated": true,
            "successorDid": "did:wba:example.invalid:users:alice:e1_other"
        });
        assert!(unsigned_recovery_predecessor_document(
            document,
            old_did,
            "did:wba:example.invalid:users:alice:e1_new",
        )
        .is_err());
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read as _;

        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0, "request closed before headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.split_once(':').and_then(|(name, value)| {
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
            })
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0, "request closed before body");
            request.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8(request).unwrap()
    }

    fn write_json_response(stream: &mut std::net::TcpStream, value: &serde_json::Value) {
        use std::io::Write as _;

        let body = serde_json::to_vec(value).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        stream.flush().unwrap();
    }

    fn recovery_test_core(
        root: &std::path::Path,
        endpoint: &str,
        vault_key: [u8; 32],
    ) -> crate::ImCore {
        recovery_test_core_with_vault_scope(
            root,
            endpoint,
            vault_key,
            root.join("vault"),
            "recovery-reopen-workspace",
            "recovery-reopen-device",
        )
    }

    fn recovery_test_core_with_vault_scope(
        root: &std::path::Path,
        endpoint: &str,
        vault_key: [u8; 32],
        vault_dir: std::path::PathBuf,
        workspace_id: &str,
        device_id: &str,
    ) -> crate::ImCore {
        let paths = crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities/registry.json"),
                default_identity_path: Some(root.join("identities/default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local/im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        };
        crate::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse(endpoint).unwrap(),
                did_domain: "awiki.test".to_owned(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            paths,
            crate::ImCoreOpenOptions::default()
                .with_multi_device_handle_recovery_enabled(true)
                .with_multi_device_audience("awiki-user-service")
                .with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        crate::vault::DeviceVaultRootKey::from_bytes(vault_key),
                        vault_dir,
                        workspace_id,
                        device_id,
                    ),
                ),
        )
        .unwrap()
    }

    #[cfg(feature = "provider-traits")]
    fn recovery_test_core_with_provider(
        root: &std::path::Path,
        endpoint: &str,
        vault_key: [u8; 32],
        provider: std::sync::Arc<dyn crate::internal::identity_provider::IdentityCustody>,
    ) -> crate::ImCore {
        let paths = crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities/registry.json"),
                default_identity_path: Some(root.join("identities/default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local/im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        };
        crate::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse(endpoint).unwrap(),
                did_domain: "fixture.invalid".to_owned(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            paths,
            crate::ImCoreOpenOptions::default()
                .with_multi_device_handle_recovery_enabled(true)
                .with_multi_device_audience("awiki-user-service")
                .with_external_http_allow_insecure_loopback_for_testing(true)
                .with_identity_custody_provider(provider)
                .with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        crate::vault::DeviceVaultRootKey::from_bytes(vault_key),
                        root.join("vault"),
                        "fixture-workspace-0714",
                        "fixture-device-0714",
                    ),
                ),
        )
        .unwrap()
    }

    #[cfg(feature = "provider-traits")]
    struct Fixture0714IdentityCustody {
        inner: std::sync::Arc<dyn crate::internal::identity_provider::IdentityCustody>,
        predecessor_did: String,
        predecessor_document: serde_json::Value,
        transitions: std::sync::Mutex<
            std::collections::BTreeMap<String, std::sync::Arc<Fixture0714TransitionSession>>,
        >,
    }

    #[cfg(feature = "provider-traits")]
    impl Fixture0714IdentityCustody {
        fn new(
            inner: std::sync::Arc<dyn crate::internal::identity_provider::IdentityCustody>,
            predecessor_did: impl Into<String>,
            predecessor_document: serde_json::Value,
        ) -> Self {
            Self {
                inner,
                predecessor_did: predecessor_did.into(),
                predecessor_document,
                transitions: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            }
        }
    }

    #[cfg(feature = "provider-traits")]
    struct Fixture0714TransitionSession {
        candidate: crate::internal::identity_provider::ProviderPreparedIdentityTransition,
    }

    #[cfg(feature = "provider-traits")]
    #[async_trait::async_trait]
    impl crate::internal::identity_provider::ProviderIdentityTransitionSession
        for Fixture0714TransitionSession
    {
        async fn candidate(
            &self,
        ) -> crate::internal::identity_provider::ProviderResult<
            crate::internal::identity_provider::ProviderPreparedIdentityTransition,
        > {
            Ok(self.candidate.clone())
        }

        async fn begin_publication(
            &self,
        ) -> crate::internal::identity_provider::ProviderResult<
            crate::internal::identity_provider::ProviderIdentityTransitionPublicationAttempt,
        > {
            Ok(
                crate::internal::identity_provider::ProviderIdentityTransitionPublicationAttempt {
                    operation_id: self.candidate.operation_id.clone(),
                    predecessor_digest: self.candidate.predecessor_digest.clone(),
                    successor_digest: self.candidate.successor_digest.clone(),
                    publication_generation: 1,
                },
            )
        }

        async fn complete(
            &self,
            attempt: crate::internal::identity_provider::ProviderIdentityTransitionPublicationAttempt,
            result: crate::internal::identity_provider::ProviderIdentityTransitionPublicationResult,
        ) -> crate::internal::identity_provider::ProviderResult<
            crate::internal::identity_provider::ProviderIdentityTransitionOutcome,
        > {
            use crate::internal::identity_provider::{
                IdentityProviderError, IdentityProviderErrorCode,
                ProviderIdentityTransitionOutcome, ProviderIdentityTransitionPublicationResult,
            };
            if attempt.operation_id != self.candidate.operation_id
                || attempt.predecessor_digest != self.candidate.predecessor_digest
                || attempt.successor_digest != self.candidate.successor_digest
            {
                return Err(IdentityProviderError::new(
                    IdentityProviderErrorCode::InvalidRequest,
                    false,
                ));
            }
            match result {
                ProviderIdentityTransitionPublicationResult::Unknown => {
                    Ok(ProviderIdentityTransitionOutcome::PublicationUncertain)
                }
                ProviderIdentityTransitionPublicationResult::RejectedBeforeAcceptance => {
                    Ok(ProviderIdentityTransitionOutcome::Aborted)
                }
                ProviderIdentityTransitionPublicationResult::Confirmed { evidence }
                    if evidence.predecessor_digest == self.candidate.predecessor_digest
                        && evidence.successor_digest == self.candidate.successor_digest =>
                {
                    Ok(ProviderIdentityTransitionOutcome::Committed {
                        current_did: self.candidate.successor_did.clone(),
                    })
                }
                ProviderIdentityTransitionPublicationResult::Confirmed { .. } => {
                    Err(IdentityProviderError::new(
                        IdentityProviderErrorCode::VerificationFailed,
                        false,
                    ))
                }
            }
        }

        async fn reconcile(
            &self,
            observation: crate::internal::identity_provider::ProviderIdentityTransitionRemoteObservation,
        ) -> crate::internal::identity_provider::ProviderResult<
            crate::internal::identity_provider::ProviderIdentityTransitionOutcome,
        > {
            use crate::internal::identity_provider::{
                IdentityProviderError, IdentityProviderErrorCode,
                ProviderIdentityTransitionOutcome, ProviderIdentityTransitionRemoteObservation,
            };
            match observation {
                ProviderIdentityTransitionRemoteObservation::RemoteOld { current_document }
                    if current_document == self.candidate.predecessor_document =>
                {
                    Ok(ProviderIdentityTransitionOutcome::ReadyForPublication)
                }
                ProviderIdentityTransitionRemoteObservation::Published {
                    predecessor_document,
                    successor_document,
                } if predecessor_document == self.candidate.predecessor_document
                    && successor_document == self.candidate.successor_document =>
                {
                    Ok(ProviderIdentityTransitionOutcome::Committed {
                        current_did: self.candidate.successor_did.clone(),
                    })
                }
                _ => Err(IdentityProviderError::new(
                    IdentityProviderErrorCode::VerificationFailed,
                    false,
                )),
            }
        }
    }

    #[cfg(feature = "provider-traits")]
    #[async_trait::async_trait]
    impl crate::internal::identity_provider::IdentityCustody for Fixture0714IdentityCustody {
        async fn store_info(
            &self,
        ) -> crate::internal::identity_provider::ProviderResult<
            crate::internal::identity_provider::ProviderStoreInfo,
        > {
            self.inner.store_info().await
        }

        async fn list_identities(
            &self,
        ) -> crate::internal::identity_provider::ProviderResult<
            Vec<crate::internal::identity_provider::ProviderIdentityDescriptor>,
        > {
            self.inner.list_identities().await
        }

        async fn open_identity(
            &self,
            identity: &crate::internal::identity_provider::ProviderIdentityRef,
        ) -> crate::internal::identity_provider::ProviderResult<
            std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>,
        > {
            self.inner.open_identity(identity).await
        }

        async fn create_identity(
            &self,
            request: crate::internal::identity_provider::ProviderCreateIdentityRequest,
        ) -> crate::internal::identity_provider::ProviderResult<
            std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>,
        > {
            self.inner.create_identity(request).await
        }

        async fn delete_identity(
            &self,
            identity: &crate::internal::identity_provider::ProviderIdentityRef,
        ) -> crate::internal::identity_provider::ProviderResult<()> {
            self.inner.delete_identity(identity).await
        }

        async fn prepare_identity_transition(
            &self,
            request: crate::internal::identity_provider::ProviderIdentityTransitionRequest,
        ) -> crate::internal::identity_provider::ProviderResult<
            std::sync::Arc<
                dyn crate::internal::identity_provider::ProviderIdentityTransitionSession,
            >,
        > {
            use crate::internal::identity_provider::{
                IdentityProviderError, IdentityProviderErrorCode, ProviderTransitionAssurance,
            };
            if request.expected_current_did != self.predecessor_did
                || request.transition_document.is_some()
            {
                return Err(IdentityProviderError::new(
                    IdentityProviderErrorCode::InvalidRequest,
                    false,
                ));
            }
            let successor = self.inner.open_identity(&request.successor).await?;
            let successor = successor.public_identity().await?;
            if successor.reference != request.successor {
                return Err(IdentityProviderError::new(
                    IdentityProviderErrorCode::VerificationFailed,
                    false,
                ));
            }
            let mut predecessor_document = self.predecessor_document.clone();
            predecessor_document["successorDid"] =
                serde_json::Value::String(successor.reference.did.clone());
            let candidate =
                crate::internal::identity_provider::ProviderPreparedIdentityTransition {
                    operation_id: request.operation_id.clone(),
                    expected_current_did: request.expected_current_did,
                    successor_did: successor.reference.did,
                    predecessor_digest: crate::internal::identity_wire::document::document_hash(
                        &predecessor_document,
                    )
                    .map_err(|_| {
                        IdentityProviderError::new(
                            IdentityProviderErrorCode::VerificationFailed,
                            false,
                        )
                    })?,
                    successor_digest: crate::internal::identity_wire::document::document_hash(
                        &successor.document,
                    )
                    .map_err(|_| {
                        IdentityProviderError::new(
                            IdentityProviderErrorCode::VerificationFailed,
                            false,
                        )
                    })?,
                    predecessor_document,
                    successor_document: successor.document,
                    assurance: ProviderTransitionAssurance::Verified,
                };
            let mut transitions = self.transitions.lock().map_err(|_| {
                IdentityProviderError::new(IdentityProviderErrorCode::Internal, false)
            })?;
            if let Some(existing) = transitions.get(&request.operation_id) {
                if existing.candidate != candidate {
                    return Err(IdentityProviderError::new(
                        IdentityProviderErrorCode::Conflict,
                        false,
                    ));
                }
                return Ok(existing.clone());
            }
            let session = std::sync::Arc::new(Fixture0714TransitionSession { candidate });
            transitions.insert(request.operation_id, session.clone());
            Ok(session)
        }

        async fn resume_identity_transition(
            &self,
            expected_current_did: &str,
        ) -> crate::internal::identity_provider::ProviderResult<
            Option<
                std::sync::Arc<
                    dyn crate::internal::identity_provider::ProviderIdentityTransitionSession,
                >,
            >,
        > {
            if expected_current_did != self.predecessor_did {
                return Ok(None);
            }
            let transitions = self.transitions.lock().map_err(|_| {
                crate::internal::identity_provider::IdentityProviderError::new(
                    crate::internal::identity_provider::IdentityProviderErrorCode::Internal,
                    false,
                )
            })?;
            if transitions.len() > 1 {
                return Err(
                    crate::internal::identity_provider::IdentityProviderError::new(
                        crate::internal::identity_provider::IdentityProviderErrorCode::Conflict,
                        false,
                    ),
                );
            }
            Ok(transitions.values().next().cloned().map(|session| {
                session
                    as std::sync::Arc<
                        dyn crate::internal::identity_provider::ProviderIdentityTransitionSession,
                    >
            }))
        }

        async fn begin_device_enrollment(
            &self,
            request: crate::internal::identity_provider::ProviderDeviceEnrollmentRequest,
        ) -> crate::internal::identity_provider::ProviderResult<
            std::sync::Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>,
        > {
            self.inner.begin_device_enrollment(request).await
        }

        async fn begin_request_signing_enrollment(
            &self,
            request: crate::internal::identity_provider::ProviderRequestSigningEnrollmentRequest,
        ) -> crate::internal::identity_provider::ProviderResult<
            std::sync::Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>,
        > {
            self.inner.begin_request_signing_enrollment(request).await
        }

        async fn resume_enrollment(
            &self,
            identity: &crate::internal::identity_provider::ProviderIdentityRef,
        ) -> crate::internal::identity_provider::ProviderResult<
            Option<
                std::sync::Arc<dyn crate::internal::identity_provider::ProviderEnrollmentSession>,
            >,
        > {
            self.inner.resume_enrollment(identity).await
        }

        async fn confirm_root_promotion(
            &self,
            identity: &crate::internal::identity_provider::ProviderIdentityRef,
            remote: crate::internal::identity_provider::ProviderVerifiedRemoteDocument,
        ) -> crate::internal::identity_provider::ProviderResult<()> {
            self.inner.confirm_root_promotion(identity, remote).await
        }

        async fn sign_pending_root_object_proof(
            &self,
            identity: &crate::internal::identity_provider::ProviderIdentityRef,
            request: crate::internal::identity_provider::ProviderObjectProofRequest,
        ) -> crate::internal::identity_provider::ProviderResult<serde_json::Value> {
            self.inner
                .sign_pending_root_object_proof(identity, request)
                .await
        }

        async fn recover(&self) -> crate::internal::identity_provider::ProviderResult<()> {
            self.inner.recover().await
        }
    }

    #[cfg(all(
        feature = "provider-traits",
        feature = "identity-native-anp",
        feature = "group-e2ee"
    ))]
    #[tokio::test]
    async fn phase4_0714_fixture_completes_formal_recovery_after_commit_response_loss() {
        use sha2::Digest as _;

        let Ok(fixture_dir) = std::env::var("AWIKI_0714_E2EE_FIXTURE_DIR") else {
            return;
        };
        let fixture_dir = std::path::Path::new(&fixture_dir);
        let root = tempfile::tempdir().unwrap();
        copy_fixture_tree(fixture_dir, root.path());
        std::fs::create_dir_all(root.path().join("local")).unwrap();
        std::fs::copy(
            root.path().join("core-schema-36.sqlite"),
            root.path().join("local/im.sqlite"),
        )
        .unwrap();

        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("manifest.json")).unwrap())
                .unwrap();
        let locked_digests = manifest["fileDigests"].as_object().unwrap().clone();
        let root_key = URL_SAFE_NO_PAD
            .decode(
                std::fs::read_to_string(root.path().join("vault-root-key.fixture.b64u"))
                    .unwrap()
                    .trim(),
            )
            .unwrap()
            .try_into()
            .unwrap();

        let predecessor_did =
            "did:wba:alice.fixture.invalid:agents:primary:e1_fixture_alice".to_owned();
        let mut predecessor =
            crate::internal::identity_generation::generate_handle_recovery_identity(
                "invalid", "fixture", None, None,
            )
            .unwrap();
        let generated_predecessor_did = predecessor.did.as_str().to_owned();
        replace_json_string(
            &mut predecessor.did_document,
            &generated_predecessor_did,
            &predecessor_did,
        );
        assert_eq!(
            predecessor.did_document["id"].as_str(),
            Some(predecessor_did.as_str())
        );

        let provider_root = root.path().join("fixture-provider");
        let manager =
            anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
                state_root: provider_root,
                root_key: anp_identity::RootKeySource::Injected(
                    anp_identity::InjectedStoreKey::new("phase4-0714-provider", [0x74; 32]),
                ),
            })
            .unwrap();
        let direct: std::sync::Arc<dyn crate::internal::identity_provider::IdentityCustody> =
            std::sync::Arc::new(
                crate::internal::identity_provider::DirectAnpIdentityCustody::new(manager),
            );
        let provider: std::sync::Arc<dyn crate::internal::identity_provider::IdentityCustody> =
            std::sync::Arc::new(Fixture0714IdentityCustody::new(
                direct,
                predecessor_did.clone(),
                predecessor.did_document.clone(),
            ));

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut methods = Vec::new();
            let mut remote_result = None;
            for request_index in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                let raw = read_http_request(&mut stream);
                let body = http_request_json(&raw);
                if request_index == 0 {
                    methods.push(body["method"].as_str().unwrap().to_owned());
                    write_json_response(
                        &mut stream,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": body["id"],
                            "result": {
                                "ok": true,
                                "retry_after_seconds": 60,
                                "retry_at": "2099-08-26T12:00:00Z"
                            }
                        }),
                    );
                } else if request_index == 1 {
                    methods.push("handle_recovery_exchange_v4".to_owned());
                    write_json_response(
                        &mut stream,
                        &json!({
                            "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                            "recovery_grant": "phase4-0714-fixture-grant",
                            "purpose": "awiki.identity.handle-recovery.v1",
                            "expires_at": "2099-08-26T12:05:00Z",
                            "current_binding": {
                                "account_user_id": "fixture-account-0714",
                                "full_handle": "fixture.invalid",
                                "current_did": "did:wba:alice.fixture.invalid:agents:primary:e1_fixture_alice",
                                "binding_generation": "1"
                            }
                        }),
                    );
                } else if request_index == 2 {
                    methods.push(body["method"].as_str().unwrap().to_owned());
                    let intent = &body["params"]["intent"];
                    let successor = body["params"]["new_did_document"].clone();
                    let successor_did = successor["id"].as_str().unwrap().to_owned();
                    let device = &successor["deviceManifest"]["devices"][0];
                    remote_result = Some(json!({
                        "state": "recovered",
                        "operation_id": intent["operation_id"],
                        "intent_hash": body["params"]["intent_hash"],
                        "intent_schema_version": "1",
                        "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                        "account_user_id": "fixture-account-0714",
                        "full_handle": "fixture.invalid",
                        "previous_did": intent["expected_previous_did"],
                        "current_did": successor_did,
                        "binding_generation": "2",
                        "checkpoint": {
                            "document_version": 2,
                            "document_hash": crate::internal::identity_wire::document::document_hash(&successor).unwrap(),
                            "registry_version": 2
                        },
                        "bootstrap_device": {
                            "device_id": intent["bootstrap_device_id"],
                            "status": "active",
                            "role": "admin",
                            "management_ready": true,
                            "auth_generation": 1
                        },
                        "committed_at": "2026-08-26T12:00:00Z",
                        "fixture_signing_key_id": device["signing_key_id"]
                    }));
                    // The authoritative commit has happened, but its HTTP response is lost.
                    drop(stream);
                } else if request_index == 3 {
                    methods.push(body["method"].as_str().unwrap().to_owned());
                    let result = remote_result.as_ref().unwrap();
                    let mut public_result = result.clone();
                    public_result
                        .as_object_mut()
                        .unwrap()
                        .remove("fixture_signing_key_id");
                    write_json_response(
                        &mut stream,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": body["id"],
                            "result": {"state": "committed", "result": public_result}
                        }),
                    );
                } else if request_index == 4 {
                    methods.push(body["method"].as_str().unwrap().to_owned());
                    let result = remote_result.as_ref().unwrap();
                    let did = result["current_did"].as_str().unwrap();
                    let device_id = result["bootstrap_device"]["device_id"].as_str().unwrap();
                    let key_id = result["fixture_signing_key_id"].as_str().unwrap();
                    write_json_response(
                        &mut stream,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": body["id"],
                            "result": {
                                "did": did,
                                "user_id": "fixture-account-0714",
                                "access_token": fixture_access_token(did, device_id, key_id)
                            }
                        }),
                    );
                } else {
                    methods.push(body["method"].as_str().unwrap().to_owned());
                    let publish = &body["params"]["body"];
                    write_json_response(
                        &mut stream,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": body["id"],
                            "result": {
                                "published": true,
                                "owner_did": publish["prekey_bundle"]["owner_did"],
                                "owner_device_id": publish["prekey_bundle"]["owner_device_id"],
                                "bundle_id": publish["prekey_bundle"]["bundle_id"],
                                "published_at": "2026-08-26T12:00:01Z",
                                "published_opk_count": publish["one_time_prekeys"].as_array().unwrap().len()
                            }
                        }),
                    );
                }
            }
            methods
        });

        let core =
            recovery_test_core_with_provider(root.path(), &endpoint, root_key, provider.clone());
        crate::internal::identity_store::IdentityStore::new(
            &core.inner().sdk_paths().identities,
        )
        .save_anp_identity_projection(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias: "default".to_owned(),
                did: crate::ids::Did::parse(&predecessor_did).unwrap(),
                unique_id: "fixture-owner-0714".to_owned(),
                user_id: "fixture-account-0714".to_owned(),
                display_name: "Fixture 0714".to_owned(),
                handle: "fixture".to_owned(),
                full_handle: "fixture.invalid".to_owned(),
                binding_generation: Some("1".to_owned()),
                jwt_token: String::new(),
                did_document: Some(predecessor.did_document),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                    root_key_id: format!("{predecessor_did}#fixture-root"),
                    device_signing_key_id: format!("{predecessor_did}#fixture-sign"),
                    device_e2ee_key_id: format!("{predecessor_did}#fixture-e2ee"),
                },
                device_state: None,
                key1_private_pem: String::new(),
                key1_public_pem: String::new(),
                e2ee_signing_private_pem: String::new(),
                e2ee_agreement_private_pem: String::new(),
                daemon_subkey_package: None,
                make_default: true,
            },
            crate::internal::identity_store::AnpIdentityProjectionStorage::from_core_pending_auth(
                &core,
                "fixture-legacy-provider",
                "fixture-legacy-identity",
            )
            .unwrap(),
        )
        .unwrap();

        let otp = core
            .handle_recovery()
            .request_handle_recovery_otp(HandleRecoveryOtpRequest {
                identity: None,
                full_handle: "fixture.invalid".to_owned(),
                phone: "+15555550714".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(otp.owner_identity_id.as_str(), "fixture-owner-0714");
        let prepared = core
            .handle_recovery()
            .prepare_handle_recovery(HandleRecoveryPrepareRequest {
                operation_id: otp.operation_id.clone(),
                phone: "+15555550714".to_owned(),
                code: "071407".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(prepared.phase, HandleRecoveryPhase::ReadyToCommit);
        let uncertain = core
            .handle_recovery()
            .activate_handle_recovery(HandleRecoveryActivateRequest {
                operation_id: otp.operation_id.clone(),
                user_presence_confirmed: true,
            })
            .await
            .unwrap();
        assert_eq!(uncertain.phase, HandleRecoveryPhase::RemoteOutcomeUnknown);
        assert_eq!(
            uncertain.failure_code,
            Some(HandleRecoveryErrorCode::OutcomeUnknown)
        );
        let successor_did = uncertain.current_did.as_str().to_owned();
        drop(core);

        let reopened =
            recovery_test_core_with_provider(root.path(), &endpoint, root_key, provider.clone());
        let applied = reopened
            .handle_recovery()
            .resume_handle_recovery(HandleRecoveryResumeRequest {
                operation_id: otp.operation_id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(applied.phase, HandleRecoveryPhase::Applied);
        assert_eq!(applied.owner_identity_id.as_str(), "fixture-owner-0714");
        assert_eq!(
            applied.account_user_id.as_deref(),
            Some("fixture-account-0714")
        );
        assert_eq!(applied.full_handle, "fixture.invalid");
        assert_eq!(applied.current_did.as_str(), successor_did);

        let connection = crate::internal::local_state::open_writable(
            &reopened.inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        let binding: (String, String, String, String, i64) = connection
            .query_row(
                "SELECT owner_identity_id,account_id,current_did,identity_generation,created_at FROM identity_account_bindings",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            binding,
            (
                "fixture-owner-0714".to_owned(),
                "fixture-account-0714".to_owned(),
                successor_did.clone(),
                "2".to_owned(),
                1_710_400_000,
            )
        );
        for (table, expected) in [
            ("messages", 2_i64),
            ("conversation_registry", 1),
            ("conversation_summaries", 1),
            ("direct_peer_routes", 1),
            ("attachment_manifest_cache", 1),
            ("sync_state", 1),
            ("e2ee_outbox", 1),
            ("group_rebind_outbox", 1),
        ] {
            assert_eq!(
                connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                expected,
                "formal Recovery changed the locked 0714 {table} count"
            );
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM identity_transition_pending WHERE source_id=?1 AND phase='completed'",
                    [&otp.operation_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM handle_recovery_operations_v4 WHERE operation_id=?1 AND lifecycle_class='applied'",
                    [&otp.operation_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM direct_e2ee_v2_prekey_bundles WHERE owner_identity_id='fixture-owner-0714' AND owner_did=?1 AND status='active'",
                    [&successor_did],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT local_status FROM e2ee_outbox WHERE outbox_id='fixture-outbox-0714'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "dropped"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT phase FROM group_rebind_outbox WHERE job_id='fixture-rebind-job-0714'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "blocked"
        );
        drop(connection);

        let index = crate::internal::identity_store::IdentityStore::new(
            &reopened.inner().sdk_paths().identities,
        )
        .load_index()
        .unwrap();
        assert_eq!(index.credentials.len(), 1);
        let identity = index.credentials.get("default").unwrap();
        assert_eq!(identity.unique_id, "fixture-owner-0714");
        assert_eq!(identity.user_id, "fixture-account-0714");
        assert_eq!(identity.did, successor_did);
        assert_eq!(identity.full_handle, "fixture.invalid");

        for (relative, expected) in locked_digests {
            let actual = format!(
                "{:x}",
                sha2::Sha256::digest(std::fs::read(root.path().join(&relative)).unwrap())
            );
            assert_eq!(
                actual,
                expected.as_str().unwrap(),
                "0714 artifact changed: {relative}"
            );
        }
        let methods = server.join().unwrap();
        assert_eq!(
            methods,
            vec![
                "send_otp",
                "handle_recovery_exchange_v4",
                "handle_recovery_commit_v4",
                "handle_recovery_result_get_v4",
                "get_me",
                "direct.e2ee.publish_prekey_bundle",
            ]
        );
    }

    #[cfg(all(
        feature = "provider-traits",
        feature = "identity-native-anp",
        feature = "group-e2ee"
    ))]
    fn copy_fixture_tree(source: &std::path::Path, target: &std::path::Path) {
        std::fs::create_dir_all(target).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let source_path = entry.path();
            let target_path = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_fixture_tree(&source_path, &target_path);
            } else {
                std::fs::copy(source_path, target_path).unwrap();
            }
        }
    }

    #[cfg(all(
        feature = "provider-traits",
        feature = "identity-native-anp",
        feature = "group-e2ee"
    ))]
    fn replace_json_string(value: &mut serde_json::Value, from: &str, to: &str) {
        match value {
            serde_json::Value::String(text) => *text = text.replace(from, to),
            serde_json::Value::Array(values) => {
                for value in values {
                    replace_json_string(value, from, to);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    replace_json_string(value, from, to);
                }
            }
            _ => {}
        }
    }

    #[cfg(all(
        feature = "provider-traits",
        feature = "identity-native-anp",
        feature = "group-e2ee"
    ))]
    fn http_request_json(request: &str) -> serde_json::Value {
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    #[cfg(all(
        feature = "provider-traits",
        feature = "identity-native-anp",
        feature = "group-e2ee"
    ))]
    fn fixture_access_token(did: &str, device_id: &str, key_id: &str) -> String {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": did,
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": did,
            "user_id": "fixture-account-0714",
            "device_id": device_id,
            "key_id": key_id,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now,
            "nbf": now,
            "exp": now + 300,
            "jti": "phase4-0714-recovery"
        });
        format!(
            "e30.{}.fixture",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    fn v4_awaiting_factor_pending(
        operation_id: &str,
        owner_identity_id: &str,
    ) -> PendingHandleRecoveryV4 {
        let generated = crate::internal::identity_generation::generate_handle_recovery_identity(
            "awiki.test",
            "alice",
            None,
            None,
        )
        .unwrap();
        let identity =
            crate::internal::identity_handle_recovery_pending::HandleRecoveryIdentityRef {
                store_id: "store-test".to_owned(),
                identity_id: "identity-test".to_owned(),
                did: generated.did,
                did_document: generated.did_document,
                protocol_device_id: generated.protocol_device_id,
                root_key_id: generated.root_key_id,
                device_signing_key_id: generated.device_signing_key_id,
                device_e2ee_key_id: generated.device_e2ee_key_id,
            };
        PendingHandleRecoveryV4::new_pre_otp(
            operation_id.to_owned(),
            owner_identity_id.to_owned(),
            "alice".to_owned(),
            "Alice".to_owned(),
            true,
            false,
            "alice.awiki.test".to_owned(),
            "did:wba:awiki.test:users:alice-old".to_owned(),
            identity,
        )
        .unwrap()
    }

    fn freeze_and_commit_v4_pending(
        pending: &mut PendingHandleRecoveryV4,
    ) -> crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4 {
        pending
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-v4-1".to_owned(),
                    full_handle: pending.full_handle.clone(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "grant-v4-1".to_owned(),
                "2099-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        pending
            .mark_commit_attempted("2026-08-07T00:01:00Z".to_owned())
            .unwrap();
        let result = remote_result_for_pending(pending, "user-v4-1");
        pending.record_remote_result(result.clone()).unwrap();
        result
    }

    fn remote_result_for_pending(
        pending: &PendingHandleRecoveryV4,
        account_user_id: &str,
    ) -> crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4 {
        crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4 {
            state: "recovered".to_owned(),
            operation_id: pending.operation_id.clone(),
            intent_hash: pending.intent_hash.clone().unwrap(),
            intent_schema_version: "1".to_owned(),
            contract_version:
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned(),
            account_user_id: account_user_id.to_owned(),
            full_handle: pending.full_handle.clone(),
            previous_did: pending.local_previous_did.clone(),
            current_did: pending.identity.did.as_str().to_owned(),
            binding_generation: "8".to_owned(),
            checkpoint: crate::internal::identity_handle_recovery_pending::RecoveryCheckpointV4 {
                document_version: 1,
                document_hash: crate::internal::identity_wire::document::document_hash(
                    &pending.identity.did_document,
                )
                .unwrap(),
                registry_version: 1,
            },
            bootstrap_device:
                crate::internal::identity_handle_recovery_pending::RecoveryBootstrapDeviceV4 {
                    device_id: pending.identity.protocol_device_id.as_str().to_owned(),
                    status: "active".to_owned(),
                    role: "admin".to_owned(),
                    management_ready: true,
                    auth_generation: 1,
                },
            committed_at: "2026-08-07T00:01:01Z".to_owned(),
        }
    }

    fn create_v4_awaiting_factor_operation_inner(
        core: &crate::ImCore,
        operation_id: &str,
        owner_identity_id: &str,
        with_transition_predecessor: bool,
    ) -> (
        PendingHandleRecoveryV4,
        crate::internal::secret_vault::SecretRef,
    ) {
        let mut pending = v4_awaiting_factor_pending(operation_id, owner_identity_id);
        pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
            core,
            "awiki.test",
            "alice",
        )
        .unwrap();
        if with_transition_predecessor {
            let predecessor_spec =
                crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
                    "awiki.test",
                    "alice",
                    None,
                    None,
                )
                .unwrap();
            pending.local_previous_did =
                crate::internal::identity_custody::open_controller_manager(core)
                    .unwrap()
                    .create(crate::internal::identity_custody::native_create_spec(
                        predecessor_spec.spec,
                    ))
                    .unwrap()
                    .public_identity()
                    .unwrap()
                    .reference
                    .did;
        }
        pending.validate().unwrap();
        let store = PendingHandleRecoveryStore::from_core(core).unwrap();
        let secret_ref = store.create_v4(&pending).unwrap();
        let operation = crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            operation_id.to_owned(),
            owner_identity_id.to_owned(),
            pending.full_handle.clone(),
            crate::internal::identity_handle_recovery_pending::pending_v4_key_id(operation_id),
            "2026-08-07T00:00:00Z".to_owned(),
        )
        .unwrap();
        crate::internal::identity_handle_recovery_operation::insert(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &operation,
        )
        .unwrap();
        (pending, secret_ref)
    }

    fn create_v4_awaiting_factor_operation(
        core: &crate::ImCore,
        operation_id: &str,
        owner_identity_id: &str,
    ) -> (
        PendingHandleRecoveryV4,
        crate::internal::secret_vault::SecretRef,
    ) {
        create_v4_awaiting_factor_operation_inner(core, operation_id, owner_identity_id, false)
    }

    fn create_v4_operation_with_transition_identities(
        core: &crate::ImCore,
        operation_id: &str,
        owner_identity_id: &str,
    ) -> PendingHandleRecoveryV4 {
        create_v4_awaiting_factor_operation_inner(core, operation_id, owner_identity_id, true).0
    }

    fn make_v4_operation_remote_unresolved(
        core: &crate::ImCore,
        pending: &mut PendingHandleRecoveryV4,
    ) {
        let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
        let store = PendingHandleRecoveryStore::from_core(core).unwrap();
        let revision = pending.revision;
        pending
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-refresh-1".to_owned(),
                    full_handle: pending.full_handle.clone(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "grant-refresh-1".to_owned(),
                "2099-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        store.save_v4_cas(pending, revision).unwrap();
        crate::internal::identity_handle_recovery_operation::record_frozen_intent(
            sqlite_path,
            &pending.operation_id,
            "user-refresh-1",
            pending.intent_hash.as_deref().unwrap(),
            "2026-08-07T00:00:30Z",
        )
        .unwrap();
        crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
            sqlite_path,
            &pending.operation_id,
            "2026-08-07T00:01:00Z",
        )
        .unwrap();
        let revision = pending.revision;
        pending
            .mark_commit_attempted("2026-08-07T00:01:00Z".to_owned())
            .unwrap();
        store.save_v4_cas(pending, revision).unwrap();
    }

    #[cfg(all(feature = "provider-traits", feature = "identity-native-anp"))]
    #[tokio::test]
    async fn fresh_recovery_refreshes_expired_root_proof_before_freezing_intent() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), &endpoint, [96_u8; 32]);
        let operation_id = "recover-v4-refresh-stale-document";
        let mut pending = v4_awaiting_factor_pending(operation_id, "owner-before-refresh");
        pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
            &core,
            "awiki.test",
            "alice",
        )
        .unwrap();

        let original_did = pending.identity.did.clone();
        let original_identity_id = pending.identity.identity_id.clone();
        let original_device_id = pending.identity.protocol_device_id.clone();
        let original_signing_key_id = pending.identity.device_signing_key_id.clone();
        let original_e2ee_key_id = pending.identity.device_e2ee_key_id.clone();
        let stale_created = time::OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .unwrap()
            - time::Duration::seconds(600);
        let stale_created = format_timestamp(stale_created).unwrap();
        let mut unsigned = pending.identity.did_document.clone();
        unsigned.as_object_mut().unwrap().remove("proof");
        let provider = crate::internal::identity_custody::controller_custody_provider(&core)
            .await
            .unwrap();
        let reference = crate::internal::identity_provider::ProviderIdentityRef {
            store_id: pending.identity.store_id.clone(),
            identity_id: pending.identity.identity_id.clone(),
            did: pending.identity.did.as_str().to_owned(),
        };
        pending.identity.did_document = provider
            .sign_document_proof(
                &reference,
                crate::internal::identity_provider::ProviderDocumentProofRequest {
                    key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                        pending.identity.root_key_id.clone(),
                    ),
                    document: unsigned,
                    options: crate::internal::identity_provider::ProviderDocumentProofOptions {
                        proof_purpose: Some("assertionMethod".to_owned()),
                        proof_type: Some(anp::proof::PROOF_TYPE_DATA_INTEGRITY.to_owned()),
                        cryptosuite: Some(anp::proof::CRYPTOSUITE_EDDSA_JCS_2022.to_owned()),
                        created: Some(stale_created.clone()),
                        domain: Some("awiki.test".to_owned()),
                        challenge: Some("stale-proof-fixture".to_owned()),
                    },
                },
            )
            .await
            .unwrap();
        pending.validate().unwrap();
        let stale_document_hash =
            crate::internal::identity_wire::document::document_hash(&pending.identity.did_document)
                .unwrap();
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        store.create_v4(&pending).unwrap();
        crate::internal::identity_handle_recovery_operation::insert(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
                operation_id.to_owned(),
                pending.owner_identity_id.clone(),
                pending.full_handle.clone(),
                crate::internal::identity_handle_recovery_pending::pending_v4_key_id(operation_id),
                "2026-09-03T09:00:00Z".to_owned(),
            )
            .unwrap(),
        )
        .unwrap();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            assert!(request.contains("/user-service/v1/auth/handle-recovery/v4/exchange"));
            write_json_response(
                &mut stream,
                &json!({
                    "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                    "recovery_grant": "fresh-root-proof-grant",
                    "purpose": "awiki.identity.handle-recovery.v1",
                    "expires_at": "2099-09-03T09:10:00Z",
                    "current_binding": {
                        "account_user_id": "account-stale-proof",
                        "full_handle": "alice.awiki.test",
                        "current_did": "did:wba:awiki.test:users:alice-old",
                        "binding_generation": "7",
                    },
                }),
            );
        });

        let progress = prepare(
            &core,
            HandleRecoveryPrepareRequest {
                operation_id: operation_id.to_owned(),
                phone: "+15555550100".to_owned(),
                code: "123456".to_owned(),
            },
        )
        .await
        .unwrap();
        assert_eq!(progress.phase, HandleRecoveryPhase::ReadyToCommit);
        server.join().unwrap();

        let (_, refreshed) = store.load_v4(operation_id).unwrap().unwrap();
        let refreshed_created = refreshed.identity.did_document["proof"]["created"]
            .as_str()
            .unwrap();
        let refreshed_created = parse_timestamp(refreshed_created).unwrap();
        let stale_created = parse_timestamp(&stale_created).unwrap();
        assert!(refreshed_created - stale_created > time::Duration::seconds(300));
        assert_eq!(refreshed.operation_id, operation_id);
        assert_eq!(refreshed.identity.did, original_did);
        assert_eq!(refreshed.identity.identity_id, original_identity_id);
        assert_eq!(refreshed.identity.protocol_device_id, original_device_id);
        assert_eq!(
            refreshed.identity.device_signing_key_id,
            original_signing_key_id
        );
        assert_eq!(refreshed.identity.device_e2ee_key_id, original_e2ee_key_id);
        assert_ne!(
            crate::internal::identity_wire::document::document_hash(
                &refreshed.identity.did_document,
            )
            .unwrap(),
            stale_document_hash
        );
        let mut intent_document = refreshed.identity.did_document.clone();
        intent_document.as_object_mut().unwrap().remove("proof");
        assert_eq!(
            refreshed.intent.as_ref().unwrap().new_did_document_hash,
            crate::internal::identity_wire::document::document_hash(&intent_document).unwrap()
        );
        anp::authentication::verify_active_e1_document(
            refreshed.identity.did.as_str(),
            &refreshed.identity.did_document,
        )
        .unwrap();
    }

    #[cfg(all(feature = "provider-traits", feature = "identity-native-anp"))]
    #[tokio::test]
    async fn absent_result_retry_refreshes_proof_without_rebinding_frozen_intent() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), &endpoint, [97_u8; 32]);
        let operation_id = "recover-v4-refresh-after-absent";
        let mut pending = v4_awaiting_factor_pending(operation_id, "owner-before-absent");
        pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
            &core,
            "awiki.test",
            "alice",
        )
        .unwrap();
        let previous_did = pending.local_previous_did.clone();
        pending.freeze_fresh_local_owner(&previous_did).unwrap();
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        store.create_v4(&pending).unwrap();
        crate::internal::identity_handle_recovery_operation::insert(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
                operation_id.to_owned(),
                pending.owner_identity_id.clone(),
                pending.full_handle.clone(),
                crate::internal::identity_handle_recovery_pending::pending_v4_key_id(operation_id),
                "2026-09-03T09:00:00Z".to_owned(),
            )
            .unwrap(),
        )
        .unwrap();
        make_v4_operation_remote_unresolved(&core, &mut pending);
        let frozen_intent_hash = pending.intent_hash.clone().unwrap();

        let stale_created = time::OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .unwrap()
            - time::Duration::seconds(600);
        let stale_created = format_timestamp(stale_created).unwrap();
        let mut unsigned = pending.identity.did_document.clone();
        unsigned.as_object_mut().unwrap().remove("proof");
        let provider = crate::internal::identity_custody::controller_custody_provider(&core)
            .await
            .unwrap();
        let stale_document = provider
            .sign_document_proof(
                &crate::internal::identity_provider::ProviderIdentityRef {
                    store_id: pending.identity.store_id.clone(),
                    identity_id: pending.identity.identity_id.clone(),
                    did: pending.identity.did.as_str().to_owned(),
                },
                crate::internal::identity_provider::ProviderDocumentProofRequest {
                    key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                        pending.identity.root_key_id.clone(),
                    ),
                    document: unsigned,
                    options: crate::internal::identity_provider::ProviderDocumentProofOptions {
                        proof_purpose: Some("assertionMethod".to_owned()),
                        proof_type: Some(anp::proof::PROOF_TYPE_DATA_INTEGRITY.to_owned()),
                        cryptosuite: Some(anp::proof::CRYPTOSUITE_EDDSA_JCS_2022.to_owned()),
                        created: Some(stale_created.clone()),
                        domain: Some("awiki.test".to_owned()),
                        challenge: Some("stale-post-attempt-proof".to_owned()),
                    },
                },
            )
            .await
            .unwrap();
        let revision = pending.revision;
        pending
            .replace_identity_document_proof(stale_document)
            .unwrap();
        pending
            .record_retryable_error(HandleRecoveryErrorCode::ResultAbsent.as_str().to_owned())
            .unwrap();
        store.save_v4_cas(&pending, revision).unwrap();

        let server = std::thread::spawn(move || {
            let (mut result_get, _) = listener.accept().unwrap();
            let request = read_http_request(&mut result_get);
            let body: serde_json::Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["method"], "handle_recovery_result_get_v4");
            write_json_response(
                &mut result_get,
                &json!({
                    "jsonrpc": "2.0",
                    "id": body["id"],
                    "result": {
                        "state": "result_absent",
                        "padding": crate::internal::identity_wire::handle_recovery::HANDLE_RECOVERY_RESULT_ABSENT_PADDING,
                    },
                }),
            );

            let (mut exchange, _) = listener.accept().unwrap();
            let request = read_http_request(&mut exchange);
            assert!(request.contains("/user-service/v1/auth/handle-recovery/v4/exchange"));
            write_json_response(
                &mut exchange,
                &json!({
                    "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                    "recovery_grant": "post-absent-fresh-grant",
                    "purpose": "awiki.identity.handle-recovery.v1",
                    "expires_at": "2099-09-03T09:10:00Z",
                    "current_binding": {
                        "account_user_id": "user-refresh-1",
                        "full_handle": "alice.awiki.test",
                        "current_did": "did:wba:awiki.test:users:alice-old",
                        "binding_generation": "7",
                    },
                }),
            );
        });

        let progress = prepare(
            &core,
            HandleRecoveryPrepareRequest {
                operation_id: operation_id.to_owned(),
                phone: "+15555550100".to_owned(),
                code: "123456".to_owned(),
            },
        )
        .await
        .unwrap();
        assert_eq!(progress.phase, HandleRecoveryPhase::RemoteOutcomeUnknown);
        server.join().unwrap();

        let (_, refreshed) = store.load_v4(operation_id).unwrap().unwrap();
        let refreshed_created = parse_timestamp(
            refreshed.identity.did_document["proof"]["created"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert!(
            refreshed_created - parse_timestamp(&stale_created).unwrap()
                > time::Duration::seconds(300)
        );
        assert_eq!(
            refreshed.intent_hash.as_deref(),
            Some(frozen_intent_hash.as_str())
        );
        assert_eq!(refreshed.operation_id, operation_id);
        assert!(refreshed.commit_attempted);
        anp::authentication::verify_active_e1_document(
            refreshed.identity.did.as_str(),
            &refreshed.identity.did_document,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn absent_result_with_expired_grant_requires_factor_refresh() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), &endpoint, [98_u8; 32]);
        let operation_id = "recover-v4-expired-grant-after-absent";
        let mut pending = v4_awaiting_factor_pending(operation_id, "owner-expired-grant");
        pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
            &core,
            "awiki.test",
            "alice",
        )
        .unwrap();
        let previous_did = pending.local_previous_did.clone();
        pending.freeze_fresh_local_owner(&previous_did).unwrap();
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        store.create_v4(&pending).unwrap();
        crate::internal::identity_handle_recovery_operation::insert(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
                operation_id.to_owned(),
                pending.owner_identity_id.clone(),
                pending.full_handle.clone(),
                crate::internal::identity_handle_recovery_pending::pending_v4_key_id(operation_id),
                "2026-09-03T09:00:00Z".to_owned(),
            )
            .unwrap(),
        )
        .unwrap();
        make_v4_operation_remote_unresolved(&core, &mut pending);
        let revision = pending.revision;
        let binding = pending.authoritative_binding.clone().unwrap();
        pending
            .refresh_grant(
                &binding,
                "expired-grant".to_owned(),
                "2020-01-01T00:00:00Z".to_owned(),
            )
            .unwrap();
        store.save_v4_cas(&pending, revision).unwrap();

        let server = std::thread::spawn(move || {
            let (mut result_get, _) = listener.accept().unwrap();
            let request = read_http_request(&mut result_get);
            let body: serde_json::Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["method"], "handle_recovery_result_get_v4");
            write_json_response(
                &mut result_get,
                &json!({
                    "jsonrpc": "2.0",
                    "id": body["id"],
                    "result": {
                        "state": "result_absent",
                        "padding": crate::internal::identity_wire::handle_recovery::HANDLE_RECOVERY_RESULT_ABSENT_PADDING,
                    },
                }),
            );
        });

        let progress = advance_v4(&core, operation_id).await.unwrap();
        server.join().unwrap();
        assert_eq!(progress.phase, HandleRecoveryPhase::RemoteOutcomeUnknown);
        assert_eq!(
            progress.failure_code,
            Some(HandleRecoveryErrorCode::FactorRetryRequired)
        );
        let (_, durable) = store.load_v4(operation_id).unwrap().unwrap();
        assert_eq!(
            durable.last_error_code.as_deref(),
            Some(HandleRecoveryErrorCode::FactorRetryRequired.as_str())
        );
        let index = crate::internal::identity_handle_recovery_operation::load(
            &core.inner().sdk_paths().local_state.sqlite_path,
            operation_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            index.last_error_code.as_deref(),
            Some(HandleRecoveryErrorCode::FactorRetryRequired.as_str())
        );
    }

    #[tokio::test]
    async fn discard_claims_sqlite_before_idempotent_vault_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [92_u8; 32]);
        let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();

        let operation_id = "recover-v4-discard-crash-cut";
        create_v4_awaiting_factor_operation(&core, operation_id, "owner-discard-cut");
        crate::internal::identity_handle_recovery_operation::discard_pre_attempt(
            sqlite_path,
            operation_id,
            "2026-08-07T00:00:01Z",
        )
        .unwrap();
        assert!(store.load_v4(operation_id).unwrap().is_some());

        let recovered = discard_pre_attempt(
            &core,
            HandleRecoveryDiscardRequest {
                operation_id: operation_id.to_owned(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            recovered.lifecycle_class,
            HandleRecoveryOperationLifecycle::DiscardedPreAttempt
        );
        assert_eq!(
            recovered.key_state,
            HandleRecoveryKeyState::DestroyedPreAttempt
        );
        assert!(store.load_v4(operation_id).unwrap().is_none());

        let attempted_id = "recover-v4-discard-commit-won";
        let (mut attempted, _) =
            create_v4_awaiting_factor_operation(&core, attempted_id, "owner-discard-attempted");
        make_v4_operation_remote_unresolved(&core, &mut attempted);
        let error = discard_pre_attempt(
            &core,
            HandleRecoveryDiscardRequest {
                operation_id: attempted_id.to_owned(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service { code: Some(code), .. }
                if code == HandleRecoveryErrorCode::OutcomeUnknown.as_str()
        ));
        assert!(store.load_v4(attempted_id).unwrap().is_some());
    }

    #[tokio::test]
    async fn changed_binding_after_absent_reconciles_delayed_commit_before_superseding() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), &endpoint, [93_u8; 32]);
        let operation_id = "recover-v4-delayed-commit-refresh";
        let mut pending = create_v4_operation_with_transition_identities(
            &core,
            operation_id,
            "owner-delayed-refresh",
        );
        make_v4_operation_remote_unresolved(&core, &mut pending);
        let result = remote_result_for_pending(&pending, "user-refresh-1");
        let changed_did = result.current_did.clone();
        let response_result = serde_json::to_value(&result).unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();

            let (mut first, _) = listener.accept().unwrap();
            requests.push(read_http_request(&mut first));
            write_json_response(
                &mut first,
                &json!({
                    "jsonrpc": "2.0",
                    "id": "req-1",
                    "result": {
                        "state": "result_absent",
                        "padding": crate::internal::identity_wire::handle_recovery::HANDLE_RECOVERY_RESULT_ABSENT_PADDING,
                    },
                }),
            );

            let (mut exchange, _) = listener.accept().unwrap();
            requests.push(read_http_request(&mut exchange));
            write_json_response(
                &mut exchange,
                &json!({
                    "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                    "recovery_grant": "fresh-delayed-commit-grant",
                    "purpose": "awiki.identity.handle-recovery.v1",
                    "expires_at": "2099-08-07T00:05:00Z",
                    "current_binding": {
                        "account_user_id": "user-refresh-1",
                        "full_handle": "alice.awiki.test",
                        "current_did": changed_did,
                        "binding_generation": "8",
                    },
                }),
            );

            let (mut second, _) = listener.accept().unwrap();
            requests.push(read_http_request(&mut second));
            write_json_response(
                &mut second,
                &json!({
                    "jsonrpc": "2.0",
                    "id": "req-1",
                    "result": {"state": "committed", "result": response_result},
                }),
            );
            requests
        });

        let progress = prepare(
            &core,
            HandleRecoveryPrepareRequest {
                operation_id: operation_id.to_owned(),
                phone: "+15555550100".to_owned(),
                code: "123456".to_owned(),
            },
        )
        .await
        .unwrap();
        assert_eq!(progress.phase, HandleRecoveryPhase::RemoteCommitted);
        let operation = crate::internal::identity_handle_recovery_operation::load(
            &core.inner().sdk_paths().local_state.sqlite_path,
            operation_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            operation.lifecycle_class,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].contains("handle_recovery_result_get_v4"));
        assert!(requests[1].contains("/user-service/v1/auth/handle-recovery/v4/exchange"));
        assert!(requests[2].contains("handle_recovery_result_get_v4"));
    }

    #[test]
    fn v4_factor_resend_reuses_preattempt_and_postattempt_operation_without_rebinding_intent() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [91_u8; 32]);
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();

        let pre_id = "recover-v4-factor-resend-pre";
        let (_, _) = create_v4_awaiting_factor_operation(&core, pre_id, "owner-factor-pre");
        assert_eq!(
            reusable_awaiting_factor_operation(
                &core,
                &store,
                "owner-factor-pre",
                "alice.awiki.test",
                "did:wba:awiki.test:users:alice-old",
                false,
            )
            .unwrap()
            .as_deref(),
            Some(pre_id)
        );

        let post_id = "recover-v4-factor-resend-post";
        let (mut pending, _) =
            create_v4_awaiting_factor_operation(&core, post_id, "owner-factor-post");
        make_v4_operation_remote_unresolved(&core, &mut pending);
        assert_eq!(
            reusable_awaiting_factor_operation(
                &core,
                &store,
                "owner-factor-post",
                "alice.awiki.test",
                "did:wba:awiki.test:users:alice-old",
                false,
            )
            .unwrap()
            .as_deref(),
            Some(post_id)
        );

        let frozen_hash = pending.intent_hash.clone();
        let binding = pending.authoritative_binding.clone().unwrap();
        pending
            .refresh_grant(
                &binding,
                "grant-refresh-2".to_owned(),
                "2099-08-07T00:10:00Z".to_owned(),
            )
            .unwrap();
        assert_eq!(pending.intent_hash, frozen_hash);
        let mut changed = binding;
        changed.binding_generation = "8".to_owned();
        assert!(pending
            .refresh_grant(
                &changed,
                "grant-must-not-stick".to_owned(),
                "2099-08-07T00:15:00Z".to_owned(),
            )
            .is_err());
        assert_eq!(pending.intent_hash, frozen_hash);
        assert_eq!(
            pending.recovery_grant().unwrap().expose_secret(),
            b"grant-refresh-2"
        );
        mark_post_attempt_state_changed(&core, &pending).unwrap();
        let superseded = crate::internal::identity_handle_recovery_operation::load(
            &core.inner().sdk_paths().local_state.sqlite_path,
            post_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            superseded.lifecycle_class,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::SupersededByStateChange
        );
        assert_eq!(
            superseded.last_error_code.as_deref(),
            Some("handle_recovery.state_changed_requires_new_operation")
        );
    }

    #[test]
    fn local_migration_mode_requires_explicit_break_glass_for_stale_epoch() {
        let pending =
            v4_awaiting_factor_pending("recover-v4-migration-mode", "owner-migration-mode");
        assert_eq!(
            local_migration_mode_v4(
                &pending,
                "user-1",
                &pending.full_handle,
                &pending.local_previous_did,
                "user-1",
                &pending.full_handle,
                &pending.local_previous_did,
                false,
            ),
            Some(LocalMigrationModeV4::Direct)
        );
        let authoritative = "did:wba:awiki.test:users:already-recovered";
        assert_eq!(
            local_migration_mode_v4(
                &pending,
                "user-1",
                &pending.full_handle,
                authoritative,
                "user-1",
                &pending.full_handle,
                &pending.local_previous_did,
                false,
            ),
            None
        );
        assert_eq!(
            local_migration_mode_v4(
                &pending,
                "user-1",
                &pending.full_handle,
                authoritative,
                "user-1",
                &pending.full_handle,
                &pending.local_previous_did,
                true,
            ),
            Some(LocalMigrationModeV4::ConfirmedFreshBreakGlass)
        );
        assert_eq!(
            local_migration_mode_v4(
                &pending,
                "another-user",
                &pending.full_handle,
                authoritative,
                "user-1",
                &pending.full_handle,
                &pending.local_previous_did,
                true,
            ),
            None
        );
    }

    #[test]
    fn confirmed_break_glass_keeps_local_d0_separate_from_remote_d1_and_applies_d2() {
        let mut pending = v4_awaiting_factor_pending("recover-v4-d0-d1-d2", "owner-d0-d1-d2");
        let local_d0 = pending.local_previous_did.clone();
        let remote_d1 = "did:wba:awiki.test:users:alice-remote-d1".to_owned();
        let committed_d2 = pending.identity.did.as_str().to_owned();
        pending
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-v4-1".to_owned(),
                    full_handle: pending.full_handle.clone(),
                    current_did: remote_d1.clone(),
                    binding_generation: "8".to_owned(),
                },
                "grant-v4-d0-d1-d2".to_owned(),
                "2099-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        pending
            .mark_commit_attempted("2026-08-07T00:01:00Z".to_owned())
            .unwrap();
        let result = crate::internal::identity_handle_recovery_pending::RecoveryRemoteResultV4 {
            state: "recovered".to_owned(),
            operation_id: pending.operation_id.clone(),
            intent_hash: pending.intent_hash.clone().unwrap(),
            intent_schema_version: "1".to_owned(),
            contract_version:
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION.to_owned(),
            account_user_id: "user-v4-1".to_owned(),
            full_handle: pending.full_handle.clone(),
            previous_did: remote_d1.clone(),
            current_did: committed_d2.clone(),
            binding_generation: "9".to_owned(),
            checkpoint: crate::internal::identity_handle_recovery_pending::RecoveryCheckpointV4 {
                document_version: 1,
                document_hash: crate::internal::identity_wire::document::document_hash(
                    &pending.identity.did_document,
                )
                .unwrap(),
                registry_version: 1,
            },
            bootstrap_device:
                crate::internal::identity_handle_recovery_pending::RecoveryBootstrapDeviceV4 {
                    device_id: pending.identity.protocol_device_id.as_str().to_owned(),
                    status: "active".to_owned(),
                    role: "admin".to_owned(),
                    management_ready: true,
                    auth_generation: 1,
                },
            committed_at: "2026-08-07T00:01:01Z".to_owned(),
        };
        pending.record_remote_result(result.clone()).unwrap();

        let directory = tempfile::tempdir().unwrap();
        let sqlite_path = directory.path().join("d0-d1-d2.sqlite");
        let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES (?1,?2,?3,?4,'device-d0','7','3',1,1)",
                rusqlite::params![
                    &pending.owner_identity_id,
                    "user-v4-1",
                    &pending.full_handle,
                    &local_d0,
                ],
            )
            .unwrap();
        drop(connection);

        let marker =
            crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator_v4(
                &sqlite_path,
                &pending,
                &result,
            )
            .unwrap();
        assert_eq!(result.previous_did, remote_d1);
        assert_eq!(marker.previous_did, local_d0);
        assert_eq!(marker.current_did, committed_d2);
        crate::internal::identity_transition_pending::persist(&sqlite_path, &marker).unwrap();
        crate::internal::identity_transition_pending::migrate_initiator_fresh_local_state(
            &sqlite_path,
            &marker,
            &result.bootstrap_device.device_id,
            result.bootstrap_device.auth_generation,
        )
        .unwrap();

        let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
        let applied: (String, String) = connection
            .query_row(
                "SELECT current_did,identity_generation FROM identity_account_bindings WHERE owner_identity_id=?1",
                [&pending.owner_identity_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(applied, (committed_d2, "9".to_owned()));
    }

    #[test]
    fn stale_authoritative_binding_requires_durable_quarantined_replacement_authority() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [92_u8; 32]);
        let (old, _) = create_v4_awaiting_factor_operation(
            &core,
            "recover-v4-key-unavailable-old",
            "owner-break-glass-authority",
        );
        let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
        crate::internal::identity_handle_recovery_operation::quarantine_key_unavailable(
            sqlite_path,
            &old.operation_id,
            "2026-08-07T00:01:00Z",
        )
        .unwrap();
        let (mut replacement, _) = create_v4_awaiting_factor_operation(
            &core,
            "recover-v4-key-unavailable-replacement",
            "owner-break-glass-authority",
        );
        replacement
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-v4-1".to_owned(),
                    full_handle: replacement.full_handle.clone(),
                    current_did: "did:wba:awiki.test:users:alice-remote-current".to_owned(),
                    binding_generation: "8".to_owned(),
                },
                "grant-v4-break-glass-authority".to_owned(),
                "2099-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();

        assert_eq!(
            require_v4_local_migration_authority(sqlite_path, &replacement).unwrap_err(),
            crate::ImError::PermissionDenied
        );
        assert!(
            crate::internal::identity_handle_recovery_operation::claim_quarantined_replacement(
                sqlite_path,
                &replacement.operation_id,
                &replacement.owner_identity_id,
                &replacement.full_handle,
                "2026-08-07T00:02:00Z",
            )
            .unwrap()
        );
        require_v4_local_migration_authority(sqlite_path, &replacement).unwrap();
        replacement
            .mark_commit_attempted("2026-08-07T00:03:00Z".to_owned())
            .unwrap();
        crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
            sqlite_path,
            &replacement.operation_id,
            "2026-08-07T00:03:00Z",
        )
        .unwrap();
        assert!(break_glass_authority_for_exchange(
            sqlite_path,
            &replacement,
            false,
            true,
            "2026-08-07T00:04:00Z",
        )
        .unwrap());
    }

    #[test]
    fn v4_closed_server_and_exchange_errors_have_explicit_runtime_projections() {
        use crate::internal::identity_wire::handle_recovery::{
            RecoveryExchangeErrorCodeV4 as Exchange, RecoveryServerErrorCodeV4 as Server,
        };
        let server = [
            (
                Server::InvalidRequest,
                ServerErrorProjectionV4::OutcomeUnknown,
            ),
            (
                Server::CapabilityDisabled,
                ServerErrorProjectionV4::OutcomeUnknown,
            ),
            (
                Server::GrantInvalid,
                ServerErrorProjectionV4::OutcomeUnknown,
            ),
            (
                Server::GrantExpired,
                ServerErrorProjectionV4::FactorRetryRequired,
            ),
            (
                Server::ProofInvalid,
                ServerErrorProjectionV4::OutcomeUnknown,
            ),
            (
                Server::IntentConflict,
                ServerErrorProjectionV4::FailedTerminal,
            ),
            (
                Server::StateChangedRequiresNewOperation,
                ServerErrorProjectionV4::SupersededByStateChange,
            ),
            (
                Server::TemporarilyUnavailable,
                ServerErrorProjectionV4::OutcomeUnknown,
            ),
        ];
        for (code, expected) in server {
            assert_eq!(server_error_projection(code), expected, "{}", code.as_str());
        }
        let exchange = [
            (
                Exchange::InvalidRequest,
                HandleRecoveryErrorCode::FactorRetryRequired,
            ),
            (
                Exchange::FactorInvalid,
                HandleRecoveryErrorCode::FactorRetryRequired,
            ),
            (
                Exchange::CapabilityDisabled,
                HandleRecoveryErrorCode::LocalMigrationUnsupported,
            ),
            (
                Exchange::RateLimited,
                HandleRecoveryErrorCode::FactorRetryRequired,
            ),
            (
                Exchange::TemporarilyUnavailable,
                HandleRecoveryErrorCode::OutcomeUnknown,
            ),
        ];
        for (code, expected) in exchange {
            assert_eq!(
                exchange_error_projection(code),
                expected,
                "{}",
                code.as_str()
            );
        }
    }

    #[test]
    fn v4_post_attempt_request_errors_remain_reconcilable_and_preserve_server_code() {
        use crate::internal::identity_wire::handle_recovery::RecoveryServerErrorCodeV4 as Server;

        for (suffix, code) in [
            ("invalid-request", Server::InvalidRequest),
            ("capability-disabled", Server::CapabilityDisabled),
            ("grant-invalid", Server::GrantInvalid),
            ("proof-invalid", Server::ProofInvalid),
        ] {
            let root = tempfile::tempdir().unwrap();
            let operation_id = format!("recover-v4-post-attempt-{suffix}");
            let core = recovery_test_core(
                root.path(),
                "https://example.invalid",
                [suffix.len() as u8; 32],
            );
            let (mut pending, _) = create_v4_awaiting_factor_operation(
                &core,
                &operation_id,
                &format!("owner-post-attempt-{suffix}"),
            );
            make_v4_operation_remote_unresolved(&core, &mut pending);
            let store = PendingHandleRecoveryStore::from_core(&core).unwrap();

            persist_nonterminal_server_error_v4(&core, &store, &mut pending, code).unwrap();

            let (_, durable_pending) = store.load_v4(&operation_id).unwrap().unwrap();
            let durable_index = crate::internal::identity_handle_recovery_operation::load(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &operation_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                durable_pending.phase,
                PendingRecoveryPhaseV4::RemoteOutcomeUnknown
            );
            assert_eq!(
                durable_pending.last_error_code.as_deref(),
                Some(code.as_str())
            );
            assert_eq!(
                durable_index.lifecycle_class,
                crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved
            );
            assert_eq!(
                durable_index.last_error_code.as_deref(),
                Some(code.as_str())
            );
        }
    }

    #[test]
    fn v4_retryable_outcomes_survive_core_restart_in_vault_and_index() {
        for (suffix, code) in [
            ("factor", HandleRecoveryErrorCode::FactorRetryRequired),
            ("absent", HandleRecoveryErrorCode::ResultAbsent),
            ("unknown", HandleRecoveryErrorCode::OutcomeUnknown),
        ] {
            let root = tempfile::tempdir().unwrap();
            let key = [92_u8; 32];
            let operation_id = format!("recover-v4-restart-{suffix}");
            let core = recovery_test_core(root.path(), "https://example.invalid", key);
            let (mut pending, _) = create_v4_awaiting_factor_operation(
                &core,
                &operation_id,
                &format!("owner-restart-{suffix}"),
            );
            make_v4_operation_remote_unresolved(&core, &mut pending);
            let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
            if code == HandleRecoveryErrorCode::ResultAbsent {
                let revision = pending.revision;
                pending
                    .record_result_get(
                        "2026-08-07T00:01:05Z".to_owned(),
                        Some("2026-08-07T00:01:15Z".to_owned()),
                        Some(code.as_str().to_owned()),
                    )
                    .unwrap();
                store.save_v4_cas(&pending, revision).unwrap();
                crate::internal::identity_handle_recovery_operation::record_nonterminal_error(
                    &core.inner().sdk_paths().local_state.sqlite_path,
                    &operation_id,
                    Some(code.as_str()),
                    "2026-08-07T00:01:05Z",
                )
                .unwrap();
            } else {
                persist_nonterminal_error_v4(&core, &store, &mut pending, code).unwrap();
            }
            drop(core);

            let reopened = recovery_test_core(root.path(), "https://example.invalid", key);
            let reopened_store = PendingHandleRecoveryStore::from_core(&reopened).unwrap();
            let (_, durable_pending) = reopened_store.load_v4(&operation_id).unwrap().unwrap();
            let durable_index = crate::internal::identity_handle_recovery_operation::load(
                &reopened.inner().sdk_paths().local_state.sqlite_path,
                &operation_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                durable_pending.last_error_code.as_deref(),
                Some(code.as_str())
            );
            assert_eq!(
                durable_index.last_error_code.as_deref(),
                Some(code.as_str())
            );
            assert_eq!(
                durable_index.lifecycle_class,
                crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved
            );
        }
    }

    #[test]
    fn vault_only_awaiting_factor_is_reindexed_and_multiple_orphans_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [83_u8; 32]);
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        let operation_id = "recover-v4-vault-only-001";
        let owner = "owner-vault-only-1";
        store
            .create_v4(&v4_awaiting_factor_pending(operation_id, owner))
            .unwrap();
        reconcile_vault_only_awaiting_factor_operation(
            &core,
            &store,
            owner,
            "alice.awiki.test",
            "did:wba:awiki.test:users:alice-old",
            false,
        )
        .unwrap();
        let rebuilt = crate::internal::identity_handle_recovery_operation::load(
            &core.inner().sdk_paths().local_state.sqlite_path,
            operation_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            rebuilt.lifecycle_class,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
        );
        assert!(!rebuilt.commit_attempted);
        assert_eq!(
            reusable_awaiting_factor_operation(
                &core,
                &store,
                owner,
                "alice.awiki.test",
                "did:wba:awiki.test:users:alice-old",
                false,
            )
            .unwrap()
            .as_deref(),
            Some(operation_id)
        );

        let conflict_root = tempfile::tempdir().unwrap();
        let conflict_core =
            recovery_test_core(conflict_root.path(), "https://example.invalid", [84_u8; 32]);
        let conflict_store = PendingHandleRecoveryStore::from_core(&conflict_core).unwrap();
        let conflict_owner = "owner-vault-conflict-1";
        for operation in ["recover-v4-orphan-alpha", "recover-v4-orphan-bravo"] {
            conflict_store
                .create_v4(&v4_awaiting_factor_pending(operation, conflict_owner))
                .unwrap();
        }
        let error = reconcile_vault_only_awaiting_factor_operation(
            &conflict_core,
            &conflict_store,
            conflict_owner,
            "alice.awiki.test",
            "did:wba:awiki.test:users:alice-old",
            false,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service { code: Some(code), .. }
                if code == HandleRecoveryErrorCode::UnknownEpoch.as_str()
        ));
        assert!(
            crate::internal::identity_handle_recovery_operation::list_owner(
                &conflict_core.inner().sdk_paths().local_state.sqlite_path,
                conflict_owner,
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn frozen_vault_intent_self_heals_missing_sqlite_projection() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [85_u8; 32]);
        let operation_id = "recover-v4-frozen-half-write";
        let (mut pending, _) =
            create_v4_awaiting_factor_operation(&core, operation_id, "owner-frozen-1");
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        let revision = pending.revision;
        pending
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-frozen-1".to_owned(),
                    full_handle: pending.full_handle.clone(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "grant-frozen-1".to_owned(),
                "2099-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        store.save_v4_cas(&pending, revision).unwrap();
        let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
        let before =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)
                .unwrap()
                .unwrap();
        assert!(before.account_user_id.is_none());
        assert!(before.intent_hash.is_none());
        reconcile_frozen_intent_index(sqlite_path, &before, &pending, "2026-08-07T00:02:00Z")
            .unwrap();
        let healed =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)
                .unwrap()
                .unwrap();
        assert_eq!(healed.account_user_id.as_deref(), Some("user-frozen-1"));
        assert_eq!(healed.intent_hash, pending.intent_hash);

        let mut conflicted = healed;
        conflicted.intent_hash =
            Some("sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned());
        assert_eq!(
            reconcile_frozen_intent_index(
                sqlite_path,
                &conflicted,
                &pending,
                "2026-08-07T00:03:00Z",
            )
            .unwrap_err(),
            crate::ImError::PermissionDenied
        );
    }

    #[test]
    fn lifecycle_half_writes_self_heal_only_from_exact_vault_and_receipt_state() {
        use crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass as Lifecycle;

        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [86_u8; 32]);
        let operation_id = "recover-v4-lifecycle-half-write";
        let (mut pending, _) =
            create_v4_awaiting_factor_operation(&core, operation_id, "owner-lifecycle-1");
        let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
        let result = freeze_and_commit_v4_pending(&mut pending);
        crate::internal::identity_handle_recovery_operation::record_frozen_intent(
            sqlite_path,
            operation_id,
            "user-v4-1",
            pending.intent_hash.as_deref().unwrap(),
            "2026-08-07T00:00:30Z",
        )
        .unwrap();
        crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
            sqlite_path,
            operation_id,
            "2026-08-07T00:01:00Z",
        )
        .unwrap();
        let unresolved =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)
                .unwrap()
                .unwrap();
        assert_eq!(unresolved.lifecycle_class, Lifecycle::RemoteUnresolved);
        reconcile_v4_lifecycle_index(sqlite_path, &unresolved, &pending, "2026-08-07T00:01:02Z")
            .unwrap();
        let remote_committed =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)
                .unwrap()
                .unwrap();
        assert_eq!(remote_committed.lifecycle_class, Lifecycle::RemoteCommitted);

        let marker =
            crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator_v4(
                sqlite_path,
                &pending,
                &result,
            )
            .unwrap();
        crate::internal::identity_transition_pending::persist(sqlite_path, &marker).unwrap();
        crate::internal::identity_handle_recovery_operation::update_lifecycle(
            sqlite_path,
            operation_id,
            Lifecycle::RemoteCommitted,
            Lifecycle::LocalTransitionPending,
            Some(&marker.state_root_fingerprint),
            None,
            "2026-08-07T00:01:03Z",
        )
        .unwrap();
        crate::internal::identity_transition_pending::mark_applied(
            sqlite_path,
            operation_id,
            crate::internal::identity_transition_pending::TransitionPhase::Pending,
            &result.bootstrap_device.device_id,
            "1",
            "1",
            "{}",
        )
        .unwrap();
        pending.mark_local_transition_pending().unwrap();
        pending.mark_applied().unwrap();
        let local_pending =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)
                .unwrap()
                .unwrap();
        reconcile_v4_lifecycle_index(
            sqlite_path,
            &local_pending,
            &pending,
            "2026-08-07T00:01:04Z",
        )
        .unwrap();
        let applied =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)
                .unwrap()
                .unwrap();
        assert_eq!(applied.lifecycle_class, Lifecycle::Applied);
        assert_eq!(
            applied.state_root_fingerprint,
            Some(marker.state_root_fingerprint)
        );
    }

    #[test]
    fn commit_attempted_authorities_merge_monotonically_in_both_directions() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [87_u8; 32]);
        let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();

        let vault_true_id = "recover-v4-vault-attempted";
        let (mut vault_true, _) =
            create_v4_awaiting_factor_operation(&core, vault_true_id, "owner-vault-attempted");
        vault_true
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-vault-attempted".to_owned(),
                    full_handle: vault_true.full_handle.clone(),
                    current_did: vault_true.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "grant-vault-attempted".to_owned(),
                "2099-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        crate::internal::identity_handle_recovery_operation::record_frozen_intent(
            sqlite_path,
            vault_true_id,
            "user-vault-attempted",
            vault_true.intent_hash.as_deref().unwrap(),
            "2026-08-07T00:00:10Z",
        )
        .unwrap();
        vault_true
            .mark_commit_attempted("2026-08-07T00:00:11Z".to_owned())
            .unwrap();
        let index_false =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, vault_true_id)
                .unwrap()
                .unwrap();
        merge_commit_attempted_authorities(
            sqlite_path,
            &store,
            &index_false,
            &mut vault_true,
            "2026-08-07T00:00:12Z",
        )
        .unwrap();
        assert!(
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, vault_true_id,)
                .unwrap()
                .unwrap()
                .commit_attempted
        );

        let index_true_id = "recover-v4-index-attempted";
        let (mut index_true_pending, _) =
            create_v4_awaiting_factor_operation(&core, index_true_id, "owner-index-attempted");
        index_true_pending
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "user-index-attempted".to_owned(),
                    full_handle: index_true_pending.full_handle.clone(),
                    current_did: index_true_pending.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "grant-index-attempted".to_owned(),
                "2099-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        store.save_v4_cas(&index_true_pending, 1).unwrap();
        crate::internal::identity_handle_recovery_operation::record_frozen_intent(
            sqlite_path,
            index_true_id,
            "user-index-attempted",
            index_true_pending.intent_hash.as_deref().unwrap(),
            "2026-08-07T00:00:20Z",
        )
        .unwrap();
        crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
            sqlite_path,
            index_true_id,
            "2026-08-07T00:00:21Z",
        )
        .unwrap();
        let index_true =
            crate::internal::identity_handle_recovery_operation::load(sqlite_path, index_true_id)
                .unwrap()
                .unwrap();
        merge_commit_attempted_authorities(
            sqlite_path,
            &store,
            &index_true,
            &mut index_true_pending,
            "2026-08-07T00:00:22Z",
        )
        .unwrap();
        assert!(index_true_pending.commit_attempted);
        assert!(
            store
                .load_v4(index_true_id)
                .unwrap()
                .unwrap()
                .1
                .commit_attempted
        );
    }

    #[test]
    fn awaiting_factor_otp_resend_reuses_operation_after_restart() {
        let root = tempfile::tempdir().unwrap();
        let endpoint = "https://example.invalid";
        let vault_key = [81_u8; 32];
        let operation_id = "recover-v4-resend-001";
        let owner_identity_id = "owner-resend-1";
        let core = recovery_test_core(root.path(), endpoint, vault_key);
        create_v4_awaiting_factor_operation(&core, operation_id, owner_identity_id);
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        assert_eq!(
            reusable_awaiting_factor_operation(
                &core,
                &store,
                owner_identity_id,
                "alice.awiki.test",
                "did:wba:awiki.test:users:alice-old",
                false,
            )
            .unwrap()
            .as_deref(),
            Some(operation_id)
        );
        drop(store);
        drop(core);

        let reopened = recovery_test_core(root.path(), endpoint, vault_key);
        let store = PendingHandleRecoveryStore::from_core(&reopened).unwrap();
        assert_eq!(
            reusable_awaiting_factor_operation(
                &reopened,
                &store,
                owner_identity_id,
                "alice.awiki.test",
                "did:wba:awiki.test:users:alice-old",
                false,
            )
            .unwrap()
            .as_deref(),
            Some(operation_id)
        );
        assert_eq!(
            crate::internal::identity_handle_recovery_operation::list_owner(
                &reopened.inner().sdk_paths().local_state.sqlite_path,
                owner_identity_id,
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn recovery_losing_deletion_race_discards_unpublished_pre_attempt_material() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [91_u8; 32]);
        let operation_id = "recover-v4-delete-race-001";
        let owner_identity_id = "owner-delete-race-1";
        let mut pending = v4_awaiting_factor_pending(operation_id, owner_identity_id);
        pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
            &core,
            "awiki.test",
            "alice",
        )
        .unwrap();
        pending.validate().unwrap();
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        store.create_v4(&pending).unwrap();

        crate::internal::identity_local_deletion::prepare_with_id(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &crate::internal::identity_local_deletion::LocalIdentityDeletionSnapshot {
                owner_identity_id: owner_identity_id.to_owned(),
                current_did: pending.local_previous_did.clone(),
                full_handle: Some(pending.full_handle.clone()),
                local_alias: pending.local_alias.clone(),
                identity_dir_name: None,
                next_default_alias: None,
                protocol_device_id: None,
            },
            crate::internal::identity_local_deletion::LocalIdentityDeletionMode::FullDataApp,
            "delete-race-001",
            "2026-08-29T12:00:00Z",
        )
        .unwrap();
        let operation = crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            operation_id.to_owned(),
            owner_identity_id.to_owned(),
            pending.full_handle.clone(),
            crate::internal::identity_handle_recovery_pending::pending_v4_key_id(operation_id),
            "2026-08-29T12:00:01Z".to_owned(),
        )
        .unwrap();

        let error = insert_precommit_operation_or_cleanup(&core, &store, &pending, &operation)
            .await
            .unwrap_err();
        assert!(is_local_deletion_conflict(&error));
        assert!(store.load_v4(operation_id).unwrap().is_none());
        assert!(crate::internal::identity_handle_recovery_operation::load(
            &core.inner().sdk_paths().local_state.sqlite_path,
            operation_id,
        )
        .unwrap()
        .is_none());
        assert!(
            crate::internal::identity_custody::controller_custody_provider(&core)
                .await
                .unwrap()
                .list_identities()
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn quarantine_requires_genuine_key_unavailability_and_preserves_attempt_audit() {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [82_u8; 32]);
        let readable_id = "recover-v4-readable-001";
        create_v4_awaiting_factor_operation(&core, readable_id, "owner-readable-1");
        let readable_error = quarantine_key_unavailable(
            &core,
            HandleRecoveryQuarantineRequest {
                operation_id: readable_id.to_owned(),
                user_presence_confirmed: true,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            readable_error,
            crate::ImError::Service { code: Some(code), .. }
                if code == HandleRecoveryErrorCode::UnknownEpoch.as_str()
        ));

        let missing_id = "recover-v4-missing-001";
        let (_, missing_ref) =
            create_v4_awaiting_factor_operation(&core, missing_id, "owner-missing-1");
        core.inner()
            .identity_vault()
            .unwrap()
            .vault()
            .delete(&missing_ref)
            .unwrap();
        let missing = quarantine_key_unavailable(
            &core,
            HandleRecoveryQuarantineRequest {
                operation_id: missing_id.to_owned(),
                user_presence_confirmed: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            missing.lifecycle_class,
            HandleRecoveryOperationLifecycle::QuarantinedKeyUnavailable
        );
        assert_eq!(
            missing.key_state,
            HandleRecoveryKeyState::PermanentlyUnavailable
        );
        assert!(!missing.commit_attempted);

        let attempted_id = "recover-v4-attempted-001";
        let (mut attempted_pending, attempted_ref) =
            create_v4_awaiting_factor_operation(&core, attempted_id, "owner-attempted-1");
        make_v4_operation_remote_unresolved(&core, &mut attempted_pending);
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        let current_ref = store.load_v4(attempted_id).unwrap().unwrap().0;
        assert_eq!(current_ref.key_id, attempted_ref.key_id);
        core.inner()
            .identity_vault()
            .unwrap()
            .vault()
            .delete(&current_ref)
            .unwrap();
        let attempted = quarantine_key_unavailable(
            &core,
            HandleRecoveryQuarantineRequest {
                operation_id: attempted_id.to_owned(),
                user_presence_confirmed: true,
            },
        )
        .await
        .unwrap();
        assert!(attempted.commit_attempted);
        assert_eq!(
            attempted.lifecycle_class,
            HandleRecoveryOperationLifecycle::QuarantinedKeyUnavailable
        );
        assert_eq!(
            attempted.key_state,
            HandleRecoveryKeyState::PermanentlyUnavailable
        );
        assert!(store.load_v4(attempted_id).unwrap().is_none());
    }

    #[test]
    fn handle_recovery_requires_explicit_identity() {
        assert!(require_explicit_identity(&crate::identity::IdentitySelector::Default).is_err());
        assert!(
            require_explicit_identity(&crate::identity::IdentitySelector::Id(
                crate::ids::IdentityId::parse("owner-1").unwrap(),
            ))
            .is_ok()
        );
    }

    #[tokio::test]
    async fn uninstalled_recovery_owner_can_list_its_local_operation() {
        let temporary = tempfile::tempdir().unwrap();
        let core = recovery_test_core(temporary.path(), "https://example.invalid", [71_u8; 32]);
        let owner_identity_id = "fresh-recovery-owner";
        create_v4_awaiting_factor_operation(&core, "recover-v4-list", owner_identity_id);

        let operations = list_operations(
            &core,
            crate::identity::IdentitySelector::Id(
                crate::ids::IdentityId::parse(owner_identity_id).unwrap(),
            ),
        )
        .await
        .unwrap();

        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].operation_id, "recover-v4-list");
        assert_eq!(operations[0].owner_identity_id.as_str(), owner_identity_id);
    }

    #[test]
    fn no_marker_closed_cross_generation_uses_closed_v4_errors() {
        assert_eq!(
            authorized_join_transition_error_code(true, false, false),
            HandleRecoveryErrorCode::LocalMigrationUnsupported
        );
        assert_eq!(
            authorized_join_transition_error_code(false, false, false),
            HandleRecoveryErrorCode::UnknownEpoch
        );
    }

    #[test]
    fn post_commit_runtime_failures_require_exact_local_transition_resume() {
        for error in [
            crate::ImError::TransportUnavailable {
                detail: "offline".to_owned(),
            },
            crate::ImError::AuthRequired,
            crate::ImError::SessionExpired,
            crate::ImError::Service {
                status_code: Some(503),
                code: Some("prekey_publish_unavailable".to_owned()),
                message: "unavailable".to_owned(),
                data: None,
            },
        ] {
            assert_eq!(
                local_transition_retry_code(&error),
                Some(HandleRecoveryErrorCode::LocalTransitionPending),
            );
        }
        assert_eq!(
            local_transition_retry_code(&crate::ImError::PermissionDenied),
            None,
        );

        let mut pending = v4_awaiting_factor_pending(
            "recover-v4-local-transition-retry",
            "owner-local-transition-retry",
        );
        freeze_and_commit_v4_pending(&mut pending);
        pending.mark_local_transition_pending().unwrap();
        pending
            .record_retryable_error(
                HandleRecoveryErrorCode::LocalTransitionPending
                    .as_str()
                    .to_owned(),
            )
            .unwrap();
        pending.mark_applied().unwrap();
        assert_eq!(pending.last_error_code, None);
        assert_eq!(pending.retry_metadata.last_retryable_code, None);
    }

    #[test]
    fn retired_registration_join_revalidation_rejects_changed_evidence() {
        use sha2::{Digest as _, Sha256};

        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "https://example.invalid", [71_u8; 32]);
        let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
        let connection = crate::internal::local_state::open_writable(sqlite_path).unwrap();
        let current_did = "did:wba:example.invalid:user:alice:e1_new";
        connection
            .execute(
                "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('owner-alice','account-alice','alice.example.invalid',?1,'dev-retired','8','3',1,1)",
                [current_did],
            )
            .unwrap();
        let retirement_dir = core
            .inner()
            .sdk_paths()
            .identities
            .identity_root_dir
            .join(".identity-retirements");
        std::fs::create_dir_all(&retirement_dir).unwrap();
        let retirement_path = retirement_dir.join(format!(
            "{}.json",
            URL_SAFE_NO_PAD.encode(Sha256::digest(b"owner-alice"))
        ));
        let write_marker = |device_id: &str| {
            std::fs::write(
                &retirement_path,
                serde_json::to_vec(&json!({
                    "schema_version": 1,
                    "identity_id": "owner-alice",
                    "did": current_did,
                    "local_alias": "alice",
                    "identity_dir_name": "owner-alice",
                    "protocol_device_id": device_id,
                    "phase": "completed"
                }))
                .unwrap(),
            )
            .unwrap();
        };
        write_marker("dev-retired");
        let evidence = crate::internal::identity_local_owner_matcher::RetiredOwnerEvidence {
            owner_identity_id: "owner-alice".to_owned(),
            retired_did: current_did.to_owned(),
            retired_protocol_device_id: "dev-retired".to_owned(),
            retired_binding_generation: "8".to_owned(),
            epoch_relation:
                crate::internal::identity_local_owner_matcher::RetiredOwnerEpochRelation::Current,
        };
        let snapshot = crate::internal::identity_registration_join_preparation::RegistrationJoinPreparationSnapshot {
            expected_did: crate::ids::Did::parse(current_did).unwrap(),
            full_handle: crate::ids::Handle::parse("alice.example.invalid", "").unwrap(),
            account_verification_token: b"token".to_vec(),
            transition: Some(crate::internal::identity_registration_join_preparation::RegistrationJoinTransition {
                account_user_id: "account-alice".to_owned(),
                previous_did: "did:wba:example.invalid:user:alice:e1_old".to_owned(),
                current_did: current_did.to_owned(),
                binding_generation: "8".to_owned(),
            }),
            mode: crate::identity::HandleRegistrationJoinMode::Ordinary,
            owner_identity_id: Some("owner-alice".to_owned()),
            retired_owner_evidence: Some(evidence),
            resume_join_session_id: None,
            pending_registration_cleanup: None,
            state_root_fingerprint: "sha256:state".to_owned(),
            identity_index_fingerprint: "sha256:index".to_owned(),
            join_session_id: None,
            remote_started: false,
        };
        let index = crate::internal::identity_store::IndexPayload::default();
        assert_eq!(
            revalidate_prepared_registration_owner(&core, &snapshot, &index).unwrap(),
            Some("owner-alice".to_owned())
        );

        write_marker("dev-changed");
        assert!(matches!(
            revalidate_prepared_registration_owner(&core, &snapshot, &index),
            Err(crate::ImError::Service { code: Some(code), .. })
                if code == "handle_recovery.local_state_conflict"
        ));
    }

    #[test]
    fn otp_send_boundary_requires_structured_server_retry_metadata() {
        let parsed = parse_otp_send_boundary(&json!({
            "ok": true,
            "retry_after_seconds": 60,
            "retry_at": "2026-08-06T12:00:00Z"
        }))
        .unwrap();
        assert_eq!(parsed, (true, 60, "2026-08-06T12:00:00Z".to_owned()));
        for invalid in [
            json!({"ok": true, "retry_after_seconds": 0, "retry_at": "2026-08-06T12:00:00Z"}),
            json!({"ok": true, "retry_after_seconds": 60, "retry_at": "2026-08-06T12:00:00+00:00"}),
            json!({"ok": false, "retry_after_seconds": 60, "retry_at": "2026-08-06T12:00:00Z"}),
        ] {
            assert!(parse_otp_send_boundary(&invalid).is_err());
        }
    }
}
