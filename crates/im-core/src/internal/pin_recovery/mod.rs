//! Single owner of phone/PST, DPoP, Juicebox, Vault pending and activation.
mod activation;
mod adapter;
pub(crate) mod backup_status;
mod lifecycle;
use lifecycle::progress;
pub(crate) use lifecycle::{cancel, confirm_device, list_pending, resume, status};
mod crypto;
mod custody;
mod fault;
mod http;
pub(crate) mod pending;
#[cfg(test)]
mod tests;
pub(crate) mod v4_cleanup;
mod wire;
use crate::pin_recovery::*;
use pending::{Operation, Session, Store};
use serde_json::json;
use std::collections::BTreeMap;

pub(crate) fn failure(code: &str, retryable: bool) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.into()),
        message: "PIN recovery could not complete".into(),
        data: Some(json!({"retryable":retryable})),
    }
}
pub(crate) fn error_code(error: &crate::ImError) -> &str {
    match error {
        crate::ImError::Service { code: Some(c), .. } => c.as_str(),
        crate::ImError::PermissionDenied => "permission_denied",
        _ => "local_state_unavailable",
    }
}
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn phone_token(session: &Session) -> crate::ImResult<zeroize::Zeroizing<String>> {
    let pst = session
        .pst
        .as_ref()
        .ok_or_else(|| failure("phone_verification_required", false))?;
    if !future(&pst.expires_at) {
        return Err(failure("phone_verification_required", false));
    }
    Ok(zeroize::Zeroizing::new(pst.access_token.clone()))
}
fn discovery_token(session: &Session) -> crate::ImResult<zeroize::Zeroizing<String>> {
    let s = session
        .discovery
        .as_ref()
        .ok_or_else(|| failure("phone_verification_required", false))?;
    if !future(&s.expires_at) {
        return Err(failure("phone_verification_required", false));
    }
    Ok(zeroize::Zeroizing::new(s.discovery_token.clone()))
}
fn future(raw: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(raw).is_ok_and(|t| t.timestamp() > crypto::now() as i64)
}
fn target_public(t: &wire::Target) -> PinRecoveryTarget {
    PinRecoveryTarget {
        target_id: RecoveryTargetId(t.target_id.clone()),
        full_handle: t.full_handle.clone(),
        did: t.did.clone(),
        binding_generation: t.binding_generation.clone(),
        active_epoch: t.active_epoch,
        can_enable: t.can_enable,
        can_recover: t.can_recover,
        unavailable_reason: t.unavailable_reason.clone(),
    }
}
fn trusted<'a>(
    core: &'a crate::ImCore,
    tenant: &str,
) -> crate::ImResult<&'a PinRecoveryTenantConfig> {
    let configs = core
        .inner()
        .pin_recovery_tenants
        .iter()
        .filter(|c| c.tenant_id == tenant)
        .collect::<Vec<_>>();
    if configs.len() != 1 {
        return Err(failure("capability_unavailable", false));
    }
    Ok(configs[0])
}
async fn fetch_config(
    core: &crate::ImCore,
    tenant: &str,
) -> crate::ImResult<(wire::ConfigurationPayload, String)> {
    let t = trusted(core, tenant)?;
    crypto::validate_url(
        &t.control_base_url,
        core.inner()
            .external_http_allow_insecure_loopback_for_testing(),
    )?;
    let headers = http::control_version_headers(core);
    let response = core
        .network()
        .send(
            "GET",
            &format!("{}/config", t.control_base_url.trim_end_matches('/')),
            headers,
            Vec::new(),
            false,
        )
        .await
        .map_err(|_| failure("network_unavailable", true))?;
    if response.status().as_u16() != 200 || response.content_length().is_some_and(|s| s > 32768) {
        return Err(failure("configuration_unavailable", true));
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("network_unavailable", true))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > 32768 {
            return Err(failure("invalid_configuration", false));
        }
    }
    crypto::verify_config(
        serde_json::from_slice(&bytes).map_err(|_| failure("invalid_configuration", false))?,
        t,
        core.inner()
            .external_http_allow_insecure_loopback_for_testing(),
    )
}
pub(crate) async fn request_phone_code(
    core: &crate::ImCore,
    request: PhoneChallengeRequest,
) -> crate::ImResult<PhoneChallengeSummary> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let (config, config_hash) = fetch_config(core, &request.tenant_id).await?;
    let phone = request.phone.as_bytes();
    if !(8..=16).contains(&phone.len())
        || phone[0] != b'+'
        || phone[1] == b'0'
        || !phone[1..].iter().all(u8::is_ascii_digit)
    {
        return Err(failure("invalid_phone", false));
    }
    let mut session = if let Some(id) = &request.resume_operation_id {
        let op = store.operation(&id.0)?;
        let old = store.session(&op.session_id)?;
        if old.tenant_id != request.tenant_id || old.config_hash != config_hash {
            return Err(failure("binding_changed", false));
        }
        Session {
            id: uuid(),
            tenant_id: old.tenant_id.clone(),
            config,
            config_hash,
            dpop_private: old.dpop_private.clone(),
            remote_challenge: None,
            resume_operation_id: Some(id.0.clone()),
            discovery: None,
            pst: None,
            targets: Vec::new(),
            selected: None,
            nonces: old.nonces.clone(),
            exact_requests: old
                .exact_requests
                .iter()
                .filter(|(k, _)| {
                    !k.starts_with("phone/") && !matches!(k.as_str(), "targets" | "select-target")
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    } else {
        Session {
            id: uuid(),
            tenant_id: request.tenant_id,
            config,
            config_hash,
            dpop_private: http::generate_key(),
            remote_challenge: None,
            resume_operation_id: None,
            discovery: None,
            pst: None,
            targets: Vec::new(),
            selected: None,
            nonces: BTreeMap::new(),
            exact_requests: BTreeMap::new(),
        }
    };
    store.save_session(&session)?;
    let body = json!({"protocol":wire::PROTOCOL,"deployment_id":session.config.deployment_id,"tenant_id":session.tenant_id,"phone":request.phone.as_str(),"purpose":"awiki.pin-recovery.login.v1"});
    let result: wire::PhoneChallengeResponse = http::request(
        core,
        &store,
        &mut session,
        "phone/challenge",
        "POST",
        "phone/challenges",
        Some(&body),
        None,
    )
    .await?;
    uuid::Uuid::parse_str(&result.challenge_id).map_err(|_| failure("invalid_wire", false))?;
    if !future(&result.expires_at) || result.resend_after_seconds < 60 {
        return Err(failure("invalid_wire", false));
    }
    session.remote_challenge = Some(result.clone());
    store.save_session(&session)?;
    Ok(PhoneChallengeSummary {
        challenge_handle: PhoneChallengeHandle(session.id.clone()),
        expires_at: result.expires_at,
        resend_after_seconds: result
            .resend_after_seconds
            .try_into()
            .map_err(|_| failure("invalid_wire", false))?,
        masked_phone: result.masked_phone,
    })
}
pub(crate) async fn verify_phone_code(
    core: &crate::ImCore,
    request: PhoneVerifyRequest,
) -> crate::ImResult<DiscoverySessionSummary> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut s = store.session(&request.challenge.0)?;
    let challenge = s
        .remote_challenge
        .as_ref()
        .ok_or_else(|| failure("invalid_local_handle", false))?;
    let code =
        std::str::from_utf8(request.code.bytes()).map_err(|_| failure("invalid_otp", false))?;
    let body = json!({"protocol":wire::PROTOCOL,"challenge_id":challenge.challenge_id,"code":code});
    let result: wire::PhoneDiscoveryResponse = http::request(
        core,
        &store,
        &mut s,
        &otp_submission_step(&body)?,
        "POST",
        "phone/sessions",
        Some(&body),
        None,
    )
    .await?;
    if result.token_type != "DPoP"
        || result.expires_in != 600
        || crypto::decode(&result.discovery_token)?.len() != 32
        || !future(&result.expires_at)
    {
        return Err(failure("invalid_wire", false));
    }
    s.discovery = Some(result.clone());
    store.save_session(&s)?;
    Ok(DiscoverySessionSummary {
        discovery_handle: DiscoverySessionHandle(s.id.clone()),
        tenant_id: s.tenant_id.clone(),
        expires_at: result.expires_at,
    })
}
pub(crate) async fn list_targets(
    core: &crate::ImCore,
    handle: DiscoverySessionHandle,
) -> crate::ImResult<Vec<PinRecoveryTarget>> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut s = store.session(&handle.0)?;
    if s.selected.is_some() {
        return Err(failure("discovery_consumed", false));
    }
    let token = discovery_token(&s)?;
    let targets: wire::TargetsResponse = http::request::<serde_json::Value, _>(
        core,
        &store,
        &mut s,
        "targets",
        "GET",
        "targets",
        None,
        Some(&token),
    )
    .await?;
    if targets.targets.len() > 256 {
        return Err(failure("invalid_wire", false));
    }
    let mut ids = std::collections::BTreeSet::new();
    for target in &targets.targets {
        validate_target(target)?;
        if !ids.insert(&target.target_id) {
            return Err(failure("invalid_wire", false));
        }
    }
    s.targets = targets.targets;
    store.save_session(&s)?;
    Ok(s.targets.iter().map(target_public).collect())
}
pub(crate) async fn select_target(
    core: &crate::ImCore,
    handle: DiscoverySessionHandle,
    target: RecoveryTargetId,
) -> crate::ImResult<PhoneSessionSummary> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut s = store.session(&handle.0)?;
    if s.selected.as_ref().is_some_and(|t| t.target_id != target.0) {
        return Err(failure("discovery_consumed", false));
    }
    let selected = s
        .targets
        .iter()
        .find(|t| t.target_id == target.0)
        .cloned()
        .ok_or_else(|| failure("target_not_authorized", false))?;
    let token = discovery_token(&s)?;
    let body = json!({"protocol":wire::PROTOCOL,"target_id":target.0});
    let result: wire::PhoneSessionResponse = http::request(
        core,
        &store,
        &mut s,
        "select-target",
        "POST",
        "sessions/select-target",
        Some(&body),
        Some(&token),
    )
    .await?;
    if result.token_type != "DPoP" || result.expires_in != 600 || !future(&result.expires_at) {
        return Err(failure("invalid_wire", false));
    }
    validate_pst_binding(&result.access_token, &s, &selected)?;
    s.pst = Some(result.clone());
    s.selected = Some(selected.clone());
    store.save_session(&s)?;
    Ok(PhoneSessionSummary {
        session_handle: PhoneSessionHandle(s.id.clone()),
        tenant_id: s.tenant_id.clone(),
        target: target_public(&selected),
        expires_at: result.expires_at,
    })
}
fn validate_target(t: &wire::Target) -> crate::ImResult<()> {
    if t.target_id.is_empty()
        || t.target_id.len() > 128
        || t.did.len() > 640
        || t.full_handle.len() > 320
        || t.binding_generation.is_empty()
        || t.binding_generation.len() > 255
        || t.binding_generation.starts_with('0')
        || !t.binding_generation.bytes().all(|b| b.is_ascii_digit())
        || !t.root_kid.starts_with(&format!("{}#", t.did))
    {
        return Err(failure("invalid_wire", false));
    }
    crate::ids::Did::parse(&t.did)?;
    crypto::validate_ed25519_jwk(&t.root_public_jwk)?;
    Ok(())
}
fn jwt_payload(token: &str) -> crate::ImResult<serde_json::Value> {
    // PST authenticity comes from the configured TLS control plane; this
    // projection only checks the same key/tenant and never authorizes a root.
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(failure("invalid_wire", false));
    }
    serde_json::from_slice(&crypto::decode(parts[1])?).map_err(|_| failure("invalid_wire", false))
}
fn validate_pst_binding(token: &str, s: &Session, _target: &wire::Target) -> crate::ImResult<()> {
    let p = jwt_payload(token)?;
    if p["cnf"]["jkt"].as_str() != Some(http::jkt(&s.dpop_private)?.as_str())
        || p["tenant_id"].as_str() != Some(&s.tenant_id)
        || p["deployment_id"].as_str() != Some(&s.config.deployment_id)
        || p["aud"].as_str() != Some(&s.config.control_audience)
        || p["iss"].as_str() != Some(&s.config.pst_issuer)
        || p["sub"].as_str().is_none_or(str::is_empty)
    {
        return Err(failure("invalid_wire", false));
    }
    Ok(())
}
fn new_operation(session: &Session, kind: PinRecoveryKind) -> crate::ImResult<Operation> {
    let target = session
        .selected
        .clone()
        .ok_or_else(|| failure("target_not_authorized", false))?;
    Ok(Operation {
        id: uuid(),
        session_id: session.id.clone(),
        kind,
        target,
        account_user_id: session
            .pst
            .as_ref()
            .and_then(|p| jwt_payload(&p.access_token).ok())
            .and_then(|p| p["sub"].as_str().map(str::to_owned)),
        realm_ids: session.config.realms.iter().map(|r| r.id.clone()).collect(),
        identity: None,
        identity_generation: None,
        device_label: String::new(),
        phase: PinRecoveryPhase::Authorizing,
        remote: None,
        last_status_query_started_at: None,
        envelope: None,
        wrapping_key_b64u: None,
        recovered_root_b64u: None,
        attempt: None,
        attempt_running: false,
        registration_payloads: BTreeMap::new(),
        receipts: Vec::new(),
        custody_ingress_started: false,
        custody_pending: None,
        device_challenge: None,
        commit_body: None,
        commit_attempted: false,
        recovered_identity: None,
        secure_readiness: PinRecoverySecureReadiness::NotStarted,
        last_error: None,
        v4_cleanup: None,
    })
}
async fn create_operation(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    store.save_operation(o)?;
    let token = phone_token(s)?;
    let body = json!({"protocol":wire::PROTOCOL,"operation_id":o.id,"target_id":o.target.target_id,"kind":o.kind,"expected_active_epoch":if o.kind==PinRecoveryKind::Enable{None}else{o.target.active_epoch}});
    let remote: wire::OperationResponse = http::request(
        core,
        store,
        s,
        &format!("{}:create", o.id),
        "POST",
        "operations",
        Some(&body),
        Some(&token),
    )
    .await?;
    accept_remote(o, s, remote)?;
    store.save_operation(o)
}
fn accept_remote(
    o: &mut Operation,
    s: &Session,
    r: wire::OperationResponse,
) -> crate::ImResult<()> {
    let kind = if o.kind == PinRecoveryKind::Enable {
        "enable"
    } else {
        "recover"
    };
    if r.protocol != wire::PROTOCOL
        || r.operation_id != o.id
        || r.kind != kind
        || r.target_id != o.target.target_id
        || r.did != o.target.did
        || r.binding_generation != o.target.binding_generation
        || r.config_hash != s.config_hash
        || r.op_jkt != http::jkt(&s.dpop_private)?
        || r.backup_epoch == 0
        || r.expected_active_epoch
            != if o.kind == PinRecoveryKind::Enable {
                None
            } else {
                o.target.active_epoch
            }
        || (o.kind == PinRecoveryKind::Recover
            && (Some(r.backup_epoch) != o.target.active_epoch
                || r.recovery_checkpoint.as_ref().is_none_or(|c| {
                    c.document_version == 0
                        || c.registry_version == 0
                        || c.document_digest.len() != 43
                })))
        || !r.slot_id.starts_with("rs_")
        || crypto::decode(r.slot_id.strip_prefix("rs_").unwrap_or_default())?.len() != 32
        || r.guesses_limit != 10
        || r.guesses_used > 10
        || r.cleanup_remaining > 3
    {
        return Err(failure("binding_changed", false));
    }
    if let Some(old) = &o.remote {
        if r.operation_version < old.operation_version
            || r.recovery_id != old.recovery_id
            || r.backup_epoch != old.backup_epoch
            || r.slot_id != old.slot_id
            || r.expected_active_epoch != old.expected_active_epoch
            || old
                .registration_version
                .as_ref()
                .is_some_and(|v| r.registration_version.as_ref() != Some(v))
            || old
                .backup_hash
                .as_ref()
                .is_some_and(|v| r.backup_hash.as_ref() != Some(v))
            || r.guess_cycle < old.guess_cycle
        {
            return Err(failure("binding_changed", false));
        }
    }
    o.remote = Some(r);
    Ok(())
}
pub(crate) async fn begin_enable(
    core: &crate::ImCore,
    request: EnablePinRecoveryRequest,
) -> crate::ImResult<PinRecoveryProgress> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut s = store.session(&request.session.0)?;
    let mut o = new_operation(&s, PinRecoveryKind::Enable)?;
    if !request.user_presence.is_confirmed() || !o.target.can_enable {
        return Err(failure("capability_unavailable", false));
    }
    let identity = core.identities().resolve(request.identity)?;
    if identity.did.as_str() != o.target.did
        || identity.handle.as_ref().map(|h| h.as_str()) != Some(o.target.full_handle.as_str())
    {
        return Err(failure("target_not_authorized", false));
    }
    let (reference, generation) = custody::backup_identity(core, &o.target).await?;
    o.identity = Some(reference);
    o.identity_generation = Some(generation);
    ensure_no_pending_target(&store, &o.target)?;
    create_operation(core, &store, &mut s, &mut o).await?;
    authorize_backup(core, &store, &mut s, &mut o).await?;
    o.phase = PinRecoveryPhase::AwaitingPin;
    store.save_operation(&o)?;
    Ok(progress(&o))
}
pub(crate) async fn begin_recover(
    core: &crate::ImCore,
    request: BeginPinRecoveryRequest,
) -> crate::ImResult<PinRecoveryProgress> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut s = store.session(&request.session.0)?;
    let mut o = new_operation(&s, PinRecoveryKind::Recover)?;
    if !o.target.can_recover {
        return Err(failure("backup_unavailable", false));
    }
    if request.device_label.trim().is_empty() || request.device_label.chars().count() > 128 {
        return Err(failure("invalid_device_label", false));
    }
    o.device_label = request.device_label;
    ensure_no_pending_target(&store, &o.target)?;
    create_operation(core, &store, &mut s, &mut o).await?;
    download_backup(core, &store, &mut s, &mut o).await?;
    o.phase = PinRecoveryPhase::AwaitingPin;
    store.save_operation(&o)?;
    Ok(progress(&o))
}
async fn authorize_backup(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let challenge = o
        .remote
        .as_ref()
        .and_then(|r| r.authorization_challenge.clone())
        .ok_or_else(|| failure("invalid_wire", false))?;
    validate_challenge(s, o, &challenge, "awiki.pin-recovery.authorize.v1")?;
    let proof = custody::sign_backup(core, o, s, &challenge).await?;
    let token = phone_token(s)?;
    let body = json!({"root_proof":proof});
    let remote = http::request(
        core,
        store,
        s,
        &format!("{}:authorize:{}", o.id, challenge.payload.challenge_id),
        "POST",
        &format!("operations/{}/authorize", o.id),
        Some(&body),
        Some(&token),
    )
    .await?;
    accept_remote(o, s, remote)?;
    store.save_operation(o)
}
fn validate_challenge(
    s: &Session,
    o: &Operation,
    c: &wire::Challenge,
    purpose: &str,
) -> crate::ImResult<()> {
    let r = o
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    let p = &c.payload;
    let i = &c.intent;
    if p.protocol != wire::PROTOCOL
        || p.purpose != purpose
        || p.aud != s.config.control_base_url
        || p.operation_id != o.id
        || p.did != o.target.did
        || p.root_kid != o.target.root_kid
        || p.root_jkt != crypto::jwk_thumbprint(&o.target.root_public_jwk)?
        || p.op_jkt != r.op_jkt
        || p.binding_generation != o.target.binding_generation
        || p.recovery_id != r.recovery_id
        || p.backup_epoch != r.backup_epoch
        || p.slot_id != r.slot_id
        || p.intent_hash != crypto::hash(&crypto::canonical(i)?)
        || p.iat > crypto::now() + 5
        || p.exp <= p.iat
        || p.exp <= crypto::now()
        || p.exp.saturating_sub(p.iat) > 300
        || i.kind != r.kind
        || i.operation_id != o.id
        || i.recovery_id != r.recovery_id
        || i.backup_epoch != r.backup_epoch
        || i.slot_id != r.slot_id
        || i.expected_active_epoch != r.expected_active_epoch
        || i.config_hash != s.config_hash
        || i.binding_generation != o.target.binding_generation
        || i.backup_hash != r.backup_hash
        || i.registration_version != r.registration_version
    {
        return Err(failure("intent_mismatch", false));
    }
    let token = phone_token(s)?;
    let claims = jwt_payload(&token)?;
    if claims["sub"].as_str() != Some(p.account_user_id.as_str()) {
        return Err(failure("intent_mismatch", false));
    }
    Ok(())
}
fn trusted_domain(did: &crate::ids::Did) -> crate::ImResult<String> {
    let url = crate::internal::discovery::did_document::did_document_url(did.as_str())?;
    reqwest::Url::parse(&url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .ok_or_else(|| failure("invalid_did", false))
}
async fn attempt(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
    action: &str,
) -> crate::ImResult<()> {
    let token = phone_token(s)?;
    // New key is allocated once for this explicit attempt and persisted by
    // prepare before any HTTP write. It is never reused for a new blind input.
    let key = format!("{}:attempt:{}:{}", o.id, action, uuid());
    let body = json!({"action":action});
    let a: wire::AttemptResponse = http::request(
        core,
        store,
        s,
        &key,
        "POST",
        &format!("operations/{}/attempts", o.id),
        Some(&body),
        Some(&token),
    )
    .await?;
    let r = o
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    if a.operation_id != o.id
        || a.action != action
        || a.slot_id != r.slot_id
        || a.backup_epoch != r.backup_epoch
        || a.realm_tokens.len() != 3
        || a.cleanup_job_id.is_some()
        || (action != "register" && a.registration_version != r.registration_version)
    {
        return Err(failure("invalid_wire", false));
    }
    let mut ids = std::collections::BTreeSet::new();
    for t in &a.realm_tokens {
        if t.token_type != "DPoP"
            || !s.config.realms.iter().any(|r| r.id == t.realm_id)
            || !ids.insert(t.realm_id.clone())
        {
            return Err(failure("invalid_wire", false));
        }
    }
    o.attempt = Some(a);
    o.attempt_running = true;
    store.save_operation(o)
}
async fn submit_receipts(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let receipts = receipts_for_current_attempt(o)?;
    if receipts.is_empty() {
        return Err(failure("missing_receipt", false));
    }
    let token = phone_token(s)?;
    for chunk in receipts.chunks(6) {
        let body = json!({"receipts":chunk});
        let key = format!(
            "{}:receipts:{}",
            o.id,
            crypto::hash(&crypto::canonical(&body)?)
        );
        let r = http::request(
            core,
            store,
            s,
            &key,
            "POST",
            &format!("operations/{}/receipts", o.id),
            Some(&body),
            Some(&token),
        )
        .await?;
        accept_remote(o, s, r)?;
        store.save_operation(o)?;
    }
    Ok(())
}
async fn refresh(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let query_started_at = crypto::now();
    let token = phone_token(s)?;
    let r = http::request::<serde_json::Value, _>(
        core,
        store,
        s,
        &format!("{}:get", o.id),
        "GET",
        &format!("operations/{}", o.id),
        None,
        Some(&token),
    )
    .await?;
    accept_remote(o, s, r)?;
    o.last_status_query_started_at = Some(query_started_at);
    store.save_operation(o)
}
pub(crate) async fn submit_pin(
    core: &crate::ImCore,
    id: PinRecoveryOperationId,
    pin: SecretPin,
) -> crate::ImResult<PinRecoveryProgress> {
    let store = Store::new(core)?;
    let _lock = store.lock().await?;
    let mut o = store.operation(&id.0)?;
    let mut s = store.session(&o.session_id)?;
    if matches!(
        o.phase,
        PinRecoveryPhase::Completed
            | PinRecoveryPhase::Cancelled
            | PinRecoveryPhase::LocalActivationPending
            | PinRecoveryPhase::CommitUnknown
    ) || o.custody_pending.is_some()
        || o.recovered_root_b64u.is_some()
        || o.custody_ingress_started
    {
        return Err(failure("action_not_allowed", false));
    }
    refresh(core, &store, &mut s, &mut o).await?;
    if !progress(&o)
        .allowed_actions
        .contains(&PinRecoveryAction::SubmitPin)
    {
        return Err(failure("action_not_allowed", false));
    }
    let result = match o.kind {
        PinRecoveryKind::Enable => enable_pin(core, &store, &mut s, &mut o, &pin).await,
        PinRecoveryKind::Recover => recover_pin(core, &store, &mut s, &mut o, &pin).await,
    };
    if let Err(error) = result {
        o.last_error = Some(PinRecoveryError {
            code: error_code(&error).into(),
            retryable: matches!(
                error_code(&error),
                "network_unavailable" | "realm_unavailable" | "local_activation_pending"
            ),
            retry_after_seconds: None,
        });
        o.attempt_running = false;
        if o.custody_pending.is_none()
            && o.recovered_root_b64u.is_none()
            && !o.custody_ingress_started
            && o.kind == PinRecoveryKind::Recover
        {
            o.phase = PinRecoveryPhase::AwaitingPin;
        }
        store.save_operation(&o)?;
        return Err(error);
    }
    o.last_error = None;
    store.save_operation(&o)?;
    Ok(progress(&o))
}
async fn enable_pin(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
    pin: &SecretPin,
) -> crate::ImResult<()> {
    if o.envelope.is_none() {
        let r = o
            .remote
            .as_ref()
            .ok_or_else(|| failure("corrupt_pending", false))?;
        if !r.allowed_actions.iter().any(|a| a == "upload_backup") {
            return Err(failure("action_not_allowed", false));
        }
        let root = custody::export(core, o).await?;
        let key = crypto::random32();
        let document_digest = custody::backup_document_digest(core, o).await?;
        let header = wire::BackupHeader {
            format: "awiki.root-backup.v1".into(),
            deployment_id: s.config.deployment_id.clone(),
            tenant_id: s.tenant_id.clone(),
            recovery_id: r.recovery_id.clone(),
            backup_epoch: r.backup_epoch,
            slot_id: r.slot_id.clone(),
            did: o.target.did.clone(),
            root_kid: o.target.root_kid.clone(),
            root_public_jwk: o.target.root_public_jwk.clone(),
            document_digest_at_backup: document_digest,
            config_hash: s.config_hash.clone(),
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        o.envelope = Some(crypto::encrypt_root(header, &root, key.as_ref())?);
        o.wrapping_key_b64u = Some(crypto::b64(key.as_ref()));
        store.save_operation(o)?;
    }
    let envelope = o
        .envelope
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?
        .clone();
    let token = phone_token(s)?;
    let uploaded: wire::BackupResponse = http::request(
        core,
        store,
        s,
        &format!("{}:backup:put", o.id),
        "PUT",
        &format!("operations/{}/backup", o.id),
        Some(&envelope.envelope),
        Some(&token),
    )
    .await?;
    if uploaded != envelope {
        return Err(failure("backup_hash_mismatch", false));
    }
    refresh(core, store, s, o).await?;
    if o.remote
        .as_ref()
        .is_none_or(|r| r.registered_realms.len() != 3)
    {
        o.phase = PinRecoveryPhase::Registering;
        store.save_operation(o)?;
        let key = crypto::decode(
            o.wrapping_key_b64u
                .as_ref()
                .ok_or_else(|| failure("corrupt_pending", false))?,
        )?;
        if o.registration_payloads.is_empty() {
            attempt(core, store, s, o, "register").await?;
            let adapter = adapter::Adapter::new(core, store, s.clone(), o.clone());
            let result = adapter.run_register(pin, &key).await;
            (*s, *o) = adapter.snapshot().await;
            result?;
        } else if o.registration_payloads.len() == 3 {
            refresh_tokens(core, store, s, o).await?;
            let adapter = adapter::Adapter::new(core, store, s.clone(), o.clone());
            let result = adapter.resume_register().await;
            (*s, *o) = adapter.snapshot().await;
            result?;
        } else {
            return Err(failure("registration_restart_required", false));
        }
        submit_receipts(core, store, s, o).await?;
        if o.remote
            .as_ref()
            .is_none_or(|r| r.registered_realms.len() != 3)
        {
            return Err(failure("registration_incomplete", true));
        }
    }
    o.phase = PinRecoveryPhase::VerifyingBackup;
    store.save_operation(o)?;
    attempt(core, store, s, o, "verify_stage").await?;
    let adapter = adapter::Adapter::new(core, store, s.clone(), o.clone());
    let result = adapter.run_recover(pin).await;
    (*s, *o) = adapter.snapshot().await;
    let key = result?;
    if crypto::decode(
        o.wrapping_key_b64u
            .as_ref()
            .ok_or_else(|| failure("corrupt_pending", false))?,
    )?
    .as_slice()
        != key.as_slice()
    {
        return Err(failure("backup_self_check_failed", false));
    }
    let root = crypto::decrypt_root(
        o.envelope
            .as_ref()
            .ok_or_else(|| failure("corrupt_pending", false))?,
        &key,
        &o.target,
    )?;
    crypto::verify_root(&root, &o.target)?;
    submit_receipts(core, store, s, o).await?;
    let r = o
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    if r.registered_realms.len() != 3 || r.verified_realms.len() < 2 {
        return Err(failure("backup_self_check_failed", false));
    }
    commit_backup(core, store, s, o).await
}
async fn commit_backup(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let c = o
        .remote
        .as_ref()
        .and_then(|r| r.commit_challenge.clone())
        .ok_or_else(|| failure("invalid_wire", false))?;
    validate_challenge(s, o, &c, "awiki.pin-recovery.backup-commit.v1")?;
    verify_receipt_ids(o, &c.intent)?;
    let proof = custody::sign_backup(core, o, s, &c).await?;
    let body = json!({"root_proof":proof});
    o.commit_body = Some(body.clone());
    o.commit_attempted = true;
    o.phase = PinRecoveryPhase::CommitUnknown;
    store.save_operation(o)?;
    let token = phone_token(s)?;
    let r = http::request(
        core,
        store,
        s,
        &format!("{}:backup:commit:{}", o.id, c.payload.challenge_id),
        "POST",
        &format!("operations/{}/commit", o.id),
        Some(&body),
        Some(&token),
    )
    .await?;
    accept_remote(o, s, r)?;
    if !o
        .remote
        .as_ref()
        .is_some_and(|r| r.state == "active" || r.state == "active_cleanup_pending")
    {
        return Err(failure("commit_unknown", true));
    }
    clear_ephemeral(o);
    o.phase = PinRecoveryPhase::Completed;
    store.save_operation(o)
}
async fn recover_pin(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
    pin: &SecretPin,
) -> crate::ImResult<()> {
    o.phase = PinRecoveryPhase::RecoveringKey;
    store.save_operation(o)?;
    attempt(core, store, s, o, "recover").await?;
    let adapter = adapter::Adapter::new(core, store, s.clone(), o.clone());
    let result = adapter.run_recover(pin).await;
    (*s, *o) = adapter.snapshot().await;
    let key = result?;
    let root = crypto::decrypt_root(
        o.envelope
            .as_ref()
            .ok_or_else(|| failure("corrupt_pending", false))?,
        &key,
        &o.target,
    )?;
    // Seal before any subsequent await: process death here never repeats a PIN guess.
    o.recovered_root_b64u = Some(crypto::b64(&root));
    o.attempt_running = false;
    store.save_operation(o)?;
    submit_receipts(core, store, s, o).await?;
    let _pending = custody::begin(core, s, o).await?;
    o.phase = PinRecoveryPhase::AwaitingDeviceConfirmation;
    store.save_operation(o)
}
async fn refresh_tokens(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let old = o
        .attempt
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    let attempt_id = old.attempt_id.clone();
    let token = phone_token(s)?;
    let key = format!("{}:tokens:{}:{}", o.id, attempt_id, uuid());
    let new: wire::AttemptResponse = http::request(
        core,
        store,
        s,
        &key,
        "POST",
        &format!("operations/{}/attempts/{attempt_id}/tokens", o.id),
        Some(&json!({})),
        Some(&token),
    )
    .await?;
    if new.attempt_id != attempt_id
        || new.action != old.action
        || new.slot_id != old.slot_id
        || new.registration_version != old.registration_version
    {
        return Err(failure("binding_changed", false));
    }
    o.attempt = Some(new);
    store.save_operation(o)
}
fn verify_receipt_ids(o: &Operation, intent: &wire::Intent) -> crate::ImResult<()> {
    let mut registered = std::collections::BTreeSet::new();
    let mut recovered = std::collections::BTreeSet::new();
    for receipt in receipts_for_current_attempt(o)? {
        let claims = jwt_payload(&receipt)?;
        let id = claims["jti"]
            .as_str()
            .ok_or_else(|| failure("invalid_receipt", false))?
            .to_owned();
        match claims["phase"].as_str() {
            Some("registered") => {
                registered.insert(id);
            }
            Some("recovered") => {
                recovered.insert(id);
            }
            _ => return Err(failure("invalid_receipt", false)),
        }
    }
    if intent
        .registered_receipt_ids
        .iter()
        .any(|id| !registered.contains(id))
        || intent
            .verified_receipt_ids
            .iter()
            .any(|id| !recovered.contains(id))
        || intent.verified_receipt_ids.len() < 2
        || (o.kind == PinRecoveryKind::Enable && intent.registered_receipt_ids.len() != 3)
    {
        return Err(failure("intent_mismatch", false));
    }
    Ok(())
}
fn clear_ephemeral(o: &mut Operation) {
    use zeroize::Zeroize;
    if let Some(k) = o.wrapping_key_b64u.as_mut() {
        k.zeroize();
    }
    o.wrapping_key_b64u = None;
    if let Some(k) = o.recovered_root_b64u.as_mut() {
        k.zeroize();
    }
    o.recovered_root_b64u = None;
    o.registration_payloads.clear();
    o.attempt = None;
    o.attempt_running = false;
}

async fn download_backup(
    core: &crate::ImCore,
    store: &Store,
    s: &mut Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let token = phone_token(s)?;
    let backup: wire::BackupResponse = http::request::<serde_json::Value, _>(
        core,
        store,
        s,
        &format!("{}:backup:get", o.id),
        "GET",
        &format!("operations/{}/backup", o.id),
        None,
        Some(&token),
    )
    .await?;
    crypto::validate_envelope(
        &backup,
        o.remote
            .as_ref()
            .ok_or_else(|| failure("corrupt_pending", false))?,
        &o.target,
        &s.config,
    )?;
    o.envelope = Some(backup);
    store.save_operation(o)
}

fn otp_submission_step(body: &serde_json::Value) -> crate::ImResult<String> {
    Ok(format!(
        "phone/verify:{}",
        crypto::hash(&crypto::canonical(body)?)
    ))
}

fn ensure_no_pending_target(store: &Store, target: &wire::Target) -> crate::ImResult<()> {
    if store.list_operations()?.iter().any(|o| {
        o.target.did == target.did
            && !matches!(
                o.phase,
                PinRecoveryPhase::Completed | PinRecoveryPhase::Cancelled
            )
    }) {
        return Err(failure("recovery_in_progress", false));
    }
    Ok(())
}

fn receipts_for_current_attempt(o: &Operation) -> crate::ImResult<Vec<String>> {
    let current = o.attempt.as_ref().map(|a| a.attempt_id.as_str());
    o.receipts
        .iter()
        .filter_map(|receipt| match jwt_payload(receipt) {
            Ok(claims)
                if claims["phase"] == "registered"
                    || (claims["phase"] == "recovered"
                        && claims["attempt_id"].as_str() == current) =>
            {
                Some(Ok(receipt.clone()))
            }
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
}
