//! Production orchestration and typed remote boundary for AWiki device Join.
//!
//! The two orchestration paths advance the existing restart-safe local Join
//! state; they do not maintain a second phase machine. They consume only the
//! six frozen Join RPC operations, the authenticated Registry projection and
//! standard DID resolution. Tokens cross the boundary as zeroizing bytes and
//! are sealed before a caller can resume the flow. The explicit rollout gate
//! defaults to disabled and fails closed before local or remote side effects.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::identity::{
    DeviceJoinChallenge, DeviceJoinChallengeResponse, DeviceJoinRequest, DeviceProof,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
};
use crate::internal::platform_secret::SecretBytes;
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRawJsonTransport, AsyncRpcTransport,
};

#[derive(Debug, Clone)]
pub(crate) struct DeviceJoinApprovalHandleState {
    pub(crate) admin_identity: crate::identity::IdentitySelector,
    pub(crate) join_session_id: String,
    pub(crate) operation_id: String,
    pub(crate) role: DeviceAuthorizationRole,
    pub(crate) expires_at: String,
    pub(crate) user_presence_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceJoinApprovalHandleLease {
    Ready,
    InFlight,
}

#[derive(Debug, Clone)]
struct DeviceJoinApprovalHandleEntry {
    state: DeviceJoinApprovalHandleState,
    lease: DeviceJoinApprovalHandleLease,
}

#[derive(Debug, Clone)]
pub(crate) enum DeviceJoinApprovalHandleClaim {
    Claimed(DeviceJoinApprovalHandleState),
    Expired(DeviceJoinApprovalHandleState),
}

#[derive(Default)]
pub(crate) struct DeviceJoinApprovalHandleStore {
    entries: std::sync::Mutex<HashMap<String, DeviceJoinApprovalHandleEntry>>,
}

impl std::fmt::Debug for DeviceJoinApprovalHandleStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.entries.lock().map(|entries| entries.len()).ok();
        f.debug_struct("DeviceJoinApprovalHandleStore")
            .field("entry_count", &count)
            .finish()
    }
}

impl DeviceJoinApprovalHandleStore {
    pub(crate) fn issue(&self, state: DeviceJoinApprovalHandleState) -> crate::ImResult<String> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        use rand::RngCore as _;

