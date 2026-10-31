//! Rust-only capability DTOs. Root-bearing requests deliberately lack serde/Debug.
use super::*;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPinRecoveryBinding {
    pub operation_id: String,
    pub account_user_id: String,
    pub target_id: String,
    pub handle: String,
    pub did: String,
    pub domain: String,
    pub binding_generation: String,
    pub recovery_id: String,
    pub backup_epoch: u64,
    pub slot_id: String,
    pub op_jkt: String,
    pub root_kid: String,
    pub root_public_key_multibase: String,
    pub encrypted_package_hash: Option<String>,
    pub config_hash: String,
    pub audience: String,
}
pub struct ProviderPinRootBackupRequest {
    pub expected_generation: u64,
    pub root_kid: String,
    pub user_presence_confirmed: bool,
}
pub struct ProviderPinRootBackup {
    pub pkcs8_der: Zeroizing<Vec<u8>>,
    pub root_kid: String,
    pub root_public_key_multibase: String,
    pub generation: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPinRecoveryChallenge {
    pub payload: Value,
    pub intent: Value,
}
#[derive(Clone, Copy)]
pub enum ProviderPinBackupPurpose {
    Authorize,
    BackupCommit,
}
pub struct ProviderPinBackupIntentRequest {
    pub expected_generation: u64,
    pub binding: ProviderPinRecoveryBinding,
    pub purpose: ProviderPinBackupPurpose,
    pub expected_intent: Value,
    pub challenge: ProviderPinRecoveryChallenge,
    pub user_presence_confirmed: bool,
}
pub struct ProviderBeginPinRootRecoveryRequest {
    pub binding: ProviderPinRecoveryBinding,
    pub remote: ProviderVerifiedRemoteDocument,
    pub root_key: Option<Zeroizing<Vec<u8>>>,
    pub profiles: Vec<String>,
    pub device_label: String,
    pub user_presence_confirmed: bool,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPinRecoveryProposal {
    pub identity: ProviderIdentityRef,
    pub binding: ProviderPinRecoveryBinding,
    pub device_proposal: Value,
    pub generation: u64,
    pub phase: String,
}
pub struct ProviderPinRecoverySignRequest {
    pub expected_generation: u64,
    pub challenge: ProviderPinRecoveryChallenge,
    pub remote: ProviderVerifiedRemoteDocument,
    pub candidate_document: Value,
    pub user_presence_confirmed: bool,
}
pub struct ProviderPinRecoverySignedIntent {
    pub challenge: ProviderPinRecoveryChallenge,
    pub root_proof: String,
    pub device_proof: String,
    pub signed_document: Value,
    pub generation: u64,
}
#[derive(Clone, Copy)]
pub enum ProviderPinRecoveryRemoteOutcome {
    Committed,
    NotCommitted,
}
pub struct ProviderPinRecoveryCompletion {
    pub expected_generation: u64,
    pub binding: ProviderPinRecoveryBinding,
    pub challenge_id: String,
    pub remote: ProviderVerifiedRemoteDocument,
    pub outcome: ProviderPinRecoveryRemoteOutcome,
}
#[async_trait]
pub trait ProviderPinRootRecoverySession: Send + Sync {
    async fn cancel_pin_root_recovery(
        &self,
        _expected_generation: u64,
        _binding: ProviderPinRecoveryBinding,
    ) -> ProviderResult<()> {
        Err(IdentityProviderError::new(
            IdentityProviderErrorCode::CapabilityUnavailable,
            false,
        ))
    }
    async fn proposal(&self) -> ProviderResult<ProviderPinRecoveryProposal>;
    async fn sign_pin_recovery_intent(
        &self,
        request: ProviderPinRecoverySignRequest,
    ) -> ProviderResult<ProviderPinRecoverySignedIntent>;
    async fn complete_pin_root_recovery(
        &self,
        request: ProviderPinRecoveryCompletion,
    ) -> ProviderResult<Option<Arc<dyn IdentitySession>>>;
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPinRecoveryCurrentBinding {
    pub full_handle: String,
    pub did: String,
    pub binding_generation: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderVerifiedPinRecoveryV4Supersession {
    pub v4_operation_id: String,
    pub v4_intent_hash: String,
    pub account_user_id: String,
    pub full_handle: String,
    pub previous_did: String,
    pub previous_binding_generation: String,
    pub successor_did: String,
    pub successor_binding_generation: String,
    pub committed_checkpoint: ProviderDocumentCheckpoint,
    pub current_binding: ProviderPinRecoveryCurrentBinding,
    pub current_document: ProviderVerifiedRemoteDocument,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderPinRecoveryV4RetirementScope {
    UnprojectedPinRecovery {
        never_projected: bool,
        absent_from_previous_custody: bool,
    },
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPinRecoveryV4RetirementRequest {
    pub identity: ProviderIdentityRef,
    pub expected_generation: u64,
    pub binding: ProviderPinRecoveryBinding,
    pub scope: ProviderPinRecoveryV4RetirementScope,
    pub evidence: ProviderVerifiedPinRecoveryV4Supersession,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPinRecoveryV4RetirementOutcome {
    pub identity: ProviderIdentityRef,
    pub generation: u64,
}
