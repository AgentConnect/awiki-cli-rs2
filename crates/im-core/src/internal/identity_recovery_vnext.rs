//! vNext Handle Recovery orchestration.
//!
//! This module is intentionally independent from legacy recovery. It never
//! merges owner-scoped local state and only saves a brand-new VNext identity
//! after the first-party Handle cutover has completed. Public WNS state binds
//! Handle/DID/generation; the verified internal account subject comes only
//! from the same-domain Recovery begin result.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::identity::{
    HandleRecoveryBeginRequest, HandleRecoveryCancelRequest, HandleRecoveryCancelResult,
    HandleRecoveryFinalizeRequest, HandleRecoveryFinalizeResult, HandleRecoveryPhase,
    HandleRecoveryProgress, HandleRecoverySide,
};
use crate::internal::identity_recovery_pending::{
    PendingRecoveryCancelRecord, PendingRecoveryCancelStore, PendingRecoveryRecord,
    PendingRecoveryStore,
};
use crate::internal::identity_wire::device_recovery::{
    RecoveryFinalizeParseOutcome, RecoveryFinalizeResult, RecoveryRemoteState,
};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport};

pub(crate) fn local_sessions(
    core: &crate::core::ImCore,
) -> crate::ImResult<Vec<HandleRecoveryProgress>> {
    let store = PendingRecoveryStore::from_core(core)?;
    let mut sessions = store
        .list()?
        .into_iter()
        .filter_map(|(_, record)| progress(&record).transpose())
        .collect::<crate::ImResult<Vec<_>>>()?;
    sessions.sort_by(|left, right| left.recovery_session_id.cmp(&right.recovery_session_id));
    Ok(sessions)
}

pub(crate) async fn begin(
    core: &crate::core::ImCore,
    request: HandleRecoveryBeginRequest,
) -> crate::ImResult<HandleRecoveryProgress> {
    let _guard = core.inner().handle_recovery_lock.lock().await;
    let store = PendingRecoveryStore::from_core(core)?;
    let handle = crate::ids::Handle::parse(
        request.handle.as_str(),
        core.inner().sdk_config().did_domain.as_str(),
    )?;
    let supplied_grant = request.account_verification_grant.into_secret();
    let supplied_grant = secret_utf8(&supplied_grant, "recovery_begin_grant")?;
    let existing = match store.load_by_handle(&handle)? {
        Some((secret_ref, existing)) if restartable_terminal(&existing) => {
            store.delete(&secret_ref)?;
            None
        }
        Some((_, existing)) => Some(existing),
        None => None,
    };
    let mut record = match existing {
        Some(existing) => existing,
        None => {
            if store
                .list()?
                .iter()
                .any(|(_, existing)| !restartable_terminal(existing))
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let binding = crate::internal::handle_discovery::resolve_recovery_handle_binding_async(
                core,
                handle.as_str(),
            )
            .await?;
            let pending = PendingRecoveryRecord::new(
                binding,
                random_id("recovery-begin")?,
                supplied_grant.as_str().to_owned(),
            )?;
            store.save(&pending)?;
            pending
        }
    };

    if record.session.is_none() {
        // The stable operation and Handle binding are the idempotent business
        // request. Account evidence is short-lived and may be refreshed after
        // a crash before the begin result was persisted.
        record.replace_begin_grant(Some(supplied_grant.as_str().to_owned()));
        store.save(&record)?;
        let grant = record
            .begin_grant
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let call = crate::internal::identity_wire::device_recovery::build_recovery_begin_call(
            &record.begin_operation_id,
            grant,
            record.binding.handle.as_str(),
        )?;
        let mut transport = crate::internal::transport::CorePlainTransport::new(core);
        let raw = transport
            .rpc(call.endpoint, call.method, call.params)
            .await?;
        let session = crate::internal::identity_wire::device_recovery::parse_recovery_begin_result(
            raw,
            &record.binding.did,
            OffsetDateTime::now_utc(),
        )?;
        record.session = Some(session);
        record.replace_begin_grant(None);
        store.save(&record)?;
    }
    progress(&record)?.ok_or(crate::ImError::PermissionDenied)
}

pub(crate) async fn status(
    core: &crate::core::ImCore,
    recovery_session_id: &str,
) -> crate::ImResult<HandleRecoveryProgress> {
    let _guard = core.inner().handle_recovery_lock.lock().await;
    let store = PendingRecoveryStore::from_core(core)?;
    let recovery_session_id = required(recovery_session_id, "recovery_session_id")?;
    let (_, mut record) = store
        .load_by_session(&recovery_session_id)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: recovery_session_id.to_owned(),
        })?;
    if record.remote_result.is_none() {
        refresh_status(core, &mut record).await?;
        store.save(&record)?;
    }
    progress(&record)?.ok_or(crate::ImError::PermissionDenied)
}

