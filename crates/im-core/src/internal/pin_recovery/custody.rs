//! Provider boundary: root material remains exclusively inside Rust.
use super::{
    crypto, failure,
    pending::{Operation, Session},
    wire,
};
use crate::internal::identity_provider::*;
fn map(e: IdentityProviderError) -> crate::ImError {
    if e.code == IdentityProviderErrorCode::CapabilityUnavailable {
        failure("capability_unavailable", false)
    } else {
        map_provider_error(e)
    }
}
async fn provider(core: &crate::ImCore) -> crate::ImResult<std::sync::Arc<dyn IdentityCustody>> {
    crate::internal::identity_custody::controller_custody_provider(core).await
}
async fn open(
    core: &crate::ImCore,
    op: &Operation,
) -> crate::ImResult<std::sync::Arc<dyn IdentitySession>> {
    provider(core)
        .await?
        .open_identity(
            op.identity
                .as_ref()
                .ok_or_else(|| failure("corrupt_pending", false))?,
        )
        .await
        .map_err(map)
}
pub(super) async fn backup_identity(
    core: &crate::ImCore,
    target: &wire::Target,
) -> crate::ImResult<(ProviderIdentityRef, u64)> {
    let p = provider(core).await?;
    let candidates = p
        .list_identities()
        .await
        .map_err(map)?
        .into_iter()
        .filter(|i| i.reference.did == target.did)
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(failure("capability_unavailable", false));
    }
    let identity = p
        .open_identity(&candidates[0].reference)
        .await
        .map_err(map)?;
    let public = identity.public_identity().await.map_err(map)?;
    let host = identity.host_status().await.map_err(map)?;
    if public.state != ProviderIdentityState::Active
        || host.root_capability != ProviderRootCapability::Active
        || !public.active_keys.iter().any(|k| {
            k.kid == target.root_kid && k.purposes.contains(&ProviderKeyPurpose::RootControl)
        })
    {
        return Err(failure("capability_unavailable", false));
    }
    check_root_document(&public.document, target)?;
    Ok((
        public.reference,
        identity.pin_root_generation().await.map_err(map)?,
    ))
}
pub(super) fn binding(s: &Session, o: &Operation) -> crate::ImResult<ProviderPinRecoveryBinding> {
    let r = o
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    let token = super::phone_token(s)?;
    let claims = super::jwt_payload(&token)?;
    let did = crate::ids::Did::parse(&o.target.did)?;
    Ok(ProviderPinRecoveryBinding {
        operation_id: o.id.clone(),
        account_user_id: claims["sub"]
            .as_str()
            .ok_or_else(|| failure("invalid_wire", false))?
            .into(),
        target_id: o.target.target_id.clone(),
        handle: o.target.full_handle.clone(),
        did: o.target.did.clone(),
        domain: super::trusted_domain(&did)?,
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
    })
}
pub(super) async fn sign_backup(
    core: &crate::ImCore,
    o: &mut Operation,
    s: &Session,
    c: &wire::Challenge,
) -> crate::ImResult<String> {
    let identity = open(core, o).await?;
    let local = identity.public_identity().await.map_err(map)?;
    if crypto::hash(&crypto::canonical(&local.document)?) != c.payload.document_digest {
        let document = resolve(core, &o.target).await?;
        if crypto::hash(&crypto::canonical(&document)?) != c.payload.document_digest {
            return Err(failure("document_changed", true));
        }
        identity
            .adopt_verified_sibling_document(remote(
                document,
                &wire::Checkpoint {
                    document_version: c.payload.document_version,
                    registry_version: c.payload.registry_version,
                    document_digest: c.payload.document_digest.clone(),
                },
            ))
            .await
            .map_err(map)?;
    }
    o.identity_generation = Some(identity.pin_root_generation().await.map_err(map)?);

    identity
        .sign_pin_recovery_intent(ProviderPinBackupIntentRequest {
            expected_generation: o
                .identity_generation
                .ok_or_else(|| failure("corrupt_pending", false))?,
            binding: binding(s, o)?,
            purpose: if c.payload.purpose == "awiki.pin-recovery.authorize.v1" {
                ProviderPinBackupPurpose::Authorize
            } else {
                ProviderPinBackupPurpose::BackupCommit
            },
            expected_intent: serde_json::to_value(&c.intent)
                .map_err(|_| failure("invalid_wire", false))?,
            challenge: challenge(c)?,
            user_presence_confirmed: true,
        })
        .await
        .map_err(map)
}
pub(super) async fn export(
    core: &crate::ImCore,
    o: &Operation,
) -> crate::ImResult<zeroize::Zeroizing<Vec<u8>>> {
    let root = open(core, o)
        .await?
        .export_root_for_pin_backup(ProviderPinRootBackupRequest {
            expected_generation: o
                .identity_generation
                .ok_or_else(|| failure("corrupt_pending", false))?,
            root_kid: o.target.root_kid.clone(),
            user_presence_confirmed: true,
        })
        .await
        .map_err(map)?;
    if root.root_kid != o.target.root_kid
        || root.root_public_key_multibase != crypto::root_multibase(&o.target.root_public_jwk)?
    {
        return Err(failure("root_key_mismatch", false));
    }
    crypto::verify_root(&root.pkcs8_der, &o.target)?;
    Ok(root.pkcs8_der)
}
fn challenge(c: &wire::Challenge) -> crate::ImResult<ProviderPinRecoveryChallenge> {
    Ok(ProviderPinRecoveryChallenge {
        payload: serde_json::to_value(&c.payload).map_err(|_| failure("invalid_wire", false))?,
        intent: serde_json::to_value(&c.intent).map_err(|_| failure("invalid_wire", false))?,
    })
}
fn remote(
    document: serde_json::Value,
    checkpoint: &wire::Checkpoint,
) -> ProviderVerifiedRemoteDocument {
    ProviderVerifiedRemoteDocument {
        document,
        evidence: ProviderPublicationEvidence {
            document_version: checkpoint.document_version,
            registry_version: checkpoint.registry_version,
            document_digest: format!("sha256:{}", checkpoint.document_digest),
        },
    }
}
async fn resolve(
    core: &crate::ImCore,
    target: &wire::Target,
) -> crate::ImResult<serde_json::Value> {
    let mut transport = crate::internal::transport::CorePlainTransport::new_no_redirect(core);
    let document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut transport,
        &target.did,
    )
    .await?;
    check_root_document(&document, target)?;
    Ok(document)
}
fn check_root_document(document: &serde_json::Value, target: &wire::Target) -> crate::ImResult<()> {
    if document["id"].as_str() != Some(target.did.as_str())
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(failure("root_key_mismatch", false));
    }
    let method = anp::authentication::find_verification_method(document, &target.root_kid)
        .ok_or_else(|| failure("root_key_mismatch", false))?;
    let key = crate::internal::identity_wire::document::extract_identity_public_key(&method)?;
    let anp::PublicKeyMaterial::Ed25519(key) = key else {
        return Err(failure("root_key_mismatch", false));
    };
    if key.to_bytes() != crypto::validate_ed25519_jwk(&target.root_public_jwk)? {
        return Err(failure("root_key_mismatch", false));
    }
    Ok(())
}
pub(super) async fn begin(
    core: &crate::ImCore,
    s: &Session,
    o: &mut Operation,
) -> crate::ImResult<std::sync::Arc<dyn ProviderPinRootRecoverySession>> {
    begin_inner(core, s, o, true).await
}
async fn begin_inner(
    core: &crate::ImCore,
    s: &Session,
    o: &mut Operation,
    allow_ingress: bool,
) -> crate::ImResult<std::sync::Arc<dyn ProviderPinRootRecoverySession>> {
    let current_document = resolve(core, &o.target).await?;
    let r = o
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    let (document, checkpoint) = if let Some(result) = r.device_result.as_ref() {
        // Authenticated exact-operation history can activate its committed
        // candidate even if a sibling has since advanced the public document.
        // The current public root was independently checked immediately above.
        (result.committed_document.clone(), result.checkpoint.clone())
    } else {
        (
            current_document,
            r.recovery_checkpoint
                .clone()
                .ok_or_else(|| failure("checkpoint_required", false))?,
        )
    };
    if crypto::hash(&crypto::canonical(&document)?) != checkpoint.document_digest {
        return Err(failure("document_changed", true));
    }
    let root = ingress_root(o, allow_ingress)?;
    if root.is_some() {
        o.custody_ingress_started = true;
        super::pending::Store::new(core)?.save_operation(o)?;
    }
    let pending = provider(core)
        .await?
        .begin_pin_root_recovery(ProviderBeginPinRootRecoveryRequest {
            binding: binding(s, o)?,
            remote: remote(document, &checkpoint),
            root_key: root,
            profiles: profiles(),
            device_label: o.device_label.clone(),
            user_presence_confirmed: true,
        })
        .await
        .map_err(map)?;
    let proposal = pending.proposal().await.map_err(map)?;
    if proposal.binding != binding(s, o)?
        || proposal.identity.did != o.target.did
        || proposal.generation == 0
        || !matches!(
            proposal.phase.as_str(),
            "prepared" | "publication_uncertain" | "active"
        )
    {
        return Err(failure("binding_changed", false));
    }
    o.custody_pending =
        Some(serde_json::to_value(&proposal).map_err(|_| failure("corrupt_pending", false))?);
    if let Some(mut root) = o.recovered_root_b64u.take() {
        use zeroize::Zeroize;
        root.zeroize();
    }
    super::pending::Store::new(core)?.save_operation(o)?;
    Ok(pending)
}
pub(super) fn proposal(o: &Operation) -> crate::ImResult<ProviderPinRecoveryProposal> {
    serde_json::from_value(
        o.custody_pending
            .clone()
            .ok_or_else(|| failure("corrupt_pending", false))?,
    )
    .map_err(|_| failure("corrupt_pending", false))
}
pub(super) async fn sign_device(
    core: &crate::ImCore,
    s: &Session,
    o: &mut Operation,
) -> crate::ImResult<serde_json::Value> {
    let session = begin(core, s, o).await?;
    let proposal = session.proposal().await.map_err(map)?;
    let c = o
        .device_challenge
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    let verified = resolve(core, &o.target).await?;
    if verified != c.current_document
        || crypto::hash(&crypto::canonical(&verified)?) != c.checkpoint.document_digest
    {
        return Err(failure("document_changed", true));
    }
    let signed = session
        .sign_pin_recovery_intent(ProviderPinRecoverySignRequest {
            expected_generation: proposal.generation,
            challenge: challenge(&c.challenge)?,
            remote: remote(verified, &c.checkpoint),
            candidate_document: c.candidate_document.clone(),
            user_presence_confirmed: true,
        })
        .await
        .map_err(map)?;
    o.custody_pending = Some(
        serde_json::to_value(session.proposal().await.map_err(map)?)
            .map_err(|_| failure("corrupt_pending", false))?,
    );
    Ok(
        serde_json::json!({"root_proof":signed.root_proof,"device_proof":signed.device_proof,"candidate_document":signed.signed_document}),
    )
}
pub(super) async fn complete(
    core: &crate::ImCore,
    s: &Session,
    o: &mut Operation,
    result: &wire::RemoteDeviceResult,
) -> crate::ImResult<CompletedIdentity> {
    let current = resolve(core, &o.target).await?;
    let r = o
        .remote
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?;
    let checkpoint = r
        .recovery_checkpoint
        .clone()
        .ok_or_else(|| failure("checkpoint_required", false))?;
    if crypto::hash(&crypto::canonical(&current)?) != checkpoint.document_digest
        || checkpoint.document_version < result.checkpoint.document_version
        || checkpoint.registry_version < result.checkpoint.registry_version
    {
        return Err(failure("document_changed", true));
    }
    let authorization = r
        .recovered_device_authorization
        .clone()
        .ok_or_else(|| failure("device_authorization_required", false))?;
    verify_current_device(&current, result, &authorization)?;
    let pending = begin(core, s, o).await?;
    let proposal = pending.proposal().await.map_err(map)?;
    let signed = o
        .commit_body
        .as_ref()
        .and_then(|b| b.get("candidate_document"))
        .ok_or_else(|| failure("corrupt_pending", false))?;
    if result.operation_id != o.id
        || result.did != o.target.did
        || result.root_kid != o.target.root_kid
        || result.committed_document != *signed
        || result.device_id
            != proposal.device_proposal["device_id"]
                .as_str()
                .unwrap_or_default()
        || result.signing_kid
            != proposal.device_proposal["signing_kid"]
                .as_str()
                .unwrap_or_default()
        || result.agreement_kid
            != proposal.device_proposal["agreement_kid"]
                .as_str()
                .unwrap_or_default()
        || crypto::hash(&crypto::canonical(&result.committed_document)?)
            != result.checkpoint.document_digest
    {
        return Err(failure("intent_mismatch", false));
    }
    let identity = if proposal.phase == "active" {
        provider(core)
            .await?
            .open_identity(&proposal.identity)
            .await
            .map_err(map)?
    } else {
        pending
            .complete_pin_root_recovery(ProviderPinRecoveryCompletion {
                expected_generation: proposal.generation,
                binding: binding(s, o)?,
                challenge_id: o
                    .device_challenge
                    .as_ref()
                    .ok_or_else(|| failure("corrupt_pending", false))?
                    .challenge
                    .payload
                    .challenge_id
                    .clone(),
                remote: remote(result.committed_document.clone(), &result.checkpoint),
                outcome: ProviderPinRecoveryRemoteOutcome::Committed,
            })
            .await
            .map_err(map)?
            .ok_or_else(|| failure("local_activation_pending", true))?
    };
    let public = identity.public_identity().await.map_err(map)?;
    if public.document != current {
        identity
            .adopt_verified_sibling_document(remote(current, &checkpoint))
            .await
            .map_err(map)?;
    }
    let public = identity.public_identity().await.map_err(map)?;
    if public.state != ProviderIdentityState::Active {
        return Err(failure("device_revoked", false));
    }
    Ok(CompletedIdentity {
        public,
        checkpoint,
        authorization,
    })
}
pub(super) fn profiles() -> Vec<String> {
    vec![
        "anp.core.binding.v1",
        "anp.identity.discovery.v1",
        "anp.direct.base.v1",
        "anp.direct.e2ee.v2",
        "anp.group.base.v2",
        "anp.group.e2ee.v2",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
pub(super) async fn reconcile_expired_challenge(
    core: &crate::ImCore,
    s: &Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    let c = o
        .device_challenge
        .as_ref()
        .ok_or_else(|| failure("corrupt_pending", false))?
        .clone();
    if !o.remote.as_ref().is_some_and(|r| {
        matches!(r.state.as_str(), "cancelled" | "expired") && r.device_result.is_none()
    }) {
        require_expired_noncommit(
            c.challenge.payload.exp,
            o.last_status_query_started_at.unwrap_or(0),
            o.remote.as_ref().map(|r| r.device_result.is_some()),
        )?;
    }
    let pending = begin(core, s, o).await?;
    let p = pending.proposal().await.map_err(map)?;
    let document = resolve(core, &o.target).await?;
    let checkpoint = o
        .remote
        .as_ref()
        .and_then(|r| r.recovery_checkpoint.clone())
        .ok_or_else(|| failure("checkpoint_required", false))?;
    if crypto::hash(&crypto::canonical(&document)?) != checkpoint.document_digest {
        return Err(failure("document_changed", true));
    }
    pending
        .complete_pin_root_recovery(ProviderPinRecoveryCompletion {
            expected_generation: p.generation,
            binding: binding(s, o)?,
            challenge_id: c.challenge.payload.challenge_id,
            remote: remote(document, &checkpoint),
            outcome: ProviderPinRecoveryRemoteOutcome::NotCommitted,
        })
        .await
        .map_err(map)?;
    o.custody_pending = Some(
        serde_json::to_value(pending.proposal().await.map_err(map)?)
            .map_err(|_| failure("corrupt_pending", false))?,
    );
    o.commit_body = None;
    o.commit_attempted = false;
    o.device_challenge = None;
    Ok(())
}

pub(super) async fn backup_document_digest(
    core: &crate::ImCore,
    o: &Operation,
) -> crate::ImResult<String> {
    let identity = open(core, o).await?;
    let public = identity.public_identity().await.map_err(map)?;
    check_root_document(&public.document, &o.target)?;
    let status = identity.host_status().await.map_err(map)?;
    let checkpoint = status
        .checkpoint
        .ok_or_else(|| failure("checkpoint_required", false))?;
    let actual = crypto::hash(&crypto::canonical(&public.document)?);
    if checkpoint.document_digest != format!("sha256:{actual}") {
        return Err(failure("document_changed", true));
    }
    Ok(actual)
}

async fn has_pending_identity(core: &crate::ImCore, o: &Operation) -> crate::ImResult<bool> {
    let identities = provider(core).await?.list_identities().await.map_err(map)?;
    let matches = identities
        .iter()
        .filter(|i| i.reference.did == o.target.did)
        .count();
    if matches > 1 {
        return Err(failure("binding_changed", false));
    }
    Ok(matches == 1)
}
pub(super) async fn cancel_prepared(
    core: &crate::ImCore,
    store: &super::pending::Store,
    s: &Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    if !must_observe_pending(o) {
        return Ok(());
    }
    if !has_pending_identity(core, o).await? {
        // Remote cancellation is terminal. Absence also covers deletion that
        // succeeded before the Core journal write was interrupted.
        if o.remote
            .as_ref()
            .is_none_or(|r| !matches!(r.state.as_str(), "cancelled" | "expired"))
        {
            return Err(failure("reconciliation_required", true));
        }
        o.custody_pending = None;
        o.custody_ingress_started = false;
        store.save_operation(o)?;
        return Ok(());
    }
    let live = begin_inner(core, s, o, false).await?;
    capture_proposal(store, o, live.as_ref()).await?;
    let p = live.proposal().await.map_err(map)?;
    if p.phase == "publication_uncertain"
        && o.remote
            .as_ref()
            .is_some_and(|r| matches!(r.state.as_str(), "cancelled" | "expired"))
    {
        reconcile_expired_challenge(core, s, o).await?;
        let live = begin_inner(core, s, o, false).await?;
        let p = live.proposal().await.map_err(map)?;
        if p.phase != "prepared" {
            return Err(failure("reconciliation_required", true));
        }
        live.cancel_pin_root_recovery(p.generation, p.binding)
            .await
            .map_err(map)?;
    } else {
        if p.phase != "prepared" {
            return Err(failure("reconciliation_required", true));
        }
        live.cancel_pin_root_recovery(p.generation, p.binding)
            .await
            .map_err(map)?;
    }
    o.custody_pending = None;
    o.custody_ingress_started = false;
    store.save_operation(o)
}

pub(super) fn require_expired_noncommit(
    expires_at: u64,
    observed_at: u64,
    committed: Option<bool>,
) -> crate::ImResult<()> {
    if observed_at <= expires_at.saturating_add(5) || committed != Some(false) {
        return Err(failure("reconciliation_required", true));
    }
    Ok(())
}

pub(super) async fn synchronize_pending(
    core: &crate::ImCore,
    store: &super::pending::Store,
    s: &Session,
    o: &mut Operation,
) -> crate::ImResult<()> {
    if !must_observe_pending(o) {
        return Ok(());
    }
    if !has_pending_identity(core, o).await? {
        return Ok(());
    }
    let live = begin_inner(core, s, o, false).await?;
    capture_proposal(store, o, live.as_ref()).await
}
pub(super) async fn capture_proposal(
    store: &super::pending::Store,
    o: &mut Operation,
    session: &dyn ProviderPinRootRecoverySession,
) -> crate::ImResult<()> {
    let p = session.proposal().await.map_err(map)?;
    if p.binding.operation_id != o.id
        || p.binding.target_id != o.target.target_id
        || p.binding.did != o.target.did
    {
        return Err(failure("binding_changed", false));
    }
    o.custody_pending =
        Some(serde_json::to_value(p).map_err(|_| failure("corrupt_pending", false))?);
    store.save_operation(o)
}

pub(super) fn must_observe_pending(o: &Operation) -> bool {
    o.custody_ingress_started || o.custody_pending.is_some()
}
pub(super) fn ingress_root(
    o: &Operation,
    allow_ingress: bool,
) -> crate::ImResult<Option<zeroize::Zeroizing<Vec<u8>>>> {
    if allow_ingress && o.custody_pending.is_none() {
        Ok(Some(crypto::decode(
            o.recovered_root_b64u
                .as_ref()
                .ok_or_else(|| failure("corrupt_pending", false))?,
        )?))
    } else {
        Ok(None)
    }
}

pub(super) struct CompletedIdentity {
    pub public: ProviderPublicIdentity,
    pub checkpoint: wire::Checkpoint,
    pub authorization: wire::RecoveredDeviceAuthorization,
}
pub(super) fn verify_current_device(
    document: &serde_json::Value,
    result: &wire::RemoteDeviceResult,
    auth: &wire::RecoveredDeviceAuthorization,
) -> crate::ImResult<()> {
    if auth.status != "active" {
        return Err(failure("device_revoked", false));
    }
    if auth.device_id != result.device_id
        || auth.signing_key_id != result.signing_kid
        || auth.e2ee_key_id != result.agreement_kid
        || auth.role != "admin"
        || !auth.management_ready
        || auth.auth_generation == 0
    {
        return Err(failure("device_authorization_changed", false));
    }
    let manifest = anp::authentication::validate_device_manifest(document)
        .map_err(|_| failure("device_revoked", false))?
        .ok_or_else(|| failure("device_revoked", false))?;
    let device = manifest
        .devices
        .iter()
        .find(|d| d.device_id == result.device_id)
        .ok_or_else(|| failure("device_revoked", false))?;
    if device.signing_key_id != result.signing_kid
        || device.e2ee_key_id != result.agreement_kid
        || !anp::authentication::is_authentication_authorized(document, &result.signing_kid)
        || !anp::authentication::is_assertion_method_authorized(document, &result.signing_kid)
    {
        return Err(failure("device_authorization_changed", false));
    }
    Ok(())
}
