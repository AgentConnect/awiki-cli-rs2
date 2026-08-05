//! Host-neutral orchestration for Manifest Handle Recovery v1.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::identity::{
    AuthorizedJoinActivationProgress, AuthorizedJoinActivationRequest,
    HandleRecoveryActivateRequest, HandleRecoveryErrorCode, HandleRecoveryImpact,
    HandleRecoveryOtpRequest, HandleRecoveryOtpResult, HandleRecoveryPhase,
    HandleRecoveryPrepareRequest, HandleRecoveryProgress, HandleRecoveryResetReference,
    HandleRecoveryResumeRequest, HandleRecoveryTransitionSourceKind,
};
use crate::internal::identity_handle_recovery_pending::{
    PendingHandleRecovery, PendingHandleRecoveryStore, PendingRecoveryPhase, RecoveryRemoteResult,
};
use crate::internal::transport::{AsyncRestTransport as _, AsyncRpcTransport as _};

pub(crate) async fn request_otp(
    core: &crate::core::ImCore,
    request: HandleRecoveryOtpRequest,
) -> crate::ImResult<HandleRecoveryOtpResult> {
    require_enabled(core)?;
    let call = crate::internal::identity_wire::handle_recovery::build_send_otp_call(
        &request.phone,
        &request.handle,
        &request.operation_id,
    )?;
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let _ = transport
        .rpc(call.endpoint, call.method, call.params)
        .await?;
    Ok(HandleRecoveryOtpResult {
        handle: request.handle,
        operation_id: request.operation_id,
        accepted: true,
    })
}

pub(crate) async fn prepare(
    core: &crate::core::ImCore,
    request: HandleRecoveryPrepareRequest,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    require_explicit_identity(&request.identity)?;
    let canonical =
        crate::internal::identity_wire::handle_recovery::canonical_handle(&request.handle)?;
    let operation_id = crate::internal::identity_wire::handle_recovery::validate_operation_id(
        &request.operation_id,
    )?;
    let identity = core.identities().resolve_async(request.identity).await?;
    let lock = core.inner().handle_recovery_lock(identity.id.as_str());
    let _guard = lock.lock().await;
    if identity.handle.as_ref().map(|handle| handle.as_str()) != Some(canonical.full.as_str()) {
        return Err(recovery_error(
            HandleRecoveryErrorCode::HandleRecoveryTransitionMismatch,
        ));
    }
    let store = PendingHandleRecoveryStore::from_core(core).map_err(|_| {
        recovery_error(HandleRecoveryErrorCode::HandleRecoveryLocalStateUnavailable)
    })?;
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let entry = index
        .credentials
        .values()
        .find(|entry| entry.unique_id == identity.id.as_str())
        .ok_or_else(|| {
            recovery_error(HandleRecoveryErrorCode::HandleRecoveryLocalStateUnavailable)
        })?;
    let expected_generation = entry
        .binding_generation
        .clone()
        .filter(|value| canonical_generation(value))
        .ok_or_else(|| {
            recovery_error(HandleRecoveryErrorCode::HandleRecoveryLocalStateUnavailable)
        })?;
    let local_alias = (!entry.credential_name.trim().is_empty())
        .then(|| entry.credential_name.clone())
        .or_else(|| identity.local_alias.clone())
        .ok_or_else(|| {
            recovery_error(HandleRecoveryErrorCode::HandleRecoveryLocalStateUnavailable)
        })?;
    let exchange = crate::internal::identity_wire::handle_recovery::build_grant_exchange_call(
        &request.phone,
        &request.code,
        &canonical.full,
        &operation_id,
    )?;
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let result = transport
        .rest_post(exchange.endpoint, exchange.method, exchange.body)
        .await?;
    let grant =
        crate::internal::identity_wire::handle_recovery::parse_grant_exchange_result(result)?;
    let generated = crate::internal::identity_generation::generate_handle_recovery_identity(
        &canonical.domain,
        &canonical.local_part,
        core.inner().sdk_config().anp_service_endpoint.as_ref(),
        core.inner().sdk_config().anp_service_did.as_ref(),
    )?;
    let recovery_id = random_reference("recovery")?;
    let recovery_grant = String::from_utf8(grant.recovery_grant.expose_secret().to_vec())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let pending = PendingHandleRecovery::new(
        recovery_id,
        operation_id,
        identity.id.as_str().to_owned(),
        entry.user_id.clone(),
        local_alias,
        identity
            .display_name
            .clone()
            .unwrap_or_else(|| canonical.local_part.clone()),
        identity.is_default,
        canonical.full,
        identity.did.as_str().to_owned(),
        expected_generation,
        recovery_grant,
        grant.expires_at,
        generated,
    )?;
    store.save(&pending)?;
    progress(core, &pending)
}

pub(crate) async fn activate(
    core: &crate::core::ImCore,
    request: HandleRecoveryActivateRequest,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    if !request.user_presence_confirmed {
        return Err(recovery_error(
            HandleRecoveryErrorCode::HandleRecoveryUserPresenceRequired,
        ));
    }
    advance(core, &request.recovery_id).await
}

pub(crate) async fn resume(
    core: &crate::core::ImCore,
    request: HandleRecoveryResumeRequest,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    advance(core, &request.recovery_id).await
}

pub(crate) fn status(
    core: &crate::core::ImCore,
    recovery_id: &str,
) -> crate::ImResult<HandleRecoveryProgress> {
    require_enabled(core)?;
    let store = PendingHandleRecoveryStore::from_core(core).map_err(|_| {
        recovery_error(HandleRecoveryErrorCode::HandleRecoveryLocalStateUnavailable)
    })?;
    let (_, pending) = store
        .load(recovery_id)?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::HandleRecoveryNotPrepared))?;
    progress(core, &pending)
}

