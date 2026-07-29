use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    identity::{
        DartActiveSyncAccountBinding, DartDaemonSubkeyAuthorizationRevokeResult,
        DartDaemonSubkeyPrivatePackage, DartDeleteLocalIdentityResult,
        DartDeviceJoinApprovalPrompt, DartDeviceJoinProgress, DartDeviceJoinRegistrySnapshot,
        DartDeviceJoinRejectReason, DartDeviceJoinRequestNotice, DartDeviceJoinSessionSummary,
        DartDeviceRevokeResult, DartHandleRegistrationResult, DartIdentityDeviceSummary,
        DartIdentitySelector, DartIdentitySummary, DartIdentityVaultMigrationReport,
        DartIdentityVaultStatus, DartIdentityVaultVerificationReport, DartInitialProfile,
        DartLegacyUpgradeStatus, DartRootKeyTransferError, DartRootKeyTransferPreparation,
        DartRootKeyTransferSendResult,
    },
};

pub async fn active_sync_account_binding(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartActiveSyncAccountBinding, DartImError> {
    client
        .clone_inner()?
        .active_sync_account_binding()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn revoke_device(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    target_device_id: String,
    user_presence_confirmed: bool,
) -> Result<DartDeviceRevokeResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_revoke()
        .revoke(im_core::identity::DeviceRevokeRequest {
            identity: selector.try_into()?,
            target_device_id: im_core::ids::ProtocolDeviceId::parse(target_device_id)
                .map_err(DartImError::from)?,
            user_presence_confirmed,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn prepare_root_key_transfer(
    client: &Arc<crate::api::client::DartImClient>,
    recipient_device_id: String,
) -> Result<DartRootKeyTransferPreparation, DartRootKeyTransferError> {
    let inner = client
        .clone_inner()
        .map_err(|_| DartRootKeyTransferError::temporarily_unavailable())?;
    let recipient_device_id = im_core::ids::ProtocolDeviceId::parse(recipient_device_id)
        .map_err(|_| DartRootKeyTransferError::invalid_request())?;
    inner
        .root_key_transfer()
        .prepare(im_core::identity::RootKeyTransferPrepareRequest {
            recipient_device_id,
        })
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn confirm_and_send_root_key_transfer(
    client: &Arc<crate::api::client::DartImClient>,
    authorization_handle: String,
    user_presence_confirmed: bool,
) -> Result<DartRootKeyTransferSendResult, DartRootKeyTransferError> {
    let inner = client
        .clone_inner()
        .map_err(|_| DartRootKeyTransferError::temporarily_unavailable())?;
    let authorization_handle =
        serde_json::from_value(serde_json::Value::String(authorization_handle))
            .map_err(|_| DartRootKeyTransferError::authorization_invalid())?;
    inner
        .root_key_transfer()
        .confirm_and_send(im_core::identity::RootKeyTransferSendRequest {
            authorization_handle,
            user_presence_confirmed,
        })
        .await
        .map(Into::into)
        .map_err(Into::into)
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
        .await
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

pub async fn local_device_join_requests(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<Vec<DartDeviceJoinRequestNotice>, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .local_device_join_requests(selector.try_into()?)
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(DartImError::from)
}

pub async fn local_device_join_verification_progress(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .local_device_join_verification_progress(selector.try_into()?, &join_session_id)
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn start_device_join_verification(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
    operation_id: String,
    challenge_ttl_seconds: u64,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .start_device_join_verification(
            selector.try_into()?,
            &join_session_id,
            &operation_id,
            challenge_ttl_seconds,
        )
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn prepare_device_join_approval(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
    sas_confirmed: bool,
) -> Result<DartDeviceJoinApprovalPrompt, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .prepare_device_join_approval(selector.try_into()?, &join_session_id, sas_confirmed)
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

pub async fn reject_device_join(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
    join_session_id: String,
    reason: DartDeviceJoinRejectReason,
) -> Result<DartDeviceJoinProgress, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .device_join()
        .reject_device_join(
            selector.try_into()?,
            &join_session_id,
            match reason {
                DartDeviceJoinRejectReason::UserRejected => {
                    im_core::identity::DeviceJoinRejectReason::UserRejected
                }
                DartDeviceJoinRejectReason::SasMismatch => {
                    im_core::identity::DeviceJoinRejectReason::SasMismatch
                }
            },
        )
        .await
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

pub async fn legacy_upgrade_status(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartLegacyUpgradeStatus, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .legacy_upgrade_status(selector.try_into()?)
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn upgrade_legacy_identity(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartLegacyUpgradeStatus, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .upgrade_legacy_identity_async(selector.try_into()?)
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

impl From<DartInitialProfile> for im_core::identity::InitialProfile {
    fn from(value: DartInitialProfile) -> Self {
        Self {
            display_name: value.display_name,
            avatar_url: value.avatar_url,
        }
    }
}
