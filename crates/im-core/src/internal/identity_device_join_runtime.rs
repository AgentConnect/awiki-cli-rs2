//! Typed remote boundary for the AWiki-local device Join runtime.
//!
//! The runtime consumes only the six frozen Join RPC operations plus the
//! authenticated Registry projection. Session/account tokens cross this
//! boundary as zeroizing secret bytes. Implementations and test doubles must
//! never retain those bytes in call traces, errors or Debug output.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::identity::{
    DeviceJoinChallenge, DeviceJoinChallengeResponse, DeviceJoinRequest, DeviceProof,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
};
use crate::internal::platform_secret::SecretBytes;
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeviceJoinRemoteState {
    Pending,
    Claimed,
    ChallengeSent,
    ResponseVerified,
    Consumed,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteDeviceSummary {
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) status: DeviceAuthorizationStatus,
    pub(crate) role: DeviceAuthorizationRole,
    pub(crate) management_ready: bool,
    pub(crate) auth_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemotePendingSummary {
    pub(crate) join_session_id: String,
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) requested_role: DeviceAuthorizationRole,
    pub(crate) issued_at: String,
    pub(crate) expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteRegistry {
    pub(crate) did: crate::ids::Did,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) devices: Vec<DeviceJoinRemoteDeviceSummary>,
    pub(crate) pending_join_requests: Vec<DeviceJoinRemotePendingSummary>,
}

pub(crate) struct DeviceJoinRemoteCreateRequest<'a> {
    pub(crate) operation_id: &'a str,
    pub(crate) account_verification_token: &'a SecretBytes,
    pub(crate) join_request: &'a DeviceJoinRequest,
}

impl std::fmt::Debug for DeviceJoinRemoteCreateRequest<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinRemoteCreateRequest")
            .field("operation_id", &self.operation_id)
            .field("account_verification_token", &"<redacted>")
            .field("join_session_id", &self.join_request.join_session_id)
            .finish()
    }
}

pub(crate) struct DeviceJoinRemoteCreateResult {
    pub(crate) join_session_id: String,
    pub(crate) join_session_token: SecretBytes,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) expires_at: String,
}

impl std::fmt::Debug for DeviceJoinRemoteCreateResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinRemoteCreateResult")
            .field("join_session_id", &self.join_session_id)
            .field("join_session_token", &"<redacted>")
            .field("state", &self.state)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceJoinRemoteNewDeviceStatus {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) expires_at: String,
    pub(crate) challenge: Option<DeviceJoinChallenge>,
    pub(crate) authorization: Option<DeviceJoinRemoteAuthorization>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceJoinRemoteAdminStatus {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) expires_at: String,
    pub(crate) challenge: Option<DeviceJoinChallenge>,
    pub(crate) challenge_response: Option<DeviceJoinChallengeResponse>,
    pub(crate) authorization: Option<DeviceJoinRemoteAuthorization>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteAuthorization {
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) device: DeviceJoinRemoteDeviceSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceJoinRemoteClaimRequest<'a> {
    pub(crate) operation_id: &'a str,
    pub(crate) join_session_id: &'a str,
    pub(crate) authorizing_device_id: &'a str,
    pub(crate) authorizing_device_proof: &'a DeviceProof,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceJoinRemoteClaimResult {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) claimed_by_device_id: String,
    pub(crate) claim_expires_at: String,
    pub(crate) join_request: DeviceJoinRequest,
}

pub(crate) struct DeviceJoinRemoteResponseRequest<'a> {
    pub(crate) join_session_token: &'a SecretBytes,
    pub(crate) response: &'a DeviceJoinChallengeResponse,
}

impl std::fmt::Debug for DeviceJoinRemoteResponseRequest<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinRemoteResponseRequest")
            .field("join_session_token", &"<redacted>")
            .field("operation_id", &self.response.operation_id)
            .field("join_session_id", &self.response.join_session_id)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DeviceJoinRemotePairingConfirmation {
    pub(crate) join_request_hash: String,
    pub(crate) pairing_transcript_hash: String,
    pub(crate) sas_confirmed: bool,
    pub(crate) user_presence_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceJoinRemoteApproveRequest<'a> {
    pub(crate) operation_id: &'a str,
    pub(crate) join_session_id: &'a str,
    pub(crate) expected_checkpoint: &'a IdentityInternalCheckpoint,
    pub(crate) role: DeviceAuthorizationRole,
    pub(crate) new_document: &'a Value,
    pub(crate) pairing_confirmation: &'a DeviceJoinRemotePairingConfirmation,
    pub(crate) authorizing_device_id: &'a str,
    pub(crate) authorizing_device_proof: &'a DeviceProof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteTransitionResult {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteChallengeResult {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) challenge_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteApproveResult {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) device: DeviceJoinRemoteDeviceSummary,
}

