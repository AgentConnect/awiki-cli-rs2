//! AWiki-local new-device Join orchestration.
//!
//! These DTOs are first-party control-plane objects, not ANP wire models. The
//! service keeps private keys, pairing material and remote Join tokens inside
//! SecretVault and only returns public requests, proofs, ciphertext and a
//! short-lived SAS. Local approval/cancel phases support restart-safe projection;
//! they are not ANP wire states.

use serde::{Deserialize, Serialize};

pub const DEVICE_JOIN_REQUEST_TYPE: &str = "awiki.device.join.v1";
pub const DEVICE_PROOF_TYPE: &str = "awiki-device-signature-v1";
pub const DEVICE_JOIN_CHALLENGE_ALGORITHM: &str = "X25519-HKDF-SHA256-CHACHA20POLY1305";
pub const DEVICE_JOIN_MAX_TTL_SECONDS: u64 = 600;
pub const DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS: u64 = 300;

pub const DEVICE_JOIN_VNEXT_PROFILES: &[&str] = &[
    anp::authentication::PROFILE_CORE_BINDING_V2,
    anp::authentication::PROFILE_IDENTITY_DISCOVERY_V2,
    anp::authentication::PROFILE_DIRECT_BASE_V2,
    anp::authentication::PROFILE_DIRECT_E2EE_V2,
    anp::authentication::PROFILE_GROUP_BASE_V2,
    anp::authentication::PROFILE_GROUP_E2EE_V2,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinSide {
    NewDevice,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinLocalPhase {
    Pending,
    ChallengePrepared,
    ResponsePrepared,
    ResponseVerified,
    ApprovalPrepared,
    Authorized,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinSessionSummary {
    pub join_session_id: String,
    pub did: crate::ids::Did,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub side: DeviceJoinSide,
    pub phase: DeviceJoinLocalPhase,
    pub join_request_hash: String,
    pub challenge_id: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinStartRequest {
    pub operation_id: String,
    pub did: crate::ids::Did,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceJoinRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    pub did: String,
    pub join_session_id: String,
    pub device_id: String,
    pub signing_public_key: serde_json::Value,
    pub e2ee_public_key: serde_json::Value,
    pub pairing_public_key: String,
    pub profiles: Vec<String>,
    pub requested_role: String,
    pub issued_at: String,
    pub expires_at: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinStartResult {
    pub session: DeviceJoinSessionSummary,
    pub join_request: DeviceJoinRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceProof {
    #[serde(rename = "type")]
    pub proof_type: String,
    pub key_id: String,
    pub created_at: String,
    pub expires_at: String,
    pub nonce: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedJoinChallenge {
    pub algorithm: String,
    pub nonce_b64u: String,
    pub ciphertext_b64u: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceJoinChallenge {
    pub operation_id: String,
    pub join_session_id: String,
    pub challenge_id: String,
    pub admin_device_id: String,
    pub admin_pairing_public_key: String,
    pub ciphertext: EncryptedJoinChallenge,
    pub challenge_expires_at: String,
    pub authorizing_device_proof: DeviceProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceJoinChallengeResponse {
    pub operation_id: String,
    pub join_session_id: String,
    pub challenge_id: String,
    pub challenge_hash: String,
    pub join_request_hash: String,
    pub pairing_transcript_hash: String,
    pub new_device_proof: DeviceProof,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinAdminPrepareRequest {
    pub admin_identity: super::IdentitySelector,
    pub operation_id: String,
    pub join_request: DeviceJoinRequest,
    pub challenge_ttl_seconds: u64,
    pub document_version: u64,
    pub document_hash: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinAdminPrepareResult {
    pub session: DeviceJoinSessionSummary,
    pub challenge: DeviceJoinChallenge,
    pub sas: String,
}

impl std::fmt::Debug for DeviceJoinAdminPrepareResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinAdminPrepareResult")
            .field("session", &self.session)
            .field("challenge", &self.challenge)
            .field("sas", &"<redacted-sas>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinNewDeviceRespondRequest {
    pub operation_id: String,
    pub challenge: DeviceJoinChallenge,
    pub admin_did_document: serde_json::Value,
    pub document_version: u64,
    pub document_hash: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinNewDeviceRespondResult {
    pub session: DeviceJoinSessionSummary,
    pub response: DeviceJoinChallengeResponse,
    pub sas: String,
}

impl std::fmt::Debug for DeviceJoinNewDeviceRespondResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinNewDeviceRespondResult")
            .field("session", &self.session)
            .field("response", &self.response)
            .field("sas", &"<redacted-sas>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinAdminVerifyRequest {
    pub operation_id: String,
    pub join_session_id: String,
    pub response: DeviceJoinChallengeResponse,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinAdminVerifyResult {
    pub session: DeviceJoinSessionSummary,
    pub join_request_hash: String,
    pub pairing_transcript_hash: String,
    pub sas: String,
}

impl std::fmt::Debug for DeviceJoinAdminVerifyResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinAdminVerifyResult")
            .field("session", &self.session)
            .field("join_request_hash", &self.join_request_hash)
            .field("pairing_transcript_hash", &self.pairing_transcript_hash)
            .field("sas", &"<redacted-sas>")
            .finish()
    }
}

pub struct DeviceJoinService<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> DeviceJoinService<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    pub fn start(&self, request: DeviceJoinStartRequest) -> crate::ImResult<DeviceJoinStartResult> {
        crate::internal::identity_device_join::start(self.core, request)
    }

    pub fn prepare_admin_challenge(
        &self,
        request: DeviceJoinAdminPrepareRequest,
    ) -> crate::ImResult<DeviceJoinAdminPrepareResult> {
        crate::internal::identity_device_join::prepare_admin_challenge(self.core, request)
    }

    pub fn respond_as_new_device(
        &self,
        request: DeviceJoinNewDeviceRespondRequest,
    ) -> crate::ImResult<DeviceJoinNewDeviceRespondResult> {
        crate::internal::identity_device_join::respond_as_new_device(self.core, request)
    }

    pub fn verify_response_as_admin(
        &self,
        request: DeviceJoinAdminVerifyRequest,
    ) -> crate::ImResult<DeviceJoinAdminVerifyResult> {
        crate::internal::identity_device_join::verify_response_as_admin(self.core, request)
    }

    pub fn session(
        &self,
        join_session_id: &str,
        side: DeviceJoinSide,
    ) -> crate::ImResult<DeviceJoinSessionSummary> {
        crate::internal::identity_device_join::session(self.core, join_session_id, side)
    }
}