pub(crate) async fn cancel(
    core: &crate::core::ImCore,
    request: HandleRecoveryCancelRequest,
) -> crate::ImResult<HandleRecoveryCancelResult> {
    if !request.user_presence_confirmed {
        return Err(crate::ImError::PermissionDenied);
    }
    let _guard = core.inner().handle_recovery_lock.lock().await;
    let recovery_session_id = required(&request.recovery_session_id, "recovery_session_id")?;
    // Fail before identity resolution or network I/O unless exact-retry state
    // can be persisted in the required Vault.
    let store = PendingRecoveryCancelStore::from_core(core)?;
    let (client, device_id, signing_key_id) =
        crate::internal::identity_device_join::ready_admin_context(
            core,
            &request.old_identity,
            None,
        )?;
    let registry = {
        let call =
            crate::internal::identity_wire::device_join::build_registry_call(client.did(), false);
        let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
        let raw = transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_registry_result(
            raw,
            client.did(),
            false,
        )?
    };
    let mut resolver = crate::internal::transport::CoreHttpTransport::new_signature_only(&client);
    let did_document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await?;
    validate_current_ready_admin_binding(
        client.did(),
        &device_id,
        &signing_key_id,
        &registry,
        &did_document,
    )?;

    let (secret_ref, pending) = match store.load(&recovery_session_id, &device_id)? {
        Some((secret_ref, pending)) => {
            if pending.old_did != *client.did() || pending.signing_key_id != signing_key_id {
                return Err(crate::ImError::PermissionDenied);
            }
            (secret_ref, pending)
        }
        None => {
            let prepared = prepare_cancel_with_current_device_key(
                &client,
                random_id("recovery-cancel")?,
                &recovery_session_id,
                &device_id,
                &signing_key_id,
                &did_document,
            )?;
            let pending = PendingRecoveryCancelRecord::new(
                client.did().clone(),
                signing_key_id.clone(),
                prepared,
            )?;
            let secret_ref = store.save(&pending)?;
            (secret_ref, pending)
        }
    };
    // Proof timestamps/nonces are refreshable evidence. Re-sign the same
    // persisted operation on every retry so a restart after the proof TTL does
    // not strand an otherwise idempotent cancellation.
    let pending = PendingRecoveryCancelRecord::new(
        pending.old_did.clone(),
        pending.signing_key_id.clone(),
        prepare_cancel_with_current_device_key(
            &client,
            pending.prepared.operation_id.clone(),
            &pending.prepared.recovery_session_id,
            &pending.prepared.authorizing_device_id,
            &pending.signing_key_id,
            &did_document,
        )?,
    )?;
    if store.save(&pending)? != secret_ref {
        return Err(crate::ImError::PermissionDenied);
    }
    let call = crate::internal::identity_wire::device_recovery::build_recovery_cancel_call(
        &pending.prepared,
        &did_document,
    )?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
    let raw = transport
        .authenticated_rpc(call.endpoint, call.method, call.params)
        .await?;
    let result = crate::internal::identity_wire::device_recovery::parse_recovery_cancel_result(
        raw,
        &recovery_session_id,
    )?;
    store.delete(&secret_ref)?;
    Ok(HandleRecoveryCancelResult {
        recovery_session_id: result.recovery_session_id,
        phase: public_phase(result.state),
    })
}

