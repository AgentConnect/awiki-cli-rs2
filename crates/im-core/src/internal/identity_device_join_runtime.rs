//! Production orchestration and typed remote boundary for AWiki device Join.
//!
//! The two orchestration paths advance the existing restart-safe local Join
//! state; they do not maintain a second phase machine. They consume only the
//! seven frozen Join RPC operations, the authenticated Registry projection and
//! standard DID resolution. Tokens cross the boundary as zeroizing bytes and
//! are sealed before a caller can resume the flow. Once authorized, a device
//! publishes its P5 PreKey. Ordinary Join also publishes its current P6
//! KeyPackage when Group E2EE v2 is enabled; recovery-authorized Join is
//! deliberately P5-only. A publication failure is returned after local
//! authorization so the same Join poll can retry the missing public material.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;

use crate::identity::{
    DeviceJoinChallenge, DeviceJoinChallengeResponse, DeviceJoinObjectProof, DeviceJoinRequest,
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
    ChallengeSent,
    ResponseVerified,
    Consumed,
    Cancelled,
    Rejected,
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
pub(crate) struct DeviceJoinRemoteRegistry {
    pub(crate) did: crate::ids::Did,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) devices: Vec<DeviceJoinRemoteDeviceSummary>,
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
    pub(crate) session_revision: u64,
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
    pub(crate) session_revision: u64,
    pub(crate) expires_at: String,
    pub(crate) challenge: Option<DeviceJoinChallenge>,
    pub(crate) authorization: Option<DeviceJoinRemoteAuthorization>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceJoinRemoteAuthorization {
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) device: DeviceJoinRemoteDeviceSummary,
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
    pub(crate) new_document: &'a Value,
    pub(crate) pairing_confirmation: &'a DeviceJoinRemotePairingConfirmation,
    pub(crate) authorizing_device_id: &'a str,
    pub(crate) proof: &'a DeviceJoinObjectProof,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceJoinRemoteRejectRequest<'a> {
    pub(crate) operation_id: &'a str,
    pub(crate) join_session_id: &'a str,
    pub(crate) rejecting_device_id: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) proof: &'a DeviceJoinObjectProof,
}

pub(crate) struct DeviceJoinRemoteCancelRequest<'a> {
    pub(crate) operation_id: &'a str,
    pub(crate) join_session_token: &'a SecretBytes,
    pub(crate) join_session_id: &'a str,
    pub(crate) reason: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteTransitionResult {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) session_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteChallengeResult {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) session_revision: u64,
    pub(crate) claimed_by_device_id: String,
    pub(crate) challenge_id: String,
    pub(crate) challenge_expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinRemoteApproveResult {
    pub(crate) join_session_id: String,
    pub(crate) state: DeviceJoinRemoteState,
    pub(crate) session_revision: u64,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) device: DeviceJoinRemoteDeviceSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceJoinAccessResult {
    pub(crate) user_id: String,
    pub(crate) access_token: String,
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

    async fn cancel(
        &mut self,
        request: DeviceJoinRemoteCancelRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult>;

    async fn refresh_device_access(
        &mut self,
        pending: &crate::internal::identity_join_activation_pending::PendingJoinActivation,
    ) -> crate::ImResult<DeviceJoinAccessResult>;
}

/// Ready-admin RPC seam. This intentionally cannot send token-authenticated
/// new-device calls, keeping the two server authorization views disjoint.
pub(crate) trait DeviceJoinAdminRemote {
    async fn registry(
        &mut self,
        did: &crate::ids::Did,
        _compatibility_flag: bool,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry>;

    async fn submit_challenge(
        &mut self,
        challenge: &DeviceJoinChallenge,
    ) -> crate::ImResult<DeviceJoinRemoteChallengeResult>;

    async fn approve(
        &mut self,
        request: DeviceJoinRemoteApproveRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteApproveResult>;

    async fn reject(
        &mut self,
        request: DeviceJoinRemoteRejectRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult>;
}

pub(crate) struct DeviceJoinNewDeviceHttpAdapter<'a, P> {
    plain: P,
    core: Option<&'a crate::core::ImCore>,
}

impl<P> DeviceJoinNewDeviceHttpAdapter<'static, P> {
    pub(crate) fn new(plain: P) -> Self {
        Self { plain, core: None }
    }
}

impl<'a> DeviceJoinNewDeviceHttpAdapter<'a, crate::internal::transport::CorePlainTransport<'a>> {
    pub(crate) fn production(core: &'a crate::core::ImCore) -> Self {
        Self {
            plain: crate::internal::transport::CorePlainTransport::new(core),
            core: Some(core),
        }
    }
}

impl<P> DeviceJoinNewDeviceRemote for DeviceJoinNewDeviceHttpAdapter<'_, P>
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

    async fn cancel(
        &mut self,
        request: DeviceJoinRemoteCancelRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        let expected_join_session_id = request.join_session_id.to_owned();
        let call = crate::internal::identity_wire::device_join::build_cancel_call(request)
            .map_err(redact_new_device_remote_error)?;
        let raw = self
            .plain
            .rpc(call.endpoint, call.method, call.params)
            .await
            .map_err(redact_new_device_remote_error)?;
        crate::internal::identity_wire::device_join::parse_cancel_result(
            raw,
            &expected_join_session_id,
        )
        .map_err(redact_new_device_remote_error)
    }

    async fn refresh_device_access(
        &mut self,
        pending: &crate::internal::identity_join_activation_pending::PendingJoinActivation,
    ) -> crate::ImResult<DeviceJoinAccessResult> {
        let core = self.core.ok_or_else(|| {
            crate::ImError::unsupported("pending-device-signed-get-me-not-configured")
        })?;
        refresh_join_device_access(core, pending)
            .await
            .map_err(redact_new_device_remote_error)
    }
}