        let mut random = [0_u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut random);
        let handle = format!("approval_{}", URL_SAFE_NO_PAD.encode(random));
        let mut entries =
            self.entries
                .lock()
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "device Join approval handle lock poisoned".to_owned(),
                })?;
        if entries.values().any(|existing| {
            existing.state.join_session_id == state.join_session_id
                && existing.state.admin_identity == state.admin_identity
                && existing.lease == DeviceJoinApprovalHandleLease::InFlight
        }) {
            return Err(crate::ImError::PermissionDenied);
        }
        entries.retain(|_, existing| {
            existing.state.join_session_id != state.join_session_id
                || existing.state.admin_identity != state.admin_identity
        });
        entries.insert(
            handle.clone(),
            DeviceJoinApprovalHandleEntry {
                state,
                lease: DeviceJoinApprovalHandleLease::Ready,
            },
        );
        Ok(handle)
    }

    pub(crate) fn claim(
        &self,
        handle: &str,
        confirmed_at: &str,
        now: time::OffsetDateTime,
    ) -> crate::ImResult<DeviceJoinApprovalHandleClaim> {
        let mut entries =
            self.entries
                .lock()
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "device Join approval handle lock poisoned".to_owned(),
                })?;
        let expires_at = entries
            .get(handle)
            .ok_or(crate::ImError::PermissionDenied)?
            .state
            .expires_at
            .clone();
        let expires_at = time::OffsetDateTime::parse(
            &expires_at,
            &time::format_description::well_known::Rfc3339,
        );
        let Ok(expires_at) = expires_at else {
            entries.remove(handle);
            return Err(crate::ImError::PermissionDenied);
        };
        if expires_at <= now {
            let expired = entries
                .remove(handle)
                .ok_or(crate::ImError::PermissionDenied)?;
            return Ok(DeviceJoinApprovalHandleClaim::Expired(expired.state));
        }

        let entry = entries
            .get_mut(handle)
            .ok_or(crate::ImError::PermissionDenied)?;
        if entry.lease != DeviceJoinApprovalHandleLease::Ready {
            return Err(crate::ImError::PermissionDenied);
        }
        if entry.state.user_presence_at.is_none() {
            entry.state.user_presence_at = Some(confirmed_at.to_owned());
        }
        entry.lease = DeviceJoinApprovalHandleLease::InFlight;
        Ok(DeviceJoinApprovalHandleClaim::Claimed(entry.state.clone()))
    }

    pub(crate) fn release(&self, handle: &str) -> crate::ImResult<()> {
        self.release_with_expiry(handle, None)
    }

    pub(crate) fn release_with_expiry(
        &self,
        handle: &str,
        proof_expires_at: Option<&str>,
    ) -> crate::ImResult<()> {
        let mut entries =
            self.entries
                .lock()
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "device Join approval handle lock poisoned".to_owned(),
                })?;
        let entry = entries
            .get_mut(handle)
            .ok_or(crate::ImError::PermissionDenied)?;
        if entry.lease != DeviceJoinApprovalHandleLease::InFlight {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(proof_expires_at) = proof_expires_at {
            time::OffsetDateTime::parse(
                proof_expires_at,
                &time::format_description::well_known::Rfc3339,
            )
            .map_err(|_| crate::ImError::PermissionDenied)?;
            entry.state.expires_at = proof_expires_at.to_owned();
        }
        entry.lease = DeviceJoinApprovalHandleLease::Ready;
        Ok(())
    }

    pub(crate) fn cancel_ready(&self, handle: &str) -> crate::ImResult<()> {
        let mut entries =
            self.entries
                .lock()
                .map_err(|_| crate::ImError::LocalStateUnavailable {
                    detail: "device Join approval handle lock poisoned".to_owned(),
                })?;
        let entry = entries
            .get(handle)
            .ok_or(crate::ImError::PermissionDenied)?;
        if entry.lease != DeviceJoinApprovalHandleLease::Ready {
            return Err(crate::ImError::PermissionDenied);
        }
        entries.remove(handle);
        Ok(())
    }

    pub(crate) fn consume(&self, handle: &str) -> crate::ImResult<()> {
        self.entries
            .lock()
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "device Join approval handle lock poisoned".to_owned(),
            })?
            .remove(handle);
        Ok(())
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// New-device RPC seam. This intentionally cannot issue authenticated admin
/// calls, so a pending device never gains an ambient device credential.
pub(crate) trait DeviceJoinNewDeviceRemote {
    async fn create(
        &mut self,
        request: DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteCreateResult>;

    async fn status(
        &mut self,
        expected_join_session_id: &str,
        join_session_token: &SecretBytes,
    ) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus>;

    async fn submit_response(
        &mut self,
        request: DeviceJoinRemoteResponseRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult>;

    async fn issue_device_token(
        &mut self,
        prepared: &crate::internal::identity_wire::device_genesis::PreparedDeviceTokenIssue,
        expected_auth_generation: u64,
    ) -> crate::ImResult<crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult>;
}

/// Ready-admin RPC seam. This intentionally cannot send token-authenticated
/// new-device calls, keeping the two server authorization views disjoint.
pub(crate) trait DeviceJoinAdminRemote {
    async fn registry(
        &mut self,
        did: &crate::ids::Did,
        include_pending_join_requests: bool,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry>;

    async fn status(
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

    async fn approve(
        &mut self,
        request: DeviceJoinRemoteApproveRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteApproveResult>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DeviceJoinRuntimeGate {
    enabled: bool,
}

impl DeviceJoinRuntimeGate {
    pub(crate) const fn from_rollout_flag(enabled: bool) -> Self {
        Self { enabled }
    }

    fn require_enabled(self) -> crate::ImResult<()> {
        if self.enabled {
            Ok(())
        } else {
            Err(crate::ImError::unsupported(
                "awiki-multi-device-join-disabled",
            ))
        }
    }
}

pub(crate) struct DeviceJoinNewDeviceHttpAdapter<P> {
    plain: P,
}

impl<P> DeviceJoinNewDeviceHttpAdapter<P> {
    pub(crate) fn new(plain: P) -> Self {
        Self { plain }
    }
}

impl<'a> DeviceJoinNewDeviceHttpAdapter<crate::internal::transport::CorePlainTransport<'a>> {
    pub(crate) fn production(core: &'a crate::core::ImCore) -> Self {
        Self::new(crate::internal::transport::CorePlainTransport::new(core))
    }
}

impl<P> DeviceJoinNewDeviceRemote for DeviceJoinNewDeviceHttpAdapter<P>
where
    P: AsyncRpcTransport,
{
    async fn create(
        &mut self,
        request: DeviceJoinRemoteCreateRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteCreateResult> {
        let expected_join_session_id = request.join_request.join_session_id.clone();
        let call = crate::internal::identity_wire::device_join::build_create_call(request)
            .map_err(redact_new_device_remote_error)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await
            .map_err(redact_new_device_remote_error)?;
        crate::internal::identity_wire::device_join::parse_create_result(
            raw,
            &expected_join_session_id,
        )
        .map_err(redact_new_device_remote_error)
    }

    async fn status(
        &mut self,
        expected_join_session_id: &str,
        join_session_token: &SecretBytes,
    ) -> crate::ImResult<DeviceJoinRemoteNewDeviceStatus> {
        let call =
            crate::internal::identity_wire::device_join::build_new_status_call(join_session_token)
                .map_err(redact_new_device_remote_error)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await
            .map_err(redact_new_device_remote_error)?;
        crate::internal::identity_wire::device_join::parse_new_status_result(
            raw,
            expected_join_session_id,
        )
        .map_err(redact_new_device_remote_error)
    }

    async fn submit_response(
        &mut self,
        request: DeviceJoinRemoteResponseRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        let response = request.response.clone();
        let call = crate::internal::identity_wire::device_join::build_response_call(request)
            .map_err(redact_new_device_remote_error)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await
            .map_err(redact_new_device_remote_error)?;
        crate::internal::identity_wire::device_join::parse_response_result(raw, &response)
            .map_err(redact_new_device_remote_error)
    }

    async fn issue_device_token(
        &mut self,
        prepared: &crate::internal::identity_wire::device_genesis::PreparedDeviceTokenIssue,
        expected_auth_generation: u64,
    ) -> crate::ImResult<crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult>
    {
        let call =
            crate::internal::identity_wire::device_genesis::build_device_token_issue_call(prepared)
                .map_err(redact_new_device_remote_error)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await
            .map_err(redact_new_device_remote_error)?;
        crate::internal::identity_wire::device_genesis::parse_device_token_issue_result(
            raw,
            prepared,
            expected_auth_generation,
            time::OffsetDateTime::now_utc(),
        )
        .map_err(redact_new_device_remote_error)
    }
}

fn redact_new_device_remote_error(error: crate::ImError) -> crate::ImError {
    match error {
        crate::ImError::Service {
            status_code, code, ..
        } => crate::ImError::Service {
            status_code,
            code,
            message: "device Join request failed".to_owned(),
            data: None,
        },
        crate::ImError::TransportUnavailable { .. } => crate::ImError::TransportUnavailable {
            detail: "device Join transport failed".to_owned(),
        },
        crate::ImError::Serialization { .. } => crate::ImError::Serialization {
            detail: "device Join response was invalid".to_owned(),
        },
        crate::ImError::Internal { .. } => crate::ImError::Internal {
            message: "device Join request failed".to_owned(),
        },
        other => other,
    }
}

fn token_authorization_refreshable(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::AuthRequired | crate::ImError::PermissionDenied
    ) || matches!(
        error,
        crate::ImError::Service {
            status_code: Some(401),
            ..
        }
    )
}

pub(crate) struct DeviceJoinAdminHttpAdapter<A> {
    authenticated: A,
}

impl<A> DeviceJoinAdminHttpAdapter<A> {
    pub(crate) fn new(authenticated: A) -> Self {
        Self { authenticated }
    }
}

impl<'a> DeviceJoinAdminHttpAdapter<crate::internal::transport::CoreHttpTransport<'a>> {
    pub(crate) fn production(admin_client: &'a crate::core::ImClient) -> Self {
        Self::new(crate::internal::transport::CoreHttpTransport::new(
            admin_client,
        ))
    }
}

impl<A> DeviceJoinAdminRemote for DeviceJoinAdminHttpAdapter<A>
where
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

    async fn status(
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

pub(crate) struct DeviceJoinDidResolver<T> {
    transport: T,
}

impl<T> DeviceJoinDidResolver<T> {
    pub(crate) fn new(transport: T) -> Self {
        Self { transport }
    }

    async fn resolve(&mut self, did: &crate::ids::Did) -> crate::ImResult<Value>
    where
        T: AsyncRawJsonTransport,
    {
        crate::internal::discovery::did_document::resolve_did_document_async(
            &mut self.transport,
            did.as_str(),
        )
        .await
    }
}

pub(crate) struct DeviceJoinNewDeviceRuntime<'a, R, D> {
    core: &'a crate::core::ImCore,
    remote: R,
    resolver: DeviceJoinDidResolver<D>,
    gate: DeviceJoinRuntimeGate,
}

#[derive(Clone, PartialEq)]
pub(crate) struct DeviceJoinAdvanceResult {
    pub(crate) session: crate::identity::DeviceJoinSessionSummary,
    pub(crate) remote_state: DeviceJoinRemoteState,
    pub(crate) authorization: Option<DeviceJoinRemoteAuthorization>,
    pub(crate) sas: Option<String>,
}

impl std::fmt::Debug for DeviceJoinAdvanceResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinAdvanceResult")
            .field("session", &self.session)
            .field("remote_state", &self.remote_state)
            .field("authorization", &self.authorization)
            .field("sas", &self.sas.as_ref().map(|_| "<redacted-sas>"))
            .finish()
    }
}

impl<'a, R, D> DeviceJoinNewDeviceRuntime<'a, R, D> {
    pub(crate) fn new(
        core: &'a crate::core::ImCore,
        remote: R,
        resolver: DeviceJoinDidResolver<D>,
        gate: DeviceJoinRuntimeGate,
    ) -> Self {
        Self {
            core,
            remote,
            resolver,
            gate,
        }
    }
}

impl<'a>
    DeviceJoinNewDeviceRuntime<
        'a,
        DeviceJoinNewDeviceHttpAdapter<crate::internal::transport::CorePlainTransport<'a>>,
        crate::internal::transport::CorePlainTransport<'a>,
    >
{
    pub(crate) fn production(core: &'a crate::core::ImCore) -> Self {
        Self::production_for_rollout(core, false)
    }

    pub(crate) fn production_for_rollout(core: &'a crate::core::ImCore, enabled: bool) -> Self {
        Self::new(
            core,
            DeviceJoinNewDeviceHttpAdapter::production(core),
            DeviceJoinDidResolver::new(crate::internal::transport::CorePlainTransport::new(core)),
            DeviceJoinRuntimeGate::from_rollout_flag(enabled),
        )
    }
}

impl<R, D> DeviceJoinNewDeviceRuntime<'_, R, D>
where
    R: DeviceJoinNewDeviceRemote,
    D: AsyncRawJsonTransport,
{
    pub(crate) async fn begin(
        &mut self,
        request: crate::identity::DeviceJoinStartRequest,
        account_verification_token: &SecretBytes,
    ) -> crate::ImResult<crate::identity::DeviceJoinSessionSummary> {
        self.gate.require_enabled()?;
        let operation_id = request.operation_id.clone();
        let local = self.core.device_join().start(request)?;
        if local.session.phase == crate::identity::DeviceJoinLocalPhase::Authorized {
            return Ok(local.session);
        }
        let created = self
            .remote
            .create(DeviceJoinRemoteCreateRequest {
                operation_id: &operation_id,
                account_verification_token,
                join_request: &local.join_request,
            })
            .await?;
        if created.state != DeviceJoinRemoteState::Pending {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_device_join::bind_new_device_remote_session(
            self.core,
            &local.session.join_session_id,
            &created.join_session_token,
            &created.expires_at,
        )
    }

    pub(crate) async fn advance(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        self.gate.require_enabled()?;
        let mut local = self
            .core
            .device_join()
            .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?;
        if local.phase == crate::identity::DeviceJoinLocalPhase::Authorized {
            local = crate::internal::identity_device_join::finalize_new_device_activation(
                self.core,
                join_session_id,
            )?;
            return Ok(DeviceJoinAdvanceResult {
                session: local,
                remote_state: DeviceJoinRemoteState::Consumed,
                authorization: None,
                sas: None,
            });
        }
        if let Some(pending) =
            crate::internal::identity_device_join::load_pending_new_device_activation(
                self.core,
                join_session_id,
            )?
        {
            return self.complete_new_device_activation(pending).await;
        }
        match local.phase {
            crate::identity::DeviceJoinLocalPhase::Expired => {
                return Ok(DeviceJoinAdvanceResult {
                    session: local,
                    remote_state: DeviceJoinRemoteState::Expired,
                    authorization: None,
                    sas: None,
                });
            }
            crate::identity::DeviceJoinLocalPhase::Cancelled => {
                return Err(invalid_remote_state("new-device Join is cancelled"));
            }
            _ => {}
        }
        let token = crate::internal::identity_device_join::open_new_device_remote_session_token(
            self.core,
            join_session_id,
        )?;
        let status = self.remote.status(join_session_id, &token).await?;
        if status.expires_at != local.expires_at {
            return Err(crate::ImError::PermissionDenied);
        }
        match status.state {
            DeviceJoinRemoteState::Expired => {
                let session = crate::internal::identity_device_join::mark_join_expired(
                    self.core,
                    join_session_id,
                    crate::identity::DeviceJoinSide::NewDevice,
                )?;
                Ok(DeviceJoinAdvanceResult {
                    session,
                    remote_state: status.state,
                    authorization: None,
                    sas: None,
                })
            }
            DeviceJoinRemoteState::ChallengeSent | DeviceJoinRemoteState::ResponseVerified => {
                let challenge = status
                    .challenge
                    .ok_or_else(|| invalid_remote_state("new-device challenge missing"))?;
                let did = self
                    .core
                    .device_join()
                    .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?
                    .did;
                let document = self.resolver.resolve(&did).await?;
                let prepared =
                    crate::internal::identity_device_join::respond_as_new_device_to_resolved_document(
                        self.core,
                        format!("join-response:{}", challenge.challenge_id),
                        challenge,
                        document,
                    )?;
                let transition = self
                    .remote
                    .submit_response(DeviceJoinRemoteResponseRequest {
                        join_session_token: &token,
                        response: &prepared.response,
                    })
                    .await?;
                if transition.state != DeviceJoinRemoteState::ResponseVerified {
                    return Err(crate::ImError::PermissionDenied);
                }
                Ok(DeviceJoinAdvanceResult {
                    session: prepared.session,
                    remote_state: transition.state,
                    authorization: None,
                    sas: Some(prepared.sas),
                })
            }
            DeviceJoinRemoteState::Consumed => {
                let authorization = status
                    .authorization
                    .ok_or_else(|| invalid_remote_state("new-device authorization missing"))?;
                let did = self
                    .core
                    .device_join()
                    .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?
                    .did;
                let document = self.resolver.resolve(&did).await?;
                let pending = crate::internal::identity_device_join::prepare_new_device_activation(
                    self.core,
                    join_session_id,
                    &authorization,
                    &document,
                )?;
                self.complete_new_device_activation(pending).await
            }
            DeviceJoinRemoteState::Pending | DeviceJoinRemoteState::Claimed => {
                Ok(DeviceJoinAdvanceResult {
                    session: self
                        .core
                        .device_join()
                        .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?,
                    remote_state: status.state,
                    authorization: None,
                    sas: None,
                })
            }
        }
    }

    async fn complete_new_device_activation(
        &mut self,
        mut pending: crate::internal::identity_join_activation_pending::PendingJoinActivation,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        if pending.token_result.is_none() {
            if crate::internal::identity_wire::device_genesis::device_token_authorization_needs_refresh(
                &pending.prepared_token_issue,
                time::OffsetDateTime::now_utc(),
            )? {
                pending = crate::internal::identity_device_join::refresh_new_device_token_authorization(
                    self.core,
                    &pending.join_session_id,
                )?;
            }
            let first = self
                .remote
                .issue_device_token(
                    &pending.prepared_token_issue,
                    pending.authorization.device.auth_generation,
                )
                .await;
            let result = match first {
                Ok(result) => result,
                Err(error) if token_authorization_refreshable(&error) => {
                    pending = crate::internal::identity_device_join::refresh_new_device_token_authorization(
                        self.core,
                        &pending.join_session_id,
                    )?;
                    self.remote
                        .issue_device_token(
                            &pending.prepared_token_issue,
                            pending.authorization.device.auth_generation,
                        )
                        .await?
                }
                Err(error) => return Err(error),
            };
            pending = crate::internal::identity_device_join::record_new_device_token_result(
                self.core,
                &pending.join_session_id,
                result,
            )?;
        }
        let authorization = pending.authorization.clone();
        let session = crate::internal::identity_device_join::finalize_new_device_activation(
            self.core,
            &pending.join_session_id,
        )?;
        Ok(DeviceJoinAdvanceResult {
            session,
            remote_state: DeviceJoinRemoteState::Consumed,
            authorization: Some(authorization),
            sas: None,
        })
    }

    pub(crate) fn cancel(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<crate::identity::DeviceJoinSessionSummary> {
        self.gate.require_enabled()?;
        crate::internal::identity_device_join::cancel_join(
            self.core,
            join_session_id,
            crate::identity::DeviceJoinSide::NewDevice,
        )
    }
}

pub(crate) struct DeviceJoinAdminRuntime<'a, R> {
    core: &'a crate::core::ImCore,
    admin_identity: crate::identity::IdentitySelector,
    remote: R,
    gate: DeviceJoinRuntimeGate,
}

impl<'a, R> DeviceJoinAdminRuntime<'a, R> {
    pub(crate) fn new(
        core: &'a crate::core::ImCore,
        admin_identity: crate::identity::IdentitySelector,
        remote: R,
        gate: DeviceJoinRuntimeGate,
    ) -> Self {
        Self {
            core,
            admin_identity,
            remote,
            gate,
        }
    }
}

impl<'a>
    DeviceJoinAdminRuntime<
        'a,
        DeviceJoinAdminHttpAdapter<crate::internal::transport::CoreHttpTransport<'a>>,
    >
{
    pub(crate) fn production(
        core: &'a crate::core::ImCore,
        admin_client: &'a crate::core::ImClient,
    ) -> Self {
        Self::production_for_rollout(core, admin_client, false)
    }

    pub(crate) fn production_for_rollout(
        core: &'a crate::core::ImCore,
        admin_client: &'a crate::core::ImClient,
        enabled: bool,
    ) -> Self {
        Self::new(
            core,
            crate::identity::IdentitySelector::Id(admin_client.current_identity().id.clone()),
            DeviceJoinAdminHttpAdapter::production(admin_client),
            DeviceJoinRuntimeGate::from_rollout_flag(enabled),
        )
    }
}

impl<R> DeviceJoinAdminRuntime<'_, R>
where
    R: DeviceJoinAdminRemote,
{
    pub(crate) async fn registry(&mut self) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        self.gate.require_enabled()?;
        let did = self.core.client(self.admin_identity.clone())?.did().clone();
        self.remote.registry(&did, true).await
    }

    pub(crate) async fn pending(&mut self) -> crate::ImResult<Vec<DeviceJoinRemotePendingSummary>> {
        self.gate.require_enabled()?;
        let did = self.core.client(self.admin_identity.clone())?.did().clone();
        self.remote
            .registry(&did, true)
            .await
            .map(|registry| registry.pending_join_requests)
    }

    pub(crate) async fn claim_and_challenge(
        &mut self,
        join_session_id: &str,
        operation_id: &str,
        challenge_ttl_seconds: u64,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        self.gate.require_enabled()?;
        if let Some(existing) = local_admin_session(self.core, join_session_id)? {
            match existing.phase {
                crate::identity::DeviceJoinLocalPhase::ChallengePrepared => {
                    let prepared =
                        crate::internal::identity_device_join::load_prepared_admin_challenge(
                            self.core,
                            join_session_id,
                            operation_id,
                        )?
                        .ok_or_else(|| invalid_remote_state("local prepared challenge missing"))?;
                    let submitted = self.remote.submit_challenge(&prepared.challenge).await?;
                    if submitted.state != DeviceJoinRemoteState::ChallengeSent {
                        return Err(crate::ImError::PermissionDenied);
                    }
                    crate::internal::identity_device_join::clear_admin_claim_intent(
                        self.core,
                        join_session_id,
                    )?;
                    return Ok(DeviceJoinAdvanceResult {
                        session: prepared.session,
                        remote_state: submitted.state,
                        authorization: None,
                        sas: Some(prepared.sas),
                    });
                }
                crate::identity::DeviceJoinLocalPhase::ResponseVerified
                | crate::identity::DeviceJoinLocalPhase::ApprovalPrepared => {
                    let prepared =
                        crate::internal::identity_device_join::load_prepared_admin_challenge(
                            self.core,
                            join_session_id,
                            operation_id,
                        )?
                        .ok_or_else(|| invalid_remote_state("local prepared challenge missing"))?;
                    return Ok(DeviceJoinAdvanceResult {
                        session: prepared.session,
                        remote_state: DeviceJoinRemoteState::ResponseVerified,
                        authorization: None,
                        sas: Some(prepared.sas),
                    });
                }
                crate::identity::DeviceJoinLocalPhase::Authorized => {
                    return Ok(DeviceJoinAdvanceResult {
                        session: existing,
                        remote_state: DeviceJoinRemoteState::Consumed,
                        authorization: None,
                        sas: None,
                    });
                }
                crate::identity::DeviceJoinLocalPhase::Expired => {
                    return Ok(DeviceJoinAdvanceResult {
                        session: existing,
                        remote_state: DeviceJoinRemoteState::Expired,
                        authorization: None,
                        sas: None,
                    });
                }
                crate::identity::DeviceJoinLocalPhase::Cancelled => {
                    return Err(invalid_remote_state("admin Join is cancelled"));
                }
                _ => return Err(invalid_remote_state("admin Join phase is invalid")),
            }
        }
        let admin_client = self.core.client(self.admin_identity.clone())?;
        let did = admin_client.did().clone();
        let existing_claim = crate::internal::identity_device_join::load_prepared_admin_claim(
            self.core,
            join_session_id,
        )?;
        let registry = self.remote.registry(&did, existing_claim.is_none()).await?;
        let claim = match existing_claim {
            Some((claim, _)) => {
                if claim.operation_id != operation_id {
                    return Err(crate::ImError::invalid_input(
                        Some("operation_id".to_owned()),
                        "device Join claim idempotency conflict",
                    ));
                }
                claim
            }
            None => {
                let pending = registry
                    .pending_join_requests
                    .iter()
                    .find(|pending| pending.join_session_id == join_session_id)
                    .ok_or_else(|| crate::ImError::IdentityNotFound {
                        selector: join_session_id.to_owned(),
                    })?;
                crate::internal::identity_device_join::prepare_admin_claim_intent(
                    self.core,
                    self.admin_identity.clone(),
                    operation_id,
                    join_session_id,
                    &pending.expires_at,
                )?
            }
        };
        let claimed = self
            .remote
            .claim(DeviceJoinRemoteClaimRequest {
                operation_id: &claim.operation_id,
                join_session_id: &claim.join_session_id,
                authorizing_device_id: &claim.authorizing_device_id,
                authorizing_device_proof: &claim.authorizing_device_proof,
            })
            .await?;
        if claimed.state != DeviceJoinRemoteState::Claimed
            || claimed.claimed_by_device_id != claim.authorizing_device_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let prepared = self.core.device_join().prepare_admin_challenge(
            crate::identity::DeviceJoinAdminPrepareRequest {
                admin_identity: self.admin_identity.clone(),
                operation_id: operation_id.to_owned(),
                join_request: claimed.join_request,
                challenge_ttl_seconds,
                document_version: registry.checkpoint.document_version,
                document_hash: registry.checkpoint.document_hash,
            },
        )?;
        let submitted = self.remote.submit_challenge(&prepared.challenge).await?;
        if submitted.state != DeviceJoinRemoteState::ChallengeSent {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_device_join::clear_admin_claim_intent(
            self.core,
            join_session_id,
        )?;
        Ok(DeviceJoinAdvanceResult {
            session: prepared.session,
            remote_state: submitted.state,
            authorization: None,
            sas: Some(prepared.sas),
        })
    }

    pub(crate) async fn advance(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        self.gate.require_enabled()?;
        let local = local_admin_session(self.core, join_session_id)?;
        if let Some(existing) = local.as_ref() {
            if existing.phase == crate::identity::DeviceJoinLocalPhase::Authorized {
                return Ok(DeviceJoinAdvanceResult {
                    session: existing.clone(),
                    remote_state: DeviceJoinRemoteState::Consumed,
                    authorization: None,
                    sas: None,
                });
            }
        }
        let status = self.remote.status(join_session_id).await?;
        if local
            .as_ref()
            .is_some_and(|session| status.expires_at != session.expires_at)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        match status.state {
            DeviceJoinRemoteState::Expired => {
                let session = crate::internal::identity_device_join::mark_join_expired(
                    self.core,
                    join_session_id,
                    crate::identity::DeviceJoinSide::Admin,
                )?;
                Ok(DeviceJoinAdvanceResult {
                    session,
                    remote_state: status.state,
                    authorization: None,
                    sas: None,
                })
            }
            DeviceJoinRemoteState::ResponseVerified => {
                let response = status
                    .challenge_response
                    .ok_or_else(|| invalid_remote_state("admin challenge response missing"))?;
                crate::internal::identity_device_join::reset_expired_admin_approval_after_remote_poll(
                    self.core,
                    join_session_id,
                    &status.expires_at,
                    time::OffsetDateTime::now_utc(),
                )?;
                let verified = self.core.device_join().verify_response_as_admin(
                    crate::identity::DeviceJoinAdminVerifyRequest {
                        operation_id: format!("join-verify:{}", response.operation_id),
                        join_session_id: join_session_id.to_owned(),
                        response,
                    },
                )?;
                Ok(DeviceJoinAdvanceResult {
                    session: verified.session,
                    remote_state: status.state,
                    authorization: None,
                    sas: Some(verified.sas),
                })
            }
            DeviceJoinRemoteState::Claimed | DeviceJoinRemoteState::ChallengeSent => {
                Ok(DeviceJoinAdvanceResult {
                    session: self
                        .core
                        .device_join()
                        .session(join_session_id, crate::identity::DeviceJoinSide::Admin)?,
                    remote_state: status.state,
                    authorization: None,
                    sas: None,
                })
            }
            DeviceJoinRemoteState::Consumed => {
                let authorization = status
                    .authorization
                    .ok_or_else(|| invalid_remote_state("admin authorization missing"))?;
                let session = crate::internal::identity_device_join::mark_admin_approval_consumed_after_remote_poll(
                    self.core,
                    join_session_id,
                    &status.expires_at,
                    &authorization,
                )?;
                Ok(DeviceJoinAdvanceResult {
                    session,
                    remote_state: status.state,
                    authorization: Some(authorization),
                    sas: None,
                })
            }
            DeviceJoinRemoteState::Pending => Err(invalid_remote_state(
                "claimed admin received pending Join state",
            )),
        }
    }

    /// Reconciles an expired local approval lease without advancing the Join
    /// state machine. The only remote operation is status: an already consumed
    /// approval may be finalized locally, while every other state discards the
    /// expired proof and requires a fresh user-presence operation.
    pub(crate) async fn reconcile_expired_approval(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        self.gate.require_enabled()?;
        let status = self.remote.status(join_session_id).await?;
        if status.state != DeviceJoinRemoteState::Consumed {
            crate::internal::identity_device_join::reset_expired_admin_approval_after_remote_poll(
                self.core,
                join_session_id,
                &status.expires_at,
                time::OffsetDateTime::now_utc(),
            )?;
            return Err(crate::ImError::SessionExpired);
        }

        let authorization = status
            .authorization
            .ok_or_else(|| invalid_remote_state("admin authorization missing"))?;
        let session =
            crate::internal::identity_device_join::mark_admin_approval_consumed_after_remote_poll(
                self.core,
                join_session_id,
                &status.expires_at,
                &authorization,
            )?;
        Ok(DeviceJoinAdvanceResult {
            session,
            remote_state: status.state,
            authorization: Some(authorization),
            sas: None,
        })
    }

    pub(crate) async fn approve(
        &mut self,
        join_session_id: &str,
        operation_id: &str,
        role: DeviceAuthorizationRole,
        user_presence_at: &str,
        sas_confirmed: bool,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        self.gate.require_enabled()?;
        let prepared = match crate::internal::identity_device_join::load_prepared_admin_approval(
            self.core,
            join_session_id,
        )? {
            Some(value) => {
                if value.operation_id != operation_id
                    || value.role != role
                    || value.pairing_confirmation.user_presence_at != user_presence_at
                    || value.pairing_confirmation.sas_confirmed != sas_confirmed
                {
                    return Err(crate::ImError::invalid_input(
                        Some("operation_id".to_owned()),
                        "device Join approval idempotency conflict",
                    ));
                }
                value
            }
            None => {
                let did = self.core.client(self.admin_identity.clone())?.did().clone();
                let registry = self.remote.registry(&did, false).await?;
                crate::internal::identity_device_join::prepare_admin_approval(
                    self.core,
                    operation_id,
                    join_session_id,
                    &registry.checkpoint,
                    role,
                    user_presence_at,
                    sas_confirmed,
                )?
            }
        };
        let approved = self
            .remote
            .approve(DeviceJoinRemoteApproveRequest {
                operation_id: &prepared.operation_id,
                join_session_id: &prepared.join_session_id,
                expected_checkpoint: &prepared.expected_checkpoint,
                role: prepared.role,
                new_document: &prepared.new_document,
                pairing_confirmation: &prepared.pairing_confirmation,
                authorizing_device_id: &prepared.authorizing_device_id,
                authorizing_device_proof: &prepared.authorizing_device_proof,
            })
            .await?;
        if approved.state != DeviceJoinRemoteState::Consumed {
            return Err(crate::ImError::PermissionDenied);
        }
        let authorization = DeviceJoinRemoteAuthorization {
            checkpoint: approved.checkpoint,
            device: approved.device,
        };
        let session = crate::internal::identity_device_join::mark_join_authorized(
            self.core,
            join_session_id,
            crate::identity::DeviceJoinSide::Admin,
            &authorization,
            &prepared.new_document,
        )?;
        Ok(DeviceJoinAdvanceResult {
            session,
            remote_state: approved.state,
            authorization: Some(authorization),
            sas: None,
        })
    }

    pub(crate) fn cancel(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<crate::identity::DeviceJoinSessionSummary> {
        self.gate.require_enabled()?;
        let cancelled = crate::internal::identity_device_join::cancel_join(
            self.core,
            join_session_id,
            crate::identity::DeviceJoinSide::Admin,
        );
        match cancelled {
            Ok(summary) => {
                crate::internal::identity_device_join::clear_admin_claim_intent(
                    self.core,
                    join_session_id,
                )?;
                Ok(summary)
            }
            Err(crate::ImError::IdentityNotFound { .. })
                if crate::internal::identity_device_join::load_prepared_admin_claim(
                    self.core,
                    join_session_id,
                )?
                .is_some() =>
            {
                Err(invalid_remote_state(
                    "admin claim outcome is unresolved; retry claim before cancelling",
                ))
            }
            Err(error) => Err(error),
        }
    }
}

fn local_admin_session(
    core: &crate::core::ImCore,
    join_session_id: &str,
) -> crate::ImResult<Option<crate::identity::DeviceJoinSessionSummary>> {
    match core
        .device_join()
        .session(join_session_id, crate::identity::DeviceJoinSide::Admin)
    {
        Ok(summary) => Ok(Some(summary)),
        Err(crate::ImError::IdentityNotFound { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

fn invalid_remote_state(detail: &str) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: format!("device Join remote state invalid: {detail}"),
    }
}

#[cfg(test)]
mod tests;
