use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    identity::{
        DartHandleRegistrationResult, DartIdentitySelector, DartIdentitySummary,
        DartInitialProfile, DartRecoverHandleResult,
    },
};

pub fn list_identities(
    core: Arc<crate::api::core::DartImCore>,
) -> Result<Vec<DartIdentitySummary>, DartImError> {
    core.with_inner(|inner| {
        inner
            .identities()
            .list()
            .map(|items| items.into_iter().map(Into::into).collect())
            .map_err(DartImError::from)
    })
}

pub fn default_identity(
    core: Arc<crate::api::core::DartImCore>,
) -> Result<Option<DartIdentitySummary>, DartImError> {
    core.with_inner(|inner| {
        inner
            .identities()
            .default_identity()
            .map(|item| item.map(Into::into))
            .map_err(DartImError::from)
    })
}

pub fn resolve_identity(
    core: Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<DartIdentitySummary, DartImError> {
    core.with_inner(|inner| {
        inner
            .identities()
            .resolve(selector.try_into()?)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn register_handle_with_phone(
    core: Arc<crate::api::core::DartImCore>,
    local_alias: Option<String>,
    requested_handle: String,
    phone: String,
    otp: Option<String>,
    invite_code: Option<String>,
    profile: DartInitialProfile,
    make_default: bool,
) -> Result<DartHandleRegistrationResult, DartImError> {
    core.with_inner(|inner| {
        inner
            .identities()
            .register_handle(im_core::identity::RegisterHandleRequest {
                local_alias,
                requested_handle: im_core::ids::Handle::parse(requested_handle, "")
                    .map_err(DartImError::from)?,
                verification: im_core::identity::VerificationInput::Phone { phone, otp },
                invite_code,
                profile: profile.into(),
                make_default,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn register_handle_with_email(
    core: Arc<crate::api::core::DartImCore>,
    local_alias: Option<String>,
    requested_handle: String,
    email: String,
    wait_for_verification: bool,
    invite_code: Option<String>,
    profile: DartInitialProfile,
    make_default: bool,
) -> Result<DartHandleRegistrationResult, DartImError> {
    core.with_inner(|inner| {
        inner
            .identities()
            .register_handle(im_core::identity::RegisterHandleRequest {
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
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn recover_handle(
    core: Arc<crate::api::core::DartImCore>,
    handle: String,
    phone: String,
    otp: Option<String>,
) -> Result<DartRecoverHandleResult, DartImError> {
    core.with_inner(|inner| {
        inner
            .identities()
            .recover_handle(im_core::identity::RecoverHandleRequest {
                handle: im_core::ids::Handle::parse(handle, "").map_err(DartImError::from)?,
                phone,
                otp,
                generated_identity: None,
            })
            .map(Into::into)
            .map_err(DartImError::from)
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
