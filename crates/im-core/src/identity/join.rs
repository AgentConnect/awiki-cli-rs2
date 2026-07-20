//! AWiki-local new-device Join orchestration.
//!
//! These DTOs are first-party control-plane objects, not ANP wire models. The
//! service keeps private keys, pairing material and remote Join tokens inside
//! SecretVault and only returns public requests, proofs, ciphertext and a
//! short-lived SAS. Local approval/cancel phases support restart-safe projection;
//! they are not ANP wire states.

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

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

/// Write-only account-verification grant consumed when a pending device Join
/// is created. The opaque value is never serializable or returned by Core.
pub struct DeviceJoinAccountVerificationGrant {
    secret: crate::internal::platform_secret::SecretBytes,
}

impl DeviceJoinAccountVerificationGrant {
    pub fn from_token(token: impl Into<String>) -> crate::ImResult<Self> {
        Self::from_bytes(token.into().into_bytes())
    }

    pub fn from_bytes(token: Vec<u8>) -> crate::ImResult<Self> {
        if token.is_empty() || token.iter().all(u8::is_ascii_whitespace) {
            return Err(crate::ImError::invalid_input(
                Some("account_verification_grant".to_owned()),
                "account verification grant must not be empty",
            ));
        }
        Ok(Self {
            secret: crate::internal::platform_secret::SecretBytes::from_vec(token),
        })
    }

    fn into_secret(self) -> crate::internal::platform_secret::SecretBytes {
        self.secret
    }
}

impl std::fmt::Debug for DeviceJoinAccountVerificationGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceJoinAccountVerificationGrant(<redacted>)")
    }
}

pub struct DeviceJoinBeginRequest {
    pub operation_id: String,
    pub did: crate::ids::Did,
    pub ttl_seconds: u64,
    pub account_verification_grant: DeviceJoinAccountVerificationGrant,
}

