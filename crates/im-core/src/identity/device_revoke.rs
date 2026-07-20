//! Safe public boundary for permanently revoking one AWiki device.
//!
//! Registry checkpoints, root/admin proofs, the root-signed DID Document and
//! exact-retry state remain internal. This is an AWiki same-domain control
//! operation; it does not add fields to ANP.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRevokeRequest {
    pub identity: super::IdentitySelector,
    pub target_device_id: crate::ids::ProtocolDeviceId,
    /// Set only after foreground OS/user-presence confirmation.
    pub user_presence_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRevokeStatus {
    Revoked,
}

/// Safe host-facing projection. It deliberately excludes operation IDs,
/// checkpoints, proofs, documents, keys and bearer credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRevokeResult {
    pub did: crate::ids::Did,
    pub target_device_id: crate::ids::ProtocolDeviceId,
    pub status: DeviceRevokeStatus,
}

pub struct DeviceRevokeService<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> DeviceRevokeService<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    /// Permanently revokes one other active device.
    ///
    /// Repeating the same call after a transport failure resumes the exact
    /// persisted operation. A revoked device is never reactivated in place.
    pub async fn revoke(
        &self,
        request: DeviceRevokeRequest,
    ) -> crate::ImResult<DeviceRevokeResult> {
        if !self.core.inner().device_revoke_enabled() {
            return Err(crate::ImError::unsupported("awiki-device-revoke-disabled"));
        }
        if !request.user_presence_confirmed {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_device_revoke::revoke(
            self.core,
            request.identity,
            request.target_device_id,
        )
        .await
    }
}
