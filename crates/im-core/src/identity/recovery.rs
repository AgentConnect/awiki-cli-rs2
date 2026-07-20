//! AWiki-local Handle Recovery lifecycle.
//!
//! Recovery is a same-domain control-plane operation, not an ANP exchange.
//! It creates a completely new vNext DID and never invokes the legacy
//! `IdentityRegistry::recover_handle` state-merging path.

use serde::{Deserialize, Serialize};

/// Write-only grant issued for `awiki.device.recovery.begin.v1`.
pub struct HandleRecoveryBeginGrant {
    secret: crate::internal::platform_secret::SecretBytes,
}

impl HandleRecoveryBeginGrant {
    pub fn from_token(token: impl Into<String>) -> crate::ImResult<Self> {
        Self::from_bytes(token.into().into_bytes())
    }

    pub fn from_bytes(token: Vec<u8>) -> crate::ImResult<Self> {
        validate_grant(&token, "recovery_begin_grant")?;
        Ok(Self {
            secret: crate::internal::platform_secret::SecretBytes::from_vec(token),
        })
    }

    pub(crate) fn into_secret(self) -> crate::internal::platform_secret::SecretBytes {
        self.secret
    }
}

impl std::fmt::Debug for HandleRecoveryBeginGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HandleRecoveryBeginGrant(<redacted>)")
    }
}

/// Write-only, session-bound grant issued for
/// `awiki.device.recovery.finalize.v1`.
pub struct HandleRecoveryReconfirmationGrant {
    secret: crate::internal::platform_secret::SecretBytes,
}

impl HandleRecoveryReconfirmationGrant {
    pub fn from_token(token: impl Into<String>) -> crate::ImResult<Self> {
        Self::from_bytes(token.into().into_bytes())
    }

    pub fn from_bytes(token: Vec<u8>) -> crate::ImResult<Self> {
        validate_grant(&token, "recovery_reconfirmation_grant")?;
        Ok(Self {
            secret: crate::internal::platform_secret::SecretBytes::from_vec(token),
        })
    }

    pub(crate) fn into_secret(self) -> crate::internal::platform_secret::SecretBytes {
        self.secret
    }
}

impl std::fmt::Debug for HandleRecoveryReconfirmationGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HandleRecoveryReconfirmationGrant(<redacted>)")
    }
}

fn validate_grant(value: &[u8], field: &str) -> crate::ImResult<()> {
    if value.is_empty() || value.iter().all(u8::is_ascii_whitespace) {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            "verification grant must not be empty",
        ));
    }
    std::str::from_utf8(value).map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_owned()), "verification grant must be UTF-8")
    })?;
    Ok(())
}

pub struct HandleRecoveryBeginRequest {
    pub handle: crate::ids::Handle,
    pub account_verification_grant: HandleRecoveryBeginGrant,
}

impl std::fmt::Debug for HandleRecoveryBeginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandleRecoveryBeginRequest")
            .field("handle", &self.handle)
            .field("account_verification_grant", &"<redacted>")
            .finish()
    }
}

pub struct HandleRecoveryFinalizeRequest {
    pub recovery_session_id: String,
    pub reconfirmation_grant: HandleRecoveryReconfirmationGrant,
    pub user_presence_confirmed: bool,
}

impl std::fmt::Debug for HandleRecoveryFinalizeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandleRecoveryFinalizeRequest")
            .field("recovery_session_id", &self.recovery_session_id)
            .field("reconfirmation_grant", &"<redacted>")
            .field("user_presence_confirmed", &self.user_presence_confirmed)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleRecoveryCancelRequest {
    pub old_identity: super::IdentitySelector,
    pub recovery_session_id: String,
    pub user_presence_confirmed: bool,
}

