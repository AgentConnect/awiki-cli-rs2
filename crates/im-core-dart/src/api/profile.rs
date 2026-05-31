use std::sync::Arc;

use crate::dto::{
    directory::DartIdentitySubject,
    error::DartImError,
    profile::{DartProfilePatch, DartUserProfile},
};

pub async fn load_my_profile(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartUserProfile, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .identity()
        .profile_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn update_profile(
    client: &Arc<crate::api::client::DartImClient>,
    patch: DartProfilePatch,
) -> Result<DartUserProfile, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .identity()
        .update_profile_async(patch.into())
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn load_public_profile(
    client: &Arc<crate::api::client::DartImClient>,
    subject: DartIdentitySubject,
) -> Result<DartUserProfile, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .directory()
        .public_profile_async(subject.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
