//! Frozen awiki.pin-recovery.v1 wire DTOs. Secrets have no Debug implementation.
use serde::{Deserialize, Serialize};
pub(crate) const PROTOCOL: &str = "awiki.pin-recovery.v1";
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ed25519PublicJwk {
    pub(crate) kty: String,
    pub(crate) crv: String,
    pub(crate) x: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    pub(crate) document_version: u64,
    pub(crate) registry_version: u64,
    pub(crate) document_digest: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiptKey {
    pub(crate) kid: String,
    pub(crate) public_jwk: Ed25519PublicJwk,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RealmConfig {
    pub(crate) id: String,
    pub(crate) address: String,
    pub(crate) receipt_keys: Vec<ReceiptKey>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigurationPayload {
    pub(crate) protocol: String,
    pub(crate) deployment_id: String,
    pub(crate) tenant_id: String,
    pub(crate) realm_tenant_name: String,
    pub(crate) configuration_version: u64,
    pub(crate) iat: u64,
    pub(crate) exp: u64,
    pub(crate) control_base_url: String,
    pub(crate) control_audience: String,
    pub(crate) pst_issuer: String,
    pub(crate) realms: Vec<RealmConfig>,
    pub(crate) register_threshold: u64,
    pub(crate) recover_threshold: u64,
    pub(crate) pin_hashing_mode: String,
    pub(crate) pin_format: String,
    pub(crate) num_guesses: u64,
    pub(crate) capability_enabled: bool,
    pub(crate) supported_actions: Vec<String>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigResponse {
    pub(crate) config_jws: String,
    pub(crate) config_hash: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhoneChallengeResponse {
    pub(crate) challenge_id: String,
    pub(crate) expires_at: String,
    pub(crate) resend_after_seconds: u64,
    pub(crate) masked_phone: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhoneDiscoveryResponse {
    pub(crate) discovery_token: String,
    pub(crate) token_type: String,
    pub(crate) expires_in: u64,
    pub(crate) session_id: String,
    pub(crate) expires_at: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhoneSessionResponse {
    pub(crate) access_token: String,
    pub(crate) token_type: String,
    pub(crate) expires_in: u64,
    pub(crate) session_id: String,
    pub(crate) expires_at: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Target {
    pub(crate) target_id: String,
    pub(crate) full_handle: String,
    pub(crate) did: String,
    pub(crate) binding_generation: String,
    pub(crate) recovery_id: Option<String>,
    pub(crate) active_epoch: Option<u64>,
    pub(crate) root_kid: String,
    pub(crate) root_public_jwk: Ed25519PublicJwk,
    pub(crate) can_enable: bool,
    pub(crate) can_recover: bool,
    pub(crate) unavailable_reason: Option<String>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetsResponse {
    pub(crate) targets: Vec<Target>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootChallengePayload {
    pub(crate) protocol: String,
    pub(crate) purpose: String,
    pub(crate) aud: String,
    pub(crate) operation_id: String,
    pub(crate) challenge_id: String,
    pub(crate) nonce: String,
    pub(crate) iat: u64,
    pub(crate) exp: u64,
    pub(crate) account_user_id: String,
    pub(crate) recovery_id: String,
    pub(crate) backup_epoch: u64,
    pub(crate) slot_id: String,
    pub(crate) did: String,
    pub(crate) root_kid: String,
    pub(crate) root_jkt: String,
    pub(crate) op_jkt: String,
    pub(crate) binding_generation: String,
    pub(crate) document_version: u64,
    pub(crate) registry_version: u64,
    pub(crate) document_digest: String,
    pub(crate) intent_hash: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    pub(crate) kind: String,
    pub(crate) operation_id: String,
    pub(crate) recovery_id: String,
    pub(crate) backup_epoch: u64,
    pub(crate) slot_id: String,
    pub(crate) expected_active_epoch: Option<u64>,
    pub(crate) backup_hash: Option<String>,
    pub(crate) config_hash: String,
    pub(crate) registration_version: Option<String>,
    pub(crate) registered_receipt_ids: Vec<String>,
    pub(crate) verified_receipt_ids: Vec<String>,
    pub(crate) new_device_proposal_hash: Option<String>,
    pub(crate) candidate_document_digest: Option<String>,
    pub(crate) binding_generation: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Challenge {
    pub(crate) payload: RootChallengePayload,
    pub(crate) intent: Intent,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RemoteDeviceResult {
    pub(crate) operation_id: String,
    pub(crate) did: String,
    pub(crate) device_id: String,
    pub(crate) root_kid: String,
    pub(crate) signing_kid: String,
    pub(crate) agreement_kid: String,
    pub(crate) checkpoint: Checkpoint,
    pub(crate) committed_document: serde_json::Value,
    pub(crate) committed_at: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationResponse {
    pub(crate) protocol: String,
    pub(crate) operation_id: String,
    pub(crate) kind: String,
    pub(crate) state: String,
    pub(crate) operation_version: u64,
    pub(crate) allowed_actions: Vec<String>,
    pub(crate) target_id: String,
    pub(crate) did: String,
    pub(crate) binding_generation: String,
    pub(crate) recovery_id: String,
    pub(crate) backup_epoch: u64,
    pub(crate) slot_id: String,
    pub(crate) expected_active_epoch: Option<u64>,
    pub(crate) registration_version: Option<String>,
    pub(crate) backup_hash: Option<String>,
    pub(crate) config_hash: String,
    pub(crate) op_jkt: String,
    pub(crate) authorization_epoch: u64,
    pub(crate) guess_cycle: u64,
    pub(crate) guesses_used: u64,
    pub(crate) guesses_limit: u64,
    pub(crate) registered_realms: Vec<String>,
    pub(crate) verified_realms: Vec<String>,
    pub(crate) cleanup_remaining: u64,
    pub(crate) expires_at: String,
    pub(crate) authorization_challenge: Option<Challenge>,
    pub(crate) commit_challenge: Option<Challenge>,
    pub(crate) device_result: Option<RemoteDeviceResult>,
    pub(crate) recovery_checkpoint: Option<Checkpoint>,
    pub(crate) recovered_device_authorization: Option<RecoveredDeviceAuthorization>,
    pub(crate) last_error_code: Option<String>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupHeader {
    pub(crate) format: String,
    pub(crate) deployment_id: String,
    pub(crate) tenant_id: String,
    pub(crate) recovery_id: String,
    pub(crate) backup_epoch: u64,
    pub(crate) slot_id: String,
    pub(crate) did: String,
    pub(crate) root_kid: String,
    pub(crate) root_public_jwk: Ed25519PublicJwk,
    pub(crate) document_digest_at_backup: String,
    pub(crate) config_hash: String,
    pub(crate) created_at: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupEnvelope {
    pub(crate) header: BackupHeader,
    pub(crate) algorithm: String,
    pub(crate) nonce_b64u: String,
    pub(crate) ciphertext_b64u: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupResponse {
    pub(crate) backup_hash: String,
    pub(crate) envelope: BackupEnvelope,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RealmToken {
    pub(crate) realm_id: String,
    pub(crate) access_token: String,
    pub(crate) token_type: String,
    pub(crate) expires_at: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttemptResponse {
    pub(crate) attempt_id: String,
    pub(crate) operation_id: String,
    pub(crate) action: String,
    pub(crate) expires_at: String,
    pub(crate) slot_id: String,
    pub(crate) backup_epoch: u64,
    pub(crate) registration_version: Option<String>,
    pub(crate) cleanup_job_id: Option<String>,
    pub(crate) realm_tokens: Vec<RealmToken>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceProposal {
    pub(crate) device_id: String,
    pub(crate) device_label: String,
    pub(crate) signing_kid: String,
    pub(crate) signing_public_jwk: Ed25519PublicJwk,
    pub(crate) agreement_kid: String,
    pub(crate) agreement_public_jwk: Ed25519PublicJwk,
    pub(crate) profiles: Vec<String>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceChallengeResponse {
    pub(crate) proposal: DeviceProposal,
    pub(crate) current_document: serde_json::Value,
    pub(crate) candidate_document: serde_json::Value,
    pub(crate) checkpoint: Checkpoint,
    pub(crate) challenge: Challenge,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RealmReceiptClaims {
    pub(crate) iss: String,
    pub(crate) aud: String,
    pub(crate) iat: u64,
    pub(crate) exp: u64,
    pub(crate) jti: String,
    pub(crate) realm_id: String,
    pub(crate) tenant_id: String,
    pub(crate) operation_id: String,
    pub(crate) attempt_id: String,
    pub(crate) slot_id: String,
    pub(crate) backup_epoch: u64,
    pub(crate) phase: String,
    pub(crate) registration_version: String,
    pub(crate) backup_hash: String,
    pub(crate) request_sha256: String,
    pub(crate) record_revision: u64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveredDeviceAuthorization {
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) status: String,
    pub(crate) role: String,
    pub(crate) management_ready: bool,
    pub(crate) auth_generation: u64,
}