pub(crate) async fn finalize(
    core: &crate::core::ImCore,
    request: HandleRecoveryFinalizeRequest,
) -> crate::ImResult<HandleRecoveryFinalizeResult> {
    if !request.user_presence_confirmed {
        return Err(crate::ImError::PermissionDenied);
    }
    let _guard = core.inner().handle_recovery_lock.lock().await;
    let store = PendingRecoveryStore::from_core(core)?;
    let recovery_session_id = required(&request.recovery_session_id, "recovery_session_id")?;
    let (_, mut record) = store
        .load_by_session(&recovery_session_id)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: request.recovery_session_id.clone(),
        })?;
    let supplied_reconfirmation = request.reconfirmation_grant.into_secret();
    let supplied_reconfirmation =
        secret_utf8(&supplied_reconfirmation, "recovery_reconfirmation_grant")?;

    if record.remote_result.is_none() && record.prepared_finalize.is_none() {
        refresh_status(core, &mut record).await?;
        if record.session.as_ref().map(|session| session.state) != Some(RecoveryRemoteState::Ready)
        {
            store.save(&record)?;
            return Err(crate::ImError::PermissionDenied);
        }
        let current = crate::internal::handle_discovery::resolve_recovery_handle_binding_async(
            core,
            record.binding.handle.as_str(),
        )
        .await?;
        if current != record.binding {
            return Err(crate::ImError::PermissionDenied);
        }
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            &record.binding.domain,
            &record.binding.local_part,
            core.inner().sdk_config().anp_service_endpoint.as_ref(),
            core.inner().sdk_config().anp_service_did.as_ref(),
        )?;
        if generated.did == record.binding.did {
            return Err(crate::ImError::PermissionDenied);
        }
        let prepared = crate::internal::identity_wire::device_recovery::prepare_recovery_finalize(
            &generated,
            random_id("recovery-finalize")?,
            record.binding.mapping_generation,
            OffsetDateTime::now_utc(),
        )?;
        let local_alias = recovered_local_alias(&record.binding.local_part, &generated.unique_id);
        record.generated = Some(generated);
        record.prepared_finalize = Some(prepared);
        record.replace_reconfirmation_token(supplied_reconfirmation.as_str().to_owned());
        record.local_alias = Some(local_alias);
        store.save(&record)?;
    }

    if record.remote_result.is_none() {
        // Preserve operation/document/keys while refreshing short-lived proof
        // and account evidence. user-service hashes only those stable business
        // fields for idempotency and validates fresh evidence before replay.
        let generated = record
            .generated
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let prepared = record
            .prepared_finalize
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let refreshed = crate::internal::identity_wire::device_recovery::prepare_recovery_finalize(
            generated,
            prepared.operation_id.clone(),
            prepared.expected_handle_mapping_generation,
            OffsetDateTime::now_utc(),
        )?;
        if refreshed.new_did_document != prepared.new_did_document
            || refreshed.bootstrap_device_id != prepared.bootstrap_device_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        record.prepared_finalize = Some(refreshed);
        record.replace_reconfirmation_token(supplied_reconfirmation.as_str().to_owned());
        store.save(&record)?;

        let generated = record
            .generated
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let prepared = record
            .prepared_finalize
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let session = record
            .session
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let account_user_id = session.account_user_id.clone();
        if !matches!(
            session.state,
            RecoveryRemoteState::Ready | RecoveryRemoteState::Consumed
        ) {
            return Err(crate::ImError::PermissionDenied);
        }
        let call = crate::internal::identity_wire::device_recovery::build_recovery_finalize_call(
            prepared,
            &session.recovery_session_token,
            record
                .reconfirmation_token
                .as_deref()
                .ok_or(crate::ImError::PermissionDenied)?,
        )?;
        let mut transport = crate::internal::transport::CorePlainTransport::new(core);
        let raw = transport
            .rpc(call.endpoint, call.method, call.params)
            .await?;
        let parsed =
            crate::internal::identity_wire::device_recovery::parse_recovery_finalize_result(
                raw,
                session,
                record.binding.handle.as_str(),
                &account_user_id,
                record.binding.mapping_generation,
                generated,
                OffsetDateTime::now_utc(),
            )?;
        let remote = match parsed {
            RecoveryFinalizeParseOutcome::Ready(remote) => remote,
            RecoveryFinalizeParseOutcome::TokenRefreshRequired(cutover) => {
                let fresh = issue_recovery_management_tokens(core, &store, &mut record).await?;
                let generated = record
                    .generated
                    .as_ref()
                    .ok_or(crate::ImError::PermissionDenied)?;
                crate::internal::identity_wire::device_recovery::complete_recovery_finalize_with_fresh_tokens(
                    cutover,
                    fresh,
                    &account_user_id,
                    generated,
                )?
            }
        };
        if let Some(session) = record.session.as_mut() {
            session.state = RecoveryRemoteState::Consumed;
        }
        record.token_issue_operation_id = None;
        record.remote_result = Some(remote);
        record.replace_begin_grant(None);
        store.save(&record)?;
    }

    refresh_persisted_recovery_tokens_if_needed(core, &store, &mut record).await?;
    let identity = persist_recovery_identity_async(core, &record).await?;
    let progress = progress(&record)?.ok_or(crate::ImError::PermissionDenied)?;
    Ok(HandleRecoveryFinalizeResult { progress, identity })
}

pub(crate) fn resume_activation(
    core: &crate::core::ImCore,
    recovery_session_id: &str,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let store = PendingRecoveryStore::from_core(core)?;
    let recovery_session_id = required(recovery_session_id, "recovery_session_id")?;
    let (_, record) = store
        .load_by_session(&recovery_session_id)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: recovery_session_id.to_owned(),
        })?;
    require_current_recovery_access_token(&record, OffsetDateTime::now_utc())?;
    persist_recovery_identity(core, &record)
}

pub(crate) async fn resume_activation_async(
    core: &crate::core::ImCore,
    recovery_session_id: &str,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let _guard = core.inner().handle_recovery_lock.lock().await;
    let store = PendingRecoveryStore::from_core(core)?;
    let recovery_session_id = required(recovery_session_id, "recovery_session_id")?;
    let (_, mut record) = store
        .load_by_session(&recovery_session_id)?
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: recovery_session_id.to_owned(),
        })?;
    refresh_persisted_recovery_tokens_if_needed(core, &store, &mut record).await?;
    persist_recovery_identity_async(core, &record).await
}

pub(crate) fn mark_activation_complete(
    core: &crate::core::ImCore,
    recovery_session_id: &str,
) -> crate::ImResult<()> {
    let store = PendingRecoveryStore::from_core(core)?;
    let recovery_session_id = required(recovery_session_id, "recovery_session_id")?;
    let Some((secret_ref, record)) = store.load_by_session(&recovery_session_id)? else {
        // The acknowledgement is deliberately idempotent; a previous call may
        // already have removed the only replayable Recovery record.
        return Ok(());
    };
    let generated = record
        .generated
        .as_ref()
        .filter(|_| record.remote_result.is_some())
        .ok_or(crate::ImError::PermissionDenied)?;
    let identity_id = crate::ids::IdentityId::parse(&generated.unique_id)?;
    let identity = core
        .identities()
        .resolve(crate::identity::IdentitySelector::Id(identity_id))?;
    if identity.did != generated.did {
        return Err(crate::ImError::PermissionDenied);
    }
    store.delete(&secret_ref)
}