async fn advance(
    core: &crate::core::ImCore,
    recovery_id: &str,
) -> crate::ImResult<HandleRecoveryProgress> {
    let store = PendingHandleRecoveryStore::from_core(core).map_err(|_| {
        recovery_error(HandleRecoveryErrorCode::HandleRecoveryLocalStateUnavailable)
    })?;
    let (_, pending_before_lock) = store
        .load(recovery_id)?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::HandleRecoveryNotPrepared))?;
    let lock = core
        .inner()
        .handle_recovery_lock(&pending_before_lock.owner_identity_id);
    let _guard = lock.lock().await;
    let (_, mut pending) = store
        .load(recovery_id)?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::HandleRecoveryNotPrepared))?;
    if pending.phase == PendingRecoveryPhase::Prepared {
        let created = time::OffsetDateTime::now_utc();
        let grant_expires = parse_timestamp(&pending.grant_expires_at)?;
        if grant_expires <= created {
            pending.blocked_code = Some(
                HandleRecoveryErrorCode::HandleRecoveryOutcomeUnknown
                    .as_str()
                    .to_owned(),
            );
            pending.phase = PendingRecoveryPhase::Blocked;
            store.save(&pending)?;
            return progress(core, &pending);
        }
        let expires = commit_proof_expires_at(created, grant_expires)?;
        let mut nonce = [0_u8; 24];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| crate::ImError::Internal {
                message: "generate Handle Recovery nonce failed".to_owned(),
            })?;
        pending.commit_created_at = Some(format_timestamp(created)?);
        pending.commit_expires_at = Some(format_timestamp(expires)?);
        pending.commit_nonce = Some(URL_SAFE_NO_PAD.encode(nonce));
        pending.remote_attempted = true;
        pending.phase = PendingRecoveryPhase::RemoteCommitPending;
        store.save(&pending)?;
    }
    if pending.phase == PendingRecoveryPhase::RemoteCommitPending {
        if commit_proof_expired(
            pending.commit_expires_at.as_deref().unwrap_or_default(),
            time::OffsetDateTime::now_utc(),
        )? {
            pending.blocked_code = Some(
                HandleRecoveryErrorCode::HandleRecoveryOutcomeUnknown
                    .as_str()
                    .to_owned(),
            );
            pending.phase = PendingRecoveryPhase::Blocked;
            store.save(&pending)?;
            return progress(core, &pending);
        }
        let private =
            anp::PrivateKeyMaterial::from_pem(&pending.generated.device_signing_private_pem)
                .map_err(|_| crate::ImError::PermissionDenied)?;
        let nonce = URL_SAFE_NO_PAD
            .decode(pending.commit_nonce.as_deref().unwrap_or_default())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let prepared = crate::internal::identity_wire::handle_recovery::prepare_commit(
            crate::internal::identity_wire::handle_recovery::CommitProofInput {
                operation_id: &pending.operation_id,
                handle: &pending.handle,
                recovery_grant: pending.recovery_grant(),
                expected_binding_generation: &pending.expected_binding_generation,
                new_did_document: pending.generated.did_document.clone(),
                bootstrap_device_id: pending.generated.protocol_device_id.as_str(),
                bootstrap_signing_key_id: &pending.generated.device_signing_key_id,
                bootstrap_signing_private_key: &private,
                created_at: pending.commit_created_at.as_deref().unwrap_or_default(),
                expires_at: pending.commit_expires_at.as_deref().unwrap_or_default(),
                nonce: &nonce,
            },
        )?;
        let mut transport = crate::internal::transport::CorePlainTransport::new(core);
        let raw = transport
            .rpc(
                prepared.call.endpoint,
                prepared.call.method,
                prepared.call.params,
            )
            .await
            .map_err(|error| match error {
                crate::ImError::TransportUnavailable { .. } => {
                    recovery_error(HandleRecoveryErrorCode::HandleRecoveryOutcomeUnknown)
                }
                other => other,
            })?;
        let result = parse_remote_result(raw, &pending)?;
        pending.remote_result = Some(result);
        pending.phase = PendingRecoveryPhase::RemoteCommitted;
        store.save(&pending)?;
    }
    if pending.phase == PendingRecoveryPhase::RemoteCommitted {
        let result = pending
            .remote_result
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let marker =
            crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &pending,
                result,
            )?;
        crate::internal::identity_transition_pending::persist(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &marker,
        )?;
        pending.phase = PendingRecoveryPhase::IdentityTransitionPending;
        store.save(&pending)?;
    }
    if pending.phase == PendingRecoveryPhase::IdentityTransitionPending {
        let result = pending
            .remote_result
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let marker = crate::internal::identity_transition_pending::load(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &pending.recovery_id,
        )?
        .ok_or(crate::ImError::PermissionDenied)?;
        if marker.phase
            == crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched
        {
            validate_switched_identity(core, &pending, result)?;
            pending.phase = PendingRecoveryPhase::IdentitySwitched;
            store.save(&pending)?;
        } else {
            crate::internal::identity_transition_pending::migrate_local_state(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &marker,
                &result.bootstrap_device_id,
                result.auth_generation,
            )?;
            let generated = &pending.generated;
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
                    auth_generation: result.auth_generation,
                },
            ),
            checkpoint: Some(
                crate::internal::identity_device_state::IdentityInternalCheckpoint {
                    document_version: result.document_version,
                    document_hash: result.document_hash.clone(),
                    registry_version: result.registry_version,
                },
            ),
        };
            crate::internal::identity_store::IdentityStore::new(
                &core.inner().sdk_paths().identities,
            )
            .save_recovered_identity_with_secret_storage(
                crate::internal::identity_store::SaveIdentityInput {
                    local_alias: pending.local_alias.clone(),
                    did: generated.did.clone(),
                    unique_id: pending.owner_identity_id.clone(),
                    user_id: result.account_user_id.clone(),
                    display_name: pending.display_name.clone(),
                    handle: crate::internal::identity_wire::handle_recovery::canonical_handle(
                        &pending.handle,
                    )?
                    .local_part,
                    full_handle: pending.handle.clone(),
                    binding_generation: Some(result.binding_generation.clone()),
                    // Recovery Commit intentionally returns no token. Signature-only
                    // auth refreshes this sealed auth state on the first request.
                    jwt_token: String::new(),
                    did_document: Some(generated.did_document.clone()),
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                        root_key_id: generated.root_key_id.clone(),
                        device_signing_key_id: generated.device_signing_key_id.clone(),
                        device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
                    },
                    device_state: Some(device_state),
                    key1_private_pem: generated.root_private_pem.clone(),
                    key1_public_pem: generated.root_public_pem.clone(),
                    e2ee_signing_private_pem: generated.device_signing_private_pem.clone(),
                    e2ee_agreement_private_pem: generated.device_e2ee_private_pem.clone(),
                    daemon_subkey_package: None,
                    make_default: pending.make_default,
                },
                crate::internal::identity_store::SaveIdentitySecretStorage::from_core(core)?,
                &[],
            )?;
            crate::internal::identity_transition_pending::update_phase(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &pending.recovery_id,
                crate::internal::identity_transition_pending::TransitionPhase::Pending,
                crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
            )?;
            pending.phase = PendingRecoveryPhase::IdentitySwitched;
            store.save(&pending)?;
        }
    }
    if pending.phase == PendingRecoveryPhase::IdentitySwitched {
        let result = pending
            .remote_result
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let recovered_did = crate::ids::Did::parse(&result.did)?;
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
        let _ = crate::internal::group_rebind_recovery::enqueue_recovery_jobs(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &pending.owner_identity_id,
            &pending.handle,
            std::slice::from_ref(&pending.previous_did),
            &result.did,
            &result.binding_generation,
        )?;
        let summary = client.groups().resume_rebind_recovery_async(100).await?;
        let (remaining, blocked) =
            crate::internal::group_rebind_recovery::handle_recovery_job_counts(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &pending.owner_identity_id,
                &pending.handle,
                &pending.previous_did,
                &result.did,
                &result.binding_generation,
            )?;
        if blocked > 0 || summary.blocked > 0 {
            pending.blocked_code = Some(
                HandleRecoveryErrorCode::HandleRecoveryBlocked
                    .as_str()
                    .to_owned(),
            );
            pending.phase = PendingRecoveryPhase::Blocked;
            store.save(&pending)?;
        } else if remaining == 0 {
            crate::internal::identity_transition_pending::update_phase(
                &core.inner().sdk_paths().local_state.sqlite_path,
                &pending.recovery_id,
                crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
                crate::internal::identity_transition_pending::TransitionPhase::Completed,
            )?;
            pending.phase = PendingRecoveryPhase::Completed;
            store.save(&pending)?;
        }
    }
    progress(core, &pending)
}

