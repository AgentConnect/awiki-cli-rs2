use std::sync::Arc;

use crate::dto::{
    auth::{DartAuthScope, DartAuthStatus, DartSessionBundle, DartSessionUpdate},
    error::DartImError,
};

pub async fn auth_status(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartAuthStatus, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .auth()
        .status_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn auth_login(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartSessionBundle, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .auth()
        .login_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn auth_ensure_session(
    client: &Arc<crate::api::client::DartImClient>,
    scope: DartAuthScope,
) -> Result<DartSessionBundle, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .auth()
        .ensure_session_async(scope.into())
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn auth_refresh_session(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartSessionUpdate, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .auth()
        .refresh_session_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
