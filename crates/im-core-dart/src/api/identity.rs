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