pub(crate) async fn activate_authorized_join(
    core: &crate::core::ImCore,
    request: AuthorizedJoinActivationRequest,
) -> crate::ImResult<AuthorizedJoinActivationProgress> {
    require_enabled(core)?;
    require_explicit_identity(&request.identity)?;
    if !request.user_presence_confirmed {
        return Err(recovery_error(
            HandleRecoveryErrorCode::HandleRecoveryUserPresenceRequired,
        ));
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
        let owner_entries = index
            .credentials
            .values()
            .filter(|entry| entry.unique_id == owner.id.as_str())
            .collect::<Vec<_>>();
        let account_handle_owner_closed = owner_entries.len() == 1
            && owner_entries[0].user_id == account_user_id
            && owner_entries[0].full_handle == canonical.full
            && request.did.as_str() == transition.current_did
            && exchanged.did.as_deref() == Some(request.did.as_str())
            && exchanged.handle.as_deref() == Some(canonical.full.as_str());
        if !account_handle_owner_closed || owner.did.as_str() != transition.previous_did {
            return Err(recovery_error(authorized_join_transition_error_code(
                account_handle_owner_closed,
                owner.did.as_str() == transition.previous_did,
                false,
            )));
        }
        let owner_identity_id = owner.id.as_str().to_owned();
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
        advance_joined_rebind(core, &join.session.join_session_id, &join.session.did).await?;
        let marker = crate::internal::identity_transition_pending::load_joined_device(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &join.session.join_session_id,
        )?
        .ok_or_else(|| recovery_error(HandleRecoveryErrorCode::HandleRecoveryTransitionMismatch))?;
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

pub(crate) async fn resume_authorized_join_activation(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<AuthorizedJoinActivationProgress> {
    require_enabled(core)?;
    let join = core
        .device_join()
        .poll_new_device_join(join_session_id)
        .await?;
    advance_joined_rebind(core, join_session_id, &join.session.did).await?;
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

async fn advance_joined_rebind(
    core: &crate::core::ImCore,
    join_session_id: &str,
    did: &crate::ids::Did,
) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let Some(marker) = crate::internal::identity_transition_pending::load_joined_device(
        sqlite_path,
        join_session_id,
    )?
    else {
        return Ok(());
    };
    if marker.phase
        != crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched
    {
        return Ok(());
    }
    let client = core
        .client_async(crate::identity::IdentitySelector::Did(did.clone()))
        .await?;
    let summary = client.groups().resume_rebind_recovery_async(100).await?;
    let (remaining, blocked) = crate::internal::group_rebind_recovery::handle_recovery_job_counts(
        sqlite_path,
        &marker.owner_identity_id,
        &marker.handle,
        &marker.previous_did,
        &marker.current_did,
        &marker.binding_generation,
    )?;
    if blocked > 0 || summary.blocked > 0 {
        return Err(recovery_error(
            HandleRecoveryErrorCode::HandleRecoveryBlocked,
        ));
    }
    if remaining == 0 {
        crate::internal::identity_transition_pending::update_phase(
            sqlite_path,
            &marker.recovery_id,
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
            crate::internal::identity_transition_pending::TransitionPhase::Completed,
        )?;
    }
    Ok(())
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

fn parse_remote_result(
    value: Value,
    pending: &PendingHandleRecovery,
) -> crate::ImResult<RecoveryRemoteResult> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        state: String,
        account_user_id: String,
        handle: String,
        previous_did: String,
        did: String,
        binding_generation: String,
        checkpoint: Checkpoint,
        bootstrap_device: Device,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Checkpoint {
        document_version: u64,
        document_hash: String,
        registry_version: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Device {
        device_id: String,
        status: String,
        role: String,
        management_ready: bool,
        auth_generation: u64,
    }
    let raw: Raw = serde_json::from_value(value).map_err(|_| crate::ImError::PermissionDenied)?;
    if raw.state != "recovered"
        || raw.account_user_id != pending.expected_account_user_id
        || raw.handle != pending.handle
        || raw.previous_did != pending.previous_did
        || raw.did != pending.generated.did.as_str()
        || raw.binding_generation.parse::<u128>().ok()
            != pending
                .expected_binding_generation
                .parse::<u128>()
                .ok()
                .and_then(|generation| generation.checked_add(1))
        || raw.bootstrap_device.device_id != pending.generated.protocol_device_id.as_str()
        || raw.bootstrap_device.status != "active"
        || raw.bootstrap_device.role != "admin"
        || !raw.bootstrap_device.management_ready
        || raw.bootstrap_device.auth_generation != 1
        || raw.checkpoint.document_version != 1
        || raw.checkpoint.registry_version != 1
        || raw.checkpoint.document_hash
            != crate::internal::identity_wire::document::document_hash(
                &pending.generated.did_document,
            )?
    {
        return Err(recovery_error(
            HandleRecoveryErrorCode::HandleRecoveryRemoteStateChanged,
        ));
    }
    Ok(RecoveryRemoteResult {
        account_user_id: raw.account_user_id,
        handle: raw.handle,
        previous_did: raw.previous_did,
        did: raw.did,
        binding_generation: raw.binding_generation,
        document_version: raw.checkpoint.document_version,
        document_hash: raw.checkpoint.document_hash,
        registry_version: raw.checkpoint.registry_version,
        bootstrap_device_id: raw.bootstrap_device.device_id,
        auth_generation: raw.bootstrap_device.auth_generation,
    })
}

fn progress(
    core: &crate::core::ImCore,
    pending: &PendingHandleRecovery,
) -> crate::ImResult<HandleRecoveryProgress> {
    let result = pending.remote_result.as_ref();
    let binding_generation = result.map(|result| result.binding_generation.clone());
    let reset_reference = result
        .map(|result| {
            Ok::<_, crate::ImError>(HandleRecoveryResetReference {
                account_user_id: result.account_user_id.clone(),
                owner_identity_id: pending.owner_identity_id.clone(),
                previous_did: crate::ids::Did::parse(&result.previous_did)?,
                current_did: crate::ids::Did::parse(&result.did)?,
                binding_generation: result.binding_generation.clone(),
                handle: pending.handle.clone(),
                source_kind: HandleRecoveryTransitionSourceKind::Initiator,
                source_id: pending.operation_id.clone(),
            })
        })
        .transpose()?;
    let (unsupported_e2ee_group_count, unsupported_did_only_group_count) =
        crate::internal::group_rebind_recovery::recovery_impact_counts(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &pending.owner_identity_id,
            &pending.previous_did,
        )?;
    Ok(HandleRecoveryProgress {
        recovery_id: pending.recovery_id.clone(),
        operation_id: pending.operation_id.clone(),
        owner_identity_id: crate::ids::IdentityId::parse(&pending.owner_identity_id)?,
        handle: pending.handle.clone(),
        previous_did: crate::ids::Did::parse(&pending.previous_did)?,
        current_did: pending.generated.did.clone(),
        binding_generation,
        phase: match pending.phase {
            PendingRecoveryPhase::Prepared => HandleRecoveryPhase::Prepared,
            PendingRecoveryPhase::RemoteCommitPending => HandleRecoveryPhase::RemoteCommitPending,
            PendingRecoveryPhase::RemoteCommitted => HandleRecoveryPhase::RemoteCommitted,
            PendingRecoveryPhase::IdentityTransitionPending => {
                HandleRecoveryPhase::IdentityTransitionPending
            }
            PendingRecoveryPhase::IdentitySwitched => HandleRecoveryPhase::IdentitySwitched,
            PendingRecoveryPhase::Completed => HandleRecoveryPhase::Completed,
            PendingRecoveryPhase::Blocked => HandleRecoveryPhase::Blocked,
        },
        impact: HandleRecoveryImpact {
            local_ordinary_data_will_migrate: true,
            other_devices_must_rejoin: true,
            unsupported_e2ee_group_count,
            unsupported_did_only_group_count,
        },
        reset_reference,
        blocked_code: pending.blocked_code.as_deref().and_then(public_error_code),
    })
}

fn require_enabled(core: &crate::core::ImCore) -> crate::ImResult<()> {
    if !core.inner().handle_recovery_enabled() {
        return Err(crate::ImError::unsupported("handle-recovery-v1"));
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

fn public_error_code(value: &str) -> Option<HandleRecoveryErrorCode> {
    [
        HandleRecoveryErrorCode::HandleRecoveryNotPrepared,
        HandleRecoveryErrorCode::HandleRecoveryUserPresenceRequired,
        HandleRecoveryErrorCode::HandleRecoveryTransitionMismatch,
        HandleRecoveryErrorCode::HandleRecoveryTransitionChainUnsupported,
        HandleRecoveryErrorCode::HandleRecoveryRemoteStateChanged,
        HandleRecoveryErrorCode::HandleRecoveryOutcomeUnknown,
        HandleRecoveryErrorCode::HandleRecoveryLocalStateUnavailable,
        HandleRecoveryErrorCode::HandleRecoveryBlocked,
    ]
    .into_iter()
    .find(|code| code.as_str() == value)
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

fn parse_timestamp(value: &str) -> crate::ImResult<time::OffsetDateTime> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn commit_proof_expires_at(
    created: time::OffsetDateTime,
    grant_expires: time::OffsetDateTime,
) -> crate::ImResult<time::OffsetDateTime> {
    let expires = std::cmp::min(created + time::Duration::minutes(5), grant_expires);
    if expires <= created {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(expires)
}

fn commit_proof_expired(value: &str, now: time::OffsetDateTime) -> crate::ImResult<bool> {
    Ok(parse_timestamp(value)? <= now)
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
        HandleRecoveryErrorCode::HandleRecoveryTransitionMismatch
    } else if !local_matches_previous_did {
        HandleRecoveryErrorCode::HandleRecoveryTransitionChainUnsupported
    } else {
        HandleRecoveryErrorCode::HandleRecoveryTransitionMismatch
    }
}

fn validate_switched_identity(
    core: &crate::core::ImCore,
    pending: &PendingHandleRecovery,
    result: &RecoveryRemoteResult,
) -> crate::ImResult<()> {
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let matches = index.credentials.values().filter(|entry| {
        entry.unique_id == pending.owner_identity_id
            && entry.user_id == result.account_user_id
            && entry.did == result.did
            && entry.full_handle == pending.handle
            && entry.binding_generation.as_deref() == Some(result.binding_generation.as_str())
    });
    if matches.count() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.as_bytes()[0] != b'0'
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn pending_for_remote_closure() -> PendingHandleRecovery {
        let generated = crate::internal::identity_generation::generate_handle_recovery_identity(
            "awiki.test",
            "alice",
            None,
            None,
        )
        .unwrap();
        PendingHandleRecovery::new(
            "recovery-1".to_owned(),
            "recover-001".to_owned(),
            "owner-1".to_owned(),
            "user-1".to_owned(),
            "alice".to_owned(),
            "Alice".to_owned(),
            true,
            "alice.awiki.test".to_owned(),
            "did:wba:awiki.test:users:alice-old".to_owned(),
            "7".to_owned(),
            "grant".to_owned(),
            "2026-08-03T00:05:00Z".to_owned(),
            generated,
        )
        .unwrap()
    }

    fn read_http_json(stream: &mut TcpStream) -> Value {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&buffer[..read]);
            let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let body_start = header_end + 4;
            if bytes.len() >= body_start + content_length {
                return serde_json::from_slice(&bytes[body_start..body_start + content_length])
                    .unwrap();
            }
        }
    }

    fn write_http_json(stream: &mut TcpStream, body: Value) {
        let body = serde_json::to_vec(&body).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
    }

    fn write_http_empty(stream: &mut TcpStream) {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        stream.flush().unwrap();
    }

    fn admin_access_token(did: &str, device_id: &str, signing_key_id: &str) -> String {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": did,
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": did,
            "user_id": "user-1",
            "device_id": device_id,
            "key_id": signing_key_id,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now,
            "nbf": now,
            "exp": now + 300,
            "jti": "recovery-facade-token"
        });
        format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
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

    fn scoped_pending(
        recovery_id: &str,
        operation_id: &str,
        owner_identity_id: &str,
        account_user_id: &str,
        local_part: &str,
    ) -> PendingHandleRecovery {
        let generated = crate::internal::identity_generation::generate_handle_recovery_identity(
            "awiki.test",
            local_part,
            None,
            None,
        )
        .unwrap();
        PendingHandleRecovery::new(
            recovery_id.to_owned(),
            operation_id.to_owned(),
            owner_identity_id.to_owned(),
            account_user_id.to_owned(),
            local_part.to_owned(),
            local_part.to_owned(),
            false,
            format!("{local_part}.awiki.test"),
            format!("did:wba:awiki.test:users:{local_part}-old"),
            "7".to_owned(),
            format!("grant-{local_part}"),
            "2026-08-03T00:05:00Z".to_owned(),
            generated,
        )
        .unwrap()
    }

    fn scoped_remote_result(pending: &PendingHandleRecovery) -> RecoveryRemoteResult {
        RecoveryRemoteResult {
            account_user_id: pending.expected_account_user_id.clone(),
            handle: pending.handle.clone(),
            previous_did: pending.previous_did.clone(),
            did: pending.generated.did.as_str().to_owned(),
            binding_generation: "8".to_owned(),
            document_version: 1,
            document_hash: crate::internal::identity_wire::document::document_hash(
                &pending.generated.did_document,
            )
            .unwrap(),
            registry_version: 1,
            bootstrap_device_id: pending.generated.protocol_device_id.as_str().to_owned(),
            auth_generation: 1,
        }
    }

    #[test]
    fn proof_exact_replay_remains_valid_for_two_to_five_minutes_only() {
        let created = time::OffsetDateTime::parse(
            "2026-08-03T00:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let grant_expires = created + time::Duration::minutes(5);
        let proof_expires = commit_proof_expires_at(created, grant_expires).unwrap();
        assert_eq!(proof_expires, grant_expires);
        assert!(!commit_proof_expired(
            &format_timestamp(proof_expires).unwrap(),
            created + time::Duration::minutes(3),
        )
        .unwrap());
        assert!(commit_proof_expired(
            &format_timestamp(proof_expires).unwrap(),
            created + time::Duration::minutes(5),
        )
        .unwrap());
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

    #[test]
    fn no_marker_closed_cross_generation_is_transition_chain_unsupported() {
        assert_eq!(
            authorized_join_transition_error_code(true, false, false),
            HandleRecoveryErrorCode::HandleRecoveryTransitionChainUnsupported
        );
        assert_eq!(
            authorized_join_transition_error_code(false, false, false),
            HandleRecoveryErrorCode::HandleRecoveryTransitionMismatch
        );
    }

    #[tokio::test]
    async fn recovery_two_identities_and_two_state_roots_isolate_lock_marker_vault_and_workspace() {
        let root_a = tempfile::tempdir().unwrap();
        let root_b = tempfile::tempdir().unwrap();
        let shared_vault = tempfile::tempdir().unwrap();
        let vault_key = [91_u8; 32];
        let core_a = recovery_test_core_with_vault_scope(
            root_a.path(),
            "http://127.0.0.1:9",
            vault_key,
            shared_vault.path().to_path_buf(),
            "workspace-alice",
            "device-alice",
        );
        let core_b = recovery_test_core_with_vault_scope(
            root_b.path(),
            "http://127.0.0.1:9",
            vault_key,
            shared_vault.path().to_path_buf(),
            "workspace-bob",
            "device-bob",
        );
        let pending_a = scoped_pending(
            "recovery-shared-id",
            "recover-alice-001",
            "owner-alice",
            "user-alice",
            "alice",
        );
        let pending_b = scoped_pending(
            "recovery-shared-id",
            "recover-bob-001",
            "owner-bob",
            "user-bob",
            "bob",
        );
        let store_a = PendingHandleRecoveryStore::from_core(&core_a).unwrap();
        let store_b = PendingHandleRecoveryStore::from_core(&core_b).unwrap();
        let secret_ref_a = store_a.save(&pending_a).unwrap();
        let secret_ref_b = store_b.save(&pending_b).unwrap();
        assert_eq!(secret_ref_a.workspace_id, "workspace-alice");
        assert_eq!(secret_ref_a.device_id, "device-alice");
        assert_eq!(secret_ref_b.workspace_id, "workspace-bob");
        assert_eq!(secret_ref_b.device_id, "device-bob");
        assert_ne!(secret_ref_a, secret_ref_b);

        let loaded_a = store_a.load("recovery-shared-id").unwrap().unwrap().1;
        let loaded_b = store_b.load("recovery-shared-id").unwrap().unwrap().1;
        assert_eq!(loaded_a.owner_identity_id, "owner-alice");
        assert_eq!(loaded_b.owner_identity_id, "owner-bob");
        assert_ne!(loaded_a.generated.did, loaded_b.generated.did);

        let marker_a =
            crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator(
                &core_a.inner().sdk_paths().local_state.sqlite_path,
                &pending_a,
                &scoped_remote_result(&pending_a),
            )
            .unwrap();
        let marker_b =
            crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator(
                &core_b.inner().sdk_paths().local_state.sqlite_path,
                &pending_b,
                &scoped_remote_result(&pending_b),
            )
            .unwrap();
        crate::internal::identity_transition_pending::persist(
            &core_a.inner().sdk_paths().local_state.sqlite_path,
            &marker_a,
        )
        .unwrap();
        crate::internal::identity_transition_pending::persist(
            &core_b.inner().sdk_paths().local_state.sqlite_path,
            &marker_b,
        )
        .unwrap();
        let loaded_marker_a = crate::internal::identity_transition_pending::load(
            &core_a.inner().sdk_paths().local_state.sqlite_path,
            "recovery-shared-id",
        )
        .unwrap()
        .unwrap();
        let loaded_marker_b = crate::internal::identity_transition_pending::load(
            &core_b.inner().sdk_paths().local_state.sqlite_path,
            "recovery-shared-id",
        )
        .unwrap()
        .unwrap();
        assert_eq!(loaded_marker_a.owner_identity_id, "owner-alice");
        assert_eq!(loaded_marker_b.owner_identity_id, "owner-bob");
        assert_ne!(
            loaded_marker_a.state_root_fingerprint,
            loaded_marker_b.state_root_fingerprint
        );

        let lock_a = core_a.inner().handle_recovery_lock("owner-alice");
        let lock_b = core_b.inner().handle_recovery_lock("owner-bob");
        let _guard_a = lock_a.try_lock().unwrap();
        let _guard_b = lock_b.try_lock().unwrap();
    }

    #[test]
    fn remote_result_closes_account_and_generated_document_hash() {
        let pending = pending_for_remote_closure();
        let document_hash = crate::internal::identity_wire::document::document_hash(
            &pending.generated.did_document,
        )
        .unwrap();
        let result = json!({
            "state": "recovered",
            "account_user_id": "user-1",
            "handle": pending.handle.as_str(),
            "previous_did": pending.previous_did.as_str(),
            "did": pending.generated.did.as_str(),
            "binding_generation": "8",
            "checkpoint": {
                "document_version": 1,
                "document_hash": document_hash,
                "registry_version": 1
            },
            "bootstrap_device": {
                "device_id": pending.generated.protocol_device_id.as_str(),
                "status": "active",
                "role": "admin",
                "management_ready": true,
                "auth_generation": 1
            }
        });
        assert!(parse_remote_result(result.clone(), &pending).is_ok());
        let mut wrong_account = result.clone();
        wrong_account["account_user_id"] = json!("user-other");
        assert!(parse_remote_result(wrong_account, &pending).is_err());
        let mut wrong_hash = result;
        wrong_hash["checkpoint"]["document_hash"] = json!("sha256:wrong");
        assert!(parse_remote_result(wrong_hash, &pending).is_err());
    }

    #[tokio::test]
    async fn production_facade_exact_replays_after_empty_success_response_and_process_reopen() {
        let old = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.test", "alice", None, None,
        )
        .unwrap();
        let old_did_for_server = old.did.as_str().to_owned();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let commit_attempts = Arc::new(Mutex::new(Vec::<Value>::new()));
        let activation_count = Arc::new(AtomicUsize::new(0));
        let attempts_for_server = commit_attempts.clone();
        let activations_for_server = activation_count.clone();
        let server = std::thread::spawn(move || {
            let grant_expires =
                format_timestamp(time::OffsetDateTime::now_utc() + time::Duration::minutes(5))
                    .unwrap();
            let (mut exchange, _) = listener.accept().unwrap();
            let _ = read_http_json(&mut exchange);
            write_http_json(
                &mut exchange,
                json!({
                    "recovery_grant": "grant-reopen-1",
                    "purpose": crate::internal::identity_wire::handle_recovery::HANDLE_RECOVERY_PURPOSE,
                    "expires_at": grant_expires,
                }),
            );

            let (mut first_commit, _) = listener.accept().unwrap();
            let first = read_http_json(&mut first_commit);
            let durable_operation_id = first["params"]["operation_id"].as_str().unwrap().to_owned();
            activations_for_server.fetch_add(1, Ordering::SeqCst);
            attempts_for_server.lock().unwrap().push(first);
            write_http_empty(&mut first_commit);

            let (mut second_commit, _) = listener.accept().unwrap();
            let second = read_http_json(&mut second_commit);
            if second["params"]["operation_id"].as_str() != Some(durable_operation_id.as_str()) {
                activations_for_server.fetch_add(1, Ordering::SeqCst);
            }
            attempts_for_server.lock().unwrap().push(second.clone());
            let params = &second["params"];
            let document = params["new_did_document"].clone();
            let did = document["id"].as_str().unwrap();
            let document_hash =
                crate::internal::identity_wire::document::document_hash(&document).unwrap();
            write_http_json(
                &mut second_commit,
                json!({
                    "jsonrpc": "2.0",
                    "id": "req-1",
                    "result": {
                        "state": "recovered",
                        "account_user_id": "user-1",
                        "handle": "alice.awiki.test",
                        "previous_did": old_did_for_server,
                        "did": did,
                        "binding_generation": "8",
                        "checkpoint": {
                            "document_version": 1,
                            "document_hash": document_hash,
                            "registry_version": 1
                        },
                        "bootstrap_device": {
                            "device_id": params["bootstrap_device_id"],
                            "status": "active",
                            "role": "admin",
                            "management_ready": true,
                            "auth_generation": 1
                        }
                    }
                }),
            );

            let access_token = admin_access_token(
                did,
                params["bootstrap_device_id"].as_str().unwrap(),
                params["bootstrap_device_proof"]["key_id"].as_str().unwrap(),
            );
            let (mut refresh, _) = listener.accept().unwrap();
            let refresh_request = read_http_json(&mut refresh);
            assert_eq!(refresh_request["method"].as_str(), Some("get_me"));
            write_http_json(
                &mut refresh,
                json!({
                    "jsonrpc": "2.0",
                    "id": refresh_request["id"],
                    "result": { "access_token": access_token }
                }),
            );

            let (mut p5, _) = listener.accept().unwrap();
            let p5_request = read_http_json(&mut p5);
            let body = &p5_request["params"]["body"];
            let bundle = &body["prekey_bundle"];
            write_http_json(
                &mut p5,
                json!({
                    "jsonrpc": "2.0",
                    "id": p5_request["id"],
                    "result": {
                        "published": true,
                        "owner_did": bundle["owner_did"],
                        "owner_device_id": bundle["owner_device_id"],
                        "bundle_id": bundle["bundle_id"],
                        "published_at": "2026-08-03T00:00:00Z",
                        "published_opk_count": body["one_time_prekeys"].as_array().unwrap().len()
                    }
                }),
            );
        });

        let root = tempfile::tempdir().unwrap();
        let vault_key = [73_u8; 32];
        let core = recovery_test_core(root.path(), &endpoint, vault_key);
        let old_did = old.did.clone();
        let owner_id = old.unique_id.clone();
        let document_hash =
            crate::internal::identity_wire::document::document_hash(&old.did_document).unwrap();
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .save_identity_with_secret_storage(
                crate::internal::identity_store::SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did: old.did.clone(),
                    unique_id: old.unique_id.clone(),
                    user_id: "user-1".to_owned(),
                    display_name: "Alice".to_owned(),
                    handle: "alice".to_owned(),
                    full_handle: "alice.awiki.test".to_owned(),
                    binding_generation: Some("7".to_owned()),
                    jwt_token: "old-token".to_owned(),
                    did_document: Some(old.did_document),
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                        root_key_id: old.root_key_id.clone(),
                        device_signing_key_id: old.device_signing_key_id.clone(),
                        device_e2ee_key_id: old.device_e2ee_key_id.clone(),
                    },
                    device_state: Some(crate::internal::identity_device_state::IdentityDeviceState {
                        schema_version: crate::internal::identity_device_state::IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                        mode: crate::internal::identity_device_state::IdentityDeviceMode::VNext,
                        authorization: Some(crate::internal::identity_device_state::DeviceAuthorizationProjection {
                            protocol_device_id: old.protocol_device_id.clone(),
                            signing_key_id: old.device_signing_key_id.clone(),
                            e2ee_key_id: old.device_e2ee_key_id.clone(),
                            status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                            management_ready: true,
                            auth_generation: 1,
                        }),
                        checkpoint: Some(crate::internal::identity_device_state::IdentityInternalCheckpoint {
                            document_version: 1,
                            document_hash,
                            registry_version: 1,
                        }),
                    }),
                    key1_private_pem: old.root_private_pem,
                    key1_public_pem: old.root_public_pem,
                    e2ee_signing_private_pem: old.device_signing_private_pem,
                    e2ee_agreement_private_pem: old.device_e2ee_private_pem,
                    daemon_subkey_package: None,
                    make_default: true,
                },
                crate::internal::identity_store::SaveIdentitySecretStorage::from_core(&core)
                    .unwrap(),
            )
            .unwrap();
        let db = crate::internal::local_state::open_writable(
            &core.inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        db.execute(
            "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES (?1,'user-1','alice.awiki.test',?2,'old-device','7','1',1,1)",
            rusqlite::params![owner_id, old_did.as_str()],
        )
        .unwrap();
        drop(db);
        let prepared = core
            .handle_recovery()
            .prepare_handle_recovery(HandleRecoveryPrepareRequest {
                identity: crate::identity::IdentitySelector::Did(old_did.clone()),
                phone: "+8613800000000".to_owned(),
                code: "123456".to_owned(),
                handle: "alice.awiki.test".to_owned(),
                operation_id: "recover-reopen-001".to_owned(),
            })
            .await
            .unwrap();
        let first_error = core
            .handle_recovery()
            .activate_handle_recovery(HandleRecoveryActivateRequest {
                recovery_id: prepared.recovery_id.clone(),
                user_presence_confirmed: true,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            first_error,
            crate::ImError::Service { code: Some(code), .. }
                if code == HandleRecoveryErrorCode::HandleRecoveryOutcomeUnknown.as_str()
        ));
        drop(core);

        let reopened = recovery_test_core(root.path(), &endpoint, vault_key);
        let resumed = reopened
            .handle_recovery()
            .resume_handle_recovery(HandleRecoveryResumeRequest {
                recovery_id: prepared.recovery_id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(resumed.phase, HandleRecoveryPhase::Completed);
        let resumed_again = reopened
            .handle_recovery()
            .resume_handle_recovery(HandleRecoveryResumeRequest {
                recovery_id: prepared.recovery_id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(resumed_again.phase, HandleRecoveryPhase::Completed);
        server.join().unwrap();
        let attempts = commit_attempts.lock().unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0]["params"], attempts[1]["params"]);
        assert_eq!(activation_count.load(Ordering::SeqCst), 1);
        let recovered_did = attempts[1]["params"]["new_did_document"]["id"]
            .as_str()
            .unwrap();
        let index = crate::internal::identity_store::IdentityStore::new(
            &reopened.inner().sdk_paths().identities,
        )
        .load_index()
        .unwrap();
        let stable = index
            .credentials
            .values()
            .filter(|entry| entry.unique_id == owner_id)
            .collect::<Vec<_>>();
        assert_eq!(stable.len(), 1);
        assert_eq!(stable[0].user_id, "user-1");
        assert_eq!(stable[0].full_handle, "alice.awiki.test");
        assert_eq!(stable[0].did, recovered_did);
        assert_eq!(stable[0].binding_generation.as_deref(), Some("8"));
        let marker = crate::internal::identity_transition_pending::load(
            &reopened.inner().sdk_paths().local_state.sqlite_path,
            &prepared.recovery_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            marker.phase,
            crate::internal::identity_transition_pending::TransitionPhase::Completed
        );
        assert_eq!(marker.account_user_id, stable[0].user_id);
        assert_eq!(marker.owner_identity_id, stable[0].unique_id);
        assert_eq!(marker.handle, stable[0].full_handle);
        assert_eq!(marker.current_did, stable[0].did);
        assert_eq!(
            marker.binding_generation,
            stable[0].binding_generation.as_deref().unwrap()
        );
        let vault_refs = stable[0]
            .vault_migration
            .as_ref()
            .and_then(|metadata| metadata.vnext_refs.as_ref())
            .unwrap();
        assert!(vault_refs.did_document_root_private.is_some());
        let recovered = reopened
            .client(crate::identity::IdentitySelector::Did(
                crate::ids::Did::parse(recovered_did).unwrap(),
            ))
            .unwrap();
        let token = recovered
            .runtime()
            .key_provider
            .valid_auth_token()
            .unwrap()
            .unwrap();
        let authorization = stable[0]
            .device_state
            .as_ref()
            .and_then(|state| state.authorization.as_ref())
            .unwrap();
        crate::internal::access_token::validate_device_access_token(
            &token,
            &crate::internal::access_token::ExpectedDeviceAccess {
                did: recovered_did,
                user_id: "user-1",
                device_id: authorization.protocol_device_id.as_str(),
                key_id: &authorization.signing_key_id,
                auth_generation: authorization.auth_generation,
                role: authorization.role,
                management_ready: authorization.management_ready,
            },
        )
        .unwrap();
    }
}