const DEVICE_JOIN_ACCESS_PUBLIC_CODE_MAX_LEN: usize = 96;
const DEVICE_JOIN_ACCESS_PUBLIC_CODE_NAMESPACES: &[&str] = &[
    "anp",
    "attachment",
    "awiki",
    "client",
    "device",
    "direct",
    "group",
    "identity",
    "inbox",
    "read_state",
    "sync",
];

fn redact_device_join_access_error(error: crate::ImError) -> crate::ImError {
    match error {
        crate::ImError::Service {
            status_code, code, ..
        } => {
            let public_code = match code {
                Some(code) => Some(classify_device_join_access_service_code(&code)),
                None => status_code
                    .filter(|status| (100..=599).contains(status))
                    .map(|status| format!("device.join.access.http.{status:03}")),
            };
            crate::ImError::Service {
                status_code,
                code: public_code,
                message: "device Join access request failed".to_owned(),
                data: None,
            }
        }
        other => redact_new_device_remote_error(other),
    }
}

fn classify_device_join_access_service_code(code: &str) -> String {
    if let Ok(numeric_code) = code.parse::<i64>() {
        if numeric_code != 0 && numeric_code.to_string() == code {
            let sign = if numeric_code < 0 { 'n' } else { 'p' };
            return format!(
                "device.join.access.rpc.{sign}{}",
                numeric_code.unsigned_abs()
            );
        }
    }
    if code.starts_with("did_auth.") {
        return "device.join.access.did_auth".to_owned();
    }
    if is_device_join_access_public_service_code(code) {
        return code.to_owned();
    }
    "device.join.access.rpc.unclassified".to_owned()
}

fn is_device_join_access_public_service_code(code: &str) -> bool {
    if code.is_empty()
        || code.len() > DEVICE_JOIN_ACCESS_PUBLIC_CODE_MAX_LEN
        || !code.is_ascii()
        || !code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        return false;
    }

    let Some((namespace, suffix)) = code.split_once('.') else {
        return false;
    };
    DEVICE_JOIN_ACCESS_PUBLIC_CODE_NAMESPACES.contains(&namespace)
        && !suffix.is_empty()
        && code.split('.').all(|segment| !segment.is_empty())
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