async fn refresh_status(
    core: &crate::core::ImCore,
    record: &mut PendingRecoveryRecord,
) -> crate::ImResult<()> {
    let session = record
        .session
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    if terminal(session.state) {
        return Ok(());
    }
    let call = crate::internal::identity_wire::device_recovery::build_recovery_status_call(
        &session.recovery_session_token,
    )?;
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let raw = transport
        .rpc(call.endpoint, call.method, call.params)
        .await?;
    let status = crate::internal::identity_wire::device_recovery::parse_recovery_status_result(
        raw,
        session,
        OffsetDateTime::now_utc(),
    )?;
    let session = record
        .session
        .as_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    validate_transition(session.state, status.state)?;
    session.state = status.state;
    Ok(())
}

async fn issue_recovery_management_tokens(
    core: &crate::core::ImCore,
    store: &PendingRecoveryStore,
    record: &mut PendingRecoveryRecord,
) -> crate::ImResult<crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult> {
    for attempt in 0..2 {
        let operation_id = match record.token_issue_operation_id.as_ref() {
            Some(operation_id) => operation_id.clone(),
            None => {
                let operation_id = random_id("recovery-token")?;
                record.token_issue_operation_id = Some(operation_id.clone());
                store.save(record)?;
                operation_id
            }
        };
        let generated = record
            .generated
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let service_domain =
            crate::internal::identity_join_activation_pending::service_domain_from_did(
                &generated.did,
            )?;
        if service_domain
            != core
                .inner()
                .sdk_config()
                .did_domain
                .trim()
                .to_ascii_lowercase()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let signing_private =
            anp::PrivateKeyMaterial::from_pem(&generated.device_signing_private_pem)
                .map_err(|_| crate::ImError::PermissionDenied)?;
        let prepared = crate::internal::identity_wire::device_genesis::prepare_management_ready_device_token_issue(
            operation_id,
            &generated.did_document,
            generated.protocol_device_id.as_str(),
            &generated.device_signing_key_id,
            &signing_private,
            &service_domain,
        )?;
        let call = crate::internal::identity_wire::device_genesis::build_device_token_issue_call(
            &prepared,
        )?;
        let raw = crate::internal::transport::CorePlainTransport::new(core)
            .rpc(call.endpoint, call.method, call.params)
            .await?;
        match crate::internal::identity_wire::device_genesis::parse_device_token_issue_result(
            raw,
            &prepared,
            1,
            OffsetDateTime::now_utc(),
        ) {
            Ok(token) => return Ok(token),
            Err(crate::ImError::SessionExpired) if attempt == 0 => {
                // A previous token-issue response can be lost for longer than
                // its access TTL. Rotate only this credential-issuance
                // operation; Recovery cutover operation/document/keys remain
                // unchanged.
                record.token_issue_operation_id = Some(random_id("recovery-token")?);
                store.save(record)?;
            }
            Err(error) => return Err(error),
        }
    }
    Err(crate::ImError::SessionExpired)
}

async fn refresh_persisted_recovery_tokens_if_needed(
    core: &crate::core::ImCore,
    store: &PendingRecoveryStore,
    record: &mut PendingRecoveryRecord,
) -> crate::ImResult<()> {
    match require_current_recovery_access_token(record, OffsetDateTime::now_utc()) {
        Ok(()) => return Ok(()),
        Err(crate::ImError::SessionExpired) => {}
        Err(error) => return Err(error),
    }
    let fresh = issue_recovery_management_tokens(core, store, record).await?;
    let account_user_id = record
        .session
        .as_ref()
        .map(|session| session.account_user_id.as_str())
        .ok_or(crate::ImError::PermissionDenied)?
        .to_owned();
    let generated = record
        .generated
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let remote = record
        .remote_result
        .as_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    crate::internal::identity_wire::device_recovery::replace_recovery_finalize_tokens(
        remote,
        fresh,
        &account_user_id,
        generated,
    )?;
    record.token_issue_operation_id = None;
    store.save(record)?;
    Ok(())
}

