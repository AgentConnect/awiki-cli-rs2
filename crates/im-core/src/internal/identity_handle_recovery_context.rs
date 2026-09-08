//! Target-scoped recovery inspection and the shared operation action policy.

use crate::identity::{
    HandleRecoveryAction as Action, HandleRecoveryContext, HandleRecoveryContextRequest,
    HandleRecoveryErrorCode as Code,
};
use crate::internal::identity_handle_recovery_operation::{
    self as operations, RecoveryKeyState, RecoveryLifecycleClass as Lifecycle,
    RecoveryOperationRecord,
};
use crate::internal::identity_handle_recovery_pending::{
    PendingHandleRecoveryStore, PendingHandleRecoveryV4, PendingRecoveryPhaseV4 as Phase,
};
use crate::internal::identity_handle_recovery_runtime as runtime;

pub(crate) fn is_actionable(lifecycle: Lifecycle) -> bool {
    matches!(
        lifecycle,
        Lifecycle::PreCommit
            | Lifecycle::RemoteUnresolved
            | Lifecycle::RemoteCommitted
            | Lifecycle::LocalTransitionPending
    )
}

pub(crate) fn operation_actions(
    record: &RecoveryOperationRecord,
    pending: &PendingHandleRecoveryV4,
    now: time::OffsetDateTime,
) -> crate::ImResult<Vec<Action>> {
    if record.operation_id != pending.operation_id
        || record.owner_identity_id != pending.owner_identity_id
        || record.full_handle != pending.full_handle
    {
        return Err(error(Code::UnknownEpoch));
    }
    if record.key_state != RecoveryKeyState::Available {
        return Ok(
            if record.key_state == RecoveryKeyState::PermanentlyUnavailable
                && is_actionable(record.lifecycle_class)
            {
                vec![Action::QuarantineKeyUnavailable]
            } else {
                vec![]
            },
        );
    }
    if !is_actionable(record.lifecycle_class) {
        return Ok(vec![]);
    }
    let attempted = record.commit_attempted || pending.commit_attempted;
    if attempted {
        // An observed attempt is never converted back into a first submission.
        let mut actions = vec![Action::Resume];
        if record.lifecycle_class == Lifecycle::RemoteUnresolved
            && pending.phase == Phase::RemoteOutcomeUnknown
        {
            actions.extend([Action::RequestOtp, Action::Prepare]);
        }
        return Ok(actions);
    }
    if record.lifecycle_class != Lifecycle::PreCommit {
        return Err(error(Code::UnknownEpoch));
    }
    if matches!(
        pending.last_error_code.as_deref(),
        Some("state_changed_requires_new_operation" | "local_migration_unsupported")
    ) {
        return Ok(vec![Action::DiscardPreAttempt]);
    }
    Ok(match pending.phase {
        Phase::AwaitingFactor => vec![
            Action::RequestOtp,
            Action::Prepare,
            Action::DiscardPreAttempt,
        ],
        Phase::ReadyToCommit
            if grant_fresh(pending, now)?
                && pending.last_error_code.as_deref()
                    != Some(Code::FactorRetryRequired.as_str()) =>
        {
            vec![Action::Activate, Action::DiscardPreAttempt]
        }
        Phase::ReadyToCommit => vec![
            Action::RequestOtp,
            Action::Prepare,
            Action::DiscardPreAttempt,
        ],
        _ => return Err(error(Code::UnknownEpoch)),
    })
}

pub(crate) fn grant_fresh(
    pending: &PendingHandleRecoveryV4,
    now: time::OffsetDateTime,
) -> crate::ImResult<bool> {
    pending
        .grant_expires_at
        .as_deref()
        .map(|value| {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .map(|expiry| expiry > now)
                .map_err(|_| error(Code::UnknownEpoch))
        })
        .transpose()
        .map(|value| value.unwrap_or(false))
}

pub(crate) fn error(code: Code) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.as_str().to_owned()),
        message: code.as_str().to_owned(),
        data: None,
    }
}

pub(crate) fn require_registration_admission(
    core: &crate::ImCore,
    full_handle: &str,
) -> crate::ImResult<()> {
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    if operations::list_handle(path, full_handle)?
        .iter()
        .any(|record| is_actionable(record.lifecycle_class))
    {
        return Err(error(Code::RecoveryInProgress));
    }
    // Also protect an existing pre-index crash record. It is already owned by
    // Recovery even though the non-secret SQLite projection is not present yet.
    if core.inner().identity_secret_storage_policy()
        == crate::core::IdentitySecretStoragePolicy::VaultRequired
    {
        let store = PendingHandleRecoveryStore::from_core(core)?;
        for (_, pending) in store.list_v4_for_handle(full_handle)? {
            if operations::load(path, &pending.operation_id)?.is_none() {
                return Err(error(Code::RecoveryInProgress));
            }
        }
    }
    Ok(())
}