impl std::fmt::Debug for DeviceJoinBeginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinBeginRequest")
            .field("operation_id", &self.operation_id)
            .field("did", &self.did)
            .field("ttl_seconds", &self.ttl_seconds)
            .field("account_verification_grant", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinRole {
    Member,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinAuthorizationStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinRemoteState {
    Pending,
    Claimed,
    ChallengeSent,
    ResponseVerified,
    Consumed,
    Expired,
}

/// Safe host-facing Join session projection. Internal transcript hashes and
/// challenge identifiers intentionally stay in the restart-safe local state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinSessionView {
    pub join_session_id: String,
    pub did: crate::ids::Did,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub side: DeviceJoinSide,
    pub phase: DeviceJoinLocalPhase,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinAuthorizedDeviceSummary {
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub status: DeviceJoinAuthorizationStatus,
    pub role: DeviceJoinRole,
    pub management_ready: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinPendingSummary {
    pub join_session_id: String,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub requested_role: DeviceJoinRole,
    pub issued_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinRegistrySnapshot {
    pub did: crate::ids::Did,
    pub devices: Vec<DeviceJoinAuthorizedDeviceSummary>,
    pub pending_join_requests: Vec<DeviceJoinPendingSummary>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinProgress {
    pub session: DeviceJoinSessionView,
    pub remote_state: DeviceJoinRemoteState,
    pub sas: Option<String>,
    pub authorized_device: Option<DeviceJoinAuthorizedDeviceSummary>,
}

impl std::fmt::Debug for DeviceJoinProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinProgress")
            .field("session", &self.session)
            .field("remote_state", &self.remote_state)
            .field("sas", &self.sas.as_ref().map(|_| "<redacted-sas>"))
            .field("authorized_device", &self.authorized_device)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceJoinApprovalPrompt {
    pub approval_handle: String,
    pub join_session_id: String,
    pub role: DeviceJoinRole,
    pub sas: String,
    pub expires_at: String,
}

impl std::fmt::Debug for DeviceJoinApprovalPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinApprovalPrompt")
            .field("approval_handle", &"<redacted-approval-handle>")
            .field("join_session_id", &self.join_session_id)
            .field("role", &self.role)
            .field("sas", &"<redacted-sas>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

pub struct DeviceJoinConfirmApprovalRequest {
    pub approval_handle: String,
    pub user_presence_confirmed: bool,
}

impl std::fmt::Debug for DeviceJoinConfirmApprovalRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinConfirmApprovalRequest")
            .field("approval_handle", &"<redacted-approval-handle>")
            .field("user_presence_confirmed", &self.user_presence_confirmed)
            .finish()
    }
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

    pub(crate) fn start(
        &self,
        request: DeviceJoinStartRequest,
    ) -> crate::ImResult<DeviceJoinStartResult> {
        crate::internal::identity_device_join::start(self.core, request)
    }

    pub(crate) fn prepare_admin_challenge(
        &self,
        request: DeviceJoinAdminPrepareRequest,
    ) -> crate::ImResult<DeviceJoinAdminPrepareResult> {
        crate::internal::identity_device_join::prepare_admin_challenge(self.core, request)
    }

    pub(crate) fn respond_as_new_device(
        &self,
        request: DeviceJoinNewDeviceRespondRequest,
    ) -> crate::ImResult<DeviceJoinNewDeviceRespondResult> {
        crate::internal::identity_device_join::respond_as_new_device(self.core, request)
    }

    pub(crate) fn verify_response_as_admin(
        &self,
        request: DeviceJoinAdminVerifyRequest,
    ) -> crate::ImResult<DeviceJoinAdminVerifyResult> {
        crate::internal::identity_device_join::verify_response_as_admin(self.core, request)
    }

    pub(crate) fn session(
        &self,
        join_session_id: &str,
        side: DeviceJoinSide,
    ) -> crate::ImResult<DeviceJoinSessionSummary> {
        crate::internal::identity_device_join::session(self.core, join_session_id, side)
    }

    pub fn local_sessions(&self) -> crate::ImResult<Vec<DeviceJoinSessionView>> {
        self.require_enabled()?;
        crate::internal::identity_device_join::list_sessions(self.core)
            .map(|sessions| sessions.into_iter().map(Into::into).collect())
    }

    pub async fn begin_new_device_join(
        &self,
        request: DeviceJoinBeginRequest,
    ) -> crate::ImResult<DeviceJoinProgress> {
        self.require_enabled()?;
        let DeviceJoinBeginRequest {
            operation_id,
            did,
            ttl_seconds,
            account_verification_grant,
        } = request;
        let token = account_verification_grant.into_secret();
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRuntime::production_for_rollout(
                self.core,
                true,
            );
        let session = runtime
            .begin(
                DeviceJoinStartRequest {
                    operation_id,
                    did,
                    ttl_seconds,
                },
                &token,
            )
            .await?;
        Ok(DeviceJoinProgress {
            session: session.into(),
            remote_state: DeviceJoinRemoteState::Pending,
            sas: None,
            authorized_device: None,
        })
    }

    pub async fn poll_new_device_join(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinProgress> {
        self.require_enabled()?;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRuntime::production_for_rollout(
                self.core,
                true,
            );
        public_progress(runtime.advance(join_session_id).await?)
    }

    pub fn cancel_new_device_join(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinSessionView> {
        self.require_enabled()?;
        let runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRuntime::production_for_rollout(
                self.core,
                true,
            );
        runtime.cancel(join_session_id).map(Into::into)
    }

    pub async fn registry(
        &self,
        admin_identity: super::IdentitySelector,
    ) -> crate::ImResult<DeviceJoinRegistrySnapshot> {
        self.require_enabled()?;
        let admin_client = self.core.client_async(admin_identity.clone()).await?;
        let current = self
            .core
            .identities()
            .device_summary_async(admin_identity)
            .await?
            .protocol_device_id;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production_for_rollout(
                self.core,
                &admin_client,
                true,
            );
        public_registry(runtime.registry().await?, current.as_ref())
    }

    pub async fn claim_device_join(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
        operation_id: &str,
        challenge_ttl_seconds: u64,
    ) -> crate::ImResult<DeviceJoinProgress> {
        self.require_enabled()?;
        let admin_client = self.core.client_async(admin_identity).await?;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production_for_rollout(
                self.core,
                &admin_client,
                true,
            );
        runtime
            .claim_and_challenge(join_session_id, operation_id, challenge_ttl_seconds)
            .await
            .and_then(public_progress)
    }

    pub async fn poll_admin_device_join(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinProgress> {
        self.require_enabled()?;
        let admin_client = self.core.client_async(admin_identity).await?;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production_for_rollout(
                self.core,
                &admin_client,
                true,
            );
        public_progress(runtime.advance(join_session_id).await?)
    }

    pub fn prepare_device_join_approval(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
        role: DeviceJoinRole,
        sas_confirmed: bool,
    ) -> crate::ImResult<DeviceJoinApprovalPrompt> {
        self.require_enabled()?;
        if !sas_confirmed {
            return Err(crate::ImError::PermissionDenied);
        }
        let (session, sas, existing) =
            crate::internal::identity_device_join::admin_approval_context(
                self.core,
                join_session_id,
            )?;
        let internal_role = internal_role(role);
        if existing
            .as_ref()
            .is_some_and(|approval| approval.role != internal_role)
        {
            return Err(crate::ImError::invalid_input(
                Some("role".to_owned()),
                "device Join approval role is already bound",
            ));
        }
        let now = OffsetDateTime::now_utc();
        let session_expires_at = parse_approval_expiry(&session.expires_at)?;
        let (operation_id, user_presence_at, approval_expires_at) = match existing {
            Some(approval) => {
                let proof_expires_at =
                    parse_approval_expiry(&approval.authorizing_device_proof.expires_at)?;
                if proof_expires_at <= now {
                    return Err(crate::ImError::SessionExpired);
                }
                (
                    approval.operation_id,
                    Some(approval.pairing_confirmation.user_presence_at),
                    std::cmp::min(session_expires_at, proof_expires_at),
                )
            }
            None => (
                random_public_operation_id("join-approve"),
                None,
                std::cmp::min(
                    session_expires_at,
                    now + Duration::seconds(DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS as i64),
                ),
            ),
        };
        if approval_expires_at <= now {
            return Err(crate::ImError::SessionExpired);
        }
        let approval_expires_at = approval_expires_at.format(&Rfc3339).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?;
        let handle = self.core.inner().device_join_approvals.issue(
            crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleState {
                admin_identity,
                join_session_id: session.join_session_id.clone(),
                operation_id,
                role: internal_role,
                expires_at: approval_expires_at.clone(),
                user_presence_at,
            },
        )?;
        Ok(DeviceJoinApprovalPrompt {
            approval_handle: handle,
            join_session_id: session.join_session_id,
            role,
            sas,
            expires_at: approval_expires_at,
        })
    }

    pub async fn confirm_device_join_approval(
        &self,
        request: DeviceJoinConfirmApprovalRequest,
    ) -> crate::ImResult<DeviceJoinProgress> {
        self.require_enabled()?;
        if !request.user_presence_confirmed {
            self.core
                .inner()
                .device_join_approvals
                .cancel_ready(&request.approval_handle)?;
            return Err(crate::ImError::PermissionDenied);
        }
        let now = OffsetDateTime::now_utc();
        let confirmed_at = now
            .format(&Rfc3339)
            .map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        let claim = self.core.inner().device_join_approvals.claim(
            &request.approval_handle,
            &confirmed_at,
            now,
        )?;
        let state = match claim {
            crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleClaim::Claimed(
                state,
            ) => state,
            crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleClaim::Expired(
                state,
            ) => {
                let admin_client = self.core.client_async(state.admin_identity).await?;
                let mut runtime = crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production_for_rollout(
                    self.core,
                    &admin_client,
                    true,
                );
                return public_progress(
                    runtime
                        .reconcile_expired_approval(&state.join_session_id)
                        .await?,
                );
            }
        };
        let confirmed_at = state
            .user_presence_at
            .clone()
            .ok_or(crate::ImError::PermissionDenied)?;
        let result = async {
            let admin_client = self.core.client_async(state.admin_identity.clone()).await?;
            let mut runtime =
                crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production_for_rollout(
                    self.core,
                    &admin_client,
                    true,
                );
            runtime
                .approve(
                    &state.join_session_id,
                    &state.operation_id,
                    state.role,
                    &confirmed_at,
                    true,
                )
                .await
        }
        .await;
        match &result {
            Ok(_) | Err(crate::ImError::SessionExpired) => self
                .core
                .inner()
                .device_join_approvals
                .consume(&request.approval_handle)?,
            Err(error) if approval_error_is_retryable(error) => {
                let proof_expires_at =
                    crate::internal::identity_device_join::load_prepared_admin_approval(
                        self.core,
                        &state.join_session_id,
                    )
                    .ok()
                    .flatten()
                    .filter(|approval| approval.operation_id == state.operation_id)
                    .map(|approval| approval.authorizing_device_proof.expires_at);
                self.core
                    .inner()
                    .device_join_approvals
                    .release_with_expiry(&request.approval_handle, proof_expires_at.as_deref())?;
            }
            Err(_) => self
                .core
                .inner()
                .device_join_approvals
                .consume(&request.approval_handle)?,
        }
        public_progress(result?)
    }

    pub fn cancel_admin_device_join(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinSessionView> {
        self.require_enabled()?;
        let admin_client = self.core.client(admin_identity)?;
        let runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production_for_rollout(
                self.core,
                &admin_client,
                true,
            );
        runtime.cancel(join_session_id).map(Into::into)
    }

    fn require_enabled(&self) -> crate::ImResult<()> {
        if self.core.inner().device_join_enabled() {
            Ok(())
        } else {
            Err(crate::ImError::unsupported(
                "awiki-multi-device-join-disabled",
            ))
        }
    }
}

fn parse_approval_expiry(value: &str) -> crate::ImResult<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| crate::ImError::LocalStateUnavailable {
        detail: "device Join approval expiry is invalid".to_owned(),
    })
}

fn public_progress(
    value: crate::internal::identity_device_join_runtime::DeviceJoinAdvanceResult,
) -> crate::ImResult<DeviceJoinProgress> {
    let current_device = (value.session.side == DeviceJoinSide::NewDevice)
        .then(|| value.session.protocol_device_id.clone());
    Ok(DeviceJoinProgress {
        session: value.session.into(),
        remote_state: public_remote_state(value.remote_state),
        sas: value.sas,
        authorized_device: value
            .authorization
            .map(|authorization| public_device(authorization.device, current_device.as_ref()))
            .transpose()?,
    })
}

impl From<DeviceJoinSessionSummary> for DeviceJoinSessionView {
    fn from(value: DeviceJoinSessionSummary) -> Self {
        Self {
            join_session_id: value.join_session_id,
            did: value.did,
            protocol_device_id: value.protocol_device_id,
            side: value.side,
            phase: value.phase,
            expires_at: value.expires_at,
        }
    }
}

fn public_registry(
    value: crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry,
    current: Option<&crate::ids::ProtocolDeviceId>,
) -> crate::ImResult<DeviceJoinRegistrySnapshot> {
    Ok(DeviceJoinRegistrySnapshot {
        did: value.did,
        devices: value
            .devices
            .into_iter()
            .map(|device| public_device(device, current))
            .collect::<crate::ImResult<Vec<_>>>()?,
        pending_join_requests: value
            .pending_join_requests
            .into_iter()
            .map(|pending| {
                Ok(DeviceJoinPendingSummary {
                    join_session_id: pending.join_session_id,
                    protocol_device_id: crate::ids::ProtocolDeviceId::parse(pending.device_id)?,
                    signing_key_id: pending.signing_key_id,
                    e2ee_key_id: pending.e2ee_key_id,
                    requested_role: public_role(pending.requested_role),
                    issued_at: pending.issued_at,
                    expires_at: pending.expires_at,
                })
            })
            .collect::<crate::ImResult<Vec<_>>>()?,
    })
}

fn public_device(
    value: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary,
    current: Option<&crate::ids::ProtocolDeviceId>,
) -> crate::ImResult<DeviceJoinAuthorizedDeviceSummary> {
    let protocol_device_id = crate::ids::ProtocolDeviceId::parse(&value.device_id)?;
    Ok(DeviceJoinAuthorizedDeviceSummary {
        is_current: current.is_some_and(|current| current == &protocol_device_id),
        protocol_device_id,
        signing_key_id: value.signing_key_id,
        e2ee_key_id: value.e2ee_key_id,
        status: match value.status {
            crate::internal::identity_device_state::DeviceAuthorizationStatus::Active => {
                DeviceJoinAuthorizationStatus::Active
            }
            crate::internal::identity_device_state::DeviceAuthorizationStatus::Revoked => {
                DeviceJoinAuthorizationStatus::Revoked
            }
        },
        role: public_role(value.role),
        management_ready: value.management_ready,
    })
}

fn public_role(
    value: crate::internal::identity_device_state::DeviceAuthorizationRole,
) -> DeviceJoinRole {
    match value {
        crate::internal::identity_device_state::DeviceAuthorizationRole::Member => {
            DeviceJoinRole::Member
        }
        crate::internal::identity_device_state::DeviceAuthorizationRole::Admin => {
            DeviceJoinRole::Admin
        }
    }
}

fn internal_role(
    value: DeviceJoinRole,
) -> crate::internal::identity_device_state::DeviceAuthorizationRole {
    match value {
        DeviceJoinRole::Member => {
            crate::internal::identity_device_state::DeviceAuthorizationRole::Member
        }
        DeviceJoinRole::Admin => {
            crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
        }
    }
}

fn public_remote_state(
    value: crate::internal::identity_device_join_runtime::DeviceJoinRemoteState,
) -> DeviceJoinRemoteState {
    match value {
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Pending => {
            DeviceJoinRemoteState::Pending
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Claimed => {
            DeviceJoinRemoteState::Claimed
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::ChallengeSent => {
            DeviceJoinRemoteState::ChallengeSent
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::ResponseVerified => {
            DeviceJoinRemoteState::ResponseVerified
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Consumed => {
            DeviceJoinRemoteState::Consumed
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Expired => {
            DeviceJoinRemoteState::Expired
        }
    }
}

fn random_public_operation_id(prefix: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use rand::RngCore as _;

    let mut random = [0_u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut random);
    format!("{prefix}:{}", URL_SAFE_NO_PAD.encode(random))
}

fn approval_error_is_retryable(error: &crate::ImError) -> bool {
    match error {
        crate::ImError::AuthRequired
        | crate::ImError::IdentityVault { .. }
        | crate::ImError::TransportUnavailable { .. }
        | crate::ImError::LocalStateUnavailable { .. }
        | crate::ImError::LocalStateUpgradeInProgress
        | crate::ImError::Io { .. } => true,
        crate::ImError::Service { status_code, .. } => status_code
            .is_none_or(|status| status == 408 || status == 425 || status == 429 || status >= 500),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorized_device_is_current_only_in_new_device_progress() {
        for (side, expected_current) in [
            (DeviceJoinSide::NewDevice, true),
            (DeviceJoinSide::Admin, false),
        ] {
            let did = crate::ids::Did::parse("did:wba:awiki.test:user:alice:e1_test").unwrap();
            let protocol_device_id = crate::ids::ProtocolDeviceId::parse("dev-new").unwrap();
            let progress = public_progress(
                crate::internal::identity_device_join_runtime::DeviceJoinAdvanceResult {
                    session: DeviceJoinSessionSummary {
                        join_session_id: "join-test".to_owned(),
                        did,
                        protocol_device_id,
                        side,
                        phase: DeviceJoinLocalPhase::Authorized,
                        join_request_hash: "sha256:test".to_owned(),
                        challenge_id: None,
                        expires_at: "2026-07-20T00:10:00Z".to_owned(),
                    },
                    remote_state:
                        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Consumed,
                    authorization: Some(
                        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
                            checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
                                document_version: 2,
                                document_hash: "sha256:document".to_owned(),
                                registry_version: 2,
                            },
                            device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                                device_id: "dev-new".to_owned(),
                                signing_key_id: "did:wba:awiki.test:user:alice:e1_test#sign".to_owned(),
                                e2ee_key_id: "did:wba:awiki.test:user:alice:e1_test#e2ee".to_owned(),
                                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Member,
                                management_ready: false,
                                auth_generation: 1,
                            },
                        },
                    ),
                    sas: None,
                },
            )
            .unwrap();

            assert_eq!(
                progress.authorized_device.unwrap().is_current,
                expected_current
            );
        }
    }
}
