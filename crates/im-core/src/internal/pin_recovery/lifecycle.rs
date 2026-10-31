use super::*;
pub(crate) async fn confirm_device(
    core: &crate::ImCore,
    request: ConfirmRecoveredDeviceRequest,
) -> crate::ImResult<PinRecoveryProgress> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut o = store.operation(&request.operation_id.0)?;
    let mut s = store.session(&o.session_id)?;
    if !request.user_presence.is_confirmed()
        || o.kind != PinRecoveryKind::Recover
        || o.custody_pending.is_none()
    {
        return Err(failure("action_not_allowed", false));
    }
    refresh(core, &store, &mut s, &mut o).await?;
    if o.remote.as_ref().is_some_and(|r| r.device_result.is_some()) {
        activation::activate_local(core, &store, &s, &mut o).await?;
        return Ok(progress(&o));
    }
    custody::synchronize_pending(core, &store, &s, &mut o).await?;
    if !progress(&o)
        .allowed_actions
        .contains(&PinRecoveryAction::ConfirmDevice)
    {
        return Err(failure("action_not_allowed", false));
    }
    if o.commit_body.is_none() {
        let proposal = custody::proposal(&o)?;
        if proposal.phase != "prepared" {
            return Err(failure("reconciliation_required", true));
        }
        let token = phone_token(&s)?;
        let body = json!({"proposal":proposal.device_proposal});
        let key = format!("{}:device:challenge:{}", o.id, uuid());
        let challenge: wire::DeviceChallengeResponse = http::request(
            core,
            &store,
            &mut s,
            &key,
            "POST",
            &format!("operations/{}/device/challenge", o.id),
            Some(&body),
            Some(&token),
        )
        .await?;
        if challenge.proposal
            != serde_json::from_value(proposal.device_proposal)
                .map_err(|_| failure("corrupt_pending", false))?
        {
            return Err(failure("intent_mismatch", false));
        }
        validate_challenge(
            &s,
            &o,
            &challenge.challenge,
            "awiki.pin-recovery.device-commit.v1",
        )?;
        verify_receipt_ids(&o, &challenge.challenge.intent)?;
        let proposal_hash = crypto::hash(&crypto::canonical(&challenge.proposal)?);
        let mut candidate = challenge.candidate_document.clone();
        candidate
            .as_object_mut()
            .ok_or_else(|| failure("invalid_wire", false))?
            .remove("proof");
        if challenge
            .challenge
            .intent
            .new_device_proposal_hash
            .as_deref()
            != Some(&proposal_hash)
            || challenge
                .challenge
                .intent
                .candidate_document_digest
                .as_deref()
                != Some(crypto::hash(&crypto::canonical(&candidate)?).as_str())
        {
            return Err(failure("intent_mismatch", false));
        }
        o.device_challenge = Some(challenge);
        o.phase = PinRecoveryPhase::CommitUnknown;
        store.save_operation(&o)?;
        o.commit_body = Some(custody::sign_device(core, &s, &mut o).await?);
        store.save_operation(&o)?;
    }
    send_device_commit(core, &store, &mut s, &mut o).await?;
    Ok(progress(&o))
}
async fn send_device_commit(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let body = o
        .commit_body
        .clone()
        .ok_or_else(|| failure("reconciliation_required", true))?;
    o.commit_attempted = true;
    o.phase = PinRecoveryPhase::CommitUnknown;
    store.save_operation(o)?;
    let token = phone_token(s)?;
    let r = http::request(
        core,
        store,
        s,
        &format!(
            "{}:device:commit:{}",
            o.id,
            o.device_challenge
                .as_ref()
                .ok_or_else(|| failure("corrupt_pending", false))?
                .challenge
                .payload
                .challenge_id
        ),
        "POST",
        &format!("operations/{}/device/commit", o.id),
        Some(&body),
        Some(&token),
    )
    .await?;
    fault::cut("device_commit_response_received", o)?;
    accept_remote(o, s, r)?;
    store.save_operation(o)?;
    if o.remote.as_ref().is_none_or(|r| r.device_result.is_none()) {
        return Err(failure("commit_unknown", true));
    }
    refresh(core, store, s, o).await?;
    activation::activate_local(core, store, s, o).await
}
pub(crate) async fn status(
    core: &crate::ImCore,
    id: PinRecoveryOperationId,
) -> crate::ImResult<PinRecoveryProgress> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut o = store.operation(&id.0)?;
    if o.phase == PinRecoveryPhase::Cancelled {
        return Ok(progress(&o));
    }
    let mut s = store.session(&o.session_id)?;
    match refresh(core, &store, &mut s, &mut o).await {
        Ok(()) => {}
        Err(e) if error_code(&e) == "phone_verification_required" => {
            o.last_error = Some(PinRecoveryError {
                code: "phone_verification_required".into(),
                retryable: false,
                retry_after_seconds: None,
            });
        }
        Err(e)
            if matches!(
                error_code(&e),
                "device_revoked" | "device_authorization_changed"
            ) =>
        {
            o.phase = PinRecoveryPhase::Failed;
            o.last_error = Some(PinRecoveryError {
                code: error_code(&e).into(),
                retryable: false,
                retry_after_seconds: None,
            });
            store.save_operation(&o)?;
        }
        Err(e) => return Err(e),
    }
    if !o
        .last_error
        .as_ref()
        .is_some_and(|e| e.code == "phone_verification_required")
        && o.remote.as_ref().is_some_and(|r| r.device_result.is_none())
    {
        custody::synchronize_pending(core, &store, &s, &mut o).await?;
    }
    Ok(progress(&o))
}
pub(crate) async fn resume(
    core: &crate::ImCore,
    request: ResumePinRecoveryRequest,
) -> crate::ImResult<PinRecoveryProgress> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut o = store.operation(&request.operation_id.0)?;
    if o.phase == PinRecoveryPhase::Cancelled {
        return Ok(progress(&o));
    }
    let mut s = store.session(&o.session_id)?;
    if let Some(handle) = request.session {
        let mut new = store.session(&handle.0)?;
        if !new.selected.as_ref().is_some_and(|t| {
            t.target_id == o.target.target_id
                && t.did == o.target.did
                && t.full_handle == o.target.full_handle
                && t.binding_generation == o.target.binding_generation
                && t.root_kid == o.target.root_kid
                && t.root_public_jwk == o.target.root_public_jwk
        }) || new.tenant_id != s.tenant_id
            || new.config_hash != s.config_hash
            || http::jkt(&new.dpop_private)? != http::jkt(&s.dpop_private)?
        {
            return Err(failure("binding_changed", false));
        }
        let token = phone_token(&new)?;
        let r = http::request(
            core,
            &store,
            &mut new,
            &format!("{}:resume:{}", o.id, handle.0),
            "POST",
            &format!("operations/{}/resume", o.id),
            Some(&json!({})),
            Some(&token),
        )
        .await?;
        accept_remote(&mut o, &new, r)?;
        o.session_id = new.id.clone();
        s = new;
        store.save_operation(&o)?;
    }
    if o.remote.is_none() {
        create_operation(core, &store, &mut s, &mut o).await?;
    } else {
        refresh(core, &store, &mut s, &mut o).await?;
    }
    if o.remote.as_ref().is_some_and(|r| {
        matches!(r.state.as_str(), "cancelled" | "expired") && r.device_result.is_none()
    }) {
        custody::cancel_prepared(core, &store, &s, &mut o).await?;
        clear_ephemeral(&mut o);
        o.phase = PinRecoveryPhase::Cancelled;
        if o.remote.as_ref().is_some_and(|r| r.state == "expired") {
            o.last_error = Some(PinRecoveryError {
                code: "operation_expired".into(),
                retryable: false,
                retry_after_seconds: None,
            });
        }
        store.save_operation(&o)?;
        return Ok(progress(&o));
    }
    if o.kind == PinRecoveryKind::Enable
        && o.remote
            .as_ref()
            .is_some_and(|r| r.state == "active" || r.state == "active_cleanup_pending")
    {
        clear_ephemeral(&mut o);
        o.phase = PinRecoveryPhase::Completed;
        store.save_operation(&o)?;
        return Ok(progress(&o));
    }
    if o.kind == PinRecoveryKind::Recover
        && o.remote.as_ref().is_some_and(|r| r.device_result.is_some())
    {
        activation::activate_local(core, &store, &s, &mut o).await?;
        return Ok(progress(&o));
    }
    if o.kind == PinRecoveryKind::Enable
        && o.remote
            .as_ref()
            .is_some_and(|r| r.allowed_actions.iter().any(|a| a == "authorize"))
    {
        authorize_backup(core, &store, &mut s, &mut o).await?;
    }
    if o.kind == PinRecoveryKind::Recover && o.envelope.is_none() {
        download_backup(core, &store, &mut s, &mut o).await?;
    }
    if o.kind == PinRecoveryKind::Enable
        && o.remote.as_ref().is_some_and(|r| {
            r.registered_realms.len() == 3
                && r.verified_realms.len() >= 2
                && r.commit_challenge.is_some()
        })
    {
        commit_backup(core, &store, &mut s, &mut o).await?;
        return Ok(progress(&o));
    }
    custody::synchronize_pending(core, &store, &s, &mut o).await?;
    if o.custody_pending.is_some() {
        let proposal = custody::proposal(&o)?;
        if proposal.phase == "publication_uncertain" {
            let c = o
                .device_challenge
                .as_ref()
                .ok_or_else(|| failure("reconciliation_required", true))?;
            if c.challenge.payload.exp.saturating_add(5) >= crypto::now() {
                if o.commit_body.is_some() {
                    send_device_commit(core, &store, &mut s, &mut o).await?;
                    return Ok(progress(&o));
                }
                return Err(failure("reconciliation_required", true));
            }
            if o.last_status_query_started_at
                .is_none_or(|started| started <= c.challenge.payload.exp.saturating_add(5))
            {
                refresh(core, &store, &mut s, &mut o).await?;
                if o.remote.as_ref().is_some_and(|r| r.device_result.is_some()) {
                    activation::activate_local(core, &store, &s, &mut o).await?;
                    return Ok(progress(&o));
                }
                custody::synchronize_pending(core, &store, &s, &mut o).await?;
            }
            custody::reconcile_expired_challenge(core, &s, &mut o).await?;
        }
        o.phase = PinRecoveryPhase::AwaitingDeviceConfirmation;
        store.save_operation(&o)?;
        return Ok(progress(&o));
    }
    if o.recovered_root_b64u.is_some() {
        submit_receipts(core, &store, &mut s, &mut o).await?;
        custody::begin(core, &s, &mut o).await?;
        o.phase = PinRecoveryPhase::AwaitingDeviceConfirmation;
        store.save_operation(&o)?;
        return Ok(progress(&o));
    }
    if o.kind == PinRecoveryKind::Enable
        && o.registration_payloads.len() == 3
        && o.remote
            .as_ref()
            .is_some_and(|r| r.registered_realms.len() < 3)
    {
        refresh_tokens(core, &store, &mut s, &mut o).await?;
        let adapter = adapter::Adapter::new(core, &store, s, o);
        let result = adapter.resume_register().await;
        let (mut s, mut o) = adapter.snapshot().await;
        result?;
        submit_receipts(core, &store, &mut s, &mut o).await?;
        o.phase = PinRecoveryPhase::AwaitingPin;
        store.save_operation(&o)?;
        return Ok(progress(&o));
    }
    if o.kind == PinRecoveryKind::Enable && !o.registration_payloads.is_empty() {
        return Err(failure("registration_restart_required", false));
    }
    // New blind input requires a new explicit PIN submission, never resume.
    o.attempt_running = false;
    o.phase = PinRecoveryPhase::AwaitingPin;
    store.save_operation(&o)?;
    Ok(progress(&o))
}
pub(crate) async fn cancel(
    core: &crate::ImCore,
    id: PinRecoveryOperationId,
) -> crate::ImResult<PinRecoveryProgress> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut o = store.operation(&id.0)?;
    if o.phase == PinRecoveryPhase::Cancelled {
        return Ok(progress(&o));
    }
    let mut s = store.session(&o.session_id)?;
    refresh(core, &store, &mut s, &mut o).await?;
    custody::synchronize_pending(core, &store, &s, &mut o).await?;
    if custody_uncertain(&o) {
        return Err(failure("reconciliation_required", true));
    }
    if o.commit_attempted
        || o.phase == PinRecoveryPhase::CommitUnknown
        || o.recovered_identity.is_some()
    {
        return Err(failure("reconciliation_required", true));
    }
    let token = phone_token(&s)?;
    let r = http::request(
        core,
        &store,
        &mut s,
        &format!("{}:cancel", o.id),
        "POST",
        &format!("operations/{}/cancel", o.id),
        Some(&json!({})),
        Some(&token),
    )
    .await?;
    accept_remote(&mut o, &s, r)?;
    if o.remote.as_ref().is_none_or(|r| r.state != "cancelled") {
        return Err(failure("reconciliation_required", true));
    }
    custody::cancel_prepared(core, &store, &s, &mut o).await?;
    clear_ephemeral(&mut o);
    o.phase = PinRecoveryPhase::Cancelled;
    store.save_operation(&o)?;
    Ok(progress(&o))
}
pub(super) fn progress(o: &Operation) -> PinRecoveryProgress {
    let mut actions = vec![PinRecoveryAction::Inspect];
    match o.phase {
        PinRecoveryPhase::AwaitingPin => {
            actions.extend([PinRecoveryAction::SubmitPin, PinRecoveryAction::Cancel])
        }
        PinRecoveryPhase::AwaitingDeviceConfirmation => {
            actions.extend([PinRecoveryAction::ConfirmDevice, PinRecoveryAction::Cancel])
        }
        PinRecoveryPhase::Completed | PinRecoveryPhase::Cancelled => {}
        PinRecoveryPhase::LocalActivationPending => {
            actions.push(PinRecoveryAction::RetryLocalActivation)
        }
        _ => actions.push(PinRecoveryAction::Resume),
    }
    if let Some(remote) = o.remote.as_ref() {
        if !matches!(
            o.phase,
            PinRecoveryPhase::Completed
                | PinRecoveryPhase::Cancelled
                | PinRecoveryPhase::CommitUnknown
        ) && !o.commit_attempted
            && remote.allowed_actions.iter().any(|a| a == "cancel")
            && !actions.contains(&PinRecoveryAction::Cancel)
        {
            actions.push(PinRecoveryAction::Cancel);
        }
        actions.retain(|action| match action {
            PinRecoveryAction::SubmitPin => remote.allowed_actions.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "recover" | "register" | "verify_stage" | "upload_backup"
                )
            }),
            PinRecoveryAction::ConfirmDevice => remote
                .allowed_actions
                .iter()
                .any(|a| matches!(a.as_str(), "prepare_device" | "commit_device")),
            PinRecoveryAction::Cancel => {
                !o.commit_attempted && remote.allowed_actions.iter().any(|a| a == "cancel")
            }
            _ => true,
        });
    }
    if o.recovered_root_b64u.is_some() || (o.custody_ingress_started && o.custody_pending.is_none())
    {
        actions.retain(|a| *a != PinRecoveryAction::SubmitPin);
        if !actions.contains(&PinRecoveryAction::Resume) {
            actions.push(PinRecoveryAction::Resume);
        }
    }
    if custody_uncertain(o) {
        actions.retain(|a| matches!(a, PinRecoveryAction::Inspect | PinRecoveryAction::Resume));
        if !actions.contains(&PinRecoveryAction::Resume) {
            actions.push(PinRecoveryAction::Resume);
        }
    }
    if o.last_error.as_ref().is_some_and(|e| {
        matches!(
            e.code.as_str(),
            "device_revoked" | "device_authorization_changed"
        )
    }) {
        actions.retain(|a| *a == PinRecoveryAction::Inspect);
    }
    let needs_phone = o
        .last_error
        .as_ref()
        .is_some_and(|e| e.code == "phone_verification_required");
    if needs_phone {
        actions.retain(|a| *a == PinRecoveryAction::Inspect);
        actions.push(PinRecoveryAction::VerifyPhone);
    }
    let r = o.remote.as_ref();
    PinRecoveryProgress {
        operation_id: PinRecoveryOperationId(o.id.clone()),
        kind: o.kind,
        phase: o.phase,
        allowed_actions: actions,
        target: target_public(&o.target),
        node_progress: o
            .realm_ids
            .iter()
            .map(|id| RealmProgress {
                realm_id: id.clone(),
                phase: if r.is_some_and(|r| r.verified_realms.contains(id)) {
                    "verified"
                } else if r.is_some_and(|r| r.registered_realms.contains(id)) {
                    "registered"
                } else {
                    "not_started"
                }
                .into(),
                error_code: None,
            })
            .collect(),
        attempts_used: r.map(|r| r.guesses_used as u16).unwrap_or(0),
        attempts_limit: 10,
        needs_phone_verification: needs_phone,
        needs_user_confirmation: o.phase == PinRecoveryPhase::AwaitingDeviceConfirmation,
        recovered_identity: o.recovered_identity.clone(),
        new_device_id: o
            .custody_pending
            .as_ref()
            .and_then(|p| p["device_proposal"]["device_id"].as_str())
            .map(str::to_owned),
        device_label: (o.kind == PinRecoveryKind::Recover).then(|| o.device_label.clone()),
        secure_readiness: o.secure_readiness,
        cleanup_remaining: r.map(|r| r.cleanup_remaining as u8).unwrap_or(0),
        error: o.last_error.clone(),
    }
}

pub(crate) async fn list_pending(
    core: &crate::ImCore,
) -> crate::ImResult<Vec<PinRecoveryProgress>> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut result = Vec::new();
    for o in store.list_operations()? {
        if matches!(
            o.phase,
            PinRecoveryPhase::Completed | PinRecoveryPhase::Cancelled
        ) {
            continue;
        }
        let s = store.session(&o.session_id)?;
        if trusted(core, &s.tenant_id).is_ok() {
            result.push(progress(&o));
        }
    }
    Ok(result)
}

fn custody_uncertain(o: &Operation) -> bool {
    o.custody_pending
        .as_ref()
        .and_then(|p| p.get("phase"))
        .and_then(|p| p.as_str())
        == Some("publication_uncertain")
}
