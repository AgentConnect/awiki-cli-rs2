use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    identity::{
        DartDaemonSubkeyAuthorizationRevokeResult, DartDaemonSubkeyPrivatePackage,
        DartDeleteLocalIdentityResult, DartDeviceJoinApprovalPrompt, DartDeviceJoinProgress,
        DartDeviceJoinRegistrySnapshot, DartDeviceJoinRole, DartDeviceJoinSessionSummary,
        DartHandleRecoveryCancelResult, DartHandleRecoveryFinalizeResult,
        DartHandleRecoveryProgress, DartHandleRegistrationResult, DartIdentityDeviceSummary,
        DartIdentitySelector, DartIdentitySummary, DartIdentityVaultMigrationReport,
        DartIdentityVaultStatus, DartIdentityVaultVerificationReport, DartInitialProfile,
        DartRecoverHandleResult, DartRootKeyTransferSendResult, DartRootKeyTransferSummary,
    },
};

pub async fn local_handle_recovery_sessions(
    core: &Arc<crate::api::core::DartImCore>,
) -> Result<Vec<DartHandleRecoveryProgress>, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .handle_recovery()
        .local_sessions()
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}

pub async fn begin_handle_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    handle: String,
    begin_verification_grant: Vec<u8>,
) -> Result<DartHandleRecoveryProgress, DartImError> {
    let inner = core.clone_inner()?;
    let grant = im_core::identity::HandleRecoveryBeginGrant::from_bytes(begin_verification_grant)
        .map_err(DartImError::from)?;
    inner
        .handle_recovery()
        .begin(im_core::identity::HandleRecoveryBeginRequest {
            handle: im_core::ids::Handle::parse(handle, "").map_err(DartImError::from)?,
            account_verification_grant: grant,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn poll_handle_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    recovery_session_id: String,
) -> Result<DartHandleRecoveryProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .handle_recovery()
        .status(&recovery_session_id)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn cancel_handle_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    old_identity: DartIdentitySelector,
    recovery_session_id: String,
    user_presence_confirmed: bool,
) -> Result<DartHandleRecoveryCancelResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .handle_recovery()
        .cancel(im_core::identity::HandleRecoveryCancelRequest {
            old_identity: old_identity.try_into()?,
            recovery_session_id,
            user_presence_confirmed,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn finalize_handle_recovery(
    core: &Arc<crate::api::core::DartImCore>,
    recovery_session_id: String,
    finalize_verification_grant: Vec<u8>,
    user_presence_confirmed: bool,
) -> Result<DartHandleRecoveryFinalizeResult, DartImError> {
    let inner = core.clone_inner()?;
    let grant = im_core::identity::HandleRecoveryReconfirmationGrant::from_bytes(
        finalize_verification_grant,
    )
    .map_err(DartImError::from)?;
    inner
        .handle_recovery()
        .finalize(im_core::identity::HandleRecoveryFinalizeRequest {
            recovery_session_id,
            reconfirmation_grant: grant,
            user_presence_confirmed,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn resume_handle_recovery_activation(
    core: &Arc<crate::api::core::DartImCore>,
    recovery_session_id: String,
) -> Result<DartIdentitySummary, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .handle_recovery()
        .resume_activation_async(&recovery_session_id)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn mark_handle_recovery_activation_complete(
    core: &Arc<crate::api::core::DartImCore>,
    recovery_session_id: String,
) -> Result<(), DartImError> {
    let inner = core.clone_inner()?;
    inner
        .handle_recovery()
        .mark_activation_complete(&recovery_session_id)
        .map_err(DartImError::from)
}

pub async fn send_root_key_transfer(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    recipient_device_id: String,
    message_id: String,
    user_presence_confirmed: bool,
) -> Result<DartRootKeyTransferSendResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .root_key_transfer()
        .send(im_core::identity::RootKeyTransferSendRequest {
            identity: selector.try_into()?,
            recipient_device_id: im_core::ids::ProtocolDeviceId::parse(recipient_device_id)
                .map_err(DartImError::from)?,
            message_id: im_core::ids::MessageId::parse(message_id).map_err(DartImError::from)?,
            user_presence_confirmed,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn list_root_key_transfers(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    include_completed: bool,
) -> Result<Vec<DartRootKeyTransferSummary>, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .root_key_transfer()
        .list(im_core::identity::RootKeyTransferListRequest {
            identity: selector.try_into()?,
            include_completed,
        })
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}

pub async fn retry_root_key_transfer(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    message_id: String,
    user_presence_confirmed: bool,
) -> Result<DartRootKeyTransferSummary, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .root_key_transfer()
        .retry(im_core::identity::RootKeyTransferRetryRequest {
            identity: selector.try_into()?,
            message_id: im_core::ids::MessageId::parse(message_id).map_err(DartImError::from)?,
            user_presence_confirmed,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn local_device_join_sessions(
    core: &Arc<crate::api::core::DartImCore>,
) -> Result<Vec<DartDeviceJoinSessionSummary>, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .local_sessions()
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}

pub async fn begin_device_join(
    core: &Arc<crate::api::core::DartImCore>,
    did: String,
    operation_id: String,
    ttl_seconds: u64,
    account_verification_grant: Vec<u8>,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    let grant = im_core::identity::DeviceJoinAccountVerificationGrant::from_bytes(
        account_verification_grant,
    )
    .map_err(DartImError::from)?;
    inner
        .device_join()
        .begin_new_device_join(im_core::identity::DeviceJoinBeginRequest {
            operation_id,
            did: im_core::ids::Did::parse(did).map_err(DartImError::from)?,
            ttl_seconds,
            account_verification_grant: grant,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn poll_new_device_join(
    core: &Arc<crate::api::core::DartImCore>,
    join_session_id: String,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .poll_new_device_join(&join_session_id)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn cancel_new_device_join(
    core: &Arc<crate::api::core::DartImCore>,
    join_session_id: String,
) -> Result<DartDeviceJoinSessionSummary, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .cancel_new_device_join(&join_session_id)
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn identity_device_registry(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartDeviceJoinRegistrySnapshot, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .registry(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn claim_device_join(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
    operation_id: String,
    challenge_ttl_seconds: u64,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .claim_device_join(
            selector.try_into()?,
            &join_session_id,
            &operation_id,
            challenge_ttl_seconds,
        )
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn poll_admin_device_join(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .poll_admin_device_join(selector.try_into()?, &join_session_id)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn prepare_device_join_approval(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
    role: DartDeviceJoinRole,
    sas_confirmed: bool,
) -> Result<DartDeviceJoinApprovalPrompt, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .prepare_device_join_approval(
            selector.try_into()?,
            &join_session_id,
            match role {
                DartDeviceJoinRole::Member => im_core::identity::DeviceJoinRole::Member,
                DartDeviceJoinRole::Admin => im_core::identity::DeviceJoinRole::Admin,
            },
            sas_confirmed,
        )
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn confirm_device_join_approval(
    core: &Arc<crate::api::core::DartImCore>,
    approval_handle: String,
    user_presence_confirmed: bool,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .confirm_device_join_approval(im_core::identity::DeviceJoinConfirmApprovalRequest {
            approval_handle,
            user_presence_confirmed,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn cancel_admin_device_join(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
) -> Result<DartDeviceJoinSessionSummary, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .cancel_admin_device_join(selector.try_into()?, &join_session_id)
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn list_identities(
    core: &Arc<crate::api::core::DartImCore>,
) -> Result<Vec<DartIdentitySummary>, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .list_async()
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}

pub async fn default_identity(
    core: &Arc<crate::api::core::DartImCore>,
) -> Result<Option<DartIdentitySummary>, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .default_identity_async()
        .await
        .map(|item| item.map(Into::into))
        .map_err(DartImError::from)
}

pub async fn resolve_identity(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartIdentitySummary, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .resolve_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn identity_device_summary(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartIdentityDeviceSummary, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .device_summary_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn identity_vault_status(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartIdentityVaultStatus, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .vault_status_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn migrate_identity_vault(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartIdentityVaultMigrationReport, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .migrate_identity_vault_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn verify_identity_vault(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartIdentityVaultVerificationReport, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .verify_identity_vault_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn delete_local_identity(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartDeleteLocalIdentityResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .delete_local_identity_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn load_daemon_subkey_package(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartDaemonSubkeyPrivatePackage, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .load_daemon_subkey_package_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn ensure_daemon_subkey_package(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartDaemonSubkeyPrivatePackage, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .ensure_daemon_subkey_package_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn revoke_daemon_subkey_authorization(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartDaemonSubkeyAuthorizationRevokeResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .revoke_daemon_subkey_authorization_async(selector.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn register_handle_with_phone(
    core: &Arc<crate::api::core::DartImCore>,
    local_alias: Option<String>,
    requested_handle: String,
    phone: String,
    otp: Option<String>,
    invite_code: Option<String>,
    profile: DartInitialProfile,
    make_default: bool,
) -> Result<DartHandleRegistrationResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .register_handle_async(im_core::identity::RegisterHandleRequest {
            local_alias,
            requested_handle: im_core::ids::Handle::parse(requested_handle, "")
                .map_err(DartImError::from)?,
            verification: im_core::identity::VerificationInput::Phone { phone, otp },
            invite_code,
            profile: profile.into(),
            make_default,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn register_handle_with_email(
    core: &Arc<crate::api::core::DartImCore>,
    local_alias: Option<String>,
    requested_handle: String,
    email: String,
    wait_for_verification: bool,
    invite_code: Option<String>,
    profile: DartInitialProfile,
    make_default: bool,
) -> Result<DartHandleRegistrationResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .register_handle_async(im_core::identity::RegisterHandleRequest {
            local_alias,
            requested_handle: im_core::ids::Handle::parse(requested_handle, "")
                .map_err(DartImError::from)?,
            verification: im_core::identity::VerificationInput::Email {
                email,
                wait_for_verification,
            },
            invite_code,
            profile: profile.into(),
            make_default,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn register_handle_without_contact_verification(
    core: &Arc<crate::api::core::DartImCore>,
    local_alias: Option<String>,
    requested_handle: String,
    invite_code: Option<String>,
    profile: DartInitialProfile,
    make_default: bool,
) -> Result<DartHandleRegistrationResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .register_handle_async(im_core::identity::RegisterHandleRequest {
            local_alias,
            requested_handle: im_core::ids::Handle::parse(requested_handle, "")
                .map_err(DartImError::from)?,
            verification: im_core::identity::VerificationInput::AlreadyVerified,
            invite_code,
            profile: profile.into(),
            make_default,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn recover_handle(
    core: &Arc<crate::api::core::DartImCore>,
    handle: String,
    phone: String,
    otp: Option<String>,
) -> Result<DartRecoverHandleResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .recover_handle_async(recover_handle_request(handle, phone, otp)?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

fn recover_handle_request(
    handle: String,
    phone: String,
    otp: Option<String>,
) -> Result<im_core::identity::RecoverHandleRequest, DartImError> {
    Ok(im_core::identity::RecoverHandleRequest {
        handle: im_core::ids::Handle::parse(&handle, "").map_err(DartImError::from)?,
        raw_handle: Some(handle),
        phone,
        otp,
        generated_identity: None,
        // Handle recovery rotates the DID but must keep the local identity
        // owner stable. The CLI already opts into this path explicitly;
        // Dart hosts must use the same finalization semantics so local
        // history and group rebind work are preserved.
        local_finalize: Some(im_core::identity::RecoverHandleLocalFinalizeRequest::default()),
    })
}

impl From<DartInitialProfile> for im_core::identity::InitialProfile {
    fn from(value: DartInitialProfile) -> Self {
        Self {
            display_name: value.display_name,
            avatar_url: value.avatar_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_handle_uses_local_finalize_for_dart_hosts() {
        let request = recover_handle_request(
            "alice.awiki.test".to_string(),
            "+15551234567".to_string(),
            Some("123456".to_string()),
        )
        .unwrap();

        assert!(request.generated_identity.is_none());
        assert!(request.local_finalize.is_some());
        assert_eq!(request.raw_handle.as_deref(), Some("alice.awiki.test"));
    }
}
