use std::sync::Arc;

use crate::dto::{
    auth::{DartAuthScope, DartAuthStatus, DartSessionBundle, DartSessionUpdate},
    error::DartImError,
};

pub fn auth_status(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartAuthStatus, DartImError> {
    client.with_inner(|inner| {
        inner
            .auth()
            .status()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn auth_login(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartSessionBundle, DartImError> {
    client.with_inner(|inner| {
        inner
            .auth()
            .login()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn auth_ensure_session(
    client: &Arc<crate::api::client::DartImClient>,
    scope: DartAuthScope,
) -> Result<DartSessionBundle, DartImError> {
    client.with_inner(|inner| {
        inner
            .auth()
            .ensure_session(scope.into())
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn auth_refresh_session(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartSessionUpdate, DartImError> {
    client.with_inner(|inner| {
        inner
            .auth()
            .refresh_session()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}