pub(crate) async fn inspect(
    core: &crate::ImCore,
    request: HandleRecoveryContextRequest,
) -> crate::ImResult<HandleRecoveryContext> {
    if !core.inner().handle_recovery_enabled() {
        return Err(crate::ImError::unsupported("handle-recovery-v4"));
    }
    let handle =
        crate::internal::identity_wire::handle_recovery::canonical_handle(&request.full_handle)?;
    let lock = core
        .inner()
        .handle_recovery_lock(&format!("handle:{}", handle.full));
    let _guard = lock.lock().await;
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let matches = index
        .credentials
        .values()
        .filter(|entry| entry.full_handle == handle.full)
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(error(Code::UnknownEpoch));
    }
    let live = matches.first().copied();
    let mut explicit_owner = None;
    if let Some(selector) = request.identity {
        if matches!(selector, crate::identity::IdentitySelector::Default) {
            return Err(error(Code::UnknownEpoch));
        }
        let selected = core.identities().resolve_async(selector).await?;
        explicit_owner = Some(selected.id.as_str().to_owned());
        if live.is_none_or(|entry| entry.unique_id != selected.id.as_str()) {
            return Err(error(Code::UnknownEpoch));
        }
    }
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    let store = PendingHandleRecoveryStore::from_core(core)?;
    // A pre-OTP crash can leave the already-created encrypted operation without
    // its SQLite index. Reuse the existing narrow repair: never mint another ID.
    let vault_only = store
        .list_v4_for_handle(&handle.full)?
        .into_iter()
        .map(|(_, pending)| pending)
        .filter_map(
            |pending| match operations::load(path, &pending.operation_id) {
                Ok(None) => Some(Ok(pending)),
                Ok(Some(_)) => None,
                Err(error) => Some(Err(error)),
            },
        )
        .collect::<crate::ImResult<Vec<_>>>()?;
    if vault_only.len() > 1 {
        return Err(error(Code::UnknownEpoch));
    }
    if let Some(pending) = vault_only.first() {
        runtime::reconcile_vault_only_awaiting_factor_operation(
            core,
            &store,
            &pending.owner_identity_id,
            &handle.full,
            &pending.local_previous_did,
            pending.fresh_local_state,
        )?;
    }
    let mut records = operations::list_handle(path, &handle.full)?;
    records.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.operation_id.cmp(&a.operation_id))
    });
    let active = records
        .iter()
        .filter(|record| is_actionable(record.lifecycle_class))
        .collect::<Vec<_>>();
    if active.len() > 1 {
        return Err(error(Code::UnknownEpoch));
    }
    let mut current_applied = None;
    if active.is_empty() {
        if let Some(entry) = live {
            for record in &records {
                if record.lifecycle_class != Lifecycle::Applied
                    || record.owner_identity_id != entry.unique_id
                {
                    continue;
                }
                if let Some(marker) =
                    crate::internal::identity_transition_pending::load(path, &record.operation_id)?
                {
                    if marker.phase
                        == crate::internal::identity_transition_pending::TransitionPhase::Completed
                        && marker.current_did == entry.did
                        && entry.binding_generation.as_deref()
                            == Some(marker.binding_generation.as_str())
                    {
                        current_applied = Some(record);
                        break;
                    }
                }
            }
        }
    }
    let selected = active
        .first()
        .copied()
        .or(current_applied)
        .or_else(|| records.first())
        .cloned();
    let mut context = HandleRecoveryContext {
        full_handle: handle.full,
        local_identity_id: live
            .map(|entry| crate::ids::IdentityId::parse(&entry.unique_id))
            .transpose()?,
        operation: None,
        progress: None,
        allowed_actions: vec![],
        blocked_reason: None,
    };
    let Some(mut record) = selected else {
        context.allowed_actions.push(Action::StartNew);
        return Ok(context);
    };
    if is_actionable(record.lifecycle_class)
        && explicit_owner
            .as_deref()
            .is_some_and(|owner| owner != record.owner_identity_id)
    {
        return Err(error(Code::UnknownEpoch));
    }
    if is_actionable(record.lifecycle_class)
        || record.lifecycle_class == Lifecycle::Applied
        || record.last_error_code.as_deref() == Some(Code::LocalTransitionSuperseded.as_str())
    {
        match store.load_v4(&record.operation_id) {
            Ok(Some((_, pending))) => {
                runtime::reconcile_frozen_intent_index(
                    path,
                    &record,
                    &pending,
                    &runtime::now_second_z()?,
                )?;
                record = operations::load(path, &record.operation_id)?
                    .ok_or_else(|| error(Code::UnknownEpoch))?;
                // A durable committed/applied Vault fact must not be rendered
                // as unknown just because its SQLite projection was cut short.
                runtime::reconcile_v4_lifecycle_index(
                    path,
                    &record,
                    &pending,
                    &runtime::now_second_z()?,
                )?;
                record = operations::load(path, &record.operation_id)?
                    .ok_or_else(|| error(Code::UnknownEpoch))?;
                let mut progress = runtime::progress_v4(core, &pending)?;
                context.allowed_actions = progress.allowed_actions.clone();
                if record.lifecycle_class == Lifecycle::Applied
                    && live.is_some_and(|entry| {
                        entry.unique_id == record.owner_identity_id
                            && entry.did == pending.identity.did.as_str()
                    })
                {
                    if !context.allowed_actions.contains(&Action::ActivateIdentity) {
                        context.allowed_actions.push(Action::ActivateIdentity);
                    }
                    if !progress.allowed_actions.contains(&Action::ActivateIdentity) {
                        progress.allowed_actions.push(Action::ActivateIdentity);
                    }
                }
                context.blocked_reason = progress.failure_code;
                context.progress = Some(progress);
            }
            Ok(None) if !is_actionable(record.lifecycle_class) => {
                if current_applied.is_some() {
                    context.allowed_actions.push(Action::ActivateIdentity);
                }
                if record.last_error_code.as_deref()
                    == Some(Code::LocalTransitionSuperseded.as_str())
                {
                    context.blocked_reason = Some(Code::LocalTransitionSuperseded);
                }
            }
            Ok(None) => {
                context.blocked_reason = Some(Code::LocalKeyUnavailable);
                context
                    .allowed_actions
                    .push(Action::QuarantineKeyUnavailable);
            }
            Err(_) => {
                context.blocked_reason = Some(Code::LocalKeyUnavailable);
            }
        }
    }
    if !is_actionable(record.lifecycle_class) {
        context.allowed_actions.push(Action::StartNew);
    }
    context.operation = Some(runtime::operation_summary(record)?);
    Ok(context)
}