/// High-level seam between local Join orchestration and the AWiki user-service.
///
/// A production adapter maps these methods to the frozen Registry method and
/// six Join RPC names. A fake can exercise the local state machine without a
/// live service, but must only record redacted metadata.
pub(crate) trait DeviceJoinRemote {
    async fn registry(
        &mut self,
        did: &crate::ids::Did,
        include_pending_join_requests: bool,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry>;

    async fn create(
        &mut self,
        request: DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteCreateResult>;

    async fn status_as_new_device(
        &mut self,
        expected_join_session_id: &str,
        join_session_token: &SecretBytes,
    ) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus>;

    async fn status_as_admin(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinRemoteAdminStatus>;

    async fn claim(
        &mut self,
        request: DeviceJoinRemoteClaimRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteClaimResult>;

    async fn submit_challenge(
        &mut self,
        challenge: &DeviceJoinChallenge,
    ) -> crate::ImResult<DeviceJoinRemoteChallengeResult>;

    async fn submit_response(
        &mut self,
        request: DeviceJoinRemoteResponseRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult>;

    async fn approve(
        &mut self,
        request: DeviceJoinRemoteApproveRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteApproveResult>;
}

pub(crate) struct DeviceJoinHttpAdapter<P, A> {
    plain: P,
    authenticated: A,
}

impl<P, A> DeviceJoinHttpAdapter<P, A> {
    pub(crate) fn new(plain: P, authenticated: A) -> Self {
        Self {
            plain,
            authenticated,
        }
    }
}

impl<'a>
    DeviceJoinHttpAdapter<
        crate::internal::transport::CorePlainTransport<'a>,
        crate::internal::transport::CoreHttpTransport<'a>,
    >
{
    pub(crate) fn production(
        core: &'a crate::core::ImCore,
        admin_client: &'a crate::core::ImClient,
    ) -> Self {
        Self::new(
            crate::internal::transport::CorePlainTransport::new(core),
            crate::internal::transport::CoreHttpTransport::new(admin_client),
        )
    }
}

impl<P, A> DeviceJoinRemote for DeviceJoinHttpAdapter<P, A>
where
    P: AsyncRpcTransport,
    A: AsyncAuthenticatedRpcTransport,
{
    async fn registry(
        &mut self,
        did: &crate::ids::Did,
        include_pending_join_requests: bool,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        let call = crate::internal::identity_wire::device_join::build_registry_call(
            did,
            include_pending_join_requests,
        );
        let raw = self
            .authenticated
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_registry_result(
            raw,
            did,
            include_pending_join_requests,
        )
    }

    async fn create(
        &mut self,
        request: DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteCreateResult> {
        let expected_join_session_id = request.join_request.join_session_id.clone();
        let call = crate::internal::identity_wire::device_join::build_create_call(request)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_create_result(
            raw,
            &expected_join_session_id,
        )
    }

    async fn status_as_new_device(
        &mut self,
        expected_join_session_id: &str,
        join_session_token: &SecretBytes,
    ) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus> {
        let call =
            crate::internal::identity_wire::device_join::build_new_status_call(join_session_token)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_new_status_result(
            raw,
            expected_join_session_id,
        )
    }

    async fn status_as_admin(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinRemoteAdminStatus> {
        let call =
            crate::internal::identity_wire::device_join::build_admin_status_call(join_session_id)?;
        let raw = self
            .authenticated
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_admin_status_result(raw, join_session_id)
    }

    async fn claim(
        &mut self,
        request: DeviceJoinRemoteClaimRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteClaimResult> {
        let expected_join_session_id = request.join_session_id.to_owned();
        let call = crate::internal::identity_wire::device_join::build_claim_call(request)?;
        let raw = self
            .authenticated
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_claim_result(
            raw,
            &expected_join_session_id,
        )
    }

    async fn submit_challenge(
        &mut self,
        challenge: &DeviceJoinChallenge,
    ) -> crate::ImResult<DeviceJoinRemoteChallengeResult> {
        let call = crate::internal::identity_wire::device_join::build_challenge_call(challenge)?;
        let raw = self
            .authenticated
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_challenge_result(raw, challenge)
    }

    async fn submit_response(
        &mut self,
        request: DeviceJoinRemoteResponseRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        let response = request.response.clone();
        let call = crate::internal::identity_wire::device_join::build_response_call(request)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_response_result(raw, &response)
    }

    async fn approve(
        &mut self,
        request: DeviceJoinRemoteApproveRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteApproveResult> {
        let expected_join_session_id = request.join_session_id.to_owned();
        let call = crate::internal::identity_wire::device_join::build_approve_call(request)?;
        let raw = self
            .authenticated
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_approve_result(
            raw,
            &expected_join_session_id,
        )
    }
}

#[cfg(test)]
mod tests;