async fn refresh_join_device_access(
    core: &crate::core::ImCore,
    pending: &crate::internal::identity_join_activation_pending::PendingJoinActivation,
) -> crate::ImResult<DeviceJoinAccessResult> {
    pending.validate()?;
    let (public, identity) = crate::internal::identity_custody::active_join_provider_identity(
        core,
        &pending.did,
        &pending.custody,
        &pending.authorization.device.signing_key_id,
        &pending.authorization.device.e2ee_key_id,
    )
    .await?;
    let client = core.client_with_pending_provider_identity(
        public,
        identity,
        None,
        pending.did.as_str(),
        &crate::ids::ProtocolDeviceId::parse(&pending.authorization.device.device_id)?,
    )?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new_pending_device(
        &client,
        client.runtime().key_provider.clone(),
        crate::internal::transport::ExpectedDeviceAccessOwned {
            did: pending.did.as_str().to_owned(),
            user_id: String::new(),
            device_id: pending.authorization.device.device_id.clone(),
            key_id: pending.authorization.device.signing_key_id.clone(),
            auth_generation: pending.authorization.device.auth_generation,
            role: pending.authorization.device.role,
            management_ready: false,
        },
    );
    let access_token = transport.refresh_jwt_async().await?;
    let call =
        crate::internal::identity_wire::device_join::build_registry_call(&pending.did, false);
    let raw = transport
        .authenticated_rpc(call.endpoint, call.method, call.params)
        .await?;
    let registry = crate::internal::identity_wire::device_join::parse_registry_result(
        raw,
        &pending.did,
        false,
    )?;
    if registry.checkpoint != pending.authorization.checkpoint
        || registry
            .devices
            .iter()
            .filter(|device| {
                device.device_id == pending.authorization.device.device_id
                    || device.signing_key_id == pending.authorization.device.signing_key_id
                    || device.e2ee_key_id == pending.authorization.device.e2ee_key_id
            })
            .count()
            != 1
        || !registry
            .devices
            .iter()
            .any(|device| device == &pending.authorization.device)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(DeviceJoinAccessResult {
        user_id: transport.pending_device_user_id()?,
        access_token,
    })
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
        _compatibility_flag: bool,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        let call = crate::internal::identity_wire::device_join::build_registry_call(did, false);
        let raw = self
            .authenticated
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_registry_result(raw, did, false)
    }

    async fn submit_challenge(
        &mut self,
        challenge: &DeviceJoinChallenge,
    ) -> crate::ImResult<DeviceJoinRemoteChallengeResult> {
        let call =
            crate::internal::identity_wire::device_join::build_submit_challenge_call(challenge)?;
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

    async fn reject(
        &mut self,
        request: DeviceJoinRemoteRejectRequest<'_>,
    ) -> crate::ImResult<DeviceJoinRemoteTransitionResult> {
        let expected_join_session_id = request.join_session_id.to_owned();
        let call = crate::internal::identity_wire::device_join::build_reject_call(request)?;
        let raw = self
            .authenticated
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_reject_result(
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
    ) -> Self {
        Self {
            core,
            remote,
            resolver,
        }
    }
}

impl<'a>
    DeviceJoinNewDeviceRuntime<
        'a,
        DeviceJoinNewDeviceHttpAdapter<'a, crate::internal::transport::CorePlainTransport<'a>>,
        crate::internal::transport::CorePlainTransport<'a>,
    >
{
    pub(crate) fn production(core: &'a crate::core::ImCore) -> Self {
        Self::new(
            core,
            DeviceJoinNewDeviceHttpAdapter::production(core),
            DeviceJoinDidResolver::new(crate::internal::transport::CorePlainTransport::new(core)),
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
        self.begin_with_local_hook(request, account_verification_token, |_| Ok(()))
            .await
    }

    pub(crate) async fn begin_with_local_hook<F>(
        &mut self,
        request: crate::identity::DeviceJoinStartRequest,
        account_verification_token: &SecretBytes,
        local_hook: F,
    ) -> crate::ImResult<crate::identity::DeviceJoinSessionSummary>
    where
        F: FnOnce(&crate::identity::DeviceJoinSessionSummary) -> crate::ImResult<()>,
    {
        let operation_id = request.operation_id.clone();
        let document = self.resolver.resolve(&request.did).await?;
        let local = self.core.device_join().start(request, &document).await?;
        local_hook(&local.session)?;
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
        let mut local = self
            .core
            .device_join()
            .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?;
        if local.phase == crate::identity::DeviceJoinLocalPhase::Authorized {
            local = crate::internal::identity_device_join::finalize_new_device_activation_async(
                self.core,
                join_session_id,
            )
            .await?;
            publish_v2_messaging_material_after_activation(self.core, &local).await?;
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
        if local.phase == crate::identity::DeviceJoinLocalPhase::Cancelled {
            return Err(invalid_remote_state("new-device Join is cancelled"));
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
                )
                .await?;
                Ok(DeviceJoinAdvanceResult {
                    session,
                    remote_state: status.state,
                    authorization: None,
                    sas: None,
                })
            }
            DeviceJoinRemoteState::Cancelled | DeviceJoinRemoteState::Rejected => {
                let session = crate::internal::identity_device_join::cancel_join(
                    self.core,
                    join_session_id,
                    crate::identity::DeviceJoinSide::NewDevice,
                )
                .await?;
                Ok(DeviceJoinAdvanceResult {
                    session,
                    remote_state: status.state,
                    authorization: None,
                    sas: None,
                })
            }
            DeviceJoinRemoteState::ChallengeSent => {
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
                    )
                    .await?;
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
            DeviceJoinRemoteState::ResponseVerified => Ok(DeviceJoinAdvanceResult {
                session: self
                    .core
                    .device_join()
                    .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?,
                remote_state: status.state,
                authorization: None,
                sas: Some(
                    crate::internal::identity_device_join::local_new_device_verification_sas(
                        self.core,
                        join_session_id,
                    )?,
                ),
            }),
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
                let pending =
                    crate::internal::identity_device_join::prepare_new_device_activation_async(
                        self.core,
                        join_session_id,
                        &authorization,
                        &document,
                    )
                    .await?;
                self.complete_new_device_activation(pending).await
            }
            DeviceJoinRemoteState::Pending => Ok(DeviceJoinAdvanceResult {
                session: self
                    .core
                    .device_join()
                    .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?,
                remote_state: status.state,
                authorization: None,
                sas: None,
            }),
        }
    }

    async fn complete_new_device_activation(
        &mut self,
        mut pending: crate::internal::identity_join_activation_pending::PendingJoinActivation,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        if pending.access_result.is_none() {
            let result = self
                .remote
                .refresh_device_access(&pending)
                .await
                .map_err(redact_device_join_access_error)?;
            pending = crate::internal::identity_device_join::record_new_device_access_result(
                self.core,
                &pending.join_session_id,
                result,
            )?;
        }
        let authorization = pending.authorization.clone();
        let session = crate::internal::identity_device_join::finalize_new_device_activation_async(
            self.core,
            &pending.join_session_id,
        )
        .await?;
        publish_v2_messaging_material_after_activation(self.core, &session).await?;
        Ok(DeviceJoinAdvanceResult {
            session,
            remote_state: DeviceJoinRemoteState::Consumed,
            authorization: Some(authorization),
            sas: None,
        })
    }

    pub(crate) async fn cancel(
        &mut self,
        join_session_id: &str,
    ) -> crate::ImResult<crate::identity::DeviceJoinSessionSummary> {
        let local = self
            .core
            .device_join()
            .session(join_session_id, crate::identity::DeviceJoinSide::NewDevice)?;
        match local.phase {
            crate::identity::DeviceJoinLocalPhase::Cancelled => return Ok(local),
            crate::identity::DeviceJoinLocalPhase::Expired => {
                return Err(crate::ImError::SessionExpired)
            }
            crate::identity::DeviceJoinLocalPhase::Authorized => {
                return Err(invalid_remote_state("authorized Join cannot be cancelled"))
            }
            _ => {}
        }
        let token = crate::internal::identity_device_join::open_new_device_remote_session_token(
            self.core,
            join_session_id,
        )?;
        let transition = self
            .remote
            .cancel(DeviceJoinRemoteCancelRequest {
                operation_id: &format!("join-cancel:{join_session_id}"),
                join_session_token: &token,
                join_session_id,
                reason: "user_cancelled",
            })
            .await?;
        if transition.state != DeviceJoinRemoteState::Cancelled {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_device_join::cancel_join(
            self.core,
            join_session_id,
            crate::identity::DeviceJoinSide::NewDevice,
        )
        .await
    }
}

async fn publish_v2_messaging_material_after_activation(
    core: &crate::core::ImCore,
    session: &crate::identity::DeviceJoinSessionSummary,
) -> crate::ImResult<()> {
    let recovery_join = crate::internal::identity_transition_pending::load_joined_device(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &session.join_session_id,
    )?
    .is_some();
    if post_activation_publish_policy(recovery_join) == PostActivationPublishPolicy::PrekeysOnly {
        let client = core
            .client_async(crate::identity::IdentitySelector::Did(session.did.clone()))
            .await?;
        crate::internal::transport::CoreHttpTransport::new_signature_only(&client)
            .refresh_jwt_async()
            .await?;
        let mut prekey_publisher = ProductionDeviceJoinPrekeyPublisher;
        return publish_v2_prekeys_after_activation_with_publisher(
            core,
            session,
            &mut prekey_publisher,
        )
        .await;
    }
    let mut prekey_publisher = ProductionDeviceJoinPrekeyPublisher;
    publish_v2_prekeys_after_activation_with_publisher(core, session, &mut prekey_publisher)
        .await?;
    let mut group_key_package_publisher = ProductionDeviceJoinGroupKeyPackagePublisher;
    publish_v2_group_key_package_after_activation_with_publisher(
        core,
        session,
        &mut group_key_package_publisher,
    )
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostActivationPublishPolicy {
    PrekeysAndGroupKeyPackage,
    PrekeysOnly,
}

fn post_activation_publish_policy(recovery_join: bool) -> PostActivationPublishPolicy {
    if recovery_join {
        PostActivationPublishPolicy::PrekeysOnly
    } else {
        PostActivationPublishPolicy::PrekeysAndGroupKeyPackage
    }
}

pub(crate) trait DeviceJoinPrekeyPublisher {
    async fn publish(
        &mut self,
        core: &crate::core::ImCore,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<()>;
}

struct ProductionDeviceJoinPrekeyPublisher;

impl DeviceJoinPrekeyPublisher for ProductionDeviceJoinPrekeyPublisher {
    async fn publish(
        &mut self,
        core: &crate::core::ImCore,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<()> {
        crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published_from_authorized_document(
            core,
            client,
        )
        .await?;
        Ok(())
    }
}

pub(crate) async fn publish_v2_prekeys_after_activation_with_publisher<P>(
    core: &crate::core::ImCore,
    session: &crate::identity::DeviceJoinSessionSummary,
    publisher: &mut P,
) -> crate::ImResult<()>
where
    P: DeviceJoinPrekeyPublisher,
{
    let client = core
        .client_async(crate::identity::IdentitySelector::Did(session.did.clone()))
        .await?;
    publisher.publish(core, &client).await
}

pub(crate) trait DeviceJoinGroupKeyPackagePublisher {
    async fn publish(
        &mut self,
        client: &crate::core::ImClient,
        publish: &DeviceJoinGroupKeyPackagePublish,
    ) -> crate::ImResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceJoinGroupKeyPackagePublish {
    pub(crate) expected_device_id: String,
    pub(crate) operation_id: String,
    pub(crate) key_package_id: String,
}

struct ProductionDeviceJoinGroupKeyPackagePublisher;

#[cfg(feature = "group-e2ee")]
impl DeviceJoinGroupKeyPackagePublisher for ProductionDeviceJoinGroupKeyPackagePublisher {
    async fn publish(
        &mut self,
        client: &crate::core::ImClient,
        publish: &DeviceJoinGroupKeyPackagePublish,
    ) -> crate::ImResult<()> {
        crate::internal::group_e2ee::v2_lifecycle::publish_stable_key_package(
            client,
            &publish.expected_device_id,
            &publish.operation_id,
            &publish.key_package_id,
        )
        .await
    }
}

#[cfg(not(feature = "group-e2ee"))]
impl DeviceJoinGroupKeyPackagePublisher for ProductionDeviceJoinGroupKeyPackagePublisher {
    async fn publish(
        &mut self,
        _client: &crate::core::ImClient,
        _publish: &DeviceJoinGroupKeyPackagePublish,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::invalid_input(
            Some("multi_device_group_e2ee_enabled".to_owned()),
            "Group E2EE v2 requires the group-e2ee build feature",
        ))
    }
}

pub(crate) async fn publish_v2_group_key_package_after_activation_with_publisher<P>(
    core: &crate::core::ImCore,
    session: &crate::identity::DeviceJoinSessionSummary,
    publisher: &mut P,
) -> crate::ImResult<()>
where
    P: DeviceJoinGroupKeyPackagePublisher,
{
    if !core.inner().group_e2ee_v2_enabled() {
        return Ok(());
    }
    let client = core
        .client_async(crate::identity::IdentitySelector::Did(session.did.clone()))
        .await?;
    if client.did() != &session.did {
        return Err(crate::ImError::invalid_input(
            Some("join_session.did".to_owned()),
            "authorized Join DID does not match the selected local identity",
        ));
    }
    let device = core
        .identities()
        .device_summary_async(crate::identity::IdentitySelector::Did(session.did.clone()))
        .await?;
    if device.protocol_device_id.as_ref() != Some(&session.protocol_device_id) {
        return Err(crate::ImError::invalid_input(
            Some("join_session.protocol_device_id".to_owned()),
            "authorized Join device does not match the current local protocol device",
        ));
    }
    let publish = join_group_key_package_publish(session);
    publisher.publish(&client, &publish).await
}

fn join_group_key_package_publish(
    session: &crate::identity::DeviceJoinSessionSummary,
) -> DeviceJoinGroupKeyPackagePublish {
    let expected_device_id = session.protocol_device_id.as_str().to_owned();
    DeviceJoinGroupKeyPackagePublish {
        operation_id: deterministic_join_publish_id("operation", session),
        key_package_id: deterministic_join_publish_id("key-package", session),
        expected_device_id,
    }
}

fn deterministic_join_publish_id(
    kind: &str,
    session: &crate::identity::DeviceJoinSessionSummary,
) -> String {
    let mut digest = Sha256::new();
    for value in [
        "awiki.device.join.p6-key-package.v1",
        kind,
        session.did.as_str(),
        session.join_session_id.as_str(),
        session.protocol_device_id.as_str(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    let encoded = URL_SAFE_NO_PAD.encode(digest.finalize());
    match kind {
        "operation" => format!("join-p6-publish-{encoded}"),
        _ => format!("join-kp-{encoded}"),
    }
}

pub(crate) struct DeviceJoinAdminRuntime<'a, R> {
    core: &'a crate::core::ImCore,
    admin_identity: crate::identity::IdentitySelector,
    remote: R,
}

impl<'a, R> DeviceJoinAdminRuntime<'a, R> {
    pub(crate) fn new(
        core: &'a crate::core::ImCore,
        admin_identity: crate::identity::IdentitySelector,
        remote: R,
    ) -> Self {
        Self {
            core,
            admin_identity,
            remote,
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
        Self::new(
            core,
            crate::identity::IdentitySelector::Id(admin_client.current_identity().id.clone()),
            DeviceJoinAdminHttpAdapter::production(admin_client),
        )
    }
}

impl<R> DeviceJoinAdminRuntime<'_, R>
where
    R: DeviceJoinAdminRemote,
{
    pub(crate) async fn registry(&mut self) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        let did = self
            .core
            .client_async(self.admin_identity.clone())
            .await?
            .did()
            .clone();
        self.remote.registry(&did, false).await
    }

    pub(crate) async fn local_device_join_requests(
        &mut self,
    ) -> crate::ImResult<Vec<crate::identity::DeviceJoinRequestNotice>> {
        let client = self.core.client_async(self.admin_identity.clone()).await?;
        let current_device_id = client.exact_protocol_device_id()?;
        let notifications = client.list_verified_device_join_notifications(true).await?;
        let requests_by_session = notifications
            .iter()
            .filter_map(|notification| {
                notification
                    .initial_join_request
                    .clone()
                    .map(|request| (notification.join_session_id.clone(), request))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let mut notices = Vec::with_capacity(notifications.len());
        for notification in notifications {
            let local = local_admin_session(self.core, &notification.join_session_id)?;
            let claimed_by = match &notification.payload {
                crate::internal::system_notification::wire::JoinPayload::Claimed(payload) => {
                    Some(payload.claimed_by_device_id.as_str())
                }
                crate::internal::system_notification::wire::JoinPayload::ResponseVerified(
                    payload,
                ) => Some(payload.claimed_by_device_id.as_str()),
                _ => None,
            };
            let claimed_by_current_device =
                claimed_by.is_some_and(|claimed| current_device_id == claimed);
            if claimed_by_current_device
                && should_verify_response_from_notification(
                    local.as_ref().map(|session| session.phase),
                )
            {
                if let crate::internal::system_notification::wire::JoinPayload::ResponseVerified(
                    payload,
                ) = &notification.payload
                {
                    let response: DeviceJoinChallengeResponse = serde_json::from_value(
                        serde_json::to_value(&payload.challenge_response).map_err(|error| {
                            crate::ImError::Serialization {
                                detail: error.to_string(),
                            }
                        })?,
                    )
                    .map_err(|error| crate::ImError::Serialization {
                        detail: error.to_string(),
                    })?;
                    crate::internal::identity_device_join::verify_response_as_admin(
                        self.core,
                        crate::identity::DeviceJoinAdminVerifyRequest {
                            operation_id: format!(
                                "join-notification-verify:{}",
                                response.operation_id
                            ),
                            join_session_id: notification.join_session_id.clone(),
                            response,
                        },
                    )?;
                }
            }
            let request = match &notification.payload {
                crate::internal::system_notification::wire::JoinPayload::Requested(payload) => {
                    Some(&payload.join_request)
                }
                _ => requests_by_session.get(&notification.join_session_id),
            };
            let protocol_device_id = if let Some(request) = request {
                crate::ids::ProtocolDeviceId::parse(&request.device_id)?
            } else if let Some(local) = local.as_ref() {
                local.protocol_device_id.clone()
            } else {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "verified device Join notification has no request binding".to_owned(),
                });
            };
            let candidate_key_fingerprint = match request {
                Some(request) => verified_request_fingerprint(request)?,
                None => local
                    .as_ref()
                    .map(|session| session.join_request_hash.clone())
                    .unwrap_or_default(),
            };
            let can_start_verification = matches!(
                &notification.payload,
                crate::internal::system_notification::wire::JoinPayload::Requested(_)
            ) && local.is_none();
            notices.push(crate::identity::DeviceJoinRequestNotice {
                event_id: notification.event_id,
                join_session_id: notification.join_session_id,
                did: crate::ids::Did::parse(&notification.did)?,
                protocol_device_id,
                candidate_key_fingerprint,
                issued_at: notification.issued_at,
                expires_at: notification.expires_at,
                state: notification_state(notification.state),
                claimed_by_current_device,
                can_start_verification,
            });
        }
        Ok(notices)
    }

    pub(crate) async fn start_verification(
        &mut self,
        join_session_id: &str,
        operation_id: &str,
        challenge_ttl_seconds: u64,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        let client = self.core.client_async(self.admin_identity.clone()).await?;
        let notification = client
            .get_verified_device_join_notification(join_session_id)
            .await?
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: join_session_id.to_owned(),
            })?;
        let crate::internal::system_notification::wire::JoinPayload::Requested(payload) =
            notification.payload
        else {
            return Err(invalid_remote_state(
                "verification can start only from a requested notification",
            ));
        };
        let join_request: DeviceJoinRequest =
            serde_json::from_value(serde_json::to_value(payload.join_request).map_err(
                |error| crate::ImError::Serialization {
                    detail: error.to_string(),
                },
            )?)
            .map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        let registry = self.registry().await?;
        let prepared = crate::internal::identity_device_join::prepare_admin_challenge_async(
            self.core,
            crate::identity::DeviceJoinAdminPrepareRequest {
                admin_identity: self.admin_identity.clone(),
                operation_id: operation_id.to_owned(),
                join_request,
                challenge_ttl_seconds,
                document_version: registry.checkpoint.document_version,
                document_hash: registry.checkpoint.document_hash,
            },
        )
        .await?;
        let submitted = self.remote.submit_challenge(&prepared.challenge).await?;
        if submitted.state != DeviceJoinRemoteState::ChallengeSent {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(DeviceJoinAdvanceResult {
            session: prepared.session,
            remote_state: submitted.state,
            authorization: None,
            // The old admin must not display SAS until ResponseVerified.
            sas: None,
        })
    }

    pub(crate) async fn approve(
        &mut self,
        join_session_id: &str,
        operation_id: &str,
        user_presence_at: &str,
        sas_confirmed: bool,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        let prepared = match crate::internal::identity_device_join::load_prepared_admin_approval(
            self.core,
            join_session_id,
        )? {
            Some(value) => {
                if value.operation_id != operation_id
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
                let did = self
                    .core
                    .client_async(self.admin_identity.clone())
                    .await?
                    .did()
                    .clone();
                let registry = self.remote.registry(&did, false).await?;
                crate::internal::identity_device_join::prepare_admin_approval_async(
                    self.core,
                    operation_id,
                    join_session_id,
                    &registry.checkpoint,
                    user_presence_at,
                    sas_confirmed,
                )
                .await?
            }
        };
        let approved = self
            .remote
            .approve(DeviceJoinRemoteApproveRequest {
                operation_id: &prepared.operation_id,
                join_session_id: &prepared.join_session_id,
                expected_checkpoint: &prepared.expected_checkpoint,
                new_document: &prepared.new_document,
                pairing_confirmation: &prepared.pairing_confirmation,
                authorizing_device_id: &prepared.authorizing_device_id,
                proof: &prepared.proof,
            })
            .await?;
        if approved.state != DeviceJoinRemoteState::Consumed {
            return Err(crate::ImError::PermissionDenied);
        }
        let authorization = DeviceJoinRemoteAuthorization {
            checkpoint: approved.checkpoint,
            device: approved.device,
        };
        let client = self.core.client_async(self.admin_identity.clone()).await?;
        crate::internal::identity_device_join::complete_provider_document_change(
            &client,
            &prepared.new_document,
            &authorization.checkpoint,
        )
        .await?;
        let session = crate::internal::identity_device_join::mark_join_authorized_async(
            self.core,
            join_session_id,
            &authorization,
            &prepared.new_document,
        )
        .await?;
        Ok(DeviceJoinAdvanceResult {
            session,
            remote_state: approved.state,
            authorization: Some(authorization),
            sas: None,
        })
    }

    pub(crate) async fn reject(
        &mut self,
        join_session_id: &str,
        reason: crate::identity::DeviceJoinRejectReason,
    ) -> crate::ImResult<DeviceJoinAdvanceResult> {
        let fallback_session = if local_admin_session(self.core, join_session_id)?.is_none() {
            let client = self.core.client_async(self.admin_identity.clone()).await?;
            let notification = client
                .get_verified_device_join_notification(join_session_id)
                .await?
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: join_session_id.to_owned(),
                })?;
            let crate::internal::system_notification::wire::JoinPayload::Requested(payload) =
                notification.payload
            else {
                return Err(invalid_remote_state(
                    "an unclaimed rejection requires the verified requested notification",
                ));
            };
            Some(crate::identity::DeviceJoinSessionSummary {
                join_session_id: notification.join_session_id,
                did: crate::ids::Did::parse(&notification.did)?,
                protocol_device_id: crate::ids::ProtocolDeviceId::parse(
                    &payload.join_request.device_id,
                )?,
                side: crate::identity::DeviceJoinSide::Admin,
                phase: crate::identity::DeviceJoinLocalPhase::Cancelled,
                join_request_hash: verified_request_hash(&payload.join_request)?,
                challenge_id: None,
                expires_at: notification.expires_at,
            })
        } else {
            None
        };
        let prepared = crate::internal::identity_device_join::prepare_admin_rejection_async(
            self.core,
            self.admin_identity.clone(),
            join_session_id,
            reason,
        )
        .await?;
        let rejected = self
            .remote
            .reject(DeviceJoinRemoteRejectRequest {
                operation_id: &prepared.operation_id,
                join_session_id,
                rejecting_device_id: &prepared.rejecting_device_id,
                reason: reason.as_str(),
                proof: &prepared.proof,
            })
            .await?;
        if rejected.state != DeviceJoinRemoteState::Rejected {
            return Err(crate::ImError::PermissionDenied);
        }
        let session = match crate::internal::identity_device_join::cancel_join(
            self.core,
            join_session_id,
            crate::identity::DeviceJoinSide::Admin,
        )
        .await
        {
            Ok(session) => session,
            Err(crate::ImError::IdentityNotFound { .. }) => {
                fallback_session.ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: join_session_id.to_owned(),
                })?
            }
            Err(error) => return Err(error),
        };
        Ok(DeviceJoinAdvanceResult {
            session,
            remote_state: rejected.state,
            authorization: None,
            sas: None,
        })
    }
}