/// Secret-free, durable warning projected for one exact old-admin device.
///
/// This is intentionally independent from [`HandleRecoveryProgress`]: it is
/// discovery evidence for a possible cancel attempt, not requester lifecycle
/// state and not proof that this device is currently authorized to cancel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OldAdminRecoveryNotice {
    pub event_id: String,
    pub recovery_session_id: String,
    pub handle: crate::ids::Handle,
    pub old_did: crate::ids::Did,
    pub requested_at: String,
    pub cancellable_until: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldAdminRecoveryNoticeDismissRequest {
    pub old_identity: super::IdentitySelector,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OldAdminRecoveryNoticeDismissResult {
    pub event_id: String,
    pub dismissed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRecoverySide {
    Requester,
    OldAdmin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleRecoveryPhase {
    Cooling,
    Ready,
    Cancelled,
    Expired,
    Consumed,
}

/// Safe host-facing projection. Recovery tokens, proofs, generated documents,
/// private keys and AWiki-internal checkpoints deliberately stay below Core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryProgress {
    pub recovery_session_id: String,
    pub handle: crate::ids::Handle,
    pub old_did: crate::ids::Did,
    pub side: HandleRecoverySide,
    pub phase: HandleRecoveryPhase,
    pub cooling_until: String,
    pub expires_at: String,
    pub can_cancel_from_this_device: bool,
    pub new_did: Option<crate::ids::Did>,
    pub local_activation_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryCancelResult {
    pub recovery_session_id: String,
    pub phase: HandleRecoveryPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleRecoveryFinalizeResult {
    pub progress: HandleRecoveryProgress,
    pub identity: super::IdentitySummary,
}

pub struct HandleRecoveryService<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> HandleRecoveryService<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    pub fn local_sessions(&self) -> crate::ImResult<Vec<HandleRecoveryProgress>> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::local_sessions(self.core)
    }

    pub fn list_old_admin_notices(
        &self,
        old_identity: super::IdentitySelector,
    ) -> crate::ImResult<Vec<OldAdminRecoveryNotice>> {
        self.require_enabled()?;
        #[cfg(feature = "sqlite")]
        {
            crate::internal::identity_recovery_notice::list_old_admin_notices(
                self.core,
                old_identity,
            )
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = old_identity;
            Err(crate::ImError::unsupported(
                "old-admin-recovery-notice-local-state",
            ))
        }
    }

    pub fn get_old_admin_notice(
        &self,
        old_identity: super::IdentitySelector,
        event_id: &str,
    ) -> crate::ImResult<Option<OldAdminRecoveryNotice>> {
        self.require_enabled()?;
        #[cfg(feature = "sqlite")]
        {
            crate::internal::identity_recovery_notice::get_old_admin_notice(
                self.core,
                old_identity,
                event_id,
            )
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = (old_identity, event_id);
            Err(crate::ImError::unsupported(
                "old-admin-recovery-notice-local-state",
            ))
        }
    }

    pub fn dismiss_old_admin_notice(
        &self,
        request: OldAdminRecoveryNoticeDismissRequest,
    ) -> crate::ImResult<OldAdminRecoveryNoticeDismissResult> {
        self.require_enabled()?;
        #[cfg(feature = "sqlite")]
        {
            crate::internal::identity_recovery_notice::dismiss_old_admin_notice(self.core, request)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported(
                "old-admin-recovery-notice-local-state",
            ))
        }
    }

    pub async fn begin(
        &self,
        request: HandleRecoveryBeginRequest,
    ) -> crate::ImResult<HandleRecoveryProgress> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::begin(self.core, request).await
    }

    pub async fn status(
        &self,
        recovery_session_id: &str,
    ) -> crate::ImResult<HandleRecoveryProgress> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::status(self.core, recovery_session_id).await
    }

    pub async fn cancel(
        &self,
        request: HandleRecoveryCancelRequest,
    ) -> crate::ImResult<HandleRecoveryCancelResult> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::cancel(self.core, request).await
    }

    pub async fn finalize(
        &self,
        request: HandleRecoveryFinalizeRequest,
    ) -> crate::ImResult<HandleRecoveryFinalizeResult> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::finalize(self.core, request).await
    }

    pub fn resume_activation(
        &self,
        recovery_session_id: &str,
    ) -> crate::ImResult<super::IdentitySummary> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::resume_activation(self.core, recovery_session_id)
    }

    pub async fn resume_activation_async(
        &self,
        recovery_session_id: &str,
    ) -> crate::ImResult<super::IdentitySummary> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::resume_activation_async(
            self.core,
            recovery_session_id,
        )
        .await
    }

    pub fn mark_activation_complete(&self, recovery_session_id: &str) -> crate::ImResult<()> {
        self.require_enabled()?;
        crate::internal::identity_recovery_vnext::mark_activation_complete(
            self.core,
            recovery_session_id,
        )
    }

    fn require_enabled(&self) -> crate::ImResult<()> {
        if self.core.inner().handle_recovery_enabled() {
            Ok(())
        } else {
            Err(crate::ImError::unsupported(
                "awiki-handle-recovery-disabled",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_are_write_only_and_debug_redacted() {
        let begin = HandleRecoveryBeginGrant::from_token("begin-secret").unwrap();
        let finalize = HandleRecoveryReconfirmationGrant::from_token("finalize-secret").unwrap();
        assert_eq!(format!("{begin:?}"), "HandleRecoveryBeginGrant(<redacted>)");
        assert_eq!(
            format!("{finalize:?}"),
            "HandleRecoveryReconfirmationGrant(<redacted>)"
        );
        assert!(!format!("{begin:?}").contains("begin-secret"));
        assert!(!format!("{finalize:?}").contains("finalize-secret"));
    }
}
