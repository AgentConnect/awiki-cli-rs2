use super::*;
use crate::internal::identity_provider::*;
use anp_identity::host::{PinRootBackupPort, PinRootRecoveryPort};
fn with_pin_identity<T>(
    handle: &DirectIdentityHandle,
    f: impl FnOnce(&anp_identity::ManagedIdentity) -> ProviderResult<T>,
) -> ProviderResult<T> {
    match handle {
        DirectIdentityHandle::Owned(i) => {
            let guard = i.lock().map_err(|_| internal())?;
            f(&guard)
        }
        DirectIdentityHandle::Shared(i) => f(i),
    }
}
fn invalid() -> IdentityProviderError {
    IdentityProviderError::new(IdentityProviderErrorCode::InvalidRequest, false)
}
fn convert<T: serde::de::DeserializeOwned, V: serde::Serialize>(value: V) -> ProviderResult<T> {
    serde_json::from_value(serde_json::to_value(value).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}
fn remote(value: ProviderVerifiedRemoteDocument) -> anp_identity::VerifiedRemoteDocument {
    anp_identity::VerifiedRemoteDocument {
        document: anp_identity::DidDocument::from_value(value.document),
        evidence: anp_identity::VerifiedPublicationEvidence {
            document_version: value.evidence.document_version,
            registry_version: value.evidence.registry_version,
            document_digest: value.evidence.document_digest,
        },
    }
}
pub(super) async fn generation(identity: DirectIdentityHandle) -> ProviderResult<u64> {
    run_blocking(move || {
        with_pin_identity(&identity, |i| {
            i.pin_root_generation().map_err(map_identity_error)
        })
    })
    .await
}
pub(super) async fn export(
    identity: DirectIdentityHandle,
    r: ProviderPinRootBackupRequest,
) -> ProviderResult<ProviderPinRootBackup> {
    run_blocking(move || {
        with_pin_identity(&identity, |i| {
            let b = i
                .export_root_for_pin_backup(anp_identity::host::PinRootBackupRequest {
                    expected_generation: r.expected_generation,
                    root_kid: r.root_kid,
                    user_presence_confirmed: r.user_presence_confirmed,
                })
                .map_err(map_identity_error)?;
            Ok(ProviderPinRootBackup {
                pkcs8_der: zeroize::Zeroizing::new(b.as_pkcs8_der().to_vec()),
                root_kid: b.root_kid,
                root_public_key_multibase: b.root_public_key_multibase,
                generation: b.generation,
            })
        })
    })
    .await
}
pub(super) async fn sign_backup(
    identity: DirectIdentityHandle,
    r: ProviderPinBackupIntentRequest,
) -> ProviderResult<String> {
    run_blocking(move || {
        with_pin_identity(&identity, |i| {
            i.sign_pin_recovery_intent(anp_identity::host::PinBackupIntentRequest {
                expected_generation: r.expected_generation,
                binding: convert(r.binding)?,
                purpose: match r.purpose {
                    ProviderPinBackupPurpose::Authorize => {
                        anp_identity::host::PinBackupPurpose::Authorize
                    }
                    ProviderPinBackupPurpose::BackupCommit => {
                        anp_identity::host::PinBackupPurpose::BackupCommit
                    }
                },
                expected_intent: r.expected_intent,
                challenge: convert(r.challenge)?,
                user_presence_confirmed: r.user_presence_confirmed,
            })
            .map_err(map_identity_error)
        })
    })
    .await
}
struct Pending {
    session: Arc<Mutex<anp_identity::host::PinRootRecoverySession>>,
}
fn proposal(
    p: &anp_identity::host::PinRecoveryProposal,
) -> ProviderResult<ProviderPinRecoveryProposal> {
    Ok(ProviderPinRecoveryProposal {
        identity: p.identity.clone().into(),
        binding: convert(p.binding.clone())?,
        device_proposal: p.device_proposal.clone(),
        generation: p.generation,
        phase: serde_json::to_value(p.phase)
            .map_err(|_| invalid())?
            .as_str()
            .ok_or_else(invalid)?
            .to_owned(),
    })
}
pub(super) async fn begin(
    manager: Arc<Mutex<anp_identity::IdentityManager>>,
    r: ProviderBeginPinRootRecoveryRequest,
) -> ProviderResult<Arc<dyn ProviderPinRootRecoverySession>> {
    run_blocking(move || {
        let session = manager
            .lock()
            .map_err(|_| internal())?
            .begin_pin_root_recovery(anp_identity::host::BeginPinRootRecoveryRequest {
                binding: convert(r.binding)?,
                remote: remote(r.remote),
                root_key: r.root_key,
                profiles: r.profiles,
                device_label: r.device_label,
                user_presence_confirmed: r.user_presence_confirmed,
            })
            .map_err(map_identity_error)?;
        Ok(Arc::new(Pending {
            session: Arc::new(Mutex::new(session)),
        }) as Arc<dyn ProviderPinRootRecoverySession>)
    })
    .await
}
#[async_trait]
impl ProviderPinRootRecoverySession for Pending {
    async fn cancel_pin_root_recovery(
        &self,
        expected_generation: u64,
        binding: ProviderPinRecoveryBinding,
    ) -> ProviderResult<()> {
        let session = self.session.clone();
        run_blocking(move || {
            session
                .lock()
                .map_err(|_| internal())?
                .cancel_pin_root_recovery(anp_identity::host::PinRecoveryCancelRequest {
                    expected_generation,
                    binding: convert(binding)?,
                })
                .map_err(map_identity_error)
        })
        .await
    }

    async fn proposal(&self) -> ProviderResult<ProviderPinRecoveryProposal> {
        let session = self.session.clone();
        run_blocking(move || proposal(session.lock().map_err(|_| internal())?.proposal())).await
    }
    async fn sign_pin_recovery_intent(
        &self,
        r: ProviderPinRecoverySignRequest,
    ) -> ProviderResult<ProviderPinRecoverySignedIntent> {
        let session = self.session.clone();
        run_blocking(move || {
            let signed = session
                .lock()
                .map_err(|_| internal())?
                .sign_pin_recovery_intent(anp_identity::host::PinRecoverySignRequest {
                    expected_generation: r.expected_generation,
                    challenge: convert(r.challenge)?,
                    remote: remote(r.remote),
                    candidate_document: r.candidate_document,
                    user_presence_confirmed: r.user_presence_confirmed,
                })
                .map_err(map_identity_error)?;
            Ok(ProviderPinRecoverySignedIntent {
                challenge: convert(signed.challenge)?,
                root_proof: signed.root_proof,
                device_proof: signed.device_proof,
                signed_document: signed.signed_document,
                generation: signed.generation,
            })
        })
        .await
    }
    async fn complete_pin_root_recovery(
        &self,
        r: ProviderPinRecoveryCompletion,
    ) -> ProviderResult<Option<Arc<dyn IdentitySession>>> {
        let session = self.session.clone();
        run_blocking(move || {
            let managed = session
                .lock()
                .map_err(|_| internal())?
                .complete_pin_root_recovery(anp_identity::host::PinRecoveryCompletion {
                    expected_generation: r.expected_generation,
                    binding: convert(r.binding)?,
                    challenge_id: r.challenge_id,
                    remote: remote(r.remote),
                    outcome: match r.outcome {
                        ProviderPinRecoveryRemoteOutcome::Committed => {
                            anp_identity::host::PinRecoveryRemoteOutcome::Committed
                        }
                        ProviderPinRecoveryRemoteOutcome::NotCommitted => {
                            anp_identity::host::PinRecoveryRemoteOutcome::NotCommitted
                        }
                    },
                })
                .map_err(map_identity_error)?;
            Ok(managed
                .map(|m| Arc::new(DirectAnpIdentitySession::new(m)) as Arc<dyn IdentitySession>))
        })
        .await
    }
}
pub(super) async fn retire(
    manager: Arc<Mutex<anp_identity::IdentityManager>>,
    r: ProviderPinRecoveryV4RetirementRequest,
) -> ProviderResult<ProviderPinRecoveryV4RetirementOutcome> {
    run_blocking(move || {
        let e = r.evidence;
        let outcome = manager
            .lock()
            .map_err(|_| internal())?
            .retire_pin_root_recovery_after_v4(anp_identity::host::PinRecoveryV4RetirementRequest {
                identity: r.identity.into(),
                expected_generation: r.expected_generation,
                binding: convert(r.binding)?,
                scope: match r.scope {
                    ProviderPinRecoveryV4RetirementScope::UnprojectedPinRecovery {
                        never_projected,
                        absent_from_previous_custody,
                    } => anp_identity::host::PinRecoveryV4RetirementScope::UnprojectedPinRecovery {
                        never_projected,
                        absent_from_previous_custody,
                    },
                },
                evidence: anp_identity::host::VerifiedPinRecoveryV4Supersession {
                    v4_operation_id: e.v4_operation_id,
                    v4_intent_hash: e.v4_intent_hash,
                    account_user_id: e.account_user_id,
                    full_handle: e.full_handle,
                    previous_did: e.previous_did,
                    previous_binding_generation: e.previous_binding_generation,
                    successor_did: e.successor_did,
                    successor_binding_generation: e.successor_binding_generation,
                    committed_checkpoint: anp_identity::host::HostDocumentCheckpoint {
                        document_version: e.committed_checkpoint.document_version,
                        registry_version: e.committed_checkpoint.registry_version,
                        document_digest: e.committed_checkpoint.document_digest,
                    },
                    current_binding: convert(e.current_binding)?,
                    current_document: remote(e.current_document),
                },
            })
            .map_err(map_identity_error)?;
        Ok(ProviderPinRecoveryV4RetirementOutcome {
            identity: outcome.identity.into(),
            generation: outcome.generation,
        })
    })
    .await
}