fn verified_request_fingerprint(
    request: &crate::internal::system_notification::wire::JoinRequest,
) -> crate::ImResult<String> {
    let value = serde_json::json!({
        "signing_public_key": request.signing_public_key,
        "e2ee_public_key": request.e2ee_public_key,
    });
    let canonical = serde_json_canonicalizer::to_vec(&value).map_err(|error| {
        crate::ImError::Serialization {
            detail: error.to_string(),
        }
    })?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

fn verified_request_hash(
    request: &crate::internal::system_notification::wire::JoinRequest,
) -> crate::ImResult<String> {
    let canonical = serde_json_canonicalizer::to_vec(request).map_err(|error| {
        crate::ImError::Serialization {
            detail: error.to_string(),
        }
    })?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

fn notification_state(
    state: crate::system_notifications::SystemNotificationState,
) -> crate::identity::DeviceJoinRemoteState {
    match state {
        crate::system_notifications::SystemNotificationState::Pending => {
            crate::identity::DeviceJoinRemoteState::Pending
        }
        crate::system_notifications::SystemNotificationState::ChallengeSent => {
            crate::identity::DeviceJoinRemoteState::ChallengeSent
        }
        crate::system_notifications::SystemNotificationState::ResponseVerified => {
            crate::identity::DeviceJoinRemoteState::ResponseVerified
        }
        crate::system_notifications::SystemNotificationState::Consumed => {
            crate::identity::DeviceJoinRemoteState::Consumed
        }
        crate::system_notifications::SystemNotificationState::Cancelled => {
            crate::identity::DeviceJoinRemoteState::Cancelled
        }
        crate::system_notifications::SystemNotificationState::Rejected => {
            crate::identity::DeviceJoinRemoteState::Rejected
        }
        crate::system_notifications::SystemNotificationState::Expired => {
            crate::identity::DeviceJoinRemoteState::Expired
        }
    }
}

fn should_verify_response_from_notification(
    local_phase: Option<crate::identity::DeviceJoinLocalPhase>,
) -> bool {
    local_phase == Some(crate::identity::DeviceJoinLocalPhase::ChallengePrepared)
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