fn require_current_recovery_access_token(
    record: &PendingRecoveryRecord,
    now: OffsetDateTime,
) -> crate::ImResult<()> {
    let remote = record
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let expires_at = OffsetDateTime::parse(
        remote.token_expires_at.trim(),
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    if expires_at <= now {
        Err(crate::ImError::SessionExpired)
    } else {
        Ok(())
    }
}

fn progress(record: &PendingRecoveryRecord) -> crate::ImResult<Option<HandleRecoveryProgress>> {
    let Some(session) = record.session.as_ref() else {
        return Ok(None);
    };
    let new_did = record
        .remote_result
        .as_ref()
        .map(|result| crate::ids::Did::parse(&result.did))
        .transpose()?;
    Ok(Some(HandleRecoveryProgress {
        recovery_session_id: session.recovery_session_id.clone(),
        handle: record.binding.handle.clone(),
        old_did: record.binding.did.clone(),
        side: HandleRecoverySide::Requester,
        phase: public_phase(session.state),
        cooling_until: session.cooling_until.clone(),
        expires_at: session.expires_at.clone(),
        can_cancel_from_this_device: false,
        new_did,
        local_activation_pending: record.remote_result.is_some(),
    }))
}

async fn persist_recovery_identity_async(
    core: &crate::core::ImCore,
    record: &PendingRecoveryRecord,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let generated = record
        .generated
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let remote = record
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let local_alias = record
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let paths = core.inner().sdk_paths().identities.clone();
    let stored =
        crate::internal::identity_store::IdentityStore::save_identity_with_secret_storage_async(
            paths.clone(),
            recovery_save_input(record, generated, remote, local_alias),
            secret_storage.clone(),
        )
        .await?;
    crate::internal::identity_store::IdentityStore::persist_vnext_auth_token_pair_async(
        paths,
        local_alias.to_owned(),
        remote.access_token.clone(),
        remote.refresh_token.clone(),
        remote.token_expires_at.clone(),
        secret_storage,
    )
    .await?;
    recovered_summary(stored, generated)
}

fn persist_recovery_identity(
    core: &crate::core::ImCore,
    record: &PendingRecoveryRecord,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let generated = record
        .generated
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let remote = record
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let local_alias = record
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let secret_storage =
        crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?;
    let store =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities);
    let stored = store.save_identity_with_secret_storage(
        recovery_save_input(record, generated, remote, local_alias),
        secret_storage.clone(),
    )?;
    store.persist_vnext_auth_token_pair(
        local_alias,
        &remote.access_token,
        &remote.refresh_token,
        &remote.token_expires_at,
        &secret_storage,
    )?;
    recovered_summary(stored, generated)
}

fn recovery_save_input(
    record: &PendingRecoveryRecord,
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    remote: &RecoveryFinalizeResult,
    local_alias: &str,
) -> crate::internal::identity_store::SaveIdentityInput {
    crate::internal::identity_store::SaveIdentityInput {
        local_alias: local_alias.to_owned(),
        did: generated.did.clone(),
        unique_id: generated.unique_id.clone(),
        user_id: remote.user_id.clone(),
        display_name: record.binding.local_part.clone(),
        handle: record.binding.local_part.clone(),
        full_handle: record.binding.handle.as_str().to_owned(),
        jwt_token: remote.access_token.clone(),
        did_document: Some(generated.did_document.clone()),
        key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
            root_key_id: generated.root_key_id.clone(),
            device_signing_key_id: generated.device_signing_key_id.clone(),
            device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
        },
        device_state: Some(remote.device_state()),
        key1_private_pem: generated.root_private_pem.clone(),
        key1_public_pem: generated.root_public_pem.clone(),
        e2ee_signing_private_pem: generated.device_signing_private_pem.clone(),
        e2ee_agreement_private_pem: generated.device_e2ee_private_pem.clone(),
        daemon_subkey_package: Some(generated.daemon_subkey_package.clone()),
        make_default: true,
    }
}

