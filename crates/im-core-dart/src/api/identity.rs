use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    identity::{
        DartDaemonSubkeyAuthorizationRevokeResult, DartDaemonSubkeyPrivatePackage,
        DartDeleteLocalIdentityResult, DartHandleRegistrationResult, DartIdentitySelector,
        DartIdentitySummary, DartIdentityVaultMigrationReport, DartIdentityVaultStatus,
        DartIdentityVaultVerificationReport, DartInitialProfile, DartRecoverHandleResult,
    },
};

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

pub async fn recover_handle(
    core: &Arc<crate::api::core::DartImCore>,
    handle: String,
    phone: String,
    otp: Option<String>,
) -> Result<DartRecoverHandleResult, DartImError> {
    let inner = core.clone_inner()?;
    inner
        .identities()
        .recover_handle_async(im_core::identity::RecoverHandleRequest {
            handle: im_core::ids::Handle::parse(&handle, "").map_err(DartImError::from)?,
            raw_handle: Some(handle),
            phone,
            otp,
            generated_identity: None,
            local_finalize: None,
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
