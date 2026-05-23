use std::sync::Arc;

use crate::dto::{
    directory::DartIdentitySubject,
    error::DartImError,
    profile::{DartProfilePatch, DartUserProfile},
};

pub fn load_my_profile(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartUserProfile, DartImError> {
    client.with_inner(|inner| {
        inner
            .identity()
            .profile()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn update_profile(
    client: &Arc<crate::api::client::DartImClient>,
    patch: DartProfilePatch,
) -> Result<DartUserProfile, DartImError> {
    client.with_inner(|inner| {
        inner
            .identity()
            .update_profile(patch.into())
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn load_public_profile(
    client: &Arc<crate::api::client::DartImClient>,
    subject: DartIdentitySubject,
) -> Result<DartUserProfile, DartImError> {
    client.with_inner(|inner| {
        inner
            .directory()
            .public_profile(subject.try_into()?)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}