fn prepare_cancel_with_current_device_key(
    client: &crate::core::ImClient,
    operation_id: String,
    recovery_session_id: &str,
    device_id: &str,
    signing_key_id: &str,
    did_document: &serde_json::Value,
) -> crate::ImResult<crate::internal::identity_wire::device_recovery::PreparedRecoveryCancel> {
    let signing_pem = Zeroizing::new(
        client
            .runtime()
            .key_provider
            .device_request_signing_private_pem()?,
    );
    let signing_private = anp::PrivateKeyMaterial::from_pem(&signing_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    crate::internal::identity_wire::device_recovery::prepare_recovery_cancel(
        operation_id,
        recovery_session_id,
        device_id,
        signing_key_id,
        &signing_private,
        did_document,
        OffsetDateTime::now_utc(),
    )
}

fn recovered_summary(
    stored: crate::internal::identity_store::StoredIdentity,
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let mut identity =
        crate::internal::identity_registration_runtime::identity_summary_from_stored(&stored)?;
    if identity.id.as_str() != generated.unique_id || identity.did != generated.did {
        return Err(crate::ImError::PermissionDenied);
    }
    identity.device_id = Some(generated.protocol_device_id.as_str().to_owned());
    Ok(identity)
}

fn validate_current_ready_admin_binding(
    did: &crate::ids::Did,
    device_id: &str,
    signing_key_id: &str,
    registry: &crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry,
    document: &serde_json::Value,
) -> crate::ImResult<()> {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationRole, DeviceAuthorizationStatus,
    };

    if document.get("id").and_then(serde_json::Value::as_str) != Some(did.as_str())
        || registry.did != *did
        || registry.checkpoint.document_hash
            != crate::internal::identity_wire::device_genesis::document_hash(document)?
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut current_devices = registry
        .devices
        .iter()
        .filter(|entry| entry.device_id == device_id);
    let current = current_devices
        .next()
        .ok_or(crate::ImError::PermissionDenied)?;
    // Active/admin/management-ready are AWiki-domain authorization facts, not
    // public ANP deviceManifest fields. They must come from the authenticated
    // current Registry rather than the possibly stale local projection.
    if current_devices.next().is_some()
        || current.signing_key_id != signing_key_id
        || current.status != DeviceAuthorizationStatus::Active
        || current.role != DeviceAuthorizationRole::Admin
        || !current.management_ready
        || current.auth_generation == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    let entry = manifest
        .devices
        .iter()
        .find(|entry| entry.device_id == device_id)
        .ok_or(crate::ImError::PermissionDenied)?;
    if entry.signing_key_id != signing_key_id || entry.e2ee_key_id != current.e2ee_key_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_transition(
    current: RecoveryRemoteState,
    next: RecoveryRemoteState,
) -> crate::ImResult<()> {
    let allowed = match current {
        RecoveryRemoteState::Cooling => matches!(
            next,
            RecoveryRemoteState::Cooling
                | RecoveryRemoteState::Ready
                | RecoveryRemoteState::Cancelled
                | RecoveryRemoteState::Expired
        ),
        RecoveryRemoteState::Ready => matches!(
            next,
            RecoveryRemoteState::Ready
                | RecoveryRemoteState::Cancelled
                | RecoveryRemoteState::Consumed
                | RecoveryRemoteState::Expired
        ),
        RecoveryRemoteState::Cancelled => next == RecoveryRemoteState::Cancelled,
        RecoveryRemoteState::Consumed => next == RecoveryRemoteState::Consumed,
        RecoveryRemoteState::Expired => next == RecoveryRemoteState::Expired,
    };
    if allowed {
        Ok(())
    } else {
        Err(crate::ImError::PermissionDenied)
    }
}

fn terminal(state: RecoveryRemoteState) -> bool {
    matches!(
        state,
        RecoveryRemoteState::Cancelled
            | RecoveryRemoteState::Consumed
            | RecoveryRemoteState::Expired
    )
}

fn restartable_terminal(record: &PendingRecoveryRecord) -> bool {
    record.remote_result.is_none()
        && record.session.as_ref().is_some_and(|session| {
            matches!(
                session.state,
                RecoveryRemoteState::Cancelled | RecoveryRemoteState::Expired
            )
        })
}

fn public_phase(state: RecoveryRemoteState) -> HandleRecoveryPhase {
    match state {
        RecoveryRemoteState::Cooling => HandleRecoveryPhase::Cooling,
        RecoveryRemoteState::Ready => HandleRecoveryPhase::Ready,
        RecoveryRemoteState::Cancelled => HandleRecoveryPhase::Cancelled,
        RecoveryRemoteState::Consumed => HandleRecoveryPhase::Consumed,
        RecoveryRemoteState::Expired => HandleRecoveryPhase::Expired,
    }
}

fn recovered_local_alias(local_part: &str, unique_id: &str) -> String {
    let suffix = unique_id
        .chars()
        .rev()
        .take(12)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{local_part}-recovered-{suffix}")
}

fn secret_utf8(
    secret: &crate::internal::platform_secret::SecretBytes,
    field: &str,
) -> crate::ImResult<Zeroizing<String>> {
    let value = std::str::from_utf8(secret.expose_secret()).map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_owned()), "verification grant must be UTF-8")
    })?;
    required(value, field).map(Zeroizing::new)
}

