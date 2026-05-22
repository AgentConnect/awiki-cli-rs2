use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    identity::{DartIdentitySelector, DartIdentitySummary},
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