fn required(value: &str, field: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn random_id(prefix: &str) -> crate::ImResult<String> {
    let mut bytes = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(format!("{prefix}-{}", URL_SAFE_NO_PAD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_core(root: &std::path::Path, endpoint: &str) -> crate::ImCore {
        let config = crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse(endpoint).unwrap(),
            did_domain: "awiki.info".to_owned(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        };
        let paths = crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        };
        crate::ImCore::new_with_options(
            config,
            paths,
            crate::ImCoreOpenOptions::default()
                .with_multi_device_handle_recovery_enabled(true)
                .with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        crate::vault::DeviceVaultRootKey::from_bytes([73_u8; 32]),
                        root.join("vault"),
                        "recovery-vnext-test-workspace",
                        "recovery-vnext-test-device",
                    ),
                ),
        )
        .unwrap()
    }

    fn pending_after_cutover(
        now: OffsetDateTime,
        access_expires_at: OffsetDateTime,
    ) -> PendingRecoveryRecord {
        let binding = crate::internal::handle_discovery::recovery_handle_binding_from_value(
            "alice.awiki.info",
            &serde_json::json!({
                "did": "did:wba:awiki.info:user:alice:e1_old",
                "full_handle": "alice.awiki.info",
                "status": "active",
                "binding_generation": "8"
            }),
        )
        .unwrap();
        let mut record = PendingRecoveryRecord::new(
            binding,
            "recovery-begin-stable".to_owned(),
            "begin-grant-secret".to_owned(),
        )
        .unwrap();
        record.session = Some(
            crate::internal::identity_wire::device_recovery::RecoverySessionResult {
                recovery_session_id: "recovery-activation-pending".to_owned(),
                recovery_session_token: "session-token-secret".to_owned(),
                account_user_id: "user-alice".to_owned(),
                old_did: record.binding.did.as_str().to_owned(),
                state: RecoveryRemoteState::Consumed,
                cooling_until: format_test_time(now - time::Duration::hours(1)),
                expires_at: format_test_time(now + time::Duration::days(1)),
            },
        );
        record.replace_begin_grant(None);
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let prepared = crate::internal::identity_wire::device_recovery::prepare_recovery_finalize(
            &generated,
            "recovery-finalize-stable".to_owned(),
            record.binding.mapping_generation,
            now,
        )
        .unwrap();
        let document_hash =
            crate::internal::identity_wire::device_genesis::document_hash(&generated.did_document)
                .unwrap();
        let result = RecoveryFinalizeResult {
            recovery_session_id: "recovery-activation-pending".to_owned(),
            state: RecoveryRemoteState::Consumed,
            old_did: record.binding.did.as_str().to_owned(),
            did: generated.did.as_str().to_owned(),
            handle: record.binding.handle.as_str().to_owned(),
            handle_mapping_generation: 9,
            user_id: record.session.as_ref().unwrap().account_user_id.clone(),
            checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: 1,
                document_hash,
                registry_version: 1,
            },
            device: crate::internal::identity_wire::device_genesis::GenesisDeviceResult {
                device_id: generated.protocol_device_id.as_str().to_owned(),
                signing_key_id: generated.device_signing_key_id.clone(),
                e2ee_key_id: generated.device_e2ee_key_id.clone(),
                status: "active".to_owned(),
                role: "admin".to_owned(),
                management_ready: true,
                auth_generation: 1,
            },
            access_token: "expired-access-secret".to_owned(),
            refresh_token: "expired-refresh-secret".to_owned(),
            token_expires_at: format_test_time(access_expires_at),
        };
        record.generated = Some(generated);
        record.prepared_finalize = Some(prepared);
        record.replace_reconfirmation_token("reconfirmation-secret".to_owned());
        record.remote_result = Some(result);
        record.local_alias = Some("alice-recovered-activation-pending".to_owned());
        record.validate().unwrap();
        record
    }

    fn format_test_time(value: OffsetDateTime) -> String {
        value
            .replace_nanosecond(0)
            .unwrap_or(value)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap()
    }

    #[test]
    fn recovery_alias_is_new_owner_specific() {
        let first = recovered_local_alias("alice", "e1_first-owner");
        let second = recovered_local_alias("alice", "e1_second-owner");
        assert_ne!(first, second);
        assert!(first.starts_with("alice-recovered-"));
    }

    #[test]
    fn transition_rejects_terminal_revival() {
        assert!(
            validate_transition(RecoveryRemoteState::Cancelled, RecoveryRemoteState::Ready,)
                .is_err()
        );
        assert!(
            validate_transition(RecoveryRemoteState::Cooling, RecoveryRemoteState::Ready,).is_ok()
        );
    }

    #[test]
    fn synchronous_resume_refuses_an_expired_persisted_access_token() {
        let root = tempfile::tempdir().unwrap();
        let core = test_core(root.path(), "http://127.0.0.1:9");
        let now = OffsetDateTime::now_utc();
        let record = pending_after_cutover(now, now - time::Duration::seconds(1));
        let store = PendingRecoveryStore::from_core(&core).unwrap();
        store.save(&record).unwrap();

        assert_eq!(
            resume_activation(&core, "recovery-activation-pending"),
            Err(crate::ImError::SessionExpired)
        );
        assert!(core.identities().list().unwrap().is_empty());
        assert!(store
            .load_by_session("recovery-activation-pending")
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn async_resume_keeps_pending_state_when_token_refresh_transport_fails() {
        let root = tempfile::tempdir().unwrap();
        let core = test_core(root.path(), "http://127.0.0.1:9");
        let now = OffsetDateTime::now_utc();
        let record = pending_after_cutover(now, now - time::Duration::seconds(1));
        let stable_finalize_operation = record
            .prepared_finalize
            .as_ref()
            .unwrap()
            .operation_id
            .clone();
        let stable_document = record
            .prepared_finalize
            .as_ref()
            .unwrap()
            .new_did_document
            .clone();
        let stable_did = record.generated.as_ref().unwrap().did.clone();
        let store = PendingRecoveryStore::from_core(&core).unwrap();
        store.save(&record).unwrap();

        let result = resume_activation_async(&core, "recovery-activation-pending").await;
        assert!(result.is_err());
        assert!(core.identities().list().unwrap().is_empty());
        let (_, pending) = store
            .load_by_session("recovery-activation-pending")
            .unwrap()
            .unwrap();
        assert!(pending.token_issue_operation_id.is_some());
        assert!(pending.remote_result.is_some());
        assert!(pending
            .remote_result
            .as_ref()
            .is_some_and(|remote| remote.token_expires_at
                == format_test_time(now - time::Duration::seconds(1))));
        assert_eq!(
            pending.prepared_finalize.as_ref().unwrap().operation_id,
            stable_finalize_operation
        );
        assert_eq!(
            pending.prepared_finalize.as_ref().unwrap().new_did_document,
            stable_document
        );
        assert_eq!(pending.generated.as_ref().unwrap().did, stable_did);
    }

    #[test]
    fn cancel_binding_rejects_revoked_member_or_not_ready_registry_device() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
        };

        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let current =
            crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                device_id: generated.protocol_device_id.as_str().to_owned(),
                signing_key_id: generated.device_signing_key_id.clone(),
                e2ee_key_id: generated.device_e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: 1,
            };
        let registry = crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry {
            did: generated.did.clone(),
            checkpoint: IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: crate::internal::identity_wire::device_genesis::document_hash(
                    &generated.did_document,
                )
                .unwrap(),
                registry_version: 1,
            },
            devices: vec![current],
            pending_join_requests: Vec::new(),
        };
        let validate = |registry| {
            validate_current_ready_admin_binding(
                &generated.did,
                generated.protocol_device_id.as_str(),
                &generated.device_signing_key_id,
                registry,
                &generated.did_document,
            )
        };
        validate(&registry).unwrap();

        let mut revoked = registry.clone();
        revoked.devices[0].status = DeviceAuthorizationStatus::Revoked;
        assert_eq!(validate(&revoked), Err(crate::ImError::PermissionDenied));

        let mut member = registry.clone();
        member.devices[0].role = DeviceAuthorizationRole::Member;
        member.devices[0].management_ready = false;
        assert_eq!(validate(&member), Err(crate::ImError::PermissionDenied));

        let mut not_ready = registry.clone();
        not_ready.devices[0].management_ready = false;
        assert_eq!(validate(&not_ready), Err(crate::ImError::PermissionDenied));

        let mut stale_document = registry.clone();
        stale_document.checkpoint.document_hash = format!("sha256:{}", "A".repeat(43));
        assert_eq!(
            validate(&stale_document),
            Err(crate::ImError::PermissionDenied)
        );
    }

    #[test]
    fn recovery_generation_and_save_input_are_isolated_from_the_old_owner() {
        let old = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let new = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        assert_ne!(old.did, new.did);
        assert_ne!(old.unique_id, new.unique_id);
        assert_ne!(old.protocol_device_id, new.protocol_device_id);
        assert!(old.root_private_pem != new.root_private_pem);
        assert!(old.device_signing_private_pem != new.device_signing_private_pem);
        assert!(old.device_e2ee_private_pem != new.device_e2ee_private_pem);

        let raw_binding = serde_json::json!({
            "did": old.did.as_str(),
            "full_handle": "alice.awiki.info",
            "status": "active",
            "binding_generation": "8"
        });
        let binding = crate::internal::handle_discovery::recovery_handle_binding_from_value(
            "alice.awiki.info",
            &raw_binding,
        )
        .unwrap();
        let record =
            PendingRecoveryRecord::new(binding, "recovery-op".to_owned(), "begin-grant".to_owned())
                .unwrap();
        let remote = RecoveryFinalizeResult {
            recovery_session_id: "recovery-session".to_owned(),
            state: RecoveryRemoteState::Consumed,
            old_did: old.did.as_str().to_owned(),
            did: new.did.as_str().to_owned(),
            handle: "alice.awiki.info".to_owned(),
            handle_mapping_generation: 9,
            user_id: "user-alice".to_owned(),
            checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: crate::internal::identity_wire::device_genesis::document_hash(
                    &new.did_document,
                )
                .unwrap(),
                registry_version: 1,
            },
            device: crate::internal::identity_wire::device_genesis::GenesisDeviceResult {
                device_id: new.protocol_device_id.as_str().to_owned(),
                signing_key_id: new.device_signing_key_id.clone(),
                e2ee_key_id: new.device_e2ee_key_id.clone(),
                status: "active".to_owned(),
                role: "admin".to_owned(),
                management_ready: true,
                auth_generation: 1,
            },
            access_token: "new-access-token".to_owned(),
            refresh_token: "new-refresh-token".to_owned(),
            token_expires_at: "2030-01-01T00:00:00Z".to_owned(),
        };
        let input = recovery_save_input(&record, &new, &remote, "alice-recovered-new");

        assert_eq!(input.did, new.did);
        assert_eq!(input.unique_id, new.unique_id);
        assert_ne!(input.unique_id, old.unique_id);
        assert!(input.key1_private_pem == new.root_private_pem);
        assert!(input.key1_private_pem != old.root_private_pem);
        assert!(input.e2ee_signing_private_pem == new.device_signing_private_pem);
        assert!(input.e2ee_agreement_private_pem == new.device_e2ee_private_pem);

        let runtime_source = include_str!("identity_recovery_vnext.rs");
        let legacy_merge_symbol = ["merge_recovered_handle", "_local_state"].concat();
        let forbidden_group_symbol = ["group", "_rebind"].concat();
        assert!(!runtime_source.contains(&legacy_merge_symbol));
        assert!(!runtime_source.contains(&forbidden_group_symbol));
    }
}
